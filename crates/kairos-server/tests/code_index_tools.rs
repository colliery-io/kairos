//! The builder downloads its tools on first start (COLLIERY-T-2525).
//!
//! The scenarios of the task, against a small file server on 127.0.0.1 that
//! counts the requests. No test reaches the internet: the tool set of the
//! tests has small files of the same formats (a gzip rust-analyzer, tar.gz
//! toolchain archives) with their own sha256 values.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kairos_index::tools::{Download, ToolSet};
use kairos_server::code_index::{CodeIndexService, TOOLS_DIR, prepare_tools};
use sha2::{Digest, Sha256};

const TARGET: &str = "aarch64-unknown-linux-gnu";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(bytes).unwrap();
    gz.finish().unwrap()
}

/// A toolchain archive as static.rust-lang.org makes it: one top folder,
/// with one folder for the component and a file at the top.
fn component(top: &str, component: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let mut add = |path: String, data: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, path, data).unwrap();
    };
    add(format!("{top}/components"), component.as_bytes());
    add(format!("{top}/{component}/manifest.in"), b"file:bin/x\n");
    for (path, data) in files {
        add(format!("{top}/{component}/{path}"), data);
    }
    gzip(&tar.into_inner().unwrap())
}

/// The files of the test tool set, by file name.
struct Fixture {
    files: HashMap<String, Vec<u8>>,
    rust_analyzer_binary: Vec<u8>,
}

fn fixture() -> Fixture {
    let rust_analyzer_binary = b"#!/bin/sh\necho rust-analyzer\n".to_vec();
    let mut files = HashMap::new();
    files.insert(
        "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf".to_string(),
        b"GGUF a small model".to_vec(),
    );
    files.insert(
        format!("rust-analyzer-{TARGET}.gz"),
        gzip(&rust_analyzer_binary),
    );
    files.insert(
        "rust-src-1.99.0.tar.gz".to_string(),
        b"a small std source".to_vec(),
    );
    files.insert(
        format!("rustc-1.93.0-{TARGET}.tar.gz"),
        component(
            &format!("rustc-1.93.0-{TARGET}"),
            "rustc",
            &[("bin/rustc", b"rustc"), ("lib/librustc_driver.so", b"lib")],
        ),
    );
    files.insert(
        format!("cargo-1.93.0-{TARGET}.tar.gz"),
        component(
            &format!("cargo-1.93.0-{TARGET}"),
            "cargo",
            &[("bin/cargo", b"cargo")],
        ),
    );
    files.insert(
        format!("rust-std-1.93.0-{TARGET}.tar.gz"),
        component(
            &format!("rust-std-1.93.0-{TARGET}"),
            &format!("rust-std-{TARGET}"),
            &[(&format!("lib/rustlib/{TARGET}/lib/libstd.rlib"), b"std")],
        ),
    );
    Fixture {
        files,
        rust_analyzer_binary,
    }
}

impl Fixture {
    /// The tool set of the fixture, downloaded from `base`.
    fn set(&self, base: &str) -> ToolSet {
        let d = |name: &str| Download::new(format!("{base}/{name}"), sha256(&self.files[name]));
        ToolSet {
            model: d("Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf"),
            rust_analyzer: d(&format!("rust-analyzer-{TARGET}.gz")),
            rust_analyzer_binary_sha256: sha256(&self.rust_analyzer_binary),
            rust_src: d("rust-src-1.99.0.tar.gz"),
            toolchain: vec![
                d(&format!("rustc-1.93.0-{TARGET}.tar.gz")),
                d(&format!("cargo-1.93.0-{TARGET}.tar.gz")),
                d(&format!("rust-std-1.93.0-{TARGET}.tar.gz")),
            ],
            toolchain_dir: "rust-1.93.0".to_string(),
        }
    }
}

/// A file server on 127.0.0.1 that counts each request.
struct FileServer {
    base: String,
    requests: Arc<AtomicUsize>,
}

impl FileServer {
    fn start(files: HashMap<String, Vec<u8>>) -> FileServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                count.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                        break;
                    }
                }
                let name = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .trim_start_matches('/');
                let (status, body) = match files.get(name) {
                    Some(body) => ("200 OK", body.clone()),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        FileServer { base, requests }
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

/// `prepare_tools` runs off the async threads in the server; here it runs
/// on the test thread.
fn prepare(
    dir: Option<&Path>,
    set: &ToolSet,
) -> Result<Option<kairos_index::tools::Tools>, String> {
    prepare_tools(dir, set)
}

// Scenario: A server with no builder downloads nothing.
#[test]
fn a_server_with_no_builder_downloads_nothing() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();

    let tools = prepare(None, &fixture.set(&server.base)).unwrap();

    assert!(tools.is_none());
    assert_eq!(server.requests(), 0, "no download");
    assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    assert!(!work.path().join(TOOLS_DIR).exists(), "no tools folder");
}

// Scenario: The first start downloads and checks the tools.
#[test]
fn the_first_start_downloads_and_checks_the_tools() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);

    let tools = prepare(Some(work.path()), &set).unwrap().expect("tools");

    assert_eq!(server.requests(), 6, "one download for each tool");
    assert_eq!(tools.downloaded.len(), 6);
    let dir = work.path().join(TOOLS_DIR);
    for d in set.downloads() {
        let bytes = std::fs::read(dir.join(&d.file)).unwrap();
        assert_eq!(sha256(&bytes), d.sha256, "{}", d.file);
    }
    assert_eq!(
        tools.model,
        dir.join("Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf")
    );
    assert_eq!(
        std::fs::read(&tools.rust_analyzer).unwrap(),
        fixture.rust_analyzer_binary
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&tools.rust_analyzer)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "rust-analyzer can run");
    }
    assert_eq!(tools.sysroot, dir.join("rust-1.93.0"));
    for file in [
        "bin/rustc".to_string(),
        "bin/cargo".to_string(),
        "lib/librustc_driver.so".to_string(),
        format!("lib/rustlib/{TARGET}/lib/libstd.rlib"),
    ] {
        assert!(tools.sysroot.join(&file).is_file(), "{file}");
    }
    assert!(!tools.sysroot.join("manifest.in").exists());
    assert!(!tools.sysroot.join("components").exists());

    // The builder runs with these tools: its SCIP run uses them and
    // downloads nothing.
    let service = CodeIndexService::from_repo_url(work.path().to_path_buf());
    service.use_tools(&tools);
    let options = service.build_options();
    assert_eq!(options.rust_analyzer.as_ref(), Some(&tools.rust_analyzer));
    assert_eq!(options.std_source.as_ref(), Some(&tools.rust_src));
    assert_eq!(options.sysroot.as_ref(), Some(&tools.sysroot));
    assert!(!options.download);
}

// Scenario: A second start downloads nothing.
#[test]
fn a_second_start_downloads_nothing() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);
    let first = prepare(Some(work.path()), &set).unwrap().expect("tools");
    let after_first = server.requests();

    let second = prepare(Some(work.path()), &set).unwrap().expect("tools");

    assert_eq!(server.requests(), after_first, "no download");
    assert!(second.downloaded.is_empty());
    assert_eq!(second.model, first.model);
    assert_eq!(second.sysroot, first.sysroot);
}

// Scenario: A wrong checksum keeps the builder off.
#[test]
fn a_wrong_checksum_keeps_the_builder_off() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);
    let dir = work.path().join(TOOLS_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join(&set.model.file);
    std::fs::write(&model, b"GGUF another model").unwrap();

    let reason = prepare(Some(work.path()), &set).unwrap_err();

    assert!(reason.contains(&model.display().to_string()), "{reason}");
    assert!(reason.contains(&set.model.sha256), "{reason}");
    assert_eq!(
        server.requests(),
        0,
        "the wrong file is not downloaded again"
    );
    assert_eq!(std::fs::read(&model).unwrap(), b"GGUF another model");
}

// Scenario: Files put in first are used with no network.
#[test]
fn files_put_in_first_are_used_with_no_network() {
    let fixture = fixture();
    let work = tempfile::tempdir().unwrap();
    // Port 1 on 127.0.0.1 refuses each connection: a download fails.
    let set = fixture.set("http://127.0.0.1:1");
    let dir = work.path().join(TOOLS_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in &fixture.files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    let tools = prepare(Some(work.path()), &set).unwrap().expect("tools");

    assert!(tools.downloaded.is_empty(), "no download");
    assert!(tools.sysroot.join("bin/rustc").is_file());
    assert!(tools.rust_analyzer.is_file());
}

// A download that fails, or that does not match its pin, keeps the builder
// off and leaves no file.
#[test]
fn a_failed_download_leaves_no_file() {
    let fixture = fixture();
    let mut files = fixture.files.clone();
    files.insert(
        "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf".to_string(),
        b"GGUF changed on the server".to_vec(),
    );
    let server = FileServer::start(files);
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);

    let reason = prepare(Some(work.path()), &set).unwrap_err();

    assert!(reason.contains(&set.model.url), "{reason}");
    assert!(reason.contains(&set.model.sha256), "{reason}");
    let dir = work.path().join(TOOLS_DIR);
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert!(left.is_empty(), "{left:?}");

    let reason = prepare(Some(work.path()), &fixture.set("http://127.0.0.1:1")).unwrap_err();
    assert!(reason.contains("The download of a tool failed"), "{reason}");
    assert!(reason.contains("http://127.0.0.1:1/"), "{reason}");
}

// COLLIERY-T-2529: the hash cache of the tools. After a full check, the
// size and the modified time of each file are recorded in tools/. A file
// whose size and modified time did not change is not hashed again. A file
// that changed is hashed in full.

/// Set the modified time of `path` to `secs` after the epoch, so that a
/// change is seen also when the clock has a coarse resolution.
fn set_modified(path: &Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn a_second_start_does_not_hash_the_tools_again() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);

    let first = prepare(Some(work.path()), &set).unwrap().expect("tools");
    // The first start downloads each file and hashes it as it comes, and
    // hashes the unpacked rust-analyzer.
    assert_eq!(first.downloaded.len(), 6);
    let second = prepare(Some(work.path()), &set).unwrap().expect("tools");
    assert!(second.hashed.is_empty(), "{:?}", second.hashed);

    // Files put in first are hashed one time, on the first start.
    let put = tempfile::tempdir().unwrap();
    let dir = put.path().join(TOOLS_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in &fixture.files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let set = fixture.set("http://127.0.0.1:1");
    let first = prepare(Some(put.path()), &set).unwrap().expect("tools");
    let mut hashed = first.hashed.clone();
    hashed.sort();
    let mut expected: Vec<String> = set.downloads().map(|d| d.file.clone()).collect();
    expected.push("rust-analyzer".into());
    expected.sort();
    assert_eq!(hashed, expected);
    let second = prepare(Some(put.path()), &set).unwrap().expect("tools");
    assert!(second.hashed.is_empty(), "{:?}", second.hashed);
}

#[test]
fn a_changed_file_is_hashed_in_full_and_refused_when_wrong() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);
    prepare(Some(work.path()), &set).unwrap().expect("tools");
    let model = work.path().join(TOOLS_DIR).join(&set.model.file);
    let right = std::fs::read(&model).unwrap();

    // The same size, another content, another modified time.
    let mut wrong = right.clone();
    wrong[0] ^= 0xff;
    std::fs::write(&model, &wrong).unwrap();
    set_modified(&model, 1_000_000);
    let reason = prepare(Some(work.path()), &set).unwrap_err();
    assert!(reason.contains(&model.display().to_string()), "{reason}");
    assert!(reason.contains(&set.model.sha256), "{reason}");
    // Refused again at the next start: the wrong file is not recorded.
    let reason = prepare(Some(work.path()), &set).unwrap_err();
    assert!(reason.contains(&set.model.sha256), "{reason}");

    // The right content again: hashed in full, then accepted.
    std::fs::write(&model, &right).unwrap();
    set_modified(&model, 2_000_000);
    let tools = prepare(Some(work.path()), &set).unwrap().expect("tools");
    assert_eq!(tools.hashed, vec![set.model.file.clone()]);
    assert!(tools.downloaded.is_empty());
}

#[test]
fn only_a_new_modified_time_hashes_the_file_again() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let set = fixture.set(&server.base);
    prepare(Some(work.path()), &set).unwrap().expect("tools");
    let model = work.path().join(TOOLS_DIR).join(&set.model.file);

    set_modified(&model, 3_000_000);
    let tools = prepare(Some(work.path()), &set).unwrap().expect("tools");
    assert_eq!(tools.hashed, vec![set.model.file.clone()]);
    let tools = prepare(Some(work.path()), &set).unwrap().expect("tools");
    assert!(tools.hashed.is_empty(), "{:?}", tools.hashed);
}

#[test]
fn a_new_pin_hashes_the_file_again() {
    let fixture = fixture();
    let server = FileServer::start(fixture.files.clone());
    let work = tempfile::tempdir().unwrap();
    let mut set = fixture.set(&server.base);
    prepare(Some(work.path()), &set).unwrap().expect("tools");

    // A release of Kairos with another pin for the same file name.
    set.model.sha256 = "0".repeat(64);
    let reason = prepare(Some(work.path()), &set).unwrap_err();
    assert!(reason.contains(&set.model.sha256), "{reason}");
}
