//! `kairos self-update` and the daily version check, end to end
//! (KAIROS-T-0595).
//!
//! Each test runs a copy of the built binary in a temp folder, so a swap
//! changes the copy and never the build. A local stub plays the server
//! (`/version`, a mirror of the tarball) and the release source
//! (`KAIROS_RELEASES_URL`). No test talks to GitHub or to a deployment.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

const KAIROS: &str = env!("CARGO_BIN_EXE_kairos");
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The release target of this platform, as the release assets name it.
fn target() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        other => panic!("no release target for {other:?}"),
    }
}

/// The answers of the stub: a path gives a status and a body.
type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

/// A local HTTP server: a path gives a status and a body. It logs each
/// path that it gets.
#[derive(Clone)]
struct Stub {
    url: String,
    routes: Routes,
    log: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stub = Stub {
            url: format!("http://{}", listener.local_addr().unwrap()),
            routes: Arc::default(),
            log: Arc::default(),
        };
        let served = stub.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                served.log.lock().unwrap().push(path.clone());
                let (status, body) = served
                    .routes
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"{}".to_vec()));
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        stub
    }

    fn route(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (status, body.into()));
    }

    fn hits(&self, path: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }

    fn paths(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn tarball(binary: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_size(binary.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "./kairos", binary)
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

fn sha_line(data: &[u8], name: &str) -> String {
    let hex: String = Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{hex}  {name}\n")
}

/// The server says `version`. It names a mirror tarball with `mirrored`
/// bytes, and a `.sha256` of its own that matches them. The release source
/// publishes `published` bytes and their checksum.
fn serve_release(stub: &Stub, version: &str, mirrored: &[u8], published: &[u8]) {
    let asset = format!("kairos-{version}-{}.tar.gz", target());
    let mirror = format!("{}/mirror/{asset}", stub.url);
    stub.route(
        "/version",
        200,
        serde_json::json!({
            "version": version,
            "cli": [{"target": target(), "url": mirror, "sha256_url": format!("{mirror}.sha256")}],
        })
        .to_string(),
    );
    stub.route(&format!("/mirror/{asset}"), 200, mirrored.to_vec());
    stub.route(
        &format!("/mirror/{asset}.sha256"),
        200,
        sha_line(mirrored, &asset),
    );
    let download = format!("/releases/download/v{version}/{asset}");
    stub.route(&download, 200, published.to_vec());
    stub.route(
        &format!("{download}.sha256"),
        200,
        sha_line(published, &asset),
    );
}

/// A copy of the built binary in a temp folder, and a config folder.
struct Sandbox {
    dir: tempfile::TempDir,
    exe: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("bin").join("kairos");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::copy(KAIROS, &exe).unwrap();
        Sandbox { dir, exe }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    /// Run the copy with a clean environment: no deployment, no key, a
    /// temp HOME, the release source on `stub`.
    fn run(&self, stub: &Stub, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(&self.exe);
        command
            .args(args)
            .env_remove("KAIROS_URL")
            .env_remove("KAIROS_KEY")
            .env_remove("KAIROS_MCP_KEY")
            .env_remove("KAIROS_AUTO_UPDATE")
            .env_remove("XDG_CONFIG_HOME")
            .env("HOME", self.dir.path())
            .env("KAIROS_CONFIG_DIR", self.config())
            .env("KAIROS_RELEASES_URL", format!("{}/releases", stub.url));
        for (name, value) in env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn binary(&self) -> Vec<u8> {
        std::fs::read(&self.exe).unwrap()
    }

    fn backup(&self) -> PathBuf {
        self.exe.with_file_name("kairos.bak")
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("not JSON ({err}): {}", stdout(output)))
}

fn unchanged(sandbox: &Sandbox) -> bool {
    sandbox.binary() == std::fs::read(KAIROS).unwrap() && !sandbox.backup().exists()
}

const NEW_BINARY: &[u8] = b"#!/bin/sh\necho stub kairos\n";

#[test]
fn an_older_server_is_refused_without_an_option() {
    let stub = Stub::start();
    serve_release(&stub, "0.0.1", &tarball(NEW_BINARY), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(&stub, &["self-update", "--url", &stub.url], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("older than this CLI"), "{err}");
    assert!(err.contains("--allow-downgrade"), "{err}");
    assert!(err.contains("--version 0.0.1"), "{err}");
    assert!(unchanged(&sandbox), "the binary changed");
    assert_eq!(stub.paths(), vec!["/version"], "no download");
}

#[test]
fn json_names_the_versions_and_the_refusal() {
    let stub = Stub::start();
    serve_release(&stub, "0.0.1", &tarball(NEW_BINARY), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(&stub, &["self-update", "--url", &stub.url, "--json"], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let value = json(&output);
    assert_eq!(value["action"], "refused");
    assert_eq!(value["cli_version"], CLI_VERSION);
    assert_eq!(value["server_version"], "0.0.1");
    assert_eq!(value["version"], "0.0.1");
    assert_eq!(value["deployment"], stub.url.as_str());
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("--allow-downgrade")
    );
    assert!(unchanged(&sandbox));
}

#[test]
fn allow_downgrade_installs_the_published_tarball_from_the_mirror() {
    let stub = Stub::start();
    serve_release(&stub, "0.0.1", &tarball(NEW_BINARY), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(
        &stub,
        &[
            "self-update",
            "--url",
            &stub.url,
            "--allow-downgrade",
            "--json",
        ],
        &[],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json(&output);
    assert_eq!(value["action"], "installed");
    assert_eq!(value["version"], "0.0.1");
    assert_eq!(sandbox.binary(), NEW_BINARY);
    assert_eq!(
        std::fs::read(sandbox.backup()).unwrap(),
        std::fs::read(KAIROS).unwrap()
    );
    let asset = format!("kairos-0.0.1-{}.tar.gz", target());
    // The tarball from the mirror of the server; the checksum from the
    // release source, never the one of the server.
    assert_eq!(stub.hits(&format!("/mirror/{asset}")), 1);
    assert_eq!(
        stub.hits(&format!("/releases/download/v0.0.1/{asset}.sha256")),
        1
    );
    assert_eq!(stub.hits(&format!("/mirror/{asset}.sha256")), 0);
}

#[test]
fn a_tarball_that_is_not_the_published_one_is_refused() {
    let stub = Stub::start();
    // The server's own checksum matches its tarball; the published one
    // does not.
    serve_release(
        &stub,
        "999.0.0",
        &tarball(b"#!/bin/sh\necho not the release\n"),
        &tarball(NEW_BINARY),
    );
    let sandbox = Sandbox::new();

    let output = sandbox.run(&stub, &["self-update", "--url", &stub.url], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("does not match"),
        "{}",
        stderr(&output)
    );
    assert!(unchanged(&sandbox), "the binary changed");
}

#[test]
fn a_newer_server_is_installed() {
    let stub = Stub::start();
    serve_release(&stub, "999.0.0", &tarball(NEW_BINARY), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(&stub, &["self-update", "--url", &stub.url], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).starts_with(&format!("Updated the CLI from {CLI_VERSION} to 999.0.0")),
        "{}",
        stdout(&output)
    );
    assert_eq!(sandbox.binary(), NEW_BINARY);
}

/// KAIROS-T-0745 (KAIROS-A-0080): a server with the tarballs in its
/// image sends paths. With no `KAIROS_RELEASES_URL`, the CLI downloads the
/// tarball and the checksum from the server, and nothing from a release
/// source.
#[test]
fn the_tarball_and_the_checksum_come_from_the_server() {
    let stub = Stub::start();
    let asset = format!("kairos-999.0.0-{}.tar.gz", target());
    let archive = tarball(NEW_BINARY);
    stub.route(
        "/version",
        200,
        serde_json::json!({
            "version": "999.0.0",
            "cli": [{
                "target": target(),
                "url": format!("/cli/{asset}"),
                "sha256_url": format!("/cli/{asset}.sha256"),
            }],
        })
        .to_string(),
    );
    stub.route(&format!("/cli/{asset}"), 200, archive.clone());
    stub.route(
        &format!("/cli/{asset}.sha256"),
        200,
        sha_line(&archive, &asset),
    );
    let sandbox = Sandbox::new();

    let output = sandbox.run(
        &stub,
        &["self-update", "--url", &stub.url],
        &[("KAIROS_RELEASES_URL", "")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(sandbox.binary(), NEW_BINARY);
    assert_eq!(stub.hits(&format!("/cli/{asset}.sha256")), 1);
    assert!(
        stub.paths()
            .iter()
            .all(|path| !path.starts_with("/releases")),
        "{:?}",
        stub.paths()
    );
}

#[test]
fn version_installs_an_older_release_without_allow_downgrade() {
    let stub = Stub::start();
    serve_release(&stub, "0.0.1", &tarball(b"unused"), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(&stub, &["self-update", "--version", "0.0.1", "--json"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json(&output);
    assert_eq!(value["action"], "installed");
    assert!(value["server_version"].is_null());
    assert_eq!(sandbox.binary(), NEW_BINARY);
    assert_eq!(stub.hits("/version"), 0, "--version asks no server");
}

#[test]
fn check_with_json_changes_nothing() {
    let stub = Stub::start();
    serve_release(&stub, "999.0.0", &tarball(NEW_BINARY), &tarball(NEW_BINARY));
    let sandbox = Sandbox::new();

    let output = sandbox.run(
        &stub,
        &["self-update", "--url", &stub.url, "--check", "--json"],
        &[],
    );
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    let value = json(&output);
    assert_eq!(value["action"], "checked");
    assert_eq!(value["server_version"], "999.0.0");
    assert!(unchanged(&sandbox));
    assert_eq!(stub.paths(), vec!["/version"]);
}

/// A stub that answers `whoami`, so a command succeeds against it.
fn whoami_server(version: Option<&str>) -> Stub {
    let stub = Stub::start();
    stub.route(
        "/api/whoami",
        200,
        serde_json::json!({
            "user": {"id": "u1", "external_id": "e1", "email": "robin@example.test", "display_name": "Robin"},
            "organization": {"id": "o1", "slug": "acme", "role": "member"},
            "teams": [],
        })
        .to_string(),
    );
    match version {
        Some(version) => stub.route("/version", 200, format!(r#"{{"version": "{version}"}}"#)),
        None => stub.route("/version", 503, "{}"),
    }
    stub
}

/// The daily check asks the deployment of `--url`, not `KAIROS_URL`; its
/// warning names `--check`; the answer is recorded for a day.
#[test]
fn the_daily_check_asks_the_deployment_of_the_command() {
    let command = whoami_server(Some("999.0.0"));
    let default = whoami_server(Some("999.0.0"));
    let sandbox = Sandbox::new();
    let env = [
        ("KAIROS_URL", default.url.as_str()),
        ("KAIROS_KEY", "test-key"),
    ];

    let output = sandbox.run(&command, &["whoami", "--url", &command.url], &env);
    assert!(output.status.success(), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains(&command.url), "{err}");
    assert!(err.contains("`kairos self-update --check`"), "{err}");
    assert_eq!(command.hits("/version"), 1);
    assert_eq!(default.hits("/version"), 0, "not the default deployment");

    let output = sandbox.run(&command, &["whoami", "--url", &command.url], &env);
    assert!(output.status.success());
    assert!(!stderr(&output).contains("warning"), "{}", stderr(&output));
    assert_eq!(command.hits("/version"), 1, "once a day");
}

/// A server with no version is asked again by the next command.
#[test]
fn the_daily_check_does_not_record_a_server_with_no_answer() {
    let server = whoami_server(None);
    let sandbox = Sandbox::new();
    let env = [("KAIROS_KEY", "test-key")];
    for _ in 0..2 {
        let output = sandbox.run(&server, &["whoami", "--url", &server.url], &env);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!stderr(&output).contains("warning"), "{}", stderr(&output));
    }
    assert_eq!(server.hits("/version"), 2);
    let state = sandbox.config().join("version-check.json");
    assert!(
        !state.exists()
            || !std::fs::read_to_string(&state)
                .unwrap()
                .contains(&server.url)
    );
}

/// `KAIROS_AUTO_UPDATE=1` does not install from an `http://` deployment.
#[test]
fn auto_update_refuses_an_http_deployment() {
    let server = whoami_server(Some("999.0.0"));
    let sandbox = Sandbox::new();
    let env = [("KAIROS_KEY", "test-key"), ("KAIROS_AUTO_UPDATE", "1")];
    let output = sandbox.run(&server, &["whoami", "--url", &server.url], &env);
    assert!(output.status.success(), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("KAIROS_AUTO_UPDATE does not update"), "{err}");
    assert!(unchanged(&sandbox));
    assert_eq!(server.paths(), vec!["/api/whoami", "/version"]);
}
