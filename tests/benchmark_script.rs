//! Checks the Markdown rendering of `scripts/benchmark-terminals.py` on a fixture and
//! that its payloads write a measurement and their grid, that every terminal is launched
//! with the same grid, and that `run` builds Graphene in release mode.
//! Skipped when `python3` is missing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "graphene-benchmark-script-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn python_available() -> bool {
    let found = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !found {
        eprintln!("skipping: python3 is not available");
    }
    found
}

fn script(args: &[&str]) -> Output {
    Command::new("python3")
        .arg(manifest_dir().join("scripts/benchmark-terminals.py"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run benchmark-terminals.py")
}

fn render_fixture() -> String {
    let fixture = manifest_dir().join("tests/fixtures/benchmark-results.json");
    let output = script(&["render", fixture.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The measurement and the grid line a payload wrote.
fn read_result(path: &Path) -> (f64, String) {
    let text = fs::read_to_string(path).expect("result file written");
    let mut lines = text.lines();
    let value = lines
        .next()
        .expect("measurement line")
        .parse()
        .expect("result is a number");
    (value, lines.next().expect("grid line").to_owned())
}

/// Runs `code` with the script imported as `bt`, and returns its stdout.
fn python(code: &str, env: &[(&str, &Path)]) -> String {
    let prelude = format!(
        "import importlib.util, json, sys\n\
         spec = importlib.util.spec_from_file_location('bt', {:?})\n\
         bt = importlib.util.module_from_spec(spec)\n\
         spec.loader.exec_module(bt)\n",
        manifest_dir().join("scripts/benchmark-terminals.py")
    );
    let mut command = Command::new("python3");
    command.arg("-c").arg(prelude + code).stdin(Stdio::null());
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().expect("run python3");
    assert!(
        output.status.success(),
        "python failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn renders_the_table_header() {
    if !python_available() {
        return;
    }
    let table = render_fixture();
    let mut lines = table.lines();
    assert_eq!(
        lines.next(),
        Some(
            "| Terminal | Throughput (s) | Refresh rate (fps) | Startup (ms) \
             | Memory idle (MiB) | Memory under load (MiB) |"
        )
    );
    assert_eq!(lines.next(), Some("|---|---|---|---|---|---|"));
}

#[test]
fn puts_graphene_first_in_bold_with_medians_and_best_values() {
    if !python_available() {
        return;
    }
    let table = render_fixture();
    let rows: Vec<&str> = table.lines().skip(2).take(3).collect();
    assert_eq!(
        rows,
        [
            "| **Graphene** | 1.50 | **120.0** | 85 | **60.0** | **92.0** |",
            "| Alacritty | **1.00** | 100.0 | **72** | 80.0 | 111.0 |",
            "| GNOME Terminal | n/a | 61.0 | 205 | 101.0 | n/a |",
        ]
    );
}

#[test]
fn prints_the_machine_block() {
    if !python_available() {
        return;
    }
    let table = render_fixture();
    for line in [
        "Machine:",
        "- CPU: Test CPU 9000",
        "- GPU: Test GPU",
        "- Kernel: 6.0.0-test",
        "- Session: x11",
        "- Resolution: 1920x1080",
        "- Runs: median of 3",
        "- Versions: graphene 0.1.3, alacritty 0.17.0, GNOME Terminal 3.60.0",
        "- Grid: 100x30 cells",
        "- Grid mismatch: GNOME Terminal ran at 98x30",
    ] {
        assert!(table.contains(line), "missing {line:?} in:\n{table}");
    }
    assert_eq!(table.matches("Grid mismatch").count(), 1, "{table}");
}

#[test]
fn every_terminal_is_launched_with_the_grid() {
    if !python_available() {
        return;
    }
    let commands = python(
        "out = {}\n\
         for spec in bt.TERMINALS:\n\
         \x20   t = bt.Terminal(spec, spec['binary'])\n\
         \x20   t.extra = {'config': 'CONFIG'}\n\
         \x20   t.set_grid(123, 45, 999, 777)\n\
         \x20   out[spec['name']] = ' '.join(t.command(['prog']))\n\
         print(json.dumps(out))",
        &[],
    );
    let expected = [
        ("Graphene", "graphene --columns 123 --lines 45 -e prog"),
        (
            "Alacritty",
            "alacritty -o window.dimensions.columns=123 -o window.dimensions.lines=45 -e prog",
        ),
        (
            "Kitty",
            "kitty -o remember_window_size=no -o initial_window_width=123c \
             -o initial_window_height=45c prog",
        ),
        (
            "WezTerm",
            "wezterm --config initial_cols=123 --config initial_rows=45 start \
             --always-new-process -- prog",
        ),
        (
            "Terminator",
            "terminator -u -g CONFIG --geometry=999x777 -x prog",
        ),
        (
            "GNOME Terminal",
            "gnome-terminal --wait --geometry=123x45 -- prog",
        ),
    ];
    for (name, command) in expected {
        assert!(
            commands.contains(&format!("{name:?}: {command:?}")),
            "{name} not launched with {command:?}: {commands}"
        );
    }
    for dropped in ["foot", "Konsole"] {
        assert!(
            !commands.contains(&format!("{dropped:?}:")),
            "{dropped} should not be benchmarked: {commands}"
        );
    }
}

#[test]
fn terminator_config_copies_the_user_config_unmaximized() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let home = sandbox.0.join("config");
    fs::create_dir_all(home.join("terminator")).unwrap();
    fs::write(
        home.join("terminator/config"),
        "[global_config]\n  window_state = maximise\n  borderless = True\n[profiles]\n",
    )
    .unwrap();
    let path = python(
        &format!(
            "print(bt.terminator_config({:?})['config'])",
            sandbox.0.display().to_string()
        ),
        &[("XDG_CONFIG_HOME", &home)],
    );
    assert_eq!(
        fs::read_to_string(path.trim()).unwrap(),
        "[global_config]\n  window_state = normal\n  borderless = True\n[profiles]\n"
    );

    let empty = sandbox.0.join("empty");
    let path = python(
        &format!(
            "print(bt.terminator_config({:?})['config'])",
            sandbox.0.display().to_string()
        ),
        &[("XDG_CONFIG_HOME", &empty)],
    );
    assert_eq!(
        fs::read_to_string(path.trim()).unwrap(),
        "[global_config]\n  window_state = normal\n"
    );
}

#[test]
fn fire_payload_writes_a_positive_frame_rate() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let result = sandbox.0.join("fire");
    let output = script(&[
        "payload",
        "fire",
        "--duration",
        "0.2",
        "--result",
        result.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "fire payload failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.is_empty());
    // Not a terminal: the payload falls back to 80x24.
    let (fps, grid) = read_result(&result);
    assert!(fps > 0.0);
    assert_eq!(grid, "80 24");
}

#[test]
fn throughput_payload_writes_a_positive_duration() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let text = sandbox.0.join("text.txt");
    fs::write(
        &text,
        "To be, or not to be, that is the question.\n".repeat(2000),
    )
    .unwrap();
    let result = sandbox.0.join("throughput");
    let output = script(&[
        "payload",
        "throughput",
        "--file",
        text.to_str().unwrap(),
        "--result",
        result.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "throughput payload failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout.len(),
        fs::metadata(&text).unwrap().len() as usize
    );
    let (duration, grid) = read_result(&result);
    assert!(duration > 0.0);
    assert_eq!(grid, "80 24");
}

/// Runs `run --terminals graphene` with a fake `cargo` first on `PATH`. `run` measures
/// Linux processes only: on other systems it stops before building. The fake records
/// its arguments and working directory, then exits with `exit_code`.
#[cfg(target_os = "linux")]
fn run_with_fake_cargo(sandbox: &Sandbox, exit_code: i32) -> (Output, String) {
    use std::os::unix::fs::PermissionsExt;

    let bin = sandbox.0.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let log = sandbox.0.join("cargo.log");
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\necho \"$PWD $*\" > '{}'\nexit {exit_code}\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("python3")
        .arg(manifest_dir().join("scripts/benchmark-terminals.py"))
        .args(["run", "--terminals", "graphene", "--runs", "1"])
        .arg("--out")
        .arg(sandbox.0.join("results.json"))
        .env("PATH", path)
        .env("CARGO_TARGET_DIR", sandbox.0.join("target"))
        .stdin(Stdio::null())
        .output()
        .expect("run benchmark-terminals.py");
    let logged = fs::read_to_string(&log).unwrap_or_default();
    (output, logged)
}

#[test]
#[cfg(target_os = "linux")]
fn run_builds_graphene_in_release_mode_from_the_checkout() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let (_, logged) = run_with_fake_cargo(&sandbox, 1);
    assert_eq!(
        logged.trim(),
        format!(
            "{} build --release --bin graphene",
            manifest_dir().display()
        )
    );
}

#[test]
#[cfg(target_os = "linux")]
fn run_aborts_when_the_release_build_fails() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let (output, _) = run_with_fake_cargo(&sandbox, 3);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cargo build --release failed (exit code 3)"),
        "unexpected stderr: {stderr}"
    );
    assert!(!sandbox.0.join("results.json").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn run_aborts_when_the_built_binary_is_missing() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let (output, _) = run_with_fake_cargo(&sandbox, 0);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = sandbox.0.join("target/release/graphene");
    assert!(
        stderr.contains(&format!("{} not found after the build", expected.display())),
        "unexpected stderr: {stderr}"
    );
}

#[test]
#[cfg(not(target_os = "linux"))]
fn run_refuses_other_systems_than_linux() {
    if !python_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let output = script(&[
        "run",
        "--terminals",
        "graphene",
        "--runs",
        "1",
        "--out",
        sandbox.0.join("results.json").to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the benchmark runs on Linux only"),
        "unexpected stderr: {stderr}"
    );
    assert!(!sandbox.0.join("results.json").exists());
}
