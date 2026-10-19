use std::fs::{read_to_string, write};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use serde_json::{json, Value};
use tempfile::tempdir;

#[test]
fn debug_flags_keep_json_and_redirects_clean_and_reset_between_script_commands() {
    let directory = tempdir().unwrap();
    for arguments in [
        vec!["--debug", "version", "--output", "json"],
        vec!["version", "--debug", "--output", "json"],
        vec!["--debug", "--command", "version --output json"],
    ] {
        let output = cargo_bin_cmd!("hubuum-cli")
            .env("XDG_CONFIG_HOME", directory.path())
            .env("RUST_LOG", "off")
            .args(arguments)
            .assert()
            .success()
            .stderr(contains("0 client API calls"))
            .stderr(contains("  execute:"))
            .stderr(contains("  render:"))
            .get_output()
            .clone();
        let _: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            String::from_utf8(output.stderr)
                .unwrap()
                .matches("Debug:")
                .count(),
            1
        );
    }

    let destination = directory.path().join("result.json");
    let script = directory.path().join("commands.txt");
    write(
        &script,
        format!(
            "version --debug --output json > {}\nversion --output json\n",
            destination.display()
        ),
    )
    .unwrap();
    let output = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .arg("script")
        .arg(&script)
        .assert()
        .success()
        .get_output()
        .clone();
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
    let _: Value = serde_json::from_str(&read_to_string(destination).unwrap()).unwrap();
    assert_eq!(
        String::from_utf8(output.stderr)
            .unwrap()
            .matches("Debug:")
            .count(),
        1
    );
}

#[test]
fn debug_records_client_calls_and_failures_without_counting_startup_or_leaking_arguments() {
    for fail in [false, true] {
        let directory = tempdir().unwrap();
        let token = directory.path().join("token");
        write(&token, "private-token").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    assert_eq!(stream.read(&mut byte).unwrap(), 1);
                    request.push(byte[0]);
                }
                let (status, body) = match index {
                    0 | 1 => ("200 OK", json!({})),
                    2 => (
                        "200 OK",
                        json!({
                            "id":9,"name":"private-class","description":"",
                            "collection_id":1,"json_schema":null,"validate_schema":null,
                            "revision":1,"created_at":"2026-10-19T00:00:00Z",
                            "updated_at":"2026-10-19T00:00:00Z"
                        }),
                    ),
                    _ if fail => ("403 Forbidden", json!({"message":"private-error"})),
                    _ => ("200 OK", json!([])),
                };
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let output = cargo_bin_cmd!("hubuum-cli")
            .env("XDG_CONFIG_HOME", directory.path())
            .env("XDG_DATA_HOME", directory.path())
            .env("RUST_LOG", "off")
            .args([
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--protocol",
                "http",
                "--token-file",
                token.to_str().unwrap(),
            ])
            .args([
                "object",
                "list",
                "--class",
                "private-class",
                "--output",
                "json",
                "--debug",
            ])
            .timeout(Duration::from_secs(15))
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail,
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let report = String::from_utf8(output.stderr).unwrap();
        assert!(report.contains("2 client API calls"), "{report}");
        assert!(report.contains("2 HTTP attempts"), "{report}");
        assert!(report.contains("HTTP GET attempt 1:"), "{report}");
        assert!(report.contains("[status 200]"), "{report}");
        assert_eq!(report.contains("[status 403]"), fail, "{report}");
        assert!(report.contains("1. classes.get_by_name:"), "{report}");
        assert!(report.contains("2. QueryOp.page:"), "{report}");
        assert_eq!(report.contains("(failed)"), fail, "{report}");
        assert!(!report.contains("private"), "{report}");
        if !fail {
            let _: Value = serde_json::from_slice(&output.stdout).unwrap();
        }
        server.join().unwrap();
    }
}
