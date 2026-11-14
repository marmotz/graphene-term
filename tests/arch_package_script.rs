//! Runs `scripts/build-arch-package.sh` against a local fake release (file:// URLs)
//! with a stubbed `makepkg`, and checks the resolved tag and the rendered PKGBUILD.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const ARCHIVE_SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "graphene-arch-package-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("releases")).unwrap();
        Self { dir }
    }

    /// Publishes a fake release `tag` whose SHA256SUMS lists its tarball.
    fn release(&self, tag: &str) {
        let dir = self.dir.join("releases").join(tag);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SHA256SUMS"),
            format!(
                "ffff  graphene-{tag}-x86_64.AppImage\n\
                 {ARCHIVE_SHA}  graphene-{tag}-x86_64-unknown-linux-gnu.tar.gz\n"
            ),
        )
        .unwrap();
    }

    /// Writes the releases API response listing `tags`, newest first.
    fn api(&self, tags: &[&str]) {
        let entries: Vec<String> = tags
            .iter()
            .map(|t| format!("{{\"tag_name\":\"{t}\"}}"))
            .collect();
        fs::write(
            self.dir.join("api.json"),
            format!("[{}]", entries.join(",")),
        )
        .unwrap();
    }

    /// Runs the script; the `makepkg` stub copies the PKGBUILD it was given next to
    /// the sandbox and prints a package path for `--packagelist`.
    fn run(&self, args: &[&str]) -> Output {
        let makepkg = self.dir.join("makepkg");
        fs::write(
            &makepkg,
            format!(
                "#!/usr/bin/env bash\n\
                 if [[ $1 == --packagelist ]]; then echo \"$PKGDEST/pkg.tar.zst\"; exit; fi\n\
                 cp PKGBUILD '{}'\n",
                self.dir.join("PKGBUILD").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&makepkg, fs::Permissions::from_mode(0o755)).unwrap();

        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/build-arch-package.sh"))
            .args(args)
            .env(
                "GRAPHENE_RELEASES_API",
                format!("file://{}", self.dir.join("api.json").display()),
            )
            .env(
                "GRAPHENE_DOWNLOAD_URL",
                format!("file://{}", self.dir.join("releases").display()),
            )
            .env("MAKEPKG", makepkg)
            .env("DIST_DIR", self.dir.join("dist"))
            .output()
            .expect("run build-arch-package.sh")
    }

    fn pkgbuild(&self) -> String {
        fs::read_to_string(self.dir.join("PKGBUILD")).unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "build-arch-package.sh failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn builds_the_most_recent_release_by_default() {
    let sandbox = Sandbox::new();
    sandbox.api(&["v0.1.0-dev.20261113205256", "v0.1.0-dev.20261112211438"]);
    sandbox.release("v0.1.0-dev.20261113205256");

    let output = sandbox.run(&[]);
    assert_success(&output);

    let stderr = stderr(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let package = stdout.trim_end();
    assert!(package.ends_with("/dist/pkg.tar.zst"), "{stdout}");
    assert!(
        stderr.ends_with(&format!("install it with:\n  sudo pacman -U {package}\n")),
        "{stderr}"
    );
    let pkgbuild = sandbox.pkgbuild();
    assert!(
        pkgbuild.contains("pkgver=0.1.0.r20261113205256\n"),
        "{pkgbuild}"
    );
    assert!(
        pkgbuild.contains(
            "releases/download/v0.1.0-dev.20261113205256/\
             graphene-v0.1.0-dev.20261113205256-x86_64-unknown-linux-gnu.tar.gz"
        ),
        "{pkgbuild}"
    );
    assert!(
        pkgbuild.contains(&format!("sha256sums=('{ARCHIVE_SHA}')")),
        "{pkgbuild}"
    );
    assert!(!sandbox.dir.join("dist/build").exists());
}

#[test]
fn builds_the_requested_version_with_or_without_the_v_prefix() {
    for arg in ["v0.2.0", "0.2.0"] {
        let sandbox = Sandbox::new();
        sandbox.api(&["v0.3.0"]);
        sandbox.release("v0.2.0");

        let output = sandbox.run(&[arg]);
        assert_success(&output);

        let pkgbuild = sandbox.pkgbuild();
        assert!(pkgbuild.contains("pkgver=0.2.0\n"), "{arg}: {pkgbuild}");
        assert!(
            pkgbuild.contains("cd \"graphene-v0.2.0-x86_64-unknown-linux-gnu\""),
            "{arg}: {pkgbuild}"
        );
    }
}

#[test]
fn rejects_an_unexpected_version() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["0.2"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unexpected version"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fails_when_the_release_does_not_exist() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["v9.9.9"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cannot download SHA256SUMS"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fails_when_there_is_no_release() {
    let sandbox = Sandbox::new();
    sandbox.api(&[]);
    let output = sandbox.run(&[]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no release found"),
        "{}",
        stderr(&output)
    );
}
