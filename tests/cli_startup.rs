use std::fs::{read_to_string, write};
use std::io::{Read, Write as _};
use std::net::TcpListener;
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
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
    cargo_bin_cmd!("hubuum-cli")
        .args([
            "--command",
            "config show | F output | P key value | S key | L 1",
        ])
        .assert()
        .success()
        .stdout(contains("key"))
        .stdout(contains("output."));
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
