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
