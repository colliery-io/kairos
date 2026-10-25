//! `kairos adrs check`, end to end with a stub server (KAIROS-T-0701).
//!
//! Each test runs the built binary in a temp folder with a clean
//! environment: no deployment and no key, unless the test gives the stub. A
//! local stub plays the deployment: it answers the item graph of each ADR.
//! No test talks to a real deployment.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

const KAIROS: &str = env!("CARGO_BIN_EXE_kairos");

type Routes = Arc<Mutex<HashMap<String, (u16, String)>>>;

/// A local HTTP server: a path (with its query) gives a status and a body.
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
                    .unwrap_or((
                    404,
                    json!({"error": {"code": "NOT_FOUND", "message": "No item.", "details": null}})
                        .to_string(),
                ));
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        stub
    }

    fn route(&self, path: &str, status: u16, body: Value) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (status, body.to_string()));
    }

    /// The graph of `code`: the focus with `current` as its code, and its
    /// neighbours `(id, code, relationship)`, each an edge to the focus.
    fn adr(
        &self,
        code: &str,
        current: &str,
        status: &str,
        archived: bool,
        inbound: &[(&str, &str)],
    ) {
        let mut nodes = vec![json!({
            "id": "focus", "short_code": current, "entity_type": "adr",
            "title": format!("Title of {current}"), "status": status, "depth": 0,
            "degree": inbound.len(), "done": false,
            "archived_at": if archived { json!("2026-10-01T00:00:00Z") } else { Value::Null },
        })];
        let mut edges = Vec::new();
        for (index, (neighbour, relationship)) in inbound.iter().enumerate() {
            let id = format!("n{index}");
            nodes.push(json!({
                "id": id, "short_code": neighbour, "entity_type": "adr",
                "title": format!("Title of {neighbour}"), "status": "Decided", "depth": 1,
                "degree": 1, "done": false,
            }));
            edges.push(json!({
                "source_id": id, "target_id": "focus", "relationship": relationship, "depth": 1,
            }));
        }
        self.route(
            &format!("/api/adrs/{code}/graph?depth=1"),
            200,
            json!({"focus": code, "depth": 1, "nodes": nodes, "edges": edges}),
        );
    }

    fn hits(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    fn graph_hits(&self) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.contains("/graph"))
            .count()
    }
}

/// A temp folder with a checkout and a config folder.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("repo")).unwrap();
        Sandbox { dir }
    }

    fn repo(&self) -> std::path::PathBuf {
        self.dir.path().join("repo")
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.repo().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Run `kairos` in the checkout with a clean environment.
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(KAIROS);
        command
            .current_dir(self.repo())
            .args(args)
            .env_remove("KAIROS_URL")
            .env_remove("KAIROS_KEY")
            .env_remove("KAIROS_MCP_KEY")
            .env_remove("KAIROS_AUTO_UPDATE")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env("HOME", self.dir.path())
            .env("KAIROS_CONFIG_DIR", self.dir.path().join("config"));
        for (name, value) in env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    /// Run against the stub, with a stub key.
    fn run_stub(&self, stub: &Stub, args: &[&str]) -> Output {
        let mut all = args.to_vec();
        all.extend(["--url", &stub.url]);
        self.run(&all, &[("KAIROS_KEY", "stub-key")])
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("not JSON ({err}): {}", stdout(output)))
}

/// The tree of the tests and the answers of the stub for it.
fn citing_tree(sandbox: &Sandbox, stub: &Stub) {
    sandbox.write(
        "src/main.rs",
        "// ACME-A-0001: live.\n// ACME-A-0002: superseded and amended.\nfn main() {}\n",
    );
    sandbox.write(
        "docs/a.md",
        "See ACME-A-0005 and ACME-A-0006.\nOld: OLD-A-0001.\n",
    );
    stub.adr("ACME-A-0001", "ACME-A-0001", "Decided", false, &[]);
    stub.adr(
        "ACME-A-0002",
        "ACME-A-0002",
        "Superseded",
        false,
        &[("ACME-A-0003", "supersedes"), ("ACME-A-0004", "amends")],
    );
    // ACME-A-0005 is not on the stub: 404.
    stub.adr("ACME-A-0006", "ACME-A-0006", "Decided", true, &[]);
    stub.adr("OLD-A-0001", "ACME-A-0001", "Decided", false, &[]);
}

#[test]
fn the_check_reports_each_citation_and_fails_on_a_missing_or_archived_adr() {
    let sandbox = Sandbox::new();
    let stub = Stub::start();
    citing_tree(&sandbox, &stub);

    let output = sandbox.run_stub(&stub, &["adrs", "check"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    for line in [
        "src/main.rs:2: ACME-A-0002 warn: The ADR ACME-A-0002 is superseded by ACME-A-0003.",
        "src/main.rs:2: ACME-A-0002 info: The ADR ACME-A-0002 has amendments: ACME-A-0004.",
        "docs/a.md:1: ACME-A-0005 fail: The ADR ACME-A-0005 does not exist.",
        "docs/a.md:1: ACME-A-0006 fail: The ADR ACME-A-0006 is archived",
        "docs/a.md:2: OLD-A-0001 warn: OLD-A-0001 is a retired code. Cite the current code ACME-A-0001.",
        "5 citations of 5 ADRs (deployment): 2 fail, 2 warn, 0 info.",
    ] {
        assert!(text.contains(line), "missing {line:?} in:\n{text}");
    }
    assert!(!text.contains("src/main.rs:1"), "{text}");

    // The alias `adr`, with --json: the report is on stdout, also on a fail.
    let output = sandbox.run_stub(&stub, &["adr", "check", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let report = json_of(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["source"], "deployment");
    assert_eq!(
        report["summary"],
        json!({"citations": 5, "adrs": 5, "fail": 2, "warn": 2, "info": 0})
    );
    let first = &report["citations"][0];
    assert_eq!(first["file"], "docs/a.md");
    assert_eq!(first["severity"], "fail");
    let adr2 = report["adrs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|adr| adr["code"] == "ACME-A-0002")
        .unwrap();
    assert_eq!(adr2["superseded_by"][0]["code"], "ACME-A-0003");
    assert_eq!(adr2["amended_by"][0]["code"], "ACME-A-0004");
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
}

#[test]
fn a_path_and_a_prefix_narrow_the_check() {
    let sandbox = Sandbox::new();
    let stub = Stub::start();
    citing_tree(&sandbox, &stub);

    let output = sandbox.run_stub(&stub, &["adrs", "check", "src/main.rs"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("2 citations of 2 ADRs"),
        "{}",
        stdout(&output)
    );

    let output = sandbox.run_stub(&stub, &["adrs", "check", "--prefix", "old"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout(&output).contains("1 citations of 1 ADRs"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_saved_snapshot_gives_the_same_report_with_no_deployment() {
    let sandbox = Sandbox::new();
    let stub = Stub::start();
    citing_tree(&sandbox, &stub);
    let snapshot = sandbox.dir.path().join("adrs.json");
    let snapshot = snapshot.to_str().unwrap();

    let online = sandbox.run_stub(
        &stub,
        &["adrs", "check", "--json", "--save-snapshot", snapshot],
    );
    assert_eq!(online.status.code(), Some(1));
    let hits = stub.hits();
    assert_eq!(stub.graph_hits(), 5, "one graph read for each ADR");

    let offline = sandbox.run(&["adrs", "check", "--json", "--snapshot", snapshot], &[]);
    assert_eq!(offline.status.code(), Some(1), "{}", stderr(&offline));
    assert_eq!(stub.hits(), hits, "the snapshot run reads no deployment");
    let (mut online, offline) = (json_of(&online), json_of(&offline));
    assert_eq!(offline["source"], "snapshot");
    online["source"] = json!("snapshot");
    assert_eq!(online, offline);

    // A new citation that the snapshot does not hold fails, and says to save
    // the snapshot again.
    sandbox.write("new.md", "ACME-A-0042\n");
    let output = sandbox.run(&["adrs", "check", "new.md", "--snapshot", snapshot], &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("Save the snapshot again"),
        "{}",
        stdout(&output)
    );
    assert!(Path::new(snapshot).exists());
}

#[test]
fn with_no_deployment_and_no_snapshot_the_check_is_skipped() {
    let sandbox = Sandbox::new();
    sandbox.write("a.md", "ACME-A-0001\n");

    let output = sandbox.run(&["adrs", "check"], &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("The ADR check is skipped"),
        "{}",
        stderr(&output)
    );

    let output = sandbox.run(&["adrs", "check", "--json"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let report = json_of(&output);
    assert_eq!(report["skipped"], true);
    assert_eq!(report["source"], "none");
}

#[test]
fn a_snapshot_with_no_adrs_is_a_placeholder_and_the_check_is_skipped() {
    let sandbox = Sandbox::new();
    sandbox.write("a.md", "ACME-A-0001\n");
    let snapshot = sandbox.dir.path().join("placeholder.json");
    std::fs::write(&snapshot, "{\"kairos_adr_snapshot\": 1, \"adrs\": []}\n").unwrap();
    let snapshot = snapshot.to_str().unwrap();

    let output = sandbox.run(&["adrs", "check", "--snapshot", snapshot], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("has no ADRs") && stderr(&output).contains("--save-snapshot"),
        "{}",
        stderr(&output)
    );

    let output = sandbox.run(&["adrs", "check", "--json", "--snapshot", snapshot], &[]);
    assert_eq!(output.status.code(), Some(0));
    let report = json_of(&output);
    assert_eq!(report["skipped"], true);
}

#[test]
fn a_refused_key_exits_2_and_a_missing_path_exits_1() {
    let sandbox = Sandbox::new();
    let stub = Stub::start();
    sandbox.write("a.md", "ACME-A-0001\n");
    stub.route(
        "/api/adrs/ACME-A-0001/graph?depth=1",
        401,
        json!({"error": {"code": "UNAUTHORIZED", "message": "No.", "details": null}}),
    );
    let output = sandbox.run_stub(&stub, &["adrs", "check", "--json"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("\"error\""), "{}", stderr(&output));

    let output = sandbox.run_stub(&stub, &["adrs", "check", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("does not exist"),
        "{}",
        stderr(&output)
    );
}

/// The metadata of an ADR (KAIROS-A-0070) on the stub.
fn metadata(stub: &Stub, code: &str, values: &[(&str, &str)]) {
    let values: Vec<Value> = values
        .iter()
        .map(|(slug, value)| {
            json!({"definition_id": "d", "slug": slug, "name": slug, "field_type": "string",
                   "value": value})
        })
        .collect();
    stub.route(
        &format!("/api/adrs/{code}/metadata"),
        200,
        json!({"short_code": code, "values": values}),
    );
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Robin",
            "-c",
            "user.email=robin@example.com",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// KAIROS-T-0705: an ADR whose citing files changed much since its
/// accepted_commit, or whose revisit date has passed, gets a warning.
#[test]
fn a_stale_adr_and_a_passed_revisit_date_give_warnings() {
    let sandbox = Sandbox::new();
    let stub = Stub::start();
    let repo = sandbox.repo();
    git(&repo, &["init", "-q"]);
    sandbox.write(
        "src/refund.rs",
        "// ACME-A-0001: refunds.\nfn refund() {}\n",
    );
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "one"]);
    let commit = git(&repo, &["rev-parse", "--short", "HEAD"]);
    let body: String = (0..60).map(|n| format!("fn f{n}() {{}}\n")).collect();
    sandbox.write(
        "src/refund.rs",
        &format!("// ACME-A-0001: refunds.\n{body}"),
    );
    stub.adr("ACME-A-0001", "ACME-A-0001", "Decided", false, &[]);
    metadata(
        &stub,
        "ACME-A-0001",
        &[
            ("accepted_commit", &commit),
            ("revisit_on", "2020-01-01"),
            ("revisit_when", "a second forge"),
        ],
    );

    let output = sandbox.run_stub(&stub, &["adrs", "check"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "warnings exit 0: {}",
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("src/refund.rs:1: ACME-A-0001 warn: The revisit date 2020-01-01"),
        "{text}"
    );
    assert!(text.contains("Revisit when: a second forge"), "{text}");
    assert!(
        text.contains(&format!("changed by 61 lines since the commit {commit}")),
        "{text}"
    );

    let output = sandbox.run_stub(&stub, &["adrs", "check", "--json", "--stale-lines", "100"]);
    let report = json_of(&output);
    let reasons: Vec<&str> = report["citations"][0]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, vec!["revisit_due"]);
    assert_eq!(report["adrs"][0]["accepted_commit"], json!(commit));
    assert_eq!(report["adrs"][0]["revisit_when"], "a second forge");
}
