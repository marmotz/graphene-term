//! The session autostart entry of the quick terminal: a file managed by Graphene,
//! created or removed to follow `quick_terminal.autostart`, which starts
//! `graphene --quick --hidden` when the user's session opens.
//!
//! - Linux: an XDG autostart `.desktop` entry.
//! - macOS: a `launchd` LaunchAgent property list.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::atomic_file::write_atomic;

/// Arguments of the autostarted process, exactly what a hidden `spawn_quick` launches.
const ARGUMENTS: [&str; 2] = ["--quick", "--hidden"];

/// File name of the XDG autostart entry.
const DESKTOP_FILE: &str = "graphene-quick-terminal.desktop";

/// `Label` of the LaunchAgent, also its file name without extension.
const LAUNCH_AGENT_LABEL: &str = "dev.marmotz.graphene.quick-terminal";

/// Makes the session autostart entry follow `enabled`.
pub fn sync(enabled: bool) -> io::Result<()> {
    let path = entry_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no home directory to hold the autostart entry",
        )
    })?;
    let contents = if enabled {
        let exe = executable_from(std::env::var_os("APPIMAGE"), std::env::current_exe())?;
        Some(entry_contents(&exe))
    } else {
        None
    };
    sync_at(&path, contents.as_deref())
}

/// Writes `contents` at `path` when it differs from the file, or removes the file when
/// `contents` is `None`. A missing file to remove is not an error.
fn sync_at(path: &Path, contents: Option<&str>) -> io::Result<()> {
    match contents {
        Some(contents) => {
            if fs::read_to_string(path).is_ok_and(|current| current == contents) {
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomic(path, contents)
        }
        None => match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        },
    }
}

/// The stable path of the running executable: the `.AppImage` file when running from
/// one (`current_exe()` is then a temporary mount point), else `current_exe()`.
fn executable_from(
    appimage: Option<OsString>,
    current: io::Result<PathBuf>,
) -> io::Result<PathBuf> {
    match appimage.map(PathBuf::from) {
        Some(path) if path.is_file() => Ok(path),
        _ => current,
    }
}

#[cfg(not(target_os = "macos"))]
fn entry_path() -> Option<PathBuf> {
    autostart_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
    .map(|dir| dir.join(DESKTOP_FILE))
}

#[cfg(target_os = "macos")]
fn entry_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| launch_agent_path(Path::new(&home)))
}

#[cfg(not(target_os = "macos"))]
fn entry_contents(exe: &Path) -> String {
    desktop_entry(exe)
}

#[cfg(target_os = "macos")]
fn entry_contents(exe: &Path) -> String {
    launch_agent_plist(exe)
}

/// `$XDG_CONFIG_HOME/autostart`, else `~/.config/autostart`. An empty or relative
/// `XDG_CONFIG_HOME` is ignored, as the XDG spec requires.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn autostart_dir_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let xdg = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| {
        home.filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".config"))
    })?;
    Some(base.join("autostart"))
}

/// `~/Library/LaunchAgents/<label>.plist`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// The XDG autostart entry launching `exe` with [`ARGUMENTS`].
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Graphene Quick Terminal\n\
         Exec=\"{}\" {}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        escape_exec_argument(&exe.to_string_lossy()),
        ARGUMENTS.join(" "),
    )
}

/// Escapes a quoted `Exec` argument: `\`, `"`, `` ` `` and `$` take a backslash, `%`
/// is doubled so it is not read as a field code.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn escape_exec_argument(argument: &str) -> String {
    let mut escaped = String::with_capacity(argument.len());
    for character in argument.chars() {
        match character {
            '\\' | '"' | '`' | '$' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// The LaunchAgent starting `exe` with [`ARGUMENTS`] at login. No `KeepAlive`: the
/// resident process ends with the session and must not be relaunched when quit.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent_plist(exe: &Path) -> String {
    let mut program_arguments = format!(
        "        <string>{}</string>\n",
        escape_xml(&exe.to_string_lossy())
    );
    for argument in ARGUMENTS {
        program_arguments.push_str(&format!("        <string>{argument}</string>\n"));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20   <key>Label</key>\n\
         \x20   <string>{LAUNCH_AGENT_LABEL}</string>\n\
         \x20   <key>ProgramArguments</key>\n\
         \x20   <array>\n\
         {program_arguments}\
         \x20   </array>\n\
         \x20   <key>RunAtLoad</key>\n\
         \x20   <true/>\n\
         \x20   <key>ProcessType</key>\n\
         \x20   <string>Interactive</string>\n\
         </dict>\n\
         </plist>\n"
    )
}

/// Escapes the XML special characters of a text node.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphene-autostart-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn desktop_entry_launches_the_hidden_quick_terminal() {
        let entry = desktop_entry(Path::new("/usr/bin/graphene"));
        assert_eq!(
            entry,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Graphene Quick Terminal\n\
             Exec=\"/usr/bin/graphene\" --quick --hidden\n\
             Terminal=false\n\
             NoDisplay=true\n\
             X-GNOME-Autostart-enabled=true\n"
        );
    }

    #[test]
    fn desktop_entry_escapes_the_executable_path() {
        let entry = desktop_entry(Path::new("/opt/a \"b\" `c` $d \\e 100%/graphene"));
        assert!(entry.contains(
            "Exec=\"/opt/a \\\"b\\\" \\`c\\` \\$d \\\\e 100%%/graphene\" --quick --hidden\n"
        ));
    }

    #[test]
    fn launch_agent_runs_at_load_without_keep_alive() {
        let plist = launch_agent_plist(Path::new("/Applications/Graphene.app/graphene"));
        assert!(plist.contains("<string>dev.marmotz.graphene.quick-terminal</string>"));
        assert!(plist.contains(
            "<array>\n        <string>/Applications/Graphene.app/graphene</string>\n        \
             <string>--quick</string>\n        <string>--hidden</string>\n    </array>"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains("<key>ProcessType</key>\n    <string>Interactive</string>"));
        assert!(!plist.contains("KeepAlive"));
    }

    #[test]
    fn launch_agent_escapes_xml_in_the_executable_path() {
        let plist = launch_agent_plist(Path::new("/opt/a&b<c>/graphene"));
        assert!(plist.contains("<string>/opt/a&amp;b&lt;c&gt;/graphene</string>"));
    }

    #[test]
    fn launch_agent_path_is_in_the_user_launch_agents() {
        assert_eq!(
            launch_agent_path(Path::new("/Users/me")),
            PathBuf::from(
                "/Users/me/Library/LaunchAgents/dev.marmotz.graphene.quick-terminal.plist"
            )
        );
    }

    #[test]
    fn autostart_dir_prefers_an_absolute_xdg_config_home() {
        assert_eq!(
            autostart_dir_from(Some("/xdg".into()), Some("/home/me".into())),
            Some(PathBuf::from("/xdg/autostart"))
        );
    }

    #[test]
    fn autostart_dir_ignores_an_empty_or_relative_xdg_config_home() {
        for xdg in ["", "relative/config"] {
            assert_eq!(
                autostart_dir_from(Some(xdg.into()), Some("/home/me".into())),
                Some(PathBuf::from("/home/me/.config/autostart"))
            );
        }
        assert_eq!(
            autostart_dir_from(None, Some("/home/me".into())),
            Some(PathBuf::from("/home/me/.config/autostart"))
        );
    }

    #[test]
    fn autostart_dir_needs_a_home_without_xdg_config_home() {
        assert_eq!(autostart_dir_from(None, None), None);
        assert_eq!(autostart_dir_from(None, Some("".into())), None);
    }

    #[test]
    fn executable_prefers_an_existing_appimage() {
        let dir = temp_dir("appimage");
        fs::create_dir_all(&dir).unwrap();
        let appimage = dir.join("Graphene.AppImage");
        fs::write(&appimage, "").unwrap();
        let exe = executable_from(
            Some(appimage.clone().into_os_string()),
            Ok(PathBuf::from("/tmp/.mount_x/graphene")),
        )
        .unwrap();
        assert_eq!(exe, appimage);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn executable_falls_back_to_the_current_exe() {
        let current = PathBuf::from("/usr/bin/graphene");
        assert_eq!(executable_from(None, Ok(current.clone())).unwrap(), current);
        assert_eq!(
            executable_from(
                Some("/missing/Graphene.AppImage".into()),
                Ok(current.clone())
            )
            .unwrap(),
            current
        );
    }

    #[test]
    fn sync_creates_the_entry_and_its_folder() {
        let dir = temp_dir("create");
        let path = dir.join("autostart").join(DESKTOP_FILE);
        sync_at(&path, Some("entry\n")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "entry\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_does_not_rewrite_an_identical_entry() {
        let dir = temp_dir("identical");
        let path = dir.join(DESKTOP_FILE);
        sync_at(&path, Some("entry\n")).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        sync_at(&path, Some("entry\n")).unwrap();
        // An atomic rewrite renames a new file into place, which changes the inode.
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_rewrites_an_entry_whose_executable_changed() {
        let dir = temp_dir("rewrite");
        let path = dir.join(DESKTOP_FILE);
        sync_at(&path, Some(&desktop_entry(Path::new("/old/graphene")))).unwrap();
        let updated = desktop_entry(Path::new("/new/graphene"));
        sync_at(&path, Some(&updated)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), updated);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_removes_the_entry_when_disabled() {
        let dir = temp_dir("remove");
        let path = dir.join(DESKTOP_FILE);
        sync_at(&path, Some("entry\n")).unwrap();
        sync_at(&path, None).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn sync_ignores_a_missing_entry_when_disabled() {
        let dir = temp_dir("missing");
        sync_at(&dir.join(DESKTOP_FILE), None).unwrap();
    }
}
