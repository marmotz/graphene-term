//! Shell integration: makes zsh, bash and fish emit OSC 133 marks without touching the
//! user's dotfiles.
//!
//! The scripts of `shell-integration/` are embedded in the binary, written at startup
//! to a private runtime folder, and injected into the spawned shell through its startup
//! mechanism: `ZDOTDIR` for zsh, `--posix` + `ENV` for bash, `XDG_DATA_DIRS` for fish.
//! Each script restores the variable it came through before loading the user's files.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::warn;

/// Opt-out variable: `0` starts the shell without integration.
pub const OPT_OUT_VAR: &str = "GRAPHENE_SHELL_INTEGRATION";

/// zsh entry point, relative to the integration root.
const ZSH_DIR: &str = "zsh";
/// bash entry point, relative to the integration root.
const BASH_SCRIPT: &str = "bash/graphene.bash";

/// Embedded scripts, by path relative to the integration root.
const SCRIPTS: [(&str, &str); 3] = [
    (
        "zsh/.zshenv",
        include_str!("../shell-integration/zsh/.zshenv"),
    ),
    (
        BASH_SCRIPT,
        include_str!("../shell-integration/bash/graphene.bash"),
    ),
    (
        "fish/vendor_conf.d/graphene.fish",
        include_str!("../shell-integration/fish/vendor_conf.d/graphene.fish"),
    ),
];

/// `XDG_DATA_DIRS` value when the variable is unset or empty (XDG base directory spec).
const DEFAULT_XDG_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// How to start a supported shell with the integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    pub program: String,
    pub args: Vec<String>,
    /// Variables to set in the shell's environment.
    pub env: Vec<(String, String)>,
}

/// Returns how to start the user's shell with the integration, or `None` to start it as
/// usual: opted out, `$SHELL` unset or not zsh / bash / fish, or scripts not writable.
pub fn prepare() -> Option<Injection> {
    let var = |name: &str| std::env::var(name).ok();
    if !enabled(var(OPT_OUT_VAR).as_deref()) {
        return None;
    }
    let shell = var("SHELL")?;
    let root = root_dir(
        var("XDG_RUNTIME_DIR").as_deref(),
        &std::env::temp_dir(),
        uid(),
    );
    let injection = injection(&shell, &root, var)?;
    // Written once per process: every shell spawned afterwards reuses the folder.
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    let installed = *INSTALLED.get_or_init(|| match install(&root) {
        Ok(()) => true,
        Err(err) => {
            warn!(
                "shell integration disabled, cannot write {}: {err}",
                root.display()
            );
            false
        }
    });
    installed.then_some(injection)
}

/// Whether the integration is on, from the value of [`OPT_OUT_VAR`].
fn enabled(opt_out: Option<&str>) -> bool {
    opt_out != Some("0")
}

/// Folder the scripts are written to: `$XDG_RUNTIME_DIR/graphene/shell-integration`,
/// or a per-user folder of `temp_dir` when the runtime directory is unknown.
fn root_dir(runtime_dir: Option<&str>, temp_dir: &Path, uid: u32) -> PathBuf {
    let base = match runtime_dir {
        Some(dir) if !dir.is_empty() => Path::new(dir).join("graphene"),
        _ => temp_dir.join(format!("graphene-{uid}")),
    };
    base.join("shell-integration")
}

/// Injection for `shell` (a path or a name), reading the current environment through
/// `var`. `None` for a shell without integration.
fn injection(shell: &str, root: &Path, var: impl Fn(&str) -> Option<String>) -> Option<Injection> {
    let name = Path::new(shell).file_name().and_then(OsStr::to_str)?;
    let path = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    let mut args = Vec::new();
    let mut env = Vec::new();
    // The original value travels in a GRAPHENE_* variable, absent when it was unset.
    let mut save = |name: &str, saved: &str| {
        if let Some(value) = var(name) {
            env.push((saved.to_owned(), value));
        }
    };
    match name {
        "zsh" => {
            save("ZDOTDIR", "GRAPHENE_ZDOTDIR");
            env.push(("ZDOTDIR".to_owned(), path(ZSH_DIR)));
        }
        "bash" => {
            save("ENV", "GRAPHENE_BASH_ENV");
            args.push("--posix".to_owned());
            env.push(("ENV".to_owned(), path(BASH_SCRIPT)));
        }
        "fish" => {
            save("XDG_DATA_DIRS", "GRAPHENE_XDG_DATA_DIRS");
            let rest = var("XDG_DATA_DIRS")
                .filter(|dirs| !dirs.is_empty())
                .unwrap_or_else(|| DEFAULT_XDG_DATA_DIRS.to_owned());
            env.push((
                "XDG_DATA_DIRS".to_owned(),
                format!("{}:{rest}", root.to_string_lossy()),
            ));
        }
        _ => return None,
    }
    Some(Injection {
        program: shell.to_owned(),
        args,
        env,
    })
}

/// Writes the scripts under `root`. Its parent is created private to the user, or must
/// already be: the shell sources these files, so nobody else may plant them.
fn install(root: &Path) -> io::Result<()> {
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "no parent folder"))?;
    private_dir(parent)?;
    for (relative, content) in SCRIPTS {
        let path = root.join(relative);
        let dir = path.parent().expect("script paths have a folder");
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        // Write then rename, so a shell started by another Graphene never reads half a
        // script.
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&tmp, content)?;
        fs::rename(&tmp, &path)?;
    }
    Ok(())
}

/// Creates `dir` with mode 700, or checks that the existing one is a real folder of the
/// current user that nobody else can write to.
fn private_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(dir)?;
            if meta.is_dir() && meta.uid() == uid() && meta.mode() & 0o022 == 0 {
                Ok(())
            } else {
                Err(io::Error::new(
                    ErrorKind::PermissionDenied,
                    "not a private folder of the current user",
                ))
            }
        }
        Err(err) => Err(err),
    }
}

fn uid() -> u32 {
    // SAFETY: getuid has no precondition and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    /// Fresh folder under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "graphene-shell-integration-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn env_of(injection: &Injection) -> HashMap<&str, &str> {
        injection
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    const ROOT: &str = "/run/user/1000/graphene/shell-integration";

    #[test]
    fn opt_out_only_on_zero() {
        assert!(enabled(None));
        assert!(enabled(Some("1")));
        assert!(enabled(Some("")));
        assert!(!enabled(Some("0")));
    }

    #[test]
    fn root_in_runtime_dir_or_per_user_temp_dir() {
        let tmp = Path::new("/tmp");
        assert_eq!(root_dir(Some("/run/user/1000"), tmp, 1000), Path::new(ROOT));
        assert_eq!(
            root_dir(None, tmp, 1000),
            Path::new("/tmp/graphene-1000/shell-integration")
        );
        assert_eq!(
            root_dir(Some(""), tmp, 42),
            Path::new("/tmp/graphene-42/shell-integration")
        );
    }

    #[test]
    fn zsh_through_zdotdir() {
        let inj = injection("/usr/bin/zsh", Path::new(ROOT), vars(&[])).unwrap();
        assert_eq!(inj.program, "/usr/bin/zsh");
        assert!(inj.args.is_empty());
        assert_eq!(
            env_of(&inj),
            HashMap::from([("ZDOTDIR", "/run/user/1000/graphene/shell-integration/zsh")])
        );

        let inj = injection(
            "zsh",
            Path::new(ROOT),
            vars(&[("ZDOTDIR", "/home/me/.zsh")]),
        )
        .unwrap();
        assert_eq!(env_of(&inj)["GRAPHENE_ZDOTDIR"], "/home/me/.zsh");
    }

    #[test]
    fn bash_through_posix_env() {
        let inj = injection("/bin/bash", Path::new(ROOT), vars(&[])).unwrap();
        assert_eq!(inj.args, ["--posix"]);
        assert_eq!(
            env_of(&inj),
            HashMap::from([(
                "ENV",
                "/run/user/1000/graphene/shell-integration/bash/graphene.bash"
            )])
        );

        let inj = injection("bash", Path::new(ROOT), vars(&[("ENV", "/home/me/.shrc")])).unwrap();
        assert_eq!(env_of(&inj)["GRAPHENE_BASH_ENV"], "/home/me/.shrc");
    }

    #[test]
    fn fish_through_xdg_data_dirs() {
        let inj = injection("/usr/bin/fish", Path::new(ROOT), vars(&[])).unwrap();
        assert!(inj.args.is_empty());
        assert_eq!(
            env_of(&inj),
            HashMap::from([(
                "XDG_DATA_DIRS",
                "/run/user/1000/graphene/shell-integration:/usr/local/share:/usr/share"
            )])
        );

        let inj = injection(
            "fish",
            Path::new(ROOT),
            vars(&[("XDG_DATA_DIRS", "/opt/share")]),
        )
        .unwrap();
        let env = env_of(&inj);
        assert_eq!(
            env["XDG_DATA_DIRS"],
            "/run/user/1000/graphene/shell-integration:/opt/share"
        );
        assert_eq!(env["GRAPHENE_XDG_DATA_DIRS"], "/opt/share");
    }

    #[test]
    fn other_shells_unchanged() {
        for shell in ["/bin/sh", "/usr/bin/nu", "/usr/bin/zsh-wrapper", "/", ""] {
            assert_eq!(
                injection(shell, Path::new(ROOT), vars(&[])),
                None,
                "{shell}"
            );
        }
    }

    #[test]
    fn install_writes_every_script_in_a_private_folder() {
        let dir = TempDir::new("install");
        let root = dir.0.join("graphene").join("shell-integration");
        install(&root).unwrap();
        // A second start overwrites the files.
        install(&root).unwrap();

        for (relative, content) in SCRIPTS {
            assert_eq!(fs::read_to_string(root.join(relative)).unwrap(), content);
        }
        let mode = fs::metadata(dir.0.join("graphene")).unwrap().mode();
        assert_eq!(mode & 0o777, 0o700);
        let leftovers: Vec<_> = fs::read_dir(root.join("zsh"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, [".zshenv"]);
    }

    #[test]
    fn install_refuses_a_folder_others_can_write() {
        let dir = TempDir::new("shared");
        let parent = dir.0.join("graphene");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();

        let err = install(&parent.join("shell-integration")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn install_refuses_a_symlink() {
        let dir = TempDir::new("symlink");
        let target = dir.0.join("target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&target, dir.0.join("graphene")).unwrap();

        let err = install(&dir.0.join("graphene").join("shell-integration")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }

    /// Runs real shells with the integration: one test per shell, skipped when the shell
    /// is not installed.
    mod shells {
        use super::*;
        use crate::osc133::{Chunk, Mark, Scanner};
        use alacritty_terminal::event::WindowSize;
        use alacritty_terminal::tty::{self, EventedReadWrite};
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};

        const TIMEOUT: Duration = Duration::from_secs(10);

        fn find(shell: &str) -> Option<PathBuf> {
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|dir| dir.join(shell))
                .find(|path| path.is_file())
        }

        /// A shell in a PTY, with the integration installed in `home` and `home` as
        /// `HOME` and XDG folders, so the user's real configuration stays out.
        struct Session {
            pty: tty::Pty,
            output: Vec<u8>,
            marks: Vec<Mark>,
            scanner: Scanner,
            /// Primary device attributes queries answered so far.
            answered: usize,
        }

        /// Primary device attributes queries (`CSI c`, `CSI 0 c`) in `output`.
        fn count_da1(output: &[u8]) -> usize {
            output.windows(3).filter(|w| *w == b"\x1b[c").count()
                + output.windows(4).filter(|w| *w == b"\x1b[0c").count()
        }

        impl Drop for Session {
            fn drop(&mut self) {
                // A shell waiting for a terminal reply may defer the SIGHUP sent when the
                // PTY drops, which then waits for it.
                // SAFETY: kill has no memory precondition; the child is not reaped yet.
                unsafe {
                    libc::kill(self.pty.child().id() as i32, libc::SIGKILL);
                }
            }
        }

        impl Session {
            fn spawn(shell: &Path, home: &Path, extra_args: &[&str]) -> Self {
                let root = home
                    .join("runtime")
                    .join("graphene")
                    .join("shell-integration");
                install(&root).unwrap();
                let mut injection = injection(shell.to_str().unwrap(), &root, vars(&[])).unwrap();
                injection
                    .args
                    .extend(extra_args.iter().map(|arg| (*arg).to_owned()));

                let home_str = home.to_string_lossy().into_owned();
                let mut env: HashMap<String, String> = injection.env.into_iter().collect();
                env.insert("HOME".to_owned(), home_str.clone());
                env.insert("TERM".to_owned(), "xterm-256color".to_owned());
                for (var, sub) in [
                    ("XDG_CONFIG_HOME", ".config"),
                    ("XDG_DATA_HOME", ".local/share"),
                    ("XDG_STATE_HOME", ".local/state"),
                    ("XDG_CACHE_HOME", ".cache"),
                ] {
                    env.insert(var.to_owned(), format!("{home_str}/{sub}"));
                }
                let options = tty::Options {
                    shell: Some(tty::Shell::new(injection.program, injection.args)),
                    working_directory: Some(home.to_path_buf()),
                    env,
                    ..Default::default()
                };
                let size = WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 8,
                    cell_height: 16,
                };
                Self {
                    pty: tty::new(&options, size, 0).unwrap(),
                    output: Vec::new(),
                    marks: Vec::new(),
                    scanner: Scanner::new(),
                    answered: 0,
                }
            }

            /// Reads the output until `done` holds for the marks, answering the primary
            /// device attributes query fish waits for.
            fn read_until(&mut self, what: &str, done: impl Fn(&[Mark]) -> bool) {
                let deadline = Instant::now() + TIMEOUT;
                let mut buf = [0u8; 4096];
                while !done(&self.marks) {
                    assert!(
                        Instant::now() < deadline,
                        "timeout waiting for {what}: marks {:?}, output {:?}",
                        self.marks,
                        String::from_utf8_lossy(&self.output)
                    );
                    match self.pty.reader().read(&mut buf) {
                        Ok(0) => panic!("shell exited waiting for {what}"),
                        Ok(n) => {
                            let read = &buf[..n];
                            for chunk in self.scanner.split(read) {
                                if let Chunk::Mark(mark) = chunk {
                                    self.marks.push(mark);
                                }
                            }
                            self.output.extend_from_slice(read);
                            // Queries may be split across reads: count them in the
                            // whole output.
                            let queries = count_da1(&self.output);
                            while self.answered < queries {
                                self.write("\x1b[?62;22c");
                                self.answered += 1;
                            }
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("read error waiting for {what}: {err}"),
                    }
                }
            }

            fn write(&mut self, text: &str) {
                let writer = self.pty.writer();
                let mut bytes = text.as_bytes();
                while !bytes.is_empty() {
                    match writer.write(bytes) {
                        Ok(n) => bytes = &bytes[n..],
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("write error: {err}"),
                    }
                }
            }

            /// Writes `command` and waits for its command end and the next prompt.
            fn run(&mut self, command: &str) {
                let ends = self.count(|mark| matches!(mark, Mark::CommandEnd(_)));
                let prompts = self.count(|mark| mark == Mark::PromptStart);
                self.write(&format!("{command}\r"));
                self.read_until(command, |marks| {
                    let count = |f: fn(Mark) -> bool| marks.iter().filter(|m| f(**m)).count();
                    count(|mark| matches!(mark, Mark::CommandEnd(_))) > ends
                        && count(|mark| mark == Mark::PromptStart) > prompts
                });
            }

            fn count(&self, f: impl Fn(Mark) -> bool) -> usize {
                self.marks.iter().filter(|m| f(**m)).count()
            }
        }

        /// Marks of the left prompt and the commands: hyperlinks dropped, and each right
        /// prompt dropped with the prompt end that closes it.
        fn command_marks(marks: &[Mark]) -> Vec<Mark> {
            let mut kept = Vec::new();
            let mut in_right_prompt = false;
            for &mark in marks {
                match mark {
                    Mark::Hyperlink => {}
                    Mark::RightPrompt => in_right_prompt = true,
                    Mark::InputStart if in_right_prompt => in_right_prompt = false,
                    mark => kept.push(mark),
                }
            }
            kept
        }

        /// Runs `true`, `false` then `probe`, checks the marks and that `probe` printed
        /// `expected` (the user's rc files were sourced, the injection variables restored).
        fn check(
            shell: &str,
            home: &Path,
            extra_args: &[&str],
            probe: &str,
            expected: &str,
        ) -> Session {
            let path = find(shell).expect("shell present");
            let mut session = Session::spawn(&path, home, extra_args);
            session.read_until("the first prompt", |marks| {
                marks.contains(&Mark::PromptStart)
            });
            session.run("true");
            session.run("false");
            session.run(probe);

            use Mark::*;
            let marks = command_marks(&session.marks);
            assert_eq!(
                marks[..9],
                [
                    PromptStart,
                    InputStart,
                    OutputStart,
                    CommandEnd(Some(0)),
                    PromptStart,
                    InputStart,
                    OutputStart,
                    CommandEnd(Some(1)),
                    PromptStart,
                ],
                "{shell}: {:?}",
                session.marks
            );
            let output = String::from_utf8_lossy(&session.output);
            assert!(output.contains(expected), "{shell}: {output:?}");
            session
        }

        const ZSH_PROBE: &str = "echo rc:${GRAPHENE_TEST_RC}:${ZDOTDIR}:${GRAPHENE_ZDOTDIR}:end";
        const BASH_PROBE: &str = "echo rc:${GRAPHENE_TEST_RC}:${ENV}:${GRAPHENE_BASH_ENV}:$(set -o | grep -c 'posix *on'):end";
        // Quoted: in fish, an unset variable empties the whole unquoted word. System
        // vendor files (flatpak) may set XDG_DATA_DIRS again: only check that the
        // integration folder left it.
        const FISH_PROBE: &str = "echo \"rc:$GRAPHENE_TEST_RC:\"(count (string match '*/runtime/graphene/*' -- $XDG_DATA_DIRS))\":$GRAPHENE_XDG_DATA_DIRS:end\"";

        #[test]
        fn zsh_marks_and_user_files() {
            if find("zsh").is_none() {
                eprintln!("zsh not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh");
            fs::write(home.0.join(".zshenv"), "export GRAPHENE_TEST_ENV=yes\n").unwrap();
            fs::write(
                home.0.join(".zshrc"),
                "export GRAPHENE_TEST_RC=$GRAPHENE_TEST_ENV\nPS1='%# '\nRPS1='[r]'\n",
            )
            .unwrap();
            let session = check("zsh", &home.0, &[], ZSH_PROBE, "rc:yes:::end");
            assert!(session.marks.contains(&Mark::RightPrompt));
        }

        #[test]
        fn zsh_wraps_a_prompt_rebuilt_by_a_later_hook() {
            if find("zsh").is_none() {
                eprintln!("zsh not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh-theme");
            fs::write(
                home.0.join(".zshrc"),
                "export GRAPHENE_TEST_RC=yes\n\
                 theme() { PS1='theme%# ' }\n\
                 precmd_functions+=(theme)\n",
            )
            .unwrap();
            check("zsh", &home.0, &[], ZSH_PROBE, "rc:yes:::end");
        }

        #[test]
        fn bash_marks_and_user_files() {
            if find("bash").is_none() {
                eprintln!("bash not installed, skipped");
                return;
            }
            let home = TempDir::new("bash");
            fs::write(
                home.0.join(".bashrc"),
                "export GRAPHENE_TEST_RC=yes\nPS1='$ '\nPROMPT_COMMAND='PS1=\"pc\\$ \"'\n",
            )
            .unwrap();
            // POSIX mode is off once the integration ran.
            check("bash", &home.0, &[], BASH_PROBE, "rc:yes:::0:end");
        }

        #[test]
        fn bash_no_command_end_for_an_empty_line() {
            if find("bash").is_none() {
                eprintln!("bash not installed, skipped");
                return;
            }
            let home = TempDir::new("bash-empty");
            fs::write(home.0.join(".bashrc"), "PS1='$ '\n").unwrap();
            let path = find("bash").unwrap();
            let mut session = Session::spawn(&path, &home.0, &[]);
            session.read_until("the first prompt", |marks| {
                marks.contains(&Mark::PromptStart)
            });
            session.write("\r");
            session.read_until("the second prompt", |marks| {
                marks.iter().filter(|m| **m == Mark::PromptStart).count() == 2
            });
            session.run("true");
            use Mark::*;
            assert_eq!(
                command_marks(&session.marks)[..6],
                [
                    PromptStart,
                    InputStart,
                    PromptStart,
                    InputStart,
                    OutputStart,
                    CommandEnd(Some(0))
                ]
            );
        }

        fn write_fish_config(home: &Path) {
            let dir = home.join(".config").join("fish");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("config.fish"),
                "set -gx GRAPHENE_TEST_RC yes\nfunction fish_prompt; echo -n '> '; end\n",
            )
            .unwrap();
        }

        #[test]
        fn fish_marks_and_user_files() {
            if find("fish").is_none() {
                eprintln!("fish not installed, skipped");
                return;
            }
            let home = TempDir::new("fish");
            write_fish_config(&home.0);
            check("fish", &home.0, &[], FISH_PROBE, "rc:yes:0::end");
        }

        /// With the native marks switched off, the script emits all of them: the
        /// fallback of a fish older than 4.0.
        #[test]
        fn fish_without_native_marks() {
            let Some(fish) = find("fish") else {
                eprintln!("fish not installed, skipped");
                return;
            };
            // `mark-prompt` exists since fish 4.0.6.
            let switchable = std::process::Command::new(fish)
                .args([
                    "--features",
                    "no-mark-prompt",
                    "-c",
                    "status test-feature mark-prompt",
                ])
                .status()
                .is_ok_and(|status| status.code() == Some(1));
            if !switchable {
                eprintln!("fish without the mark-prompt feature, skipped");
                return;
            }
            let home = TempDir::new("fish-fallback");
            write_fish_config(&home.0);
            check(
                "fish",
                &home.0,
                &["--features", "no-mark-prompt"],
                FISH_PROBE,
                "rc:yes:0::end",
            );
        }
    }
}
