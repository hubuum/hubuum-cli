use std::fs::{read_to_string, write};
use std::io::{Read, Write as _};
use std::net::TcpListener;
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn help_and_version_do_not_require_login() {
    cargo_bin_cmd!("hubuum-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("hubuum-cli <command...>"));

    cargo_bin_cmd!("hubuum-cli")
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(format!("v{}", env!("CARGO_PKG_VERSION"))));

    cargo_bin_cmd!("hubuum-cli")
        .arg("version")
        .assert()
        .success()
        .stdout(contains(format!("v{}", env!("CARGO_PKG_VERSION"))))
        .stdout(contains("Target"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["version", "--output", "json"])
        .assert()
        .success()
        .stdout(contains("\"cli_version\""))
        .stdout(contains("\"target\""));
}

#[test]
fn direct_help_and_config_paths_do_not_require_login() {
    cargo_bin_cmd!("hubuum-cli")
        .arg("help")
        .assert()
        .success()
        .stdout(contains("Available commands"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "pipe"])
        .assert()
        .success()
        .stdout(contains("grep os_version"))
        .stdout(contains("V 129.240"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "shell"])
        .assert()
        .success()
        .stdout(contains("Type a scope name"))
        .stdout(contains("next to fetch the next page"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "paths"])
        .assert()
        .success()
        .stdout(contains("System"))
        .stdout(contains("User"))
        .stdout(contains("Write"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "auth", "providers"])
        .assert()
        .success()
        .stdout(contains("without logging in"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "admin", "config"])
        .assert()
        .success()
        .stdout(contains("Secrets are redacted"));
}

#[test]
fn personal_aliases_expand_before_offline_detection_and_pipelines() {
    let directory = tempdir().expect("temporary directory");
    let config = directory.path().join("config.toml");
    write(
        &config,
        r#"
[aliases.quick-help]
command = "help | F Available"
description = "Show available top-level commands"
"#,
    )
    .expect("alias config should be written");

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("UTF-8 config path"),
            "quick-help",
            "|",
            "C",
        ])
        .assert()
        .success()
        .stdout(contains("1"));

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--config",
            config.to_str().expect("UTF-8 config path"),
            "help",
            "quick-help",
        ])
        .assert()
        .success()
        .stdout(contains("Show available top-level commands"))
        .stdout(contains("help | F Available"));
}

#[test]
fn metrics_uses_the_configured_path_without_authentication() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("metrics listener should bind");
    let port = listener
        .local_addr()
        .expect("metrics listener should have an address")
        .port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("metrics request should arrive");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream
                .read(&mut buffer)
                .expect("request should be readable");
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let request = String::from_utf8(request).expect("request should be UTF-8");
        assert!(request.starts_with("GET /internal/metrics HTTP/1.1\r\n"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));

        let body = "# TYPE hubuum_up gauge\nhubuum_up 1\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("metrics response should be written");
    });

    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--protocol",
            "http",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "metrics",
            "--path",
            "/internal/metrics",
        ])
        .assert()
        .success()
        .stdout(contains("# TYPE hubuum_up gauge"))
        .stdout(contains("hubuum_up 1"));

    server.join().expect("metrics server should finish");
}

#[test]
fn unreachable_server_fails_before_password_prompt() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener
        .local_addr()
        .expect("temporary listener should have an address")
        .port();
    drop(listener);
    let directory = tempdir().expect("temporary directory");

    cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .env("XDG_DATA_HOME", directory.path())
        .args([
            "--protocol",
            "http",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--username",
            "admin",
        ])
        .assert()
        .failure()
        .stderr(contains("ServerUnreachable").and(contains("Password for").not()));
}

#[test]
fn theme_preview_includes_a_dense_banded_table() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--color", "never", "theme", "preview", "rose-pink"])
        .assert()
        .success()
        .stdout(contains("Dense table with alternating row bands"))
        .stdout(contains("Name            | os_version   | status"))
        .stdout(contains("edge-gateway-01"))
        .stdout(contains("lab-console-07"));
}

#[test]
fn direct_command_redirects_to_an_unstyled_file() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("help.txt");

    cargo_bin_cmd!("hubuum-cli")
        .args(["help", ">", path.to_str().expect("UTF-8 path")])
        .assert()
        .success();

    let output = read_to_string(path).expect("redirected help");
    assert!(output.contains("Available commands"));
    assert!(!output.contains('\x1b'));
}

#[test]
fn direct_command_supports_each_redirects() {
    let dir = tempdir().expect("tempdir");
    let template = format!("each:{}/{{n}}.txt", dir.path().display());

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", ">", &template])
        .assert()
        .success();

    let first = read_to_string(dir.path().join("1.txt")).expect("first per-item redirect output");
    assert!(first.contains("key"));
}

#[test]
fn script_applies_successful_redirects_before_a_later_failure() {
    let dir = tempdir().expect("tempdir");
    let redirected = dir.path().join("help.txt");
    let script = dir.path().join("commands.hubuum");
    write(
        &script,
        format!(
            "help > {}\nhelp definitely-not-a-command\n",
            redirected.display()
        ),
    )
    .expect("script should be written");

    cargo_bin_cmd!("hubuum-cli")
        .args(["script", script.to_str().expect("UTF-8 script path")])
        .assert()
        .failure();

    let output =
        read_to_string(redirected).expect("the first command's redirect should already exist");
    assert!(output.contains("Available commands"));
}

#[test]
fn hidden_command_alias_still_works() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "help"])
        .assert()
        .success()
        .stdout(contains("Available commands"));
}

#[test]
fn hidden_command_alias_supports_pipeline_stages() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "help | grep Available | count"])
        .assert()
        .success()
        .stdout(contains("1"));
}

#[test]
fn direct_command_errors_exit_nonzero() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["help", "definitely-not-a-command"])
        .assert()
        .failure()
        .stdout(contains("Command not found"));
}

#[test]
fn offline_config_show_supports_semantic_output_formats() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--output", "csv"])
        .assert()
        .success()
        .stdout(contains("key,value,source,detail"))
        .stdout(contains("output.format"));

    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--output", "jsonl"])
        .assert()
        .success()
        .stdout(contains("\"key\":\"output.format\""));
}

#[test]
fn offline_config_show_supports_semantic_pipeline_projection() {
    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!("config show --output {format} | F output | P key value | S key | L 1"),
            ])
            .assert()
            .success()
            .stdout(contains("key"))
            .stdout(contains("output."));
    }
}

#[test]
fn structured_pipeline_semantics_are_stable_across_renderers() {
    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!(
                    "config show --output {format} | JQ '[{{\"g\":\"x\",\"v\":2}},{{\"g\":\"x\",\"v\":1}}]' | F v>=1 | G g | A sum(v) AS total | S total AS num | Z | P g total | VALUE total"
                ),
            ])
            .assert()
            .success()
            .stdout(contains('3'));
    }
}

#[test]
fn grouped_summary_filters_keep_aggregates_consistent_across_renderers() {
    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!(
                    "config show --output {format} | JQ '[{{\"g\":\"x\",\"v\":1}},{{\"g\":\"x\",\"v\":2}},{{\"g\":\"y\",\"v\":3}}]' | G g | A count AS n | F n>=2 | A sum(v) AS total | Z"
                ),
            ])
            .assert()
            .success()
            .stdout(contains('x'))
            .stdout(contains('2'))
            .stdout(contains('3'))
            .stdout(contains('y').not());
    }
}

#[test]
fn detail_and_list_pipelines_are_stable_across_renderers() {
    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!("config show --key output.format --output {format} | P key value"),
            ])
            .assert()
            .success()
            .stdout(contains("output.format"));

        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!("theme list --output {format} | P name | L 1"),
            ])
            .assert()
            .success()
            .stdout(contains("name"));
    }
}

#[test]
fn duplicate_pipeline_output_names_fail_for_every_renderer_and_each_redirect() {
    for format in ["text", "json", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .args([
                "--command",
                &format!("config show --output {format} | P key key"),
            ])
            .assert()
            .failure()
            .stdout(contains("stage 'P' has duplicate output column 'key'"));
    }

    let directory = tempdir().expect("temporary directory");
    let target = directory.path().join("{key}.json");
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--command",
            &format!("config show | P key key > each:{}", target.display()),
        ])
        .assert()
        .failure()
        .stdout(contains("stage 'P' has duplicate output column 'key'"));
    assert_eq!(
        directory
            .path()
            .read_dir()
            .expect("temporary directory")
            .count(),
        0
    );
}

#[test]
fn offline_config_show_supports_documented_jq_transforms() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["--command", "config show | JQ 'map({key, value})' | L 1"])
        .assert()
        .success()
        .stdout(contains("key"))
        .stdout(contains("value"));
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
        .stdout(contains(" | "))
        .stdout(contains("key"))
        .stdout(contains("value"));
}

#[test]
fn json_alias_conflicts_with_non_json_output() {
    cargo_bin_cmd!("hubuum-cli")
        .args(["config", "show", "--json", "--output", "csv"])
        .assert()
        .failure()
        .stdout(contains("--json conflicts"));
}
