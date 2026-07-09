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
