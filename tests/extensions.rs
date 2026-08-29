#![cfg(unix)]

use std::fs::{create_dir_all, read_to_string, set_permissions, write, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use hubuum_extension_protocol::ExtensionManifest;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::{json, Value};
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
    let manifest = json!({
        "schema_version": 1,
        "kind": "executable",
        "name": "demo",
        "version": version,
        "requires_cli": ">=0.0.9,<0.1",
        "protocol": "hubuum-cli.extension/v1",
        "executable": "bin/demo",
        "config": {
            "label": { "type": "string", "required": true }
        },
        "commands": {
            "demo": {
                "path": ["demo"],
                "about": "Run a command sharing the pack name",
                "options": {
                    "target": { "kind": "string", "position": 1, "required": true }
                }
            },
            "inventory_list": {
                "path": ["inventory", "list"],
                "arguments": ["inventory", "list"],
                "about": "List demo inventory",
                "options": {
                    "state": {
                        "kind": "string",
                        "long": "state",
                        "help": "Inventory state",
                        "values": ["active", "retired"]
                    }
                }
            }
        }
    });
    write(
        package.join("hubuum-extension.jsonc"),
        serde_json::to_string_pretty(&manifest).expect("serialize extension manifest"),
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
    let manifest = json!({
        "schema_version": 1,
        "kind": "executable",
        "name": name,
        "version": "0.1.0",
        "requires_cli": ">=0.0.9,<0.1",
        "protocol": "hubuum-cli.extension/v1",
        "executable": "bin/run",
        "commands": {
            "run": { "path": ["run"], "about": "Run protocol fixture" }
        }
    });
    write(
        package.join("hubuum-extension.jsonc"),
        serde_json::to_string_pretty(&manifest).expect("serialize extension manifest"),
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
        .map(|segment| Value::String((*segment).to_string()))
        .collect::<Vec<_>>();
    let capabilities = if allows_mutation {
        vec!["mutate"]
    } else {
        Vec::new()
    };
    let bindings = bindings
        .iter()
        .map(|(name, value)| ((*name).to_string(), Value::String((*value).to_string())))
        .collect::<serde_json::Map<_, _>>();
    let manifest = json!({
        "schema_version": 1,
        "kind": "portable",
        "name": name,
        "version": "0.1.0",
        "requires_cli": ">=0.0.9,<0.1",
        "workflows": {
            "snapshot": {
                "output": { "shape": "detail", "type": "json" },
                "steps": [{
                    "id": "items",
                    "kind": "run",
                    "run": command,
                    "with": bindings
                }],
                "capabilities": capabilities,
                "result": "{ items: .steps.items }"
            }
        },
        "commands": {
            "snapshot": {
                "path": ["snapshot"],
                "workflow": "snapshot",
                "about": "Compose built-in commands"
            }
        }
    });
    write(
        package.join("hubuum-extension.jsonc"),
        serde_json::to_string_pretty(&manifest).expect("serialize extension manifest"),
    )
    .expect("workflow manifest");
}

fn write_composable_workflow_pack(root: &Path) -> PathBuf {
    let package = root.join("composable");
    create_dir_all(&package).expect("package directory");
    write(
        package.join("hubuum-extension.jsonc"),
        r#"{
  "schema_version": 1,
  "kind": "portable",
  "name": "composable",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "workflows": {
    "capture": {
      "inputs": { "item": { "type": "json", "required": true } },
      "output": { "shape": "values", "type": "json" },
      "steps": [{ "id": "value", "kind": "let", "expr": ".input.item" }],
      "result": "[.input.item]"
    },
    "compose": {
      "inputs": {
        "enabled": { "type": "boolean", "default": true },
        "items": { "type": "json", "default": ["alpha", "beta"] }
      },
      "output": {
        "shape": "detail",
        "type": "json",
        "columns": ["one", "many", "skipped"]
      },
      "steps": [
        { "id": "seed", "kind": "let", "expr": ".input.items" },
        {
          "id": "valid",
          "kind": "assert",
          "condition": "(.input.items | length) == 2",
          "message": "two items are required"
        },
        {
          "id": "one",
          "kind": "call",
          "call": "capture",
          "when": ".input.enabled",
          "with": { "item": "single" }
        },
        {
          "id": "many",
          "kind": "for_each",
          "items": { "step": "seed" },
          "as": "item",
          "call": "capture",
          "max_items": 2,
          "when": ".input.enabled"
        },
        {
          "id": "skipped",
          "kind": "run",
          "run": ["version"],
          "when": ".input.enabled == false"
        }
      ],
      "result": "{ one: .steps.one[0], many: [.steps.many[][0]], skipped: .steps.skipped }"
    }
  },
  "commands": {
    "compose": {
      "path": ["compose"],
      "workflow": "compose",
      "about": "Exercise portable workflow composition",
      "options": {
        "enabled": { "kind": "boolean", "long": "enabled" },
        "items": { "kind": "json", "long": "items" }
      }
    }
  }
}"#,
    )
    .expect("workflow manifest");
    package
}

fn json_stdout(assertion: assert_cmd::assert::Assert) -> Value {
    let output = assertion.success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("JSON command output")
}

#[test]
fn built_in_workflow_contracts_are_discoverable() {
    let contract = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "contract",
                "object",
                "list",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(contract["command"], "object list");
    assert_eq!(contract["effects"], "read_only");
    assert_eq!(contract["reauthentication_retry"], "safe");
    assert_eq!(contract["step_output"]["shape"], "runtime");
    let inputs = contract["inputs"].as_array().expect("contract inputs");
    let where_input = inputs
        .iter()
        .find(|input| input["id"] == "where")
        .expect("where input");
    assert_eq!(where_input["cardinality"]["kind"], "repeated_fixed");
    assert_eq!(where_input["cardinality"]["count"], 3);
    assert!(!inputs.iter().any(|input| input["id"] == "output"));

    let listed = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args(["extension", "contract", "--list", "--output", "json"])
            .assert(),
    );
    assert!(listed
        .as_array()
        .expect("contract list")
        .iter()
        .any(|command| command["command"] == "object list"));
    assert!(!listed
        .as_array()
        .expect("contract list")
        .iter()
        .any(|command| command["command"] == "extension validate"));

    let mutation = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "contract",
                "relation",
                "object",
                "create",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(mutation["effects"], "mutating");
    assert_eq!(mutation["reauthentication_retry"], "unsafe");
}

#[test]
fn extension_management_help_examples_are_not_duplicated() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "extension", "validate"])
        .assert()
        .success()
        .stdout(contains("extension validate ./my-pack"))
        .stdout(contains("extension validate extension validate").not());
}

#[test]
fn init_creates_valid_starter_templates_without_overwriting() {
    let temporary = tempdir().expect("temporary directory");
    for template in ["minimal", "read-only", "executable"] {
        let target = temporary.path().join(template);
        let created = json_stdout(
            cargo_bin_cmd!("hubuum-cli")
                .args([
                    "extension",
                    "init",
                    target.to_str().expect("target path"),
                    "--template",
                    template,
                    "--output",
                    "json",
                ])
                .assert(),
        );
        assert_eq!(created["status"], "created");
        assert_eq!(created["template"], template);
        let source =
            read_to_string(target.join("hubuum-extension.jsonc")).expect("generated manifest");
        let manifest = ExtensionManifest::parse(&source).expect("generated manifest parses");
        assert_eq!(manifest.name().as_str(), template);

        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "validate",
                target.to_str().expect("target path"),
            ])
            .assert()
            .success();

        cargo_bin_cmd!("hubuum-cli")
            .args(["extension", "init", target.to_str().expect("target path")])
            .assert()
            .failure()
            .stdout(contains("already exists"));
    }
}

#[test]
fn tutorial_manifest_is_kept_valid_by_ci() {
    let tutorial = include_str!("../docs/extension-tutorial.md");
    let marked = tutorial
        .split_once("<!-- extension-manifest-example-start -->")
        .expect("tutorial example start")
        .1
        .split_once("<!-- extension-manifest-example-end -->")
        .expect("tutorial example end")
        .0
        .trim();
    let manifest = marked
        .strip_prefix("```jsonc\n")
        .and_then(|value| value.strip_suffix("\n```"))
        .expect("one fenced JSONC manifest");
    ExtensionManifest::parse(manifest).expect("tutorial manifest parses");

    let temporary = tempdir().expect("temporary directory");
    write(temporary.path().join("hubuum-extension.jsonc"), manifest)
        .expect("tutorial manifest fixture");
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "extension",
            "validate",
            temporary.path().to_str().expect("fixture path"),
        ])
        .assert()
        .success();
}

#[test]
fn bundled_manifest_workflow_uses_the_current_language() {
    let manifest = ExtensionManifest::parse(include_str!(
        "../examples/hubuum-inventory/hubuum-extension.jsonc"
    ))
    .expect("bundled workflow manifest");
    let snapshot_name = manifest
        .commands()
        .iter()
        .filter_map(|command| command.workflow())
        .find(|workflow| workflow.as_str() == "snapshot")
        .expect("snapshot workflow name");
    let snapshot = manifest.workflow(snapshot_name).expect("snapshot workflow");

    assert_eq!(snapshot.steps().len(), 3);
    assert!(snapshot.result().contains("hosts"));

    let jacks = ExtensionManifest::parse(include_str!(
        "../examples/hubuum-jacks/hubuum-extension.jsonc"
    ))
    .expect("bundled Jacks workflow manifest");
    let jack_hosts_name = jacks
        .commands()
        .iter()
        .filter_map(|command| command.workflow())
        .find(|workflow| workflow.as_str() == "jack_hosts")
        .expect("Jack Hosts workflow name");
    let jack_hosts = jacks
        .workflow(jack_hosts_name)
        .expect("Jack Hosts workflow");

    assert_eq!(jack_hosts.steps().len(), 2);
    assert_eq!(jack_hosts.steps()[0].id().as_str(), "ignored_classes");
    assert_eq!(jack_hosts.steps()[1].id().as_str(), "hosts");
}

#[test]
fn bundled_placement_extension_compiles_and_explains_composition() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("hubuum-placement");

    let validated = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "validate",
                package.to_str().expect("placement package path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(validated["status"], "valid");
    assert_eq!(validated["name"], "placement");
    assert_eq!(validated["kind"], "portable");
    assert_eq!(validated["workflow_plan"]["workflow_count"], 24);

    let explained = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "explain",
                package.to_str().expect("placement package path"),
                "--workflow",
                "host_move",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(explained["plan"]["workflows"][0]["name"], "host_move");
    assert_eq!(explained["plan"]["workflows"][0]["effects"], "mutating");
    assert_eq!(explained["plan"]["workflows"][0]["call_depth"], 3);
    assert_eq!(
        explained["plan"]["workflows"][0]["steps"][3]["kind"],
        "for_each"
    );
    assert_eq!(
        explained["plan"]["workflows"][0]["steps"][3]["max_items"],
        100
    );
    assert_eq!(
        explained["plan"]["workflows"][0]["steps"][3]["items"]["step"],
        "jack_names"
    );
    assert_eq!(
        explained["plan"]["workflows"][0]["steps"][3]["with"]["host"]["input"],
        "host"
    );
    assert!(explained["plan"]["workflows"][0]["result"]
        .as_str()
        .is_some_and(|result| result.contains("target_jack")));
}

#[test]
fn bundled_recipe_extension_compiles_every_language_form() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("hubuum-recipes");
    let validated = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "extension",
                "validate",
                package.to_str().expect("recipe package path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(validated["status"], "valid");
    assert_eq!(validated["workflow_plan"]["workflow_count"], 3);
    let workflows = validated["workflow_plan"]["workflows"]
        .as_array()
        .expect("workflow plans");
    let tour = workflows
        .iter()
        .find(|workflow| workflow["name"] == "tour")
        .expect("tour workflow");
    assert_eq!(tour["call_depth"], 2);
    assert_eq!(tour["worst_case_operations"], 38);
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

    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "--command",
                &format!(
                    "extension demo inventory list --state active --output {format} | P name | L 1"
                ),
            ])
            .assert()
            .success()
            .stdout(contains("name"))
            .stdout(contains("alpha"));
    }

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
fn extension_message_pipelines_are_stable_across_renderers() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    create_dir_all(&user_root).expect("extension root");
    write_response_pack(
        &user_root,
        "message",
        r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"message","value":"ready","columns":[]},"warnings":[]}"#,
        0,
    );
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "--command",
                &format!("extension message run --output {format} | F ready"),
            ])
            .assert()
            .success()
            .stdout(contains("ready"));
    }
}

#[test]
fn doctor_reports_invalid_manifests_without_loading_them() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    let broken = user_root.join("broken");
    create_dir_all(&broken).expect("broken package");
    write(
        broken.join("hubuum-extension.jsonc"),
        r#"{ "schema_version": 999, "name": "broken" }"#,
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
fn portable_workflows_compose_with_calls_iteration_conditions_and_assertions() {
    let temporary = tempdir().expect("temporary directory");
    let sources = temporary.path().join("sources");
    let user_root = temporary.path().join("installed");
    create_dir_all(&sources).expect("source root");
    let source = write_composable_workflow_pack(&sources);
    let config = temporary.path().join("config.toml");
    write_config(&config, &user_root);

    let validated = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "validate",
                source.to_str().expect("source path"),
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(validated["status"], "valid");
    assert_eq!(validated["kind"], "portable");

    let explained = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "explain",
                source.to_str().expect("source path"),
                "--workflow",
                "compose",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(explained["plan"]["workflows"][0]["name"], "compose");
    assert_eq!(explained["plan"]["workflows"][0]["call_depth"], 2);

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("config path"),
            "extension",
            "install",
            source.to_str().expect("source path"),
        ])
        .assert()
        .success();

    let output = json_stdout(
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--config",
                config.to_str().expect("config path"),
                "extension",
                "composable",
                "compose",
                "--output",
                "json",
            ])
            .assert(),
    );
    assert_eq!(
        output,
        json!({
            "one": "single",
            "many": ["alpha", "beta"],
            "skipped": null,
        })
    );
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
fn doctor_rejects_incompatible_workflow_input_types() {
    let temporary = tempdir().expect("temporary directory");
    let user_root = temporary.path().join("installed");
    let package = user_root.join("typed-flow");
    create_dir_all(&package).expect("workflow package");
    write(
        package.join("hubuum-extension.jsonc"),
        r#"{
  "schema_version": 1,
  "kind": "portable",
  "name": "typed-flow",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "workflows": {
    "snapshot": {
      "inputs": { "depth": { "type": "string" } },
      "output": { "shape": "detail", "type": "json" },
      "steps": [{
        "id": "object",
        "kind": "run",
        "run": ["object", "show"],
        "with": {
          "name": "server-01",
          "class": "Hosts",
          "max-depth": { "input": "depth" }
        }
      }],
      "result": ".steps.object"
    }
  },
  "commands": {
    "snapshot": {
      "path": ["snapshot"],
      "workflow": "snapshot",
      "options": {
        "depth": { "kind": "string", "long": "depth" }
      }
    }
  }
}"#,
    )
    .expect("workflow manifest");
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
        .contains("declared type 'string'"));
    assert!(doctor[0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("target type Integer"));
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
    assert_eq!(
        shown["workflow_plan"]["workflows"][0]["effects"],
        "mutating"
    );
    assert_eq!(
        shown["workflow_plan"]["workflows"][0]["steps"][0]["run"],
        "object create"
    );
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
fn bundled_extension_examples_appear_in_the_real_command_catalog() {
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
            "extension",
            "placement",
            "host",
            "move",
        ])
        .assert()
        .success()
        .stdout(contains("Preview or apply moving a Host to one Jack"))
        .stdout(contains("--apply"));

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
        .stdout(contains("extension inventory classes"))
        .stdout(contains("extension jacks hosts"))
        .stdout(contains("extension jacks rooms"))
        .stdout(contains("extension placement host placement"))
        .stdout(contains("extension placement jack connect-room"))
        .stdout(contains("extension placement room jacks"));

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
        .stdout(contains("extension inventory snapshot --output json"))
        .stdout(contains("extension inventory snapshot extension inventory").not());
}
