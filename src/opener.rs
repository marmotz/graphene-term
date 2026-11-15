//! Opens a URI with the platform's default handler. Shared by every feature that opens a
//! link: the update banner and the terminal links.

use std::ffi::CStr;
use std::process::Command;

/// Opens `uri` without blocking: `xdg-open` runs from a detached thread that waits for it,
/// so no zombie process is left. Failures are only logged.
pub fn open(uri: &str) {
    let mut command = command(uri);
    let uri = uri.to_owned();
    let opener = command.get_program().to_string_lossy().into_owned();
    let thread_opener = opener.clone();
    let spawned = std::thread::Builder::new()
        .name(opener.clone())
        .spawn(move || match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => log::warn!("{thread_opener} {uri} failed: {status}"),
            Err(error) => log::warn!("cannot run {thread_opener} {uri}: {error}"),
        });
    if let Err(error) = spawned {
        log::warn!("cannot start the {opener} thread: {error}");
    }
}

/// Name of this machine, read from the kernel; `None` when it cannot be read.
pub fn hostname() -> Option<String> {
    let mut buffer = [0i8; 256];
    // SAFETY: `buffer` is writable and its length is passed to the POSIX function.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len() - 1) };
    if result != 0 {
        return None;
    }
    // SAFETY: The final byte was not passed to `gethostname` and remains NUL.
    let name = unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_owned();
    (!name.is_empty()).then_some(name)
}

/// The command opening `uri`. The URI is a single argument, never parsed by a shell.
fn command(uri: &str) -> Command {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";
    let mut command = Command::new(program);
    command.arg(uri);
    command
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn command_passes_the_uri_as_one_argument() {
        let uri = "https://example.com/a b?c=1&d=$(rm -rf ~)";
        let command = command(uri);
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(not(target_os = "macos"))]
        let opener = "xdg-open";
        assert_eq!(command.get_program(), OsStr::new(opener));
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, vec![OsStr::new(uri)]);
    }

    #[test]
    fn hostname_is_read_without_its_newline() {
        let name = hostname().expect("hostname");
        assert!(!name.is_empty());
        assert_eq!(name, name.trim());
    }
}
