use std::collections::BTreeMap;
use std::env::{current_exe, var};
use std::fs::{copy, read, write};
use std::io::{Cursor, ErrorKind, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep, JoinHandle};
use std::time::{Duration, Instant};

use flate2::{write::GzEncoder, Compression};
use serde_json::json;
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use tempfile::tempdir;
use zip::{write::SimpleFileOptions, ZipWriter};

use super::{Platform, UpdateMode, UpdateStatus, Updater, Version};

const PAYLOAD: &[u8] = b"verified replacement executable";
const LATEST: &str = "/repos/hubuum/hubuum-cli/releases/latest";

#[derive(Clone, Copy)]
enum Fault {
    None,
    MissingArchive,
    MissingChecksum,
    WrongChecksum,
    MalformedChecksum,
    CorruptArchive,
    WrongDigest,
    Http,
}

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new(platform: Platform, version: &str, fault: Fault) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let artifact_version = Version::parse(version).unwrap_or_else(|_| Version::new(0, 0, 14));
        let name = platform.asset_name(&artifact_version);
        let archive = if matches!(fault, Fault::CorruptArchive) {
            b"not an archive".to_vec()
        } else {
            archive(platform)
        };
        let digest: String = Sha256::digest(&archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let sums = match fault {
            Fault::WrongChecksum => format!("{}  {name}\n", "0".repeat(64)),
            Fault::MalformedChecksum => "not a checksum\n".to_string(),
            _ => format!("{digest}  {name}\n"),
        };
        // Decoys precede the real archive: a substring matcher can select them.
        let mut assets = vec![json!({"name":format!("{name}.sig"), "url":format!("{url}/decoy")})];
        if !matches!(fault, Fault::MissingChecksum) {
            assets.push(json!({"name":format!("{name}.sha256"), "url":format!("{url}/checksum")}));
        }
        if !matches!(fault, Fault::MissingArchive) {
            let published_digest = if matches!(fault, Fault::WrongDigest) {
                "0".repeat(64)
            } else {
                digest
            };
            assets.push(json!({"name":name, "url":format!("{url}/archive"), "digest":format!("sha256:{published_digest}")}));
        }
        let release = json!({"tag_name":format!("v{version}"), "created_at":"2026-10-05T00:00:00Z", "assets":assets}).to_string().into_bytes();
        let routes = BTreeMap::from([
            (
                LATEST.to_string(),
                (
                    if matches!(fault, Fault::Http) {
                        503
                    } else {
                        200
                    },
                    release.clone(),
                ),
            ),
            (
                format!("/repos/hubuum/hubuum-cli/releases/tags/v{version}"),
                (200, release),
            ),
            ("/checksum".to_string(), (200, sums.into_bytes())),
            ("/archive".to_string(), (200, archive)),
        ]);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_stop = stop.clone();
        let server_requests = requests.clone();
        let thread = thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 4096];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buf).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&buf[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                let path = request.split_whitespace().nth(1).unwrap().to_string();
                server_requests.lock().unwrap().push(path.clone());
                let fallback = (404, b"unexpected request".to_vec());
                let (status, body) = routes.get(&path).unwrap_or(&fallback);
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn archive(platform: Platform) -> Vec<u8> {
    if matches!(platform, Platform::WindowsX86_64) {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(platform.binary_name(), SimpleFileOptions::default())
            .unwrap();
        zip.write_all(PAYLOAD).unwrap();
        zip.finish().unwrap().into_inner()
    } else {
        let mut tar = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let mut header = Header::new_gnu();
        header.set_size(PAYLOAD.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, platform.binary_name(), PAYLOAD)
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }
}

fn native_target() -> &'static str {
    if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-unknown-linux-musl"
    }
}

#[test]
fn supported_targets_match_release_archive_names() {
    for (target, expected) in [
        (
            "x86_64-unknown-linux-musl",
            "linux-x86_64-musl-v1.2.3.tar.gz",
        ),
        (
            "x86_64-unknown-linux-gnu",
            "linux-x86_64-musl-v1.2.3.tar.gz",
        ),
        (
            "aarch64-unknown-linux-musl",
            "linux-aarch64-musl-v1.2.3.tar.gz",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "linux-aarch64-musl-v1.2.3.tar.gz",
        ),
        ("aarch64-apple-darwin", "macos-aarch64-v1.2.3.tar.gz"),
        ("x86_64-pc-windows-msvc", "windows-x86_64-v1.2.3.zip"),
    ] {
        let updater = Updater::new("v0.0.13", target).unwrap();
        assert_eq!(
            updater.platform.asset_name(&Version::new(1, 2, 3)),
            format!("hubuum-cli-{expected}")
        );
    }
    for target in ["x86_64-apple-darwin", "x86_64-pc-windows-gnu", "unknown"] {
        assert!(Updater::new("0.0.13", target).is_err());
    }
    assert!(Updater::new("main-latest", native_target()).is_err());
}

#[test]
fn checking_a_new_release_fetches_only_metadata() {
    let updater = Updater::new("0.0.13", native_target()).unwrap();
    let fixture = Fixture::new(updater.platform, "0.0.14", Fault::None);
    let dir = tempdir().unwrap();
    let executable = dir.path().join("untouched");
    write(&executable, b"old").unwrap();
    let result = updater
        .run_at(UpdateMode::Check, &fixture.url, executable.clone())
        .unwrap();
    assert_eq!(result.status(), UpdateStatus::Available);
    assert_eq!(result.release_version(), "0.0.14");
    assert_eq!(read(executable).unwrap(), b"old");
    assert_eq!(fixture.requests(), [LATEST]);
}

#[test]
fn current_newer_and_rolling_builds_do_not_downgrade_or_reinstall() {
    for current in ["0.0.14", "0.0.15", "v0.0.14+main.g0123456789ab"] {
        let updater = Updater::new(current, native_target()).unwrap();
        let fixture = Fixture::new(updater.platform, "0.0.14", Fault::None);
        let dir = tempdir().unwrap();
        let executable = dir.path().join("untouched");
        write(&executable, b"old").unwrap();
        assert_eq!(
            updater
                .run_at(UpdateMode::Install, &fixture.url, executable.clone())
                .unwrap()
                .status(),
            UpdateStatus::UpToDate
        );
        assert_eq!(read(executable).unwrap(), b"old");
        assert_eq!(fixture.requests(), [LATEST]);
    }
}

#[test]
fn incomplete_releases_and_failed_verification_preserve_the_installed_binary() {
    for fault in [
        Fault::MissingArchive,
        Fault::MissingChecksum,
        Fault::WrongChecksum,
        Fault::MalformedChecksum,
        Fault::CorruptArchive,
        Fault::WrongDigest,
        Fault::Http,
    ] {
        let updater = Updater::new("0.0.13", native_target()).unwrap();
        let fixture = Fixture::new(updater.platform, "0.0.14", fault);
        let dir = tempdir().unwrap();
        let executable = dir.path().join("untouched");
        write(&executable, b"old").unwrap();
        assert!(updater
            .run_at(UpdateMode::Install, &fixture.url, executable.clone())
            .is_err());
        assert_eq!(read(executable).unwrap(), b"old");
        assert!(!fixture.requests().contains(&"/decoy".to_string()));
    }
}

#[test]
fn prereleases_and_nonversioned_releases_are_rejected() {
    for version in ["0.0.14-rc.1", "0.0.14+main.g123", "main-latest"] {
        let updater = Updater::new("0.0.13", native_target()).unwrap();
        let fixture = Fixture::new(updater.platform, version, Fault::None);
        let dir = tempdir().unwrap();
        assert!(updater
            .run_at(UpdateMode::Check, &fixture.url, dir.path().join("unused"))
            .is_err());
        assert_eq!(fixture.requests(), [LATEST]);
    }
}

#[test]
fn verified_tar_and_zip_archives_replace_only_the_selected_binary() {
    for target in ["x86_64-unknown-linux-musl", "x86_64-pc-windows-msvc"] {
        let updater = Updater::new("0.0.13", target).unwrap();
        let fixture = Fixture::new(updater.platform, "0.0.14", Fault::None);
        let dir = tempdir().unwrap();
        let executable = dir.path().join("installed");
        write(&executable, b"old").unwrap();
        let result = updater
            .run_at(UpdateMode::Install, &fixture.url, executable.clone())
            .unwrap();
        assert_eq!(result.status(), UpdateStatus::Updated);
        assert_eq!(read(executable).unwrap(), PAYLOAD);
        assert_eq!(
            fixture.requests(),
            [
                LATEST,
                "/repos/hubuum/hubuum-cli/releases/tags/v0.0.14",
                "/checksum",
                "/archive"
            ]
        );
    }
}

#[test]
fn replaces_a_running_executable_in_a_disposable_subprocess() {
    let updater = Updater::new("0.0.13", native_target()).unwrap();
    let fixture = Fixture::new(updater.platform, "0.0.14", Fault::None);
    let dir = tempdir().unwrap();
    let executable = dir
        .path()
        .join(if cfg!(windows) { "probe.exe" } else { "probe" });
    copy(current_exe().unwrap(), &executable).unwrap();
    let result = Command::new(&executable)
        .args([
            "--exact",
            "tests::replace_running_executable_child",
            "--nocapture",
        ])
        .env("HUBUUM_UPDATE_TEST_API", &fixture.url)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while read(&executable).ok().as_deref() != Some(PAYLOAD) {
        assert!(Instant::now() < deadline, "replacement did not finish");
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn replace_running_executable_child() {
    let Ok(api) = var("HUBUUM_UPDATE_TEST_API") else {
        return;
    };
    let result = Updater::new("0.0.13", native_target())
        .unwrap()
        .run_at(UpdateMode::Install, &api, current_exe().unwrap())
        .unwrap();
    assert_eq!(result.status(), UpdateStatus::Updated);
}
