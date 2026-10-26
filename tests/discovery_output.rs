use std::fs::{read, write};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{json, Value};
use tempfile::tempdir;

fn discovery_output(empty: bool, redirect: bool, debug: bool) {
    let directory = tempdir().unwrap();
    let token = directory.path().join("token");
    write(&token, "fixture-token").unwrap();
    let config = directory.path().join("config.toml");
    write(&config, "[completion]\ndisable_api_related = true\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        for index in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                header.push(byte[0]);
            }
            let body = if index < 2 {
                json!({})
            } else {
                assert!(String::from_utf8(header).unwrap().starts_with("GET /api/v1/classes/by-name/Hosts/fields?"));
                json!({"sampled":true,"sampled_objects":1,"effective_sample_limit":1,
                    "max_depth":6,"include_containers":false,"truncation":["sample_limit"],
                    "fields": if empty { json!([]) } else { json!([{
                        "field":"data.hostname","source":"data","path":[{"kind":"property","name":"hostname"}],
                        "observation_count":1,"object_count":1,"types":{"string":1},
                        "example":"host","example_truncated":false,"error_count":0
                    }]) }})
            }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let destination = directory.path().join("fields.json");
    let mut command = cargo_bin_cmd!("hubuum-cli");
    command
        .env("XDG_CONFIG_HOME", directory.path())
        .env("XDG_DATA_HOME", directory.path())
        .args([
            "--config",
            config.to_str().unwrap(),
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--protocol",
            "http",
            "--token-file",
            token.to_str().unwrap(),
            "class",
            "fields",
            "Hosts",
            "--limit",
            "1",
            "--output",
            "json",
        ])
        .timeout(Duration::from_secs(15));
    if debug {
        command.arg("--debug");
    }
    if redirect {
        command.args([">", destination.to_str().unwrap()]);
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    server.join().unwrap();
    let payload = if redirect {
        assert!(result.stdout.is_empty());
        read(destination).unwrap()
    } else {
        result.stdout
    };
    let response: Value = serde_json::from_slice(&payload).unwrap();
    let rows = if debug {
        assert!(result.stderr.is_empty());
        assert!(response["warnings"][0]
            .as_str()
            .unwrap()
            .contains("sample_limit"));
        &response["data"]
    } else {
        assert!(String::from_utf8_lossy(&result.stderr).contains("sample_limit"));
        &response
    };
    assert_eq!(rows.as_array().unwrap().len(), usize::from(!empty));
}

#[test]
fn truncated_discovery_keeps_stdout_json_and_warns_on_stderr() {
    discovery_output(false, false, false);
}

#[test]
fn empty_truncated_discovery_redirect_keeps_json_and_stderr_warning() {
    discovery_output(true, true, false);
}

#[test]
fn debug_discovery_embeds_warning_without_duplicate_stderr() {
    discovery_output(false, false, true);
}
