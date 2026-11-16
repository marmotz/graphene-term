//! Exercises macOS release packaging with stubbed Apple tools and a fake binary.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "graphene-macos-package-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        Self { dir }
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.join(name);
        fs::write(
            &path,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{body}"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn package(&self, version: &str, target: &str) -> Output {
        let binary = self.script("graphene", &format!("echo 'graphene {version}'\n"));
        self.script(
            "bin/sips",
            "while [[ $# -gt 0 ]]; do\n\
               if [[ $1 == --out ]]; then touch \"$2\"; fi\n\
               shift\n\
             done\n",
        );
        self.script(
            "bin/iconutil",
            "while [[ $# -gt 0 ]]; do\n\
               if [[ $1 == -o ]]; then touch \"$2\"; fi\n\
               shift\n\
             done\n",
        );
        self.script(
            "bin/codesign",
            &format!("echo \"$*\" >> '{}'\n", self.codesign_log().display()),
        );
        self.script("bin/hdiutil", "touch \"${@: -1}\"\n");
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );

        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package-macos.sh"))
            .arg(target)
            .env("PATH", path)
            .env("GRAPHENE_BINARY", binary)
            .env("DIST_DIR", self.dist())
            .output()
            .expect("run package-macos.sh")
    }

    fn codesign_log(&self) -> PathBuf {
        self.dir.join("codesign.log")
    }

    fn dist(&self) -> PathBuf {
        self.dir.join("dist")
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dist())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn builds_an_archive_and_dmg_for_each_supported_architecture() {
    for (target, version) in [
        ("aarch64-apple-darwin", "0.1.0-dev.20261112163249"),
        ("x86_64-apple-darwin", "0.2.0"),
    ] {
        let sandbox = Sandbox::new();
        let output = sandbox.package(version, target);
        assert!(
            output.status.success(),
            "packaging failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let tag = format!("v{version}");
        let dirname = format!("graphene-{tag}-{target}");
        assert_eq!(
            sandbox.files(),
            [
                format!("graphene-{tag}-{target}.dmg"),
                format!("{dirname}.tar.gz"),
            ]
        );
        let listing = Command::new("tar")
            .args(["-tzf"])
            .arg(sandbox.dist().join(format!("{dirname}.tar.gz")))
            .output()
            .unwrap();
        assert!(listing.status.success());
        let listing = String::from_utf8(listing.stdout).unwrap();
        assert!(listing.contains(&format!("{dirname}/graphene")));
        assert!(listing.contains(&format!("{dirname}/LICENSE-MIT")));

        let codesign = fs::read_to_string(sandbox.codesign_log()).unwrap();
        let calls: Vec<&str> = codesign.lines().collect();
        assert_eq!(calls.len(), 2, "unexpected codesign calls: {codesign}");
        assert!(calls[0].starts_with("--force --sign - --timestamp=none "));
        assert!(calls[0].ends_with("/Graphene.app"));
        assert!(calls[1].starts_with("--verify --strict "));
        assert!(calls[1].ends_with("/Graphene.app"));
    }
}

#[test]
fn rejects_unsupported_targets_and_versions() {
    let target_sandbox = Sandbox::new();
    assert!(
        !target_sandbox
            .package("0.1.0", "arm64-apple-darwin")
            .status
            .success()
    );

    let version_sandbox = Sandbox::new();
    let output = version_sandbox.package("0.1.0-dev.3", "aarch64-apple-darwin");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected"));
}
