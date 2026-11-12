use std::process::Command;

#[test]
fn version_flag_prints_build_version_and_exits() {
    let output = Command::new(env!("CARGO_BIN_EXE_graphene"))
        .arg("--version")
        .output()
        .expect("run graphene --version");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8 output");
    assert_eq!(stdout, format!("graphene {}\n", env!("GRAPHENE_VERSION")));
}

#[test]
fn build_version_starts_with_crate_version() {
    let version = env!("GRAPHENE_VERSION");
    let pkg = env!("CARGO_PKG_VERSION");
    if version != pkg {
        let timestamp = version
            .strip_prefix(&format!("{pkg}-dev."))
            .expect("dev version format");
        assert_eq!(timestamp.len(), 14);
        assert!(timestamp.bytes().all(|b| b.is_ascii_digit()));
    }
}
