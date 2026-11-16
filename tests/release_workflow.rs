//! Checks invariants of `.github/workflows/release.yml` that are easy to regress.

use std::fs;

/// Returns the `runs-on` value of the given top-level job.
fn job_runner(workflow: &str, job: &str) -> Option<String> {
    let header = format!("  {job}:");
    let mut in_job = false;
    for line in workflow.lines() {
        if line == header {
            in_job = true;
            continue;
        }
        if in_job {
            // A new top-level job starts with exactly two spaces of indentation.
            if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':')
            {
                return None;
            }
            if let Some(value) = line.trim().strip_prefix("runs-on:") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

#[test]
fn linux_release_is_built_on_a_pinned_ubuntu_with_an_old_enough_glibc() {
    let workflow = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.github/workflows/release.yml"
    ))
    .unwrap();

    let runner = job_runner(&workflow, "linux").expect("linux job must declare runs-on");
    assert_eq!(
        runner, "ubuntu-24.04",
        "Linux release binaries must be built on ubuntu-24.04 (glibc 2.39) so they run on \
         Debian 13 and similar distributions; ubuntu-latest moved to glibc 2.43"
    );
}
