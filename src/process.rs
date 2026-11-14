//! What a pane's shell is doing, read from `/proc`: its current directory and the
//! program in the foreground of its terminal.

use std::fs;
use std::path::PathBuf;

/// The terminal's foreground process group (`tpgid`) from the content of
/// `/proc/<pid>/stat`. Fields are counted after the last `)`, because the command name
/// before it may contain spaces and parentheses.
pub fn parse_tpgid(stat: &str) -> Option<i32> {
    let (_, rest) = stat.rsplit_once(')')?;
    // After `comm`: state, ppid, pgrp, session, tty_nr, tpgid.
    rest.split_whitespace().nth(5)?.parse().ok()
}

/// The foreground process group when it is not the shell itself: some program runs in
/// the shell's terminal.
pub fn foreground_group(shell_pid: u32, tpgid: i32) -> Option<u32> {
    let group = u32::try_from(tpgid).ok().filter(|&group| group > 0)?;
    (group != shell_pid).then_some(group)
}

/// The name of the program running in the foreground of the shell's terminal, `None`
/// when the shell itself is in the foreground or `/proc` cannot be read.
#[allow(dead_code, reason = "read by the close confirmation task")]
pub fn foreground_program(shell_pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?;
    let group = foreground_group(shell_pid, parse_tpgid(&stat)?)?;
    program_name(group)
}

/// The program name of the process `pid` (`/proc/<pid>/comm`), `None` when it cannot
/// be read.
pub fn program_name(pid: u32) -> Option<String> {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = comm.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The shell's current directory, `None` when `/proc` cannot be read.
pub fn current_dir(shell_pid: u32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{shell_pid}/cwd")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpgid_is_the_sixth_field_after_comm() {
        let stat = "1234 (zsh) S 1000 1234 1234 34816 5678 4194304 0 0";
        assert_eq!(parse_tpgid(stat), Some(5678));
    }

    #[test]
    fn tpgid_survives_spaces_and_parentheses_in_comm() {
        let stat = "42 (my (odd) prog) R 1 42 42 34817 99 0 0";
        assert_eq!(parse_tpgid(stat), Some(99));
    }

    #[test]
    fn tpgid_without_a_terminal_is_minus_one() {
        assert_eq!(parse_tpgid("7 (daemon) S 1 7 7 0 -1 0"), Some(-1));
    }

    #[test]
    fn malformed_stat_has_no_tpgid() {
        assert_eq!(parse_tpgid(""), None);
        assert_eq!(parse_tpgid("1234 zsh S 1000 1234 1234 34816 5678"), None);
        assert_eq!(parse_tpgid("1234 (zsh) S 1000"), None);
        assert_eq!(parse_tpgid("1234 (zsh) S 1000 1234 1234 34816 abc"), None);
    }

    #[test]
    fn shell_in_the_foreground_has_no_program() {
        assert_eq!(foreground_group(1234, 1234), None);
    }

    #[test]
    fn other_group_in_the_foreground_is_a_program() {
        assert_eq!(foreground_group(1234, 5678), Some(5678));
    }

    #[test]
    fn missing_terminal_group_is_no_program() {
        assert_eq!(foreground_group(1234, -1), None);
        assert_eq!(foreground_group(1234, 0), None);
    }

    #[test]
    fn own_process_has_a_current_dir() {
        let pid = std::process::id();
        assert_eq!(current_dir(pid), std::env::current_dir().ok());
    }

    #[test]
    fn missing_process_has_nothing() {
        // Above the kernel's maximum pid (4194304), so never a live process.
        let pid = 4_194_305 + 1000;
        assert_eq!(current_dir(pid), None);
        assert_eq!(foreground_program(pid), None);
        assert_eq!(program_name(pid), None);
    }

    #[test]
    fn own_process_has_a_program_name() {
        let name = program_name(std::process::id()).expect("own comm is readable");
        assert!(!name.is_empty());
        assert!(!name.ends_with('\n'));
    }
}
