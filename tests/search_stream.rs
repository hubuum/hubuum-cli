use std::fs::write;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::tempdir;

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1);
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes).unwrap()
}

fn event(kind: &str, data: Value) -> String {
    format!("event: {kind}\ndata: {data}\n\n")
}

// The server withholds done until the test has observed a batch on stdout.
// A buffering regression fails by timeout instead of depending on timing guesses.
fn exercise(format: &str, terminal: bool, redirect: bool) {
    let directory = tempdir().unwrap();
    let token = directory.path().join("token");
    write(&token, "fixture-token").unwrap();
    let config = directory.path().join("config.toml");
    write(&config, "[completion]\ndisable_api_related = true\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, wait_for_release) = mpsc::channel();
    let server = thread::spawn(move || {
        // Startup probes the server and validates the provided bearer.
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(!request.contains("/search"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .unwrap();
        }
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.starts_with("GET /api/v1/search/stream?"));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let first = event("started", json!({"query":"needle"}));
        let batch = event(
            "batch",
            json!({"kind":"collections","classes":[],"objects":[],"collections":[{
            "id":1,"name":"early","description":"fixture","group_id":1,"created_at":"2026-09-22T00:00:00Z","updated_at":"2026-09-22T00:00:00Z","revision":1
        }],"next":null}),
        );
        write!(stream, "{first}{batch}").unwrap();
        stream.flush().unwrap();
        wait_for_release
            .recv_timeout(Duration::from_secs(10))
            .expect("CLI must display a batch before server completion");
        if terminal {
            write!(stream, "{}", event("done", json!({"query":"needle"}))).unwrap();
        }
    });
    let destination = directory.path().join("output.json");
    write(&destination, "original file").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hubuum-cli"));
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
            "search",
            "needle",
            "--kind",
            "collection",
            "--stream",
            "--output",
            format,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if redirect {
        command.args([">", destination.to_str().unwrap()]);
    }
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (output_tx, output_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            output_tx.send(line.clone()).unwrap();
            output.push_str(&line);
            output.push('\n');
        }
        output
    });
    if redirect || format == "json" {
        release.send(()).unwrap();
    } else {
        loop {
            let line = match output_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(line) => line,
                Err(error) => {
                    child.kill().ok();
                    panic!("no incremental batch: {error}");
                }
            };
            if line.contains("early") {
                break;
            }
        }
        release.send(()).unwrap();
    }
    let status = child.wait().unwrap();
    let output = reader.join().unwrap();
    server.join().unwrap();
    assert_eq!(status.success(), terminal, "{output}");
    if redirect {
        let file = std::fs::read_to_string(destination).unwrap();
        if terminal {
            assert!(file.contains("early"));
            assert!(!output.contains("early"));
        } else {
            assert_eq!(file, "original file");
        }
    } else if terminal && format == "json" {
        let events: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(events.as_array().unwrap().len(), 3);
    } else if terminal && format == "jsonl" {
        let events: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2]["event"], "done");
    } else if !terminal {
        assert!(output.contains("incomplete"));
    }
}

#[test]
fn text_and_jsonl_batches_arrive_before_done() {
    exercise("text", true, false);
    exercise("jsonl", true, false);
}
#[test]
fn json_retains_one_complete_document() {
    exercise("json", true, false);
}
#[test]
fn truncated_stream_fails_and_preserves_redirect_destination() {
    exercise("text", false, false);
    exercise("json", false, true);
}
#[test]
fn redirects_capture_the_stream_without_stdout_leaks() {
    exercise("json", true, true);
}
