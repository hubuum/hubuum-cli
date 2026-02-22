use assert_cmd::assert::OutputAssertExt;
use predicates::prelude::*;

#[test]
fn help_flag_exits_successfully() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin!("hubuum-cli"));
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: hubuum-cli"));
}

#[test]
fn invalid_typed_argument_exits_with_error() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin!("hubuum-cli"));
    cmd.arg("--port")
        .arg("not-a-number")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
