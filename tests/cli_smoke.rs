use assert_cmd::prelude::*;
use httpmock::prelude::*;
use predicates::prelude::*;
use rstest::rstest;
use tempfile::{NamedTempFile, TempDir};

const USERNAME: &str = "tester";
const PASSWORD: &str = "secret";

fn mock_login(server: &MockServer, token: &str) {
    server.mock(|when, then| {
        when.method(POST)
            .path("/api/v0/auth/login")
            .json_body_obj(&serde_json::json!({
                "username": USERNAME,
                "password": PASSWORD,
            }));
        then.status(200)
            .header("content-type", "application/json")
            .json_body_obj(&serde_json::json!({ "token": token }));
    });
}

fn configured_command(server: &MockServer, home: &TempDir) -> std::process::Command {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin!("hubuum-cli"));
    cmd.env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("HUBUUM_CLI__SERVER__HOSTNAME", server.host())
        .env("HUBUUM_CLI__SERVER__PORT", server.port().to_string())
        .env("HUBUUM_CLI__SERVER__PROTOCOL", "http")
        .env("HUBUUM_CLI__SERVER__SSL_VALIDATION", "false")
        .env("HUBUUM_CLI__SERVER__USERNAME", USERNAME)
        .env("HUBUUM_CLI__SERVER__PASSWORD", PASSWORD);
    cmd
}

#[test]
fn help_flag_exits_successfully() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin!("hubuum-cli"));
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--hostname"));
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

#[rstest]
#[case("help object", "Scope: object")]
#[case("?", "Available commands")]
fn command_mode_help_paths_succeed(#[case] input: &str, #[case] expected: &str) {
    let server = MockServer::start();
    let home = TempDir::new().expect("temp home should be created");
    mock_login(&server, "smoke-token");

    let mut cmd = configured_command(&server, &home);
    cmd.arg("--command")
        .arg(input)
        .assert()
        .success()
        .stdout(predicate::str::contains(expected));
}

#[test]
fn source_mode_help_script_succeeds() {
    let server = MockServer::start();
    let home = TempDir::new().expect("temp home should be created");
    mock_login(&server, "smoke-token");
    let script = NamedTempFile::new().expect("temp script should be created");
    std::fs::write(script.path(), "help object\n?\n").expect("script should be written");

    let mut cmd = configured_command(&server, &home);
    cmd.arg("--source")
        .arg(script.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Scope: object"))
        .stdout(predicate::str::contains("Available commands"));
}

#[test]
fn invalid_command_exits_non_zero_with_readable_error() {
    let server = MockServer::start();
    let home = TempDir::new().expect("temp home should be created");
    mock_login(&server, "smoke-token");

    let mut cmd = configured_command(&server, &home);
    cmd.arg("--command")
        .arg("does-not-exist")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Command not found"));
}
