use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn pipeline_help_is_offline_and_never_dispatches_the_command() {
    let directory = tempdir().unwrap();
    for statement in [
        "H",
        "HELP F",
        "object delete --id 12 | H",
        "object list | F WHERE name ICONTAINS 'test' | HELP CURRENT",
    ] {
        cargo_bin_cmd!("hubuum-cli")
            .env("XDG_CONFIG_HOME", directory.path())
            .args([
                "--hostname",
                "127.0.0.1",
                "--port",
                "1",
                "--command",
                statement,
            ])
            .assert()
            .success()
            .stdout(contains("does not execute"));
    }
    cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args([
            "--hostname",
            "127.0.0.1",
            "--port",
            "1",
            "object",
            "delete",
            "|",
            "HELP",
            "F",
        ])
        .assert()
        .success()
        .stdout(contains("FUZZY"));
}

#[test]
fn existing_pipe_help_uses_the_same_updated_language_reference() {
    let directory = tempdir().unwrap();
    cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args(["help", "pipe", "search"])
        .assert()
        .success()
        .stdout(contains("ICONTAINS"))
        .stdout(contains("now-Ns/m/h/d"));
    cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args(["help", "pipe", "check"])
        .assert()
        .success()
        .stdout(contains("CHECK STRICT"));
}

#[test]
fn new_predicates_work_in_the_cli_before_json_rendering() {
    let directory = tempdir().unwrap();
    for test in [
        "key ICONTAINS 'OUTPUT.'",
        "key FUZZY 'outpt.format'",
        "key CONTAINS 'output.'",
    ] {
        let output = cargo_bin_cmd!("hubuum-cli").env("XDG_CONFIG_HOME", directory.path())
            .args(["--command", &format!("config show --key output.format --output json | CHECK STRICT | F WHERE {test} | VALUE key")])
            .assert().success().get_output().stdout.clone();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output).unwrap(),
            serde_json::json!(["output.format"])
        );
    }
}
