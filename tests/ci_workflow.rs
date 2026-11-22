//! Checks invariants of `.github/workflows/ci.yml` that keep CI fast and are easy to regress.

use std::fs;

fn workflow() -> String {
    fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.github/workflows/ci.yml"
    ))
    .unwrap()
}

/// Returns the lines of the given top-level job, header excluded.
fn job_lines<'a>(workflow: &'a str, job: &str) -> Vec<&'a str> {
    let header = format!("  {job}:");
    workflow
        .lines()
        .skip_while(|line| *line != header)
        .skip(1)
        // A new top-level job starts with exactly two spaces of indentation.
        .take_while(|line| {
            !(line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':'))
        })
        .collect()
}

/// Returns the lines of the step named `name` inside the given job lines.
fn step_lines<'a>(job: &[&'a str], name: &str) -> Vec<&'a str> {
    let header = format!("- name: {name}");
    job.iter()
        .skip_while(|line| line.trim() != header)
        .enumerate()
        .take_while(|(index, line)| *index == 0 || !line.trim_start().starts_with("- "))
        .map(|(_, line)| *line)
        .collect()
}

const JOBS: [&str; 2] = ["check-linux", "check-macos"];

#[test]
fn ci_runs_on_develop_pushes_so_caches_are_shared_across_pull_requests() {
    let workflow = workflow();
    let push = workflow
        .lines()
        .skip_while(|line| *line != "  push:")
        .nth(1)
        .expect("ci.yml must be triggered on push");
    assert_eq!(push.trim(), "branches: [develop]");
}

#[test]
fn caches_are_only_saved_from_develop() {
    let workflow = workflow();
    for job in JOBS {
        let lines = job_lines(&workflow, job);
        assert!(
            lines
                .iter()
                .any(|line| line.trim() == "- uses: Swatinem/rust-cache@v2"),
            "{job} must use Swatinem/rust-cache"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.trim() == "save-if: ${{ github.ref == 'refs/heads/develop' }}"),
            "{job} must only save its cache from develop: pull request caches are never shared"
        );
    }
}

#[test]
fn release_builds_are_skipped_in_pull_requests() {
    let workflow = workflow();
    for job in JOBS {
        let lines = job_lines(&workflow, job);
        let step = step_lines(&lines, "Release build");
        assert!(!step.is_empty(), "{job} must keep a release build step");
        assert!(
            step.iter()
                .any(|line| line.trim() == "if: github.event_name == 'push'"),
            "{job} must only run the release build on develop pushes"
        );
    }
}
