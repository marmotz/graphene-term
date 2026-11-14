use std::fs;
use std::process::Command;

fn config_dir() -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "graphene-prune-command-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_prune(root: &std::path::Path, yes: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_graphene"));
    command
        .arg("--prune-config")
        .env("XDG_CONFIG_HOME", root)
        .env("HOME", root);
    if yes {
        command.arg("--yes");
    }
    command.output().unwrap()
}

#[test]
fn prune_dispatch_creates_a_missing_default_file_without_starting_the_gui() {
    let root = config_dir();
    let output = run_prune(&root, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("nothing to prune"));
    assert!(root.join("graphene/config.toml").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_interactive_prune_without_yes_lists_keys_but_does_not_modify_them() {
    let root = config_dir();
    let config = root.join("graphene");
    fs::create_dir_all(&config).unwrap();
    let file = config.join("config.toml");
    let original = "[font]\nfuture_option = 1\nsize = 18\n";
    fs::write(&file, original).unwrap();

    let output = run_prune(&root, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("font.future_option"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a terminal or --yes"));
    assert_eq!(fs::read_to_string(file).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn prune_yes_removes_unknown_keys_without_launching_the_gui() {
    let root = config_dir();
    let config = root.join("graphene");
    fs::create_dir_all(&config).unwrap();
    let file = config.join("config.toml");
    fs::write(
        &file,
        "language = \"fr\"\n[font]\nfuture_option = 1\nsize = 16\n",
    )
    .unwrap();

    let output = run_prune(&root, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(file).unwrap();
    assert!(contents.contains("language = \"fr\""));
    assert!(contents.contains("size = 16"));
    assert!(!contents.contains("future_option"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn prune_dispatch_leaves_invalid_toml_untouched() {
    let root = config_dir();
    let config = root.join("graphene");
    fs::create_dir_all(&config).unwrap();
    let file = config.join("config.toml");
    let original = "[font\nsize = 12\n";
    fs::write(&file, original).unwrap();

    let output = run_prune(&root, true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid TOML"));
    assert_eq!(fs::read_to_string(file).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}
