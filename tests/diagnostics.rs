use std::fs::{read_to_string, write};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use serde_json::{json, Value};
use tempfile::tempdir;

fn assert_report(report: &Value, calls: u64, attempts: u64) {
    assert_eq!(report["version"], 1);
    assert!(report["duration_ms"].as_f64().unwrap() >= 0.0);
    assert_eq!(report["client_api_calls"], calls);
    assert_eq!(report["http_attempts"], attempts);
    for name in ["execute", "render"] {
        let stage = report["spans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|span| span["name"] == name)
            .unwrap();
        assert_eq!(stage["kind"], "stage");
        assert!(stage["duration_ms"].as_f64().unwrap() >= 0.0);
    }
}

#[test]
fn debug_json_wraps_data_and_redirects_without_leaking_between_script_commands() {
    let directory = tempdir().unwrap();
    let baseline = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args(["version", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let baseline: Value = serde_json::from_slice(&baseline).unwrap();
    for arguments in [
        vec!["--debug", "version", "--output", "json"],
        vec!["version", "--debug", "--output", "json"],
        vec!["version", "--debug", "--json"],
        vec!["--debug", "--command", "version --output json"],
    ] {
        let output = cargo_bin_cmd!("hubuum-cli")
            .env("XDG_CONFIG_HOME", directory.path())
            .env("RUST_LOG", "off")
            .args(arguments)
            .assert()
            .success()
            .stderr("")
            .get_output()
            .clone();
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["data"], baseline);
        assert_report(&response["debug"], 0, 0);
    }

    let destination = directory.path().join("result.json");
    let script = directory.path().join("commands.txt");
    write(&script, format!(
        "version --debug --output json > {}\nversion --output json\nversion --debug --output json > each:{}/item-{{n}}.json\n",
        destination.display(), directory.path().display()
    )).unwrap();
    let output = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .arg("script")
        .arg(&script)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .clone();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response, baseline);
    for path in [destination, directory.path().join("item-1.json")] {
        let response: Value = serde_json::from_str(&read_to_string(path).unwrap()).unwrap();
        assert_eq!(response["data"], baseline);
        assert_report(&response["debug"], 0, 0);
    }
}

#[test]
fn json_debug_is_added_after_pipeline_transforms_and_preserves_payload_fields() {
    let directory = tempdir().unwrap();
    for expression in ["42", "null", "[]", r#"[{"debug":"payload"}, null, 3]"#] {
        let command = format!("theme list --output json | JQ '{expression}'");
        let mut results = Vec::new();
        for debug in [false, true] {
            let mut process = cargo_bin_cmd!("hubuum-cli");
            process.env("XDG_CONFIG_HOME", directory.path());
            if debug {
                process.arg("--debug");
            }
            let output = process
                .args(["--command", &command])
                .assert()
                .success()
                .stderr("")
                .get_output()
                .stdout
                .clone();
            results.push(serde_json::from_slice::<Value>(&output).unwrap());
        }
        assert_eq!(results[1]["data"], results[0]);
        assert_report(&results[1]["debug"], 0, 0);
    }
}

#[test]
fn debug_keeps_text_and_other_renderers_on_stderr() {
    let directory = tempdir().unwrap();
    for format in ["text", "jsonl", "csv", "tsv"] {
        cargo_bin_cmd!("hubuum-cli")
            .env("XDG_CONFIG_HOME", directory.path())
            .args(["version", "--debug", "--output", format])
            .assert()
            .success()
            .stderr(contains("0 client API calls"))
            .stderr(contains("  execute:"))
            .stderr(contains("  render:"));
    }
}

#[test]
fn debug_json_includes_redirect_errors_in_the_response() {
    let directory = tempdir().unwrap();
    let command = format!(
        "version --debug --output json > {}/missing/file.json",
        directory.path().display()
    );
    let output = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args(["--command", &command])
        .assert()
        .failure()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let response: Value = serde_json::from_slice(&output).unwrap();
    assert!(response["data"].is_null());
    assert!(!response["errors"].as_array().unwrap().is_empty());
    assert_report(&response["debug"], 0, 0);
}

#[test]
fn pipeline_failures_keep_diagnostics_in_the_json_response() {
    let directory = tempdir().unwrap();
    let output = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .args(["--command", "version --debug --output json | JQ '42'"])
        .assert()
        .failure()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let response: Value = serde_json::from_slice(&output).unwrap();
    assert!(response["data"].is_null());
    assert!(response["errors"][0]
        .as_str()
        .unwrap()
        .contains("does not accept Lines"));
    assert_report(&response["debug"], 0, 0);
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
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        let report = &response["debug"];
        assert_report(report, 2, 2);
        let spans = report["spans"].as_array().unwrap();
        let calls: Vec<_> = spans
            .iter()
            .filter(|span| span["kind"] == "client_call")
            .collect();
        assert_eq!(calls[0]["name"], "classes.get_by_name");
        assert_eq!(calls[1]["name"], "QueryOp.page");
        assert_eq!(calls[1]["succeeded"], !fail);
        let attempts: Vec<_> = spans
            .iter()
            .filter(|span| span["name"] == "hubuum.http_attempt")
            .collect();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["client"]["status"], 200);
        assert_eq!(
            attempts[1]["client"]["status"],
            if fail { 403 } else { 200 }
        );
        assert_eq!(
            attempts[1]["client"]["outcome"],
            if fail { "failed" } else { "ok" }
        );
        assert!(!report.to_string().contains("private"), "{report}");
        if fail {
            assert!(response["data"].is_null());
            assert!(!response["errors"].as_array().unwrap().is_empty());
        } else {
            assert_eq!(response["data"], json!([]));
        }
        server.join().unwrap();
    }
}
