#![cfg(unix)]

use std::fs::{create_dir_all, read_to_string, set_permissions, write, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use hubuum_extension_protocol::ExtensionManifest;
use predicates::str::contains;
use serde_json::Value;
use tempfile::tempdir;

fn write_pack(root: &Path, version: &str) -> PathBuf {
    let package = root.join(format!("demo-{version}"));
    create_dir_all(package.join("bin")).expect("package directory");
    let executable = package.join("bin/demo");
    write(
        &executable,
        r#"#!/bin/sh
set -eu
if [ "${HUBUUM_CLI__SERVER__PASSWORD+x}" = x ]; then
    printf '%s\n' '{"protocol":"hubuum-cli.extension/v1","status":"error","error":{"code":"password_leaked","message":"password environment leaked","details":{}}}'
    exit 1
fi
if [ "${HUBUUM_CLI__CACHE__TIME+x}" = x ]; then
    printf '%s\n' '{"protocol":"hubuum-cli.extension/v1","status":"error","error":{"code":"config_leaked","message":"unrelated CLI config environment leaked","details":{}}}'
    exit 1
fi
case "${HUBUUM_EXTENSION_CONFIG_JSON:-}" in
    *configured*) ;;
    *)
        printf '%s\n' '{"protocol":"hubuum-cli.extension/v1","status":"error","error":{"code":"config_missing","message":"pack config missing","details":{}}}'
        exit 1
        ;;
esac
printf '%s\n' "$*" >"$(dirname "$0")/../invocation"
printf '%s\n' '{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"rows","value":[{"name":"alpha","state":"active"},{"name":"beta","state":"active"}],"columns":["name","state"]},"warnings":[]}'
"#,
    )
    .expect("extension executable");
    set_permissions(&executable, Permissions::from_mode(0o755)).expect("executable permissions");
    write(
        package.join("hubuum-extension.toml"),
        format!(
            r#"schema_version = 1
name = "demo"
version = "{version}"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/demo"

[[commands]]
path = ["demo"]
about = "Run a command sharing the pack name"

[[commands.options]]
name = "target"
kind = "string"
positional = true
required = true

[[commands]]
path = ["inventory", "list"]
arguments = ["inventory", "list"]
about = "List demo inventory"

[[commands.options]]
name = "state"
kind = "string"
long = "state"
help = "Inventory state"
values = ["active", "retired"]
"#
        ),
    )
    .expect("extension manifest");
    package
}

fn write_config(path: &Path, user_root: &Path) {
    write(
        path,
        format!(
            r#"[extensions]
system_roots = []
user_roots = ["{}"]

[extensions.config.demo]
label = "configured"
"#,
            user_root.display()
        ),
    )
    .expect("extension config");
}

fn write_response_pack(root: &Path, name: &str, response: &str, exit_code: u8) {
    let package = root.join(name);
    create_dir_all(package.join("bin")).expect("package directory");
    let executable = package.join("bin/run");
    write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{response}'\nexit {exit_code}\n"),
    )
    .expect("extension executable");
    set_permissions(&executable, Permissions::from_mode(0o755)).expect("executable permissions");
    write(
        package.join("hubuum-extension.toml"),
        format!(
            r#"schema_version = 1
name = "{name}"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/run"

[[commands]]
path = ["run"]
about = "Run protocol fixture"
"#
        ),
    )
    .expect("extension manifest");
}

fn write_workflow_pack(
    root: &Path,
    name: &str,
    run: &[&str],
    bindings: &[(&str, &str)],
    allows_mutation: bool,
) {
    let package = root.join(name);
    create_dir_all(&package).expect("package directory");
    let command = run
        .iter()
        .map(|segment| format!("\"{segment}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let capabilities = if allows_mutation {
        "capabilities = [\"mutate\"]"
    } else {
        ""
    };
    let bindings = bindings
        .iter()
        .map(|(name, value)| format!("{name} = {value:?}\n"))
        .collect::<String>();
    write(
        package.join("hubuum-extension.toml"),
        format!(
            r#"schema_version = 1
name = "{name}"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]
about = "Compose built-in commands"

[commands.workflow]
{capabilities}

[[commands.workflow.steps]]
id = "items"
run = [{command}]

[commands.workflow.steps.with]
{bindings}
"#
        ),
    )
    .expect("workflow manifest");
}

fn json_stdout(assertion: assert_cmd::assert::Assert) -> Value {
    let output = assertion.success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON command output")
}

#[test]
fn bundled_manifest_workflow_uses_the_current_language() {
    let manifest = ExtensionManifest::parse(include_str!(
        "../examples/hubuum-inventory/hubuum-extension.toml"
    ))
    .expect("bundled workflow manifest");
    let snapshot = manifest.commands()[0]
        .workflow()
        .expect("snapshot workflow");

    assert_eq!(snapshot.steps().len(), 3);
    assert!(snapshot.result().is_some());
}

#[test]
fn extension_commands_are_first_class_and_lifecycle_managed() {
    let temporary = tempdir().expect("temporary directory");
    let sources = temporary.path().join("sources");
    let user_root = temporary.path().join("installed");
    create_dir_all(&sources).expect("source root");
    let source_v1 = write_pack(&sources, "0.1.0");
    let source_v2 = write_pack(&sources, "0.2.0");
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let installed = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .env("HUBUUM_CLI__SERVER__PASSWORD", "must-not-leak")
            .env("HUBUUM_CLI__CACHE__TIME", "123")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "install",
                source_v1.to_str().expect("source path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(installed["status"], "installed");
    assert_eq!(installed["name"], "demo");

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "demo",
            "demo",
            "target-1",
        ])
        .assert()
        .success();
    assert_eq!(
        read_to_string(user_root.join("demo/invocation")).expect("recorded invocation"),
        "target-1\n"
    );

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "help",
            "extension",
            "demo",
            "inventory",
            "list",
        ])
        .assert()
        .success()
        .stdout(contains("List demo inventory"))
        .stdout(contains("--state"));

    cargo_bin_cmd!("hubuum-cli")
        .env("HUBUUM_CLI__SERVER__PASSWORD", "must-not-leak")
        .env("HUBUUM_CLI__CACHE__TIME", "123")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "demo",
            "inventory",
            "list",
            "--state",
            "active",
            "|",
            "C",
        ])
        .assert()
        .success()
        .stdout("2\n");
    assert_eq!(
        read_to_string(user_root.join("demo/invocation")).expect("recorded invocation"),
        "inventory list --state active\n"
    );

    write(user_root.join("demo/invocation"), "not-called\n").expect("reset invocation");
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "demo",
            "inventory",
            "list",
            "--state",
            "unknown",
        ])
        .assert()
        .failure()
        .stdout(contains("unsupported value"));
    assert_eq!(
        read_to_string(user_root.join("demo/invocation")).expect("untouched invocation"),
        "not-called\n"
    );

    let upgraded = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "upgrade",
                source_v2.to_str().expect("source path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(upgraded["status"], "upgraded");
    assert_eq!(upgraded["from"], "0.1.0");
    assert_eq!(upgraded["to"], "0.2.0");

    let disabled = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "disable",
                "demo",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(disabled["state"], "disabled");

    let list = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "list",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(list[0]["state"], "disabled");

    let enabled = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "enable",
                "demo",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(enabled["state"], "enabled");

    let removed = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "remove",
                "demo",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(removed["status"], "removed");
    assert!(!user_root.join("demo").exists());
    assert!(user_root.join(".trash").is_dir());
}

#[test]
fn doctor_reports_invalid_manifests_without_loading_them() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    let broken = user_root.join("broken");
    create_dir_all(&broken).expect("broken package");
    write(
        broken.join("hubuum-extension.toml"),
        "schema_version = 999\nname = 'broken'\n",
    )
    .expect("broken manifest");
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let doctor = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "doctor",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(doctor[0]["code"], "manifest_invalid");
    assert_eq!(doctor[0]["severity"], "error");
}

#[test]
fn manifest_only_workflows_need_no_executable() {
    let temporary = tempdir().expect("temporary directory");
    let source_root = temporary.path().join("sources");
    let user_root = temporary.path().join("installed");
    create_dir_all(&source_root).expect("source root");
    create_dir_all(&user_root).expect("extension root");
    write_workflow_pack(&source_root, "inventory", &["class", "list"], &[], false);
    let source = source_root.join("inventory");
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let installed = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "install",
                source.to_str().expect("source path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(installed["status"], "installed");

    let shown = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "show",
                "inventory",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(shown["state"], "enabled");
    assert!(shown["executable"].is_null());

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "help",
            "extension",
            "inventory",
            "snapshot",
        ])
        .assert()
        .success()
        .stdout(contains("Compose built-in commands"));
}

#[test]
fn offline_workflow_steps_run_without_login() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_workflow_pack(&user_root, "offline-flow", &["version"], &[], false);
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let output = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "offline-flow",
                "snapshot",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert!(output["items"]["cli_version"].is_string());
    assert!(output["items"]["target"].is_string());
}

#[test]
fn doctor_quarantines_mutating_workflows_without_capability() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_workflow_pack(&user_root, "unsafe-flow", &["object", "create"], &[], false);
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let doctor = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "doctor",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(doctor[0]["code"], "workflow_invalid");
    assert!(doctor[0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("may change state"));
}

#[test]
fn doctor_rejects_steps_missing_required_bindings() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_workflow_pack(&user_root, "invalid-flow", &["object", "create"], &[], true);
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let doctor = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "doctor",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(doctor[0]["code"], "workflow_invalid");
    assert!(doctor[0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("required input"));
}

#[test]
fn explicitly_mutating_workflow_steps_join_the_catalog() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_workflow_pack(
        &user_root,
        "suite-flow",
        &["object", "create"],
        &[
            ("name", "workflow-object"),
            ("class", "Hosts"),
            ("collection", "inventory"),
            ("description", "Created by workflow"),
        ],
        true,
    );
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let shown = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "show",
                "suite-flow",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(shown["state"], "enabled");
    assert_eq!(shown["commands"][0]["implementation"], "workflow");
    assert_eq!(shown["commands"][0]["capabilities"][0], "mutate");
    assert_eq!(shown["commands"][0]["steps"][0]["run"], "object create");
}

#[test]
fn protocol_failures_are_actionable() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_response_pack(
        &user_root,
        "malformed",
        r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"rows","value":{},"columns":[]}}"#,
        0,
    );
    write_response_pack(
        &user_root,
        "bad-exit",
        r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"message","value":"done","columns":[]}}"#,
        7,
    );
    write_response_pack(
        &user_root,
        "reported-error",
        r#"{"protocol":"hubuum-cli.extension/v1","status":"error","error":{"code":"expected_failure","message":"fixture failed","details":{"step":2}},"warnings":["partial work was retained"]}"#,
        3,
    );
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "malformed",
            "run",
        ])
        .assert()
        .failure()
        .stdout(contains("shape 'Rows' has an incompatible JSON value"));

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "bad-exit",
            "run",
        ])
        .assert()
        .failure()
        .stdout(contains("success response used nonzero exit status 7"));

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "reported-error",
            "run",
        ])
        .assert()
        .failure()
        .stdout(contains("Warning: partial work was retained"))
        .stdout(contains("[expected_failure]: fixture failed"))
        .stdout(contains("details: {\"step\":2}"));
}

#[test]
fn host_pilot_appears_in_the_real_command_catalog() {
    let temporary = tempdir().expect("temporary directory");
    let config = temporary.path().join("config.toml");
    let example_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    write_config(&config, &example_root);

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "help",
            "extension",
            "host",
            "show",
        ])
        .assert()
        .success()
        .stdout(contains("Show a Host and its physical placement"))
        .stdout(contains("--verbose"));

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "help",
            "--tree",
        ])
        .assert()
        .success()
        .stdout(contains("extension host show"))
        .stdout(contains("extension host create"))
        .stdout(contains("extension host move"))
        .stdout(contains("extension inventory snapshot"))
        .stdout(contains("extension inventory classes"));
}
