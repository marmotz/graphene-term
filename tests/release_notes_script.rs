//! Checks that release notes cover only commits since the prior tag and use the expected
//! conventional-commit categories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "graphene-release-notes-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.name", "Graphene Test"]);
        git(
            &dir,
            &["config", "user.email", "graphene-test@example.invalid"],
        );
        Self(dir)
    }

    fn commit(&self, subject: &str, index: usize) {
        fs::write(self.0.join(format!("change-{index}")), subject).unwrap();
        git(&self.0, &["add", "."]);
        git(&self.0, &["commit", "-q", "-m", subject]);
    }

    fn notes(&self, previous_tag: &str) -> Output {
        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/release-notes.sh"))
            .arg(previous_tag)
            .arg("HEAD")
            .arg("marmotz/graphene-term")
            .current_dir(&self.0)
            .output()
            .expect("run release-notes.sh")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn categorizes_changes_since_the_previous_tag() {
    let sandbox = Sandbox::new();
    sandbox.commit("feat(core): earlier feature (#1)", 1);
    git(&sandbox.0, &["tag", "v0.1.0"]);
    sandbox.commit("feat(palette): add command search (#12)", 2);
    sandbox.commit("fix(pty): preserve shell output (#13)", 3);
    sandbox.commit("Fix config completion", 4);
    sandbox.commit("perf(render): reduce frame time", 5);

    let output = sandbox.notes("v0.1.0");
    assert!(
        output.status.success(),
        "release-notes.sh failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let notes = String::from_utf8(output.stdout).unwrap();
    assert!(notes.contains("## Nouveautés"), "{notes}");
    assert!(notes.contains("## Modifications"), "{notes}");
    assert!(notes.contains("## Correctifs"), "{notes}");
    assert!(notes.contains("[#12](https://github.com/marmotz/graphene-term/pull/12)"));
    assert!(notes.contains("[#13](https://github.com/marmotz/graphene-term/pull/13)"));
    assert!(notes.contains("Fix config completion"));
    assert!(!notes.contains("earlier feature"), "{notes}");
}

#[test]
fn lists_the_full_history_when_there_is_no_previous_release() {
    let sandbox = Sandbox::new();
    sandbox.commit("feat(core): first release feature", 1);

    let output = sandbox.notes("");
    assert!(output.status.success());
    let notes = String::from_utf8(output.stdout).unwrap();
    assert!(notes.contains("## Nouveautés"), "{notes}");
    assert!(notes.contains("first release feature"), "{notes}");
}
