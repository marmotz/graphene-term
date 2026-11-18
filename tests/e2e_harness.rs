//! Checks the GUI end-to-end harness entry point and the pull-request job that runs it.
//! The GUI scenarios themselves need a virtual X11 or Wayland session and run in CI.
#![cfg(target_os = "linux")]

use std::fs;
use std::process::{Command, Output};

const PROBE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/e2e/gui-automation-probe.sh"
);

fn ci_workflow() -> String {
    fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.github/workflows/ci.yml"
    ))
    .unwrap()
}

/// Returns the lines of the given top-level job, header excluded.
fn job_body(workflow: &str, job: &str) -> Option<Vec<String>> {
    let header = format!("  {job}:");
    let mut lines = workflow.lines().skip_while(|line| *line != header);
    lines.next()?;
    Some(
        lines
            .take_while(|line| {
                line.is_empty() || line.starts_with("   ") || line.trim_start().starts_with('#')
            })
            .map(str::to_owned)
            .collect(),
    )
}

fn run_probe(args: &[&str], binary: &str) -> Output {
    Command::new("bash")
        .arg(PROBE)
        .args(args)
        .env("GRAPHENE_BIN", binary)
        .output()
        .unwrap()
}

#[test]
fn e2e_job_runs_the_smoke_test_on_both_backends_and_blocks_on_failure() {
    let workflow = ci_workflow();
    assert!(
        workflow.contains("on:\n  pull_request:"),
        "the CI workflow must run on pull requests"
    );
    let job = job_body(&workflow, "e2e").expect("ci.yml must define an `e2e` job");
    let job = job.join("\n");

    assert!(
        !job.contains("continue-on-error"),
        "an E2E failure must fail the `e2e` check"
    );
    assert!(
        job.contains("xvfb-run"),
        "X11 runs in a virtual Xvfb session"
    );
    assert!(job.contains("tests/e2e/gui-automation-probe.sh x11"));
    assert!(job.contains("tests/e2e/gui-automation-probe.sh wayland"));
    for package in ["sway", "wtype", "xdotool", "xvfb", "grim", "imagemagick"] {
        assert!(
            job.contains(package),
            "the `e2e` job must install {package}"
        );
    }
    assert!(job.contains("wayland-automation==0.3.1"));
    assert!(
        job.contains("actions/upload-artifact") && job.contains("if: failure()"),
        "logs and screenshots must be uploaded when the E2E check fails"
    );
}

#[test]
fn probe_rejects_an_unknown_backend() {
    let output = run_probe(&["quartz"], "/bin/true");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn probe_rejects_a_missing_graphene_executable() {
    let output = run_probe(&["x11"], "/nonexistent/graphene");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Graphene executable is missing or not executable")
    );
}
