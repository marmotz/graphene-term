//! Checks the Markdown rendering of `scripts/benchmark-terminals.py` on a fixture and
//! that its payloads write a measurement. Skipped when `python3` is missing.

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

fn read_value(path: &Path) -> f64 {
    fs::read_to_string(path)
        .expect("result file written")
        .trim()
        .parse()
        .expect("result is a number")
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
    ] {
        assert!(table.contains(line), "missing {line:?} in:\n{table}");
    }
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
    assert!(read_value(&result) > 0.0);
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
    assert!(read_value(&result) > 0.0);
}
