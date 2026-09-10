use std::fs::write;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use assert_cmd::assert::Assert;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::{from_str, json, Value};
use tempfile::tempdir;

fn monitor(args: &[&str], statuses: &[&str]) -> Assert {
    monitor_with_limit(args, statuses, 16 * 1024 * 1024)
}

fn monitor_with_limit(args: &[&str], statuses: &[&str], limit: usize) -> Assert {
    let directory = tempdir().unwrap();
    let receipt = directory.path().join("receipt.json");
    write(
        &receipt,
        json!({"restore_id":42, "capability":"one-time-secret", "sha256":"a".repeat(64)})
            .to_string(),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let responses: Vec<_> = statuses
        .iter()
        .map(|status| {
            let mut value: Value = from_str(include_str!("fixtures/restore.json")).unwrap();
            value["status"] = json!(status);
            value.to_string()
        })
        .collect();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        for body in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing status request");
                        sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /api/v1/restores/42/status http/1.1\r\n"));
            assert!(request.contains("x-hubuum-restore-capability: one-time-secret\r\n"));
            assert!(!request.contains("authorization:"));
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let split = args
        .iter()
        .position(|arg| *arg == "|")
        .unwrap_or(args.len());
    let result = cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", directory.path())
        .env(
            "HUBUUM_CLI__SERVER__MAX_RESPONSE_BODY_BYTES",
            limit.to_string(),
        )
        .args([
            "--protocol",
            "http",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--username",
            "invalid-after-restore",
            "--password",
            "invalid-after-restore",
            "restore",
        ])
        .args(&args[..split])
        .args(["--receipt", receipt.to_str().unwrap(), "--output", "json"])
        .args(&args[split..])
        .timeout(Duration::from_secs(10))
        .assert();
    server.join().unwrap();
    result
        .stdout(contains("one-time-secret").not())
        .stderr(contains("one-time-secret").not())
}

#[test]
fn status_works_after_credentials_are_invalidated_and_redacts_the_capability() {
    monitor(&["status"], &["succeeded"])
        .success()
        .stdout(contains("succeeded"))
        .stdout(contains("restore_capability").not());
}

#[test]
fn wait_polls_past_confirmation_without_login() {
    monitor(&["wait", "--timeout", "5"], &["confirmed", "succeeded"])
        .success()
        .stdout(contains("succeeded"));
}

#[test]
fn wait_returns_nonzero_for_failure_expiry_and_timeout() {
    for status in ["failed", "expired"] {
        monitor(&["wait"], &[status])
            .failure()
            .stdout(contains(status));
    }
    monitor(&["wait", "--timeout", "0"], &["confirmed"])
        .failure()
        .stdout(contains("not cancelled"));
}

#[test]
fn receipt_status_supports_semantic_pipelines() {
    monitor(&["status", "|", "P", "status"], &["succeeded"])
        .success()
        .stdout(contains("succeeded"))
        .stdout(contains("requested_by").not());
}

#[test]
fn configured_response_limit_controls_the_http_client() {
    monitor_with_limit(&["status"], &["succeeded"], 64).failure();
    monitor_with_limit(&["status"], &["succeeded"], 4096)
        .success()
        .stdout(contains("succeeded"));
}
