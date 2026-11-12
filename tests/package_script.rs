//! Runs `scripts/package.sh` on a fake binary with stubbed packaging tools and checks
//! the produced file names and package versions.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "graphene-package-test-{}-{}",
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

    /// Runs `package.sh` against a fake binary printing `version_output`.
    fn package(&self, version_output: &str) -> std::process::Output {
        let binary = self.script("graphene", &format!("echo '{version_output}'\n"));
        // `cargo deb` / `cargo generate-rpm` stub: log the arguments, create `--output`.
        self.script(
            "bin/cargo",
            &format!(
                "echo \"$*\" >> '{log}'\n\
                 while [[ $# -gt 0 ]]; do\n\
                   if [[ $1 == --output ]]; then touch \"$2\"; fi\n\
                   shift\n\
                 done\n",
                log = self.dir.join("cargo.log").display()
            ),
        );
        let appimagetool = self.script("appimagetool", "touch \"${@: -1}\"\n");
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );

        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package.sh"))
            .env("PATH", path)
            .env("GRAPHENE_BINARY", binary)
            .env("DIST_DIR", self.dist())
            .env("APPIMAGETOOL", appimagetool)
            .output()
            .expect("run package.sh")
    }

    fn dist(&self) -> PathBuf {
        self.dir.join("dist")
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dist())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn cargo_log(&self) -> String {
        fs::read_to_string(self.dir.join("cargo.log")).unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "package.sh failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dev_build_tag_and_file_names_come_from_the_binary_version() {
    let sandbox = Sandbox::new();
    let output = sandbox.package("graphene 0.1.0-dev.20261112163249");
    assert_success(&output);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().last(), Some("tag=v0.1.0-dev.20261112163249"));
    assert_eq!(
        sandbox.files(),
        [
            "SHA256SUMS",
            "aur",
            "graphene-term-0.1.0-0.dev.20261112163249.x86_64.rpm",
            "graphene-term_0.1.0-dev.20261112163249_amd64.deb",
            "graphene-v0.1.0-dev.20261112163249-x86_64-unknown-linux-gnu.tar.gz",
            "graphene-v0.1.0-dev.20261112163249-x86_64.AppImage",
        ]
    );

    let log = sandbox.cargo_log();
    assert!(
        log.contains("--deb-version 0.1.0~dev.20261112163249"),
        "{log}"
    );
    assert!(log.contains("release = \"0.dev.20261112163249\""), "{log}");

    let pkgbuild = fs::read_to_string(sandbox.dist().join("aur/PKGBUILD")).unwrap();
    assert!(
        pkgbuild.contains("pkgver=0.1.0.r20261112163249"),
        "{pkgbuild}"
    );
}

#[test]
fn stable_build_uses_the_plain_version() {
    let sandbox = Sandbox::new();
    let output = sandbox.package("graphene 0.2.0");
    assert_success(&output);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().last(), Some("tag=v0.2.0"));
    assert_eq!(
        sandbox.files(),
        [
            "SHA256SUMS",
            "aur",
            "graphene-term-0.2.0-1.x86_64.rpm",
            "graphene-term_0.2.0_amd64.deb",
            "graphene-v0.2.0-x86_64-unknown-linux-gnu.tar.gz",
            "graphene-v0.2.0-x86_64.AppImage",
        ]
    );

    let log = sandbox.cargo_log();
    assert!(log.contains("--deb-version 0.2.0 "), "{log}");
    assert!(log.contains("release = \"1\""), "{log}");
}

#[test]
fn rejects_an_unexpected_version_output() {
    let sandbox = Sandbox::new();
    let output = sandbox.package("graphene 0.1.0-dev.3");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unexpected"), "{stderr}");
}
