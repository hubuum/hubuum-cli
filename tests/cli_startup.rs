use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn help_and_version_do_not_require_login() {
    cargo_bin_cmd!("hubuum-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("hubuum-cli <command...>"));

    cargo_bin_cmd!("hubuum-cli")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn direct_help_and_config_paths_do_not_require_login() {
    cargo_bin_cmd!("hubuum-cli")
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Available commands"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "pipe"])
        .assert()
        .success()
        .stdout(predicate::str::contains("grep os_version"))
        .stdout(predicate::str::contains("V 129.240"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "shell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Type a scope name"))
        .stdout(predicate::str::contains("next to fetch the next page"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "paths"])
        .assert()
        .success()
        .stdout(predicate::str::contains("System"))
        .stdout(predicate::str::contains("User"))
        .stdout(predicate::str::contains("Write"));
}

#[test]
fn theme_preview_includes_a_dense_banded_table() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--color", "never", "theme", "preview", "rose-pink"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dense table with alternating row bands",
        ))
        .stdout(predicate::str::contains(
            "Name            | os_version   | status",
        ))
        .stdout(predicate::str::contains("edge-gateway-01"))
        .stdout(predicate::str::contains("lab-console-07"));
}

#[test]
fn direct_command_redirects_to_an_unstyled_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("help.txt");

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", ">", path.to_str().expect("UTF-8 path")])
        .assert()
        .success();

    let output = std::fs::read_to_string(path).expect("redirected help");
    assert!(output.contains("Available commands"));
    assert!(!output.contains('\x1b'));
}

#[test]
fn direct_command_supports_each_redirects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = format!("each:{}/{{n}}.txt", dir.path().display());

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", ">", &template])
        .assert()
        .success();

    let first =
        std::fs::read_to_string(dir.path().join("1.txt")).expect("first per-item redirect output");
    assert!(first.contains("key"));
}

#[test]
fn hidden_command_alias_still_works() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Available commands"));
}

#[test]
fn hidden_command_alias_supports_pipeline_stages() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "help | grep Available | count"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1"));
}

#[test]
fn direct_command_errors_exit_nonzero() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "definitely-not-a-command"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Command not found"));
}

#[test]
fn offline_config_show_supports_semantic_output_formats() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--output", "csv"])
        .assert()
        .success()
        .stdout(predicate::str::contains("key,value,source,detail"))
        .stdout(predicate::str::contains("output.format"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--output", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\":\"output.format\""));
}

#[test]
fn offline_config_show_supports_semantic_pipeline_projection() {
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--command",
            "config show | F output | P key value | S key | L 1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("key"))
        .stdout(predicate::str::contains("output."));
}

#[test]
fn offline_config_show_supports_documented_jq_transforms() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "config show | JQ 'map({key, value})' | L 1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("key"))
        .stdout(predicate::str::contains("value"));
}

#[test]
fn dense_table_style_uses_compact_field_separators() {
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--table-style",
            "dense",
            "--color",
            "never",
            "--command",
            "config show | P key value | L 1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(" | "))
        .stdout(predicate::str::contains("key"))
        .stdout(predicate::str::contains("value"));
}

#[test]
fn json_alias_conflicts_with_non_json_output() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--json", "--output", "csv"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("--json conflicts"));
}
