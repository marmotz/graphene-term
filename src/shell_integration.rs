//! Shell integration: makes zsh, bash and fish tell Graphene when a command starts and
//! ends (the block signals of [`crate::signals`]) without touching the user's dotfiles.
//!
//! The scripts of `shell-integration/` are embedded in the binary, written at startup
//! to a private runtime folder, and injected into the spawned shell through its startup
//! mechanism: `ZDOTDIR` for zsh, `--posix` + `ENV` for bash, `XDG_DATA_DIRS` for fish.
//! Each script restores the variable it came through before loading the user's files,
//! and moves the session token from [`TOKEN_VAR`] into a shell variable.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::warn;

/// Opt-out variable: `0` starts the shell without integration.
pub const OPT_OUT_VAR: &str = "GRAPHENE_SHELL_INTEGRATION";

/// Variable carrying the session token to the hooks, which unset it.
pub const TOKEN_VAR: &str = "GRAPHENE_BLOCK_TOKEN";

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
/// `token` is the session token the hooks put in their signals.
pub fn prepare(token: &str) -> Option<Injection> {
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
    let injection = injection(&shell, &root, token, var)?;
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

/// Injection for `shell` (a path or a name) with the session `token`, reading the
/// current environment through `var`. `None` for a shell without integration.
fn injection(
    shell: &str,
    root: &Path,
    token: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Option<Injection> {
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
    env.push((TOKEN_VAR.to_owned(), token.to_owned()));
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
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

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
        let inj = injection("/usr/bin/zsh", Path::new(ROOT), TOKEN, vars(&[])).unwrap();
        assert_eq!(inj.program, "/usr/bin/zsh");
        assert!(inj.args.is_empty());
        assert_eq!(
            env_of(&inj),
            HashMap::from([
                ("ZDOTDIR", "/run/user/1000/graphene/shell-integration/zsh"),
                (TOKEN_VAR, TOKEN),
            ])
        );

        let inj = injection(
            "zsh",
            Path::new(ROOT),
            TOKEN,
            vars(&[("ZDOTDIR", "/home/me/.zsh")]),
        )
        .unwrap();
        assert_eq!(env_of(&inj)["GRAPHENE_ZDOTDIR"], "/home/me/.zsh");
    }

    #[test]
    fn bash_through_posix_env() {
        let inj = injection("/bin/bash", Path::new(ROOT), TOKEN, vars(&[])).unwrap();
        assert_eq!(inj.args, ["--posix"]);
        assert_eq!(
            env_of(&inj),
            HashMap::from([
                (
                    "ENV",
                    "/run/user/1000/graphene/shell-integration/bash/graphene.bash"
                ),
                (TOKEN_VAR, TOKEN),
            ])
        );

        let inj = injection(
            "bash",
            Path::new(ROOT),
            TOKEN,
            vars(&[("ENV", "/home/me/.shrc")]),
        )
        .unwrap();
        assert_eq!(env_of(&inj)["GRAPHENE_BASH_ENV"], "/home/me/.shrc");
    }

    #[test]
    fn fish_through_xdg_data_dirs() {
        let inj = injection("/usr/bin/fish", Path::new(ROOT), TOKEN, vars(&[])).unwrap();
        assert!(inj.args.is_empty());
        assert_eq!(
            env_of(&inj),
            HashMap::from([
                (
                    "XDG_DATA_DIRS",
                    "/run/user/1000/graphene/shell-integration:/usr/local/share:/usr/share"
                ),
                (TOKEN_VAR, TOKEN),
            ])
        );

        let inj = injection(
            "fish",
            Path::new(ROOT),
            TOKEN,
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
                injection(shell, Path::new(ROOT), TOKEN, vars(&[])),
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

    /// Every script binds every private sequence of the input editor table.
    #[test]
    fn scripts_bind_every_editor_sequence() {
        for (relative, content) in SCRIPTS {
            for (sequence, bytes) in crate::input_editor::SEQUENCES {
                let code = std::str::from_utf8(&bytes[2..]).unwrap();
                assert!(content.contains(code), "{relative}: {sequence:?} ({code})");
            }
        }
    }

    /// Runs real shells with the integration: one test per case, skipped when the shell
    /// is not installed.
    mod shells {
        use super::*;
        use crate::signals::{Chunk, Scanner, Signal};
        use alacritty_terminal::event::WindowSize;
        use alacritty_terminal::tty::{self, EventedReadWrite};
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};

        const TIMEOUT: Duration = Duration::from_secs(10);

        /// Invisible OSC the test configurations print before each prompt, so the
        /// tests know when the shell waits for a command line.
        const READY: &[u8] = b"\x1b]7777;ready\x1b\\";
        const ZSH_READY: &str = "graphene_test_ready() { print -rn -- $'\\e]7777;ready\\e\\\\' }\n\
                                 precmd_functions+=(graphene_test_ready)\n";
        const BASH_READY: &str =
            "PROMPT_COMMAND+=$'\\n'\"printf '\\\\e]7777;ready\\\\e\\\\\\\\'\"\n";
        const FISH_READY: &str = "function graphene_test_ready --on-event fish_prompt; printf '\\e]7777;ready\\e\\\\'; end\n";

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
            signals: Vec<Signal>,
            scanner: Scanner,
            /// Primary device attributes queries answered so far.
            answered: usize,
        }

        /// Occurrences of `needle` in `haystack`.
        fn count(haystack: &[u8], needle: &[u8]) -> usize {
            haystack
                .windows(needle.len())
                .filter(|w| *w == needle)
                .count()
        }

        /// Primary device attributes queries (`CSI c`, `CSI 0 c`) in `output`.
        fn count_da1(output: &[u8]) -> usize {
            count(output, b"\x1b[c") + count(output, b"\x1b[0c")
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
            fn spawn(shell: &str, home: &Path) -> Self {
                let path = find(shell).expect("shell present");
                let root = home
                    .join("runtime")
                    .join("graphene")
                    .join("shell-integration");
                install(&root).unwrap();
                let injection = injection(path.to_str().unwrap(), &root, TOKEN, vars(&[])).unwrap();

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
                let mut session = Self {
                    pty: tty::new(&options, size, 0).unwrap(),
                    output: Vec::new(),
                    signals: Vec::new(),
                    scanner: Scanner::new(TOKEN),
                    answered: 0,
                };
                session.read_until("the first prompt", |s| s.prompts() >= 1);
                session
            }

            fn prompts(&self) -> usize {
                count(&self.output, READY)
            }

            fn ends(&self) -> usize {
                self.signals
                    .iter()
                    .filter(|signal| matches!(signal, Signal::End { .. }))
                    .count()
            }

            /// Reads the output until `done` holds, answering the primary device
            /// attributes query fish waits for.
            fn read_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
                let deadline = Instant::now() + TIMEOUT;
                let mut buf = [0u8; 4096];
                while !done(self) {
                    assert!(
                        Instant::now() < deadline,
                        "timeout waiting for {what}: signals {:?}, output {:?}",
                        self.signals,
                        String::from_utf8_lossy(&self.output)
                    );
                    match self.pty.reader().read(&mut buf) {
                        Ok(0) => panic!("shell exited waiting for {what}"),
                        Ok(n) => {
                            let read = &buf[..n];
                            for chunk in self.scanner.split(read) {
                                if let Chunk::Signal(signal) = chunk {
                                    self.signals.push(signal);
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
                let (ends, prompts) = (self.ends(), self.prompts());
                self.write(&format!("{command}\r"));
                self.read_until(command, |s| s.ends() > ends && s.prompts() > prompts);
            }

            /// Writes an empty command line and waits for the next prompt.
            fn empty_line(&mut self) {
                let prompts = self.prompts();
                self.write("\r");
                self.read_until("the prompt after an empty line", |s| s.prompts() > prompts);
            }
        }

        fn start(command: &str) -> Signal {
            Signal::Start {
                command: command.to_owned(),
            }
        }

        fn end(status: i32) -> Signal {
            Signal::End {
                status: Some(status),
            }
        }

        /// Runs `true`, an empty line, `false`, a command line holding `;` and quotes,
        /// then `probe`; checks one start (with the command line) and one end (with the
        /// status) per command, and that `probe` printed `expected`: the user's files
        /// were sourced, the injection variables restored and the token kept out of
        /// the environment.
        fn check(shell: &str, home: &Path, probe: &str, expected: &str) -> Session {
            let mut session = Session::spawn(shell, home);
            session.run("true");
            session.empty_line();
            session.run("false");
            session.run("echo 'a;b' \"c\"");
            session.run(probe);

            assert_eq!(
                session.signals,
                [
                    start("true"),
                    end(0),
                    start("false"),
                    end(1),
                    start("echo 'a;b' \"c\""),
                    end(0),
                    start(probe),
                    end(0),
                ],
                "{shell}: {:?}",
                String::from_utf8_lossy(&session.output)
            );
            let output = String::from_utf8_lossy(&session.output);
            assert!(output.contains(expected), "{shell}: {output:?}");
            session
        }

        // `env | grep -c` counts the token variable in a command's environment.
        const ZSH_PROBE: &str = "echo rc:${GRAPHENE_TEST_RC}:${ZDOTDIR}:${GRAPHENE_ZDOTDIR}:$(env | grep -c GRAPHENE_BLOCK_TOKEN):end";
        const BASH_PROBE: &str = "echo rc:${GRAPHENE_TEST_RC}:${ENV}:${GRAPHENE_BASH_ENV}:$(set -o | grep -c 'posix *on'):$(env | grep -c GRAPHENE_BLOCK_TOKEN):end";
        // Quoted: in fish, an unset variable empties the whole unquoted word. System
        // vendor files (flatpak) may set XDG_DATA_DIRS again: only check that the
        // integration folder left it.
        const FISH_PROBE: &str = "echo \"rc:$GRAPHENE_TEST_RC:\"(count (string match '*/runtime/graphene/*' -- $XDG_DATA_DIRS))\":$GRAPHENE_XDG_DATA_DIRS:\"(env | grep -c GRAPHENE_BLOCK_TOKEN)\":end\"";

        #[test]
        fn zsh_signals_and_user_files() {
            if find("zsh").is_none() {
                eprintln!("zsh not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh");
            fs::write(home.0.join(".zshenv"), "export GRAPHENE_TEST_ENV=yes\n").unwrap();
            fs::write(
                home.0.join(".zshrc"),
                format!(
                    "export GRAPHENE_TEST_RC=$GRAPHENE_TEST_ENV\nPS1='%# '\nRPS1='[r]'\n{ZSH_READY}"
                ),
            )
            .unwrap();
            check("zsh", &home.0, ZSH_PROBE, "rc:yes:::0:end");
        }

        /// zsh prints its `PROMPT_SP` mark after an output without a final newline
        /// before running the precmd hooks: the mark stays in the command's output,
        /// before the end signal.
        #[test]
        fn zsh_prompt_sp_mark_before_the_end() {
            if find("zsh").is_none() {
                eprintln!("zsh not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh-prompt-sp");
            fs::write(home.0.join(".zshrc"), format!("PS1='%# '\n{ZSH_READY}")).unwrap();
            let mut session = Session::spawn("zsh", &home.0);
            let from = session.output.len();
            session.run("printf nonl");
            let output = &session.output[from..];
            let end = format!("\x1b]6973;{TOKEN};end;0");
            let end_at = output
                .windows(end.len())
                .position(|w| w == end.as_bytes())
                .expect("end signal");
            let mark_at = output
                .windows(b"nonl\x1b[1m\x1b[7m%".len())
                .position(|w| w == b"nonl\x1b[1m\x1b[7m%")
                .expect("PROMPT_SP mark");
            assert!(mark_at < end_at, "{:?}", String::from_utf8_lossy(output));
        }

        const P10K_THEME: &str = "/usr/share/zsh-theme-powerlevel10k/powerlevel10k.zsh-theme";

        /// powerlevel10k rebuilds the prompt and emits its own OSC 133 marks: the hooks
        /// do not depend on the prompt, so the signals are unchanged.
        #[test]
        fn zsh_powerlevel10k_signals() {
            if find("zsh").is_none() || !Path::new(P10K_THEME).is_file() {
                eprintln!("zsh or powerlevel10k not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh-p10k");
            fs::write(
                home.0.join(".zshrc"),
                format!(
                    "export GRAPHENE_TEST_RC=yes\n\
                     POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true\n\
                     POWERLEVEL9K_INSTANT_PROMPT=off\n\
                     POWERLEVEL9K_DISABLE_GITSTATUS=true\n\
                     POWERLEVEL9K_LEFT_PROMPT_ELEMENTS=(dir)\n\
                     POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(status)\n\
                     source {P10K_THEME}\n\
                     {ZSH_READY}"
                ),
            )
            .unwrap();
            check("zsh", &home.0, ZSH_PROBE, "rc:yes:::0:end");
        }

        #[test]
        fn bash_signals_and_user_files() {
            if find("bash").is_none() {
                eprintln!("bash not installed, skipped");
                return;
            }
            let home = TempDir::new("bash");
            fs::write(
                home.0.join(".bashrc"),
                format!(
                    "export GRAPHENE_TEST_RC=yes\nPS1='$ '\nPROMPT_COMMAND='PS1=\"pc\\$ \"'\n{BASH_READY}"
                ),
            )
            .unwrap();
            // POSIX mode is off once the integration ran.
            check("bash", &home.0, BASH_PROBE, "rc:yes:::0:0:end");
        }

        /// A command line left out of the history (leading space with `ignorespace`,
        /// history off) starts with an empty command line, not an older entry.
        #[test]
        fn bash_command_line_outside_the_history() {
            if find("bash").is_none() {
                eprintln!("bash not installed, skipped");
                return;
            }
            let home = TempDir::new("bash-history");
            fs::write(
                home.0.join(".bashrc"),
                format!("HISTCONTROL=ignorespace\nPS1='$ '\n{BASH_READY}"),
            )
            .unwrap();
            let mut session = Session::spawn("bash", &home.0);
            session.run("true");
            session.run(" false");
            session.run("set +o history");
            session.run("true");
            assert_eq!(
                session.signals,
                [
                    start("true"),
                    end(0),
                    start(""),
                    end(1),
                    start("set +o history"),
                    end(0),
                    start(""),
                    end(0),
                ]
            );
        }

        #[test]
        fn fish_signals_and_user_files() {
            if find("fish").is_none() {
                eprintln!("fish not installed, skipped");
                return;
            }
            let home = TempDir::new("fish");
            let dir = home.0.join(".config").join("fish");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("config.fish"),
                format!(
                    "set -gx GRAPHENE_TEST_RC yes\nfunction fish_prompt; echo -n '> '; end\n{FISH_READY}"
                ),
            )
            .unwrap();
            check("fish", &home.0, FISH_PROBE, "rc:yes:0::0:end");
        }

        /// One editor input: typed text or a private sequence.
        enum Input {
            Text(&'static str),
            Seq(Sequence),
            Repeat(Sequence, usize),
            /// In a shell with autosuggestions, waits until the suggestion ("zz") shows:
            /// fish computes it in the background.
            Suggestion,
        }

        use crate::input_editor::Sequence;
        use Input::{Repeat, Seq, Suggestion, Text};
        use Sequence::*;

        const PRINT: &str = "printf '<%s>\\n' ";

        /// Command lines run first: the history entries the autosuggestions offer for
        /// the editor cases.
        const HISTORY: [&str; 1] = ["printf '<%s>\\n' abcdzz"];

        /// Editor cases: the inputs of one command line and what it prints. Each one
        /// only prints `expected` when every move landed where it should.
        fn editor_cases() -> Vec<(Vec<Input>, &'static str)> {
            vec![
                // Char moves clamped at both ends. At the end of the buffer the
                // autosuggestion ("zz") shows: no forward move accepts it.
                (
                    vec![
                        Text(PRINT),
                        Text("ac"),
                        Seq(CharLeft),
                        Text("b"),
                        Repeat(CharRight, 3),
                        Text("d"),
                        Suggestion,
                        Seq(CharRight),
                        Seq(WordRight),
                        Seq(DeleteWordRight),
                        Seq(LineEnd),
                        Seq(BufferEnd),
                        Repeat(CharLeft, 40),
                        Text("true;"),
                    ],
                    "<abcd>",
                ),
                // Word moves and deletions, buffer start, line end on a single line.
                (
                    vec![
                        Text(PRINT),
                        Text("one two"),
                        Seq(WordLeft),
                        Text("X"),
                        Seq(DeleteWordRight),
                        Seq(DeleteWordLeft),
                        Text("Y"),
                        Seq(BufferStart),
                        Text("true;"),
                        Seq(LineEnd),
                        Text("Q"),
                    ],
                    "<one>\r\n<YQ>",
                ),
                // Newline, vertical moves (clamped column, nothing past the first and
                // last lines, no history), line start, buffer end.
                (
                    vec![
                        Text(PRINT),
                        Text("'abcde'"),
                        Repeat(CharLeft, 3),
                        Seq(InsertNewline),
                        Repeat(LineUp, 2),
                        Text("true;"),
                        Seq(LineDown),
                        Text("X"),
                        Seq(LineDown),
                        Seq(LineStart),
                        Text("Y"),
                        Seq(BufferEnd),
                        Text("Z"),
                    ],
                    "<abc\r\nYdeXZ>",
                ),
            ]
        }

        /// Runs the history entries then every editor case in `shell`, with the
        /// configuration already written in `home`. `suggests`: the shell shows
        /// autosuggestions.
        fn check_editor(shell: &str, home: &Path, suggests: bool) {
            let mut session = Session::spawn(shell, home);
            for command in HISTORY {
                session.run(command);
            }
            for (inputs, expected) in editor_cases() {
                let from = session.output.len();
                let mut line = String::new();
                for input in &inputs {
                    match input {
                        Text(text) => line.push_str(text),
                        Seq(sequence) => {
                            line.push_str(std::str::from_utf8(sequence.bytes()).unwrap())
                        }
                        Repeat(sequence, n) => {
                            for _ in 0..*n {
                                line.push_str(std::str::from_utf8(sequence.bytes()).unwrap());
                            }
                        }
                        Suggestion if suggests => {
                            let mark = session.output.len();
                            session.write(&std::mem::take(&mut line));
                            session.read_until("the autosuggestion", |s| {
                                count(&s.output[mark..], b"zz") > 0
                            });
                        }
                        Suggestion => {}
                    }
                }
                session.run(&line);
                let output = String::from_utf8_lossy(&session.output[from..]);
                assert!(
                    output.contains(expected),
                    "{shell}: expected {expected:?} in {output:?}"
                );
            }
        }

        const ZSH_AUTOSUGGESTIONS: &str =
            "/usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh";

        #[test]
        fn zsh_editor_sequences() {
            if find("zsh").is_none() {
                eprintln!("zsh not installed, skipped");
                return;
            }
            for (name, keymap) in [("emacs", "bindkey -e"), ("vi", "bindkey -v")] {
                let home = TempDir::new(&format!("zsh-editor-{name}"));
                fs::write(
                    home.0.join(".zshrc"),
                    format!("{keymap}\nPS1='%# '\n{ZSH_READY}"),
                )
                .unwrap();
                check_editor("zsh", &home.0, false);
            }
        }

        /// zsh-autosuggestions wraps the widgets: the moves at the end of the buffer
        /// still do not accept its suggestion.
        #[test]
        fn zsh_editor_sequences_with_autosuggestions() {
            if find("zsh").is_none() || !Path::new(ZSH_AUTOSUGGESTIONS).is_file() {
                eprintln!("zsh or zsh-autosuggestions not installed, skipped");
                return;
            }
            let home = TempDir::new("zsh-editor-autosuggestions");
            fs::write(
                home.0.join(".zshrc"),
                format!("PS1='%# '\nsource {ZSH_AUTOSUGGESTIONS}\n{ZSH_READY}"),
            )
            .unwrap();
            check_editor("zsh", &home.0, true);
        }

        #[test]
        fn bash_editor_sequences() {
            if find("bash").is_none() {
                eprintln!("bash not installed, skipped");
                return;
            }
            for (name, mode) in [("emacs", "set -o emacs"), ("vi", "set -o vi")] {
                let home = TempDir::new(&format!("bash-editor-{name}"));
                fs::write(
                    home.0.join(".bashrc"),
                    format!("{mode}\nPS1='$ '\n{BASH_READY}"),
                )
                .unwrap();
                check_editor("bash", &home.0, false);
            }
        }

        /// fish shows its own autosuggestions; `fish_vi_key_bindings` erases the
        /// bindings, which come back before the prompt.
        #[test]
        fn fish_editor_sequences() {
            if find("fish").is_none() {
                eprintln!("fish not installed, skipped");
                return;
            }
            for (name, bindings) in [("emacs", ""), ("vi", "fish_vi_key_bindings\n")] {
                let home = TempDir::new(&format!("fish-editor-{name}"));
                let dir = home.0.join(".config").join("fish");
                fs::create_dir_all(&dir).unwrap();
                fs::write(
                    dir.join("config.fish"),
                    format!("{bindings}function fish_prompt; echo -n '> '; end\n{FISH_READY}"),
                )
                .unwrap();
                check_editor("fish", &home.0, true);
            }
        }
    }
}
