//! Opens a URI with the desktop's default handler (`xdg-open`). Shared by every feature
//! that opens a link: the update banner and, later, terminal hyperlinks.

use std::process::Command;

/// Opens `uri` without blocking: `xdg-open` runs from a detached thread that waits for it,
/// so no zombie process is left. Failures are only logged.
pub fn open(uri: &str) {
    let mut command = command(uri);
    let uri = uri.to_owned();
    let spawned = std::thread::Builder::new()
        .name("xdg-open".to_owned())
        .spawn(move || match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => log::warn!("xdg-open {uri} failed: {status}"),
            Err(error) => log::warn!("cannot run xdg-open {uri}: {error}"),
        });
    if let Err(error) = spawned {
        log::warn!("cannot start the xdg-open thread: {error}");
    }
}

/// The command opening `uri`. The URI is a single argument, never parsed by a shell.
fn command(uri: &str) -> Command {
    let mut command = Command::new("xdg-open");
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
        assert_eq!(command.get_program(), OsStr::new("xdg-open"));
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, vec![OsStr::new(uri)]);
    }
}
