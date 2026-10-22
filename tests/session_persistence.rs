#![cfg(target_os = "linux")]

use std::fs::{read_to_string, write};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, sleep};
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{json, Value};
use tempfile::tempdir;

fn request(stream: &mut TcpStream) -> (String, String, Value) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1);
        bytes.push(byte[0]);
    }
    let header = String::from_utf8(bytes).unwrap();
    let length = header
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .map(|n| n.parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    let path = header.split_whitespace().nth(1).unwrap().to_owned();
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (path, header.to_ascii_lowercase(), body)
}

#[test]
fn separate_cli_processes_reuse_and_coordinate_rotating_credentials() {
    let directory = tempdir().unwrap();
    let cache = directory.path().join("hubuum_cli/sessions.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let server_cache = cache.clone();
    let server = thread::spawn(move || {
        let now = chrono::Utc::now();
        let mut pair = json!({
            "session_id":"persistent-process-test", "token_type":"Bearer",
            "access_token":"initial-access", "refresh_token":"initial-refresh",
            "access_expires_at":now + chrono::Duration::minutes(15),
            "refresh_expires_at":now + chrono::Duration::days(7),
            "absolute_expires_at":now + chrono::Duration::days(30)
        });
        let mut paths = Vec::new();
        while !stopped.load(Ordering::Acquire) {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            let (path, headers, body) = request(&mut stream);
            let response = match path.as_str() {
                "/healthz" => json!({}),
                "/api/v1/auth/login" => {
                    assert_eq!(body["name"], "alice");
                    assert_eq!(body["client_id"], "hubuum-cli");
                    pair.clone()
                }
                "/api/v1/auth/refresh" => {
                    assert_eq!(body["refresh_token"], "initial-refresh");
                    // A consumed credential must already be absent on disk.
                    assert_eq!(read_to_string(&server_cache).unwrap(), "[]");
                    pair["access_token"] = json!("rotated-access");
                    pair["refresh_token"] = json!("rotated-refresh");
                    pair.clone()
                }
                _ => {
                    assert!(headers.contains(&format!(
                        "authorization: bearer {}\r\n",
                        pair["access_token"].as_str().unwrap()
                    )));
                    if path == "/api/v1/auth/validate" {
                        json!({})
                    } else {
                        json!([])
                    }
                }
            };
            paths.push(path);
            let response = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
        }
        paths
    });
    let command = || {
        let mut command = cargo_bin_cmd!("hubuum-cli");
        command
            .env("XDG_CONFIG_HOME", directory.path())
            .env("XDG_DATA_HOME", directory.path())
            .env_remove("HUBUUM_CLI__SERVER__PASSWORD")
            .env_remove("HUBUUM_CLI__SERVER__TOKEN_FILE")
            .args([
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--protocol",
                "http",
                "--username",
                "alice",
            ])
            .args(["class", "list", "--output", "json", "--debug"])
            .timeout(Duration::from_secs(15));
        command
    };
    let password = format!("fixture-{}", rand::random::<u64>());
    let first = command()
        .env("HUBUUM_CLI__SERVER__PASSWORD", &password)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let mut saved: Value = serde_json::from_str(&read_to_string(&cache).unwrap()).unwrap();
    assert!(!saved.to_string().contains(&password));
    // Exercise another process while the access token is still current.
    let second = command().output().unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    // Expire the saved access token, then start two independent CLI processes.
    saved[0]["session"]["refresh_at"] = json!(chrono::Utc::now() - chrono::Duration::seconds(1));
    saved[0]["session"]["pair"]["access_expires_at"] = saved[0]["session"]["refresh_at"].clone();
    write(&cache, serde_json::to_vec(&saved).unwrap()).unwrap();
    let results = thread::scope(|scope| {
        let one = scope.spawn(|| command().output().unwrap());
        let two = scope.spawn(|| command().output().unwrap());
        [one.join().unwrap(), two.join().unwrap()]
    });
    stop.store(true, Ordering::Release);
    let paths = server.join().unwrap();
    for result in [first, second].into_iter().chain(results) {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        for secret in [
            &password,
            "initial-access",
            "initial-refresh",
            "rotated-access",
            "rotated-refresh",
        ] {
            assert!(!output.contains(secret));
        }
    }
    assert_eq!(
        paths
            .iter()
            .filter(|p| p.as_str() == "/api/v1/auth/login")
            .count(),
        1
    );
    assert_eq!(
        paths
            .iter()
            .filter(|p| p.as_str() == "/api/v1/auth/refresh")
            .count(),
        1
    );
    let saved: Value = serde_json::from_str(&read_to_string(cache).unwrap()).unwrap();
    assert_eq!(
        saved[0]["session"]["pair"]["refresh_token"],
        "rotated-refresh"
    );
}
