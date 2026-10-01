//! The scenarios of `kairos index` and of the local code tools
//! (COLLIERY-T-1852). The `.feature` file is in `tests/features/`.
//!
//! Each scenario runs on a copy of the polyglot fixture of kairos-index,
//! with a git repository in it. The test is the agent: it starts the built
//! `kairos index mcp` and sends it JSON-RPC lines on stdin, as an MCP client
//! does. The indexes of the tool scenarios are built with the library and a
//! fake summarizer that gives fixed texts, and with the deterministic
//! vectors, so that each result is the same on each run.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use cucumber::{World, given, then, when};
use kairos_embed::DeterministicProvider;
use kairos_index::{
    BuildOptions, FakeSummarizer, Index, Level, SummarizeOptions, Summarizer, SummaryRequest,
    build_structure_with, rust_analyzer, summarize,
};
use serde_json::{Value, json};
use tempfile::TempDir;

/// The built `kairos` binary.
const KAIROS: &str = env!("CARGO_BIN_EXE_kairos");

/// The index in a checkout.
const INDEX_DB: &str = ".kairos/index.db";

/// How long the agent waits for one answer of the server.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../kairos-index/tests/fixtures/polyglot")
}

/// The entry point that the path scenario starts from. The fixture has no
/// binary, so each tool scenario adds this one: `main` calls `run`, `run`
/// calls `enqueue_all`, and `enqueue_all` calls `Queue::push`.
const MAIN_RS: &str = r#"//! The entry point of the polyglot fixture (COLLIERY-T-1852).

fn main() {
    println!("{}", run());
}

/// Puts 3 numbers on a queue and gives its length.
fn run() -> usize {
    polyglot::enqueue_all(&[1, 2, 3]).len()
}
"#;

/// A caller of the CSV `load` that the name resolves for certain: it is in
/// the same file. `build_report` is the possible caller: 2 modules define
/// `load`.
const LOAD_FIRST_PY: &str = r#"

def load_first(path):
    return load(path)[0]
"#;

/// The fixed summaries of the fake summarizer, by file and name. Each other
/// symbol gets the text of [`FakeSummarizer`].
const DESCRIPTIONS: &[(&str, &str, &str)] = &[
    (
        "src/stack.rs",
        "pop",
        "Removes the newest number from the stack and returns it, or None when the stack is empty.",
    ),
    ("src/stack.rs", "push", "Adds a number to the stack."),
    (
        "src/stack.rs",
        "Stack",
        "A last-in, first-out store of numbers.",
    ),
    (
        "src/queue.rs",
        "push",
        "Adds a number at the back of the queue.",
    ),
    (
        "src/lib.rs",
        "enqueue_all",
        "Puts each number of a slice on a new queue and returns the queue.",
    ),
];

/// The description that the agent searches with: the words of the summary
/// of `Stack::pop`, written as a question.
const POP_QUERY: &str = "which function removes the newest number, or None when empty";

/// A fake summarizer with fixed texts for some symbols.
#[derive(Default)]
struct Described {
    fake: FakeSummarizer,
}

impl Summarizer for Described {
    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        if request.level == Level::Symbol
            && let Some((_, _, text)) = DESCRIPTIONS
                .iter()
                .find(|(path, name, _)| request.path == *path && request.name == *name)
        {
            return Ok((*text).to_string());
        }
        self.fake.summarize(request)
    }
}

/// The result of one tool call.
#[derive(Debug, Clone)]
struct ToolResult {
    text: String,
    is_error: bool,
}

/// An MCP client over the stdio of `kairos index mcp`.
struct Agent {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("pid", &self.child.id())
            .finish()
    }
}

impl Agent {
    fn start(root: &Path) -> Agent {
        let mut child = Command::new(KAIROS)
            .args(["index", "mcp", "--root"])
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start kairos index mcp");
        let stdin = child.stdin.take().expect("the stdin of the server");
        let stdout = child.stdout.take().expect("the stdout of the server");
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let mut agent = Agent {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let init = agent.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "index-bdd", "version": "0"}
            }),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        agent.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        agent
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write to the server");
        self.stdin.flush().expect("flush to the server");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(ANSWER_TIMEOUT)
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                panic!("the server wrote a line that is not JSON: {line}");
            };
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> ToolResult {
        let answer = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        if let Some(error) = answer.get("error") {
            return ToolResult {
                text: format!("PROTOCOL ERROR: {error}"),
                is_error: true,
            };
        }
        let result = &answer["result"];
        ToolResult {
            text: result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            is_error: result["isError"].as_bool().unwrap_or(false),
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Default, World)]
struct CliWorld {
    /// The copy of the fixture.
    copy: Option<TempDir>,
    /// The output of `kairos index build`.
    build_out: String,
    /// The output of `kairos index status`.
    status_out: String,
    /// The server of the scenario.
    agent: Option<Agent>,
    /// The result of the last tool call.
    result: Option<ToolResult>,
}

impl CliWorld {
    fn root(&self) -> &Path {
        self.copy.as_ref().expect("no copy of the fixture").path()
    }

    fn db(&self) -> PathBuf {
        self.root().join(INDEX_DB)
    }

    fn index(&self) -> Index {
        Index::open(&self.db()).expect("open the index")
    }

    /// A copy of the fixture in a new git repository.
    fn copy_fixture(&mut self) {
        let copy = TempDir::new().expect("make a folder for the copy");
        copy_tree(&fixture_root(), copy.path());
        git(copy.path(), &["init", "-q"]);
        self.copy = Some(copy);
    }

    /// Build and summarize the index of the copy with the library.
    fn build_index(&mut self) {
        let root = self.root().to_path_buf();
        let db = self.db();
        fs::create_dir_all(db.parent().expect("a folder")).expect("make .kairos");
        let options = BuildOptions {
            rust_analyzer: None,
            std_source: None,
            download: false,
        };
        build_structure_with(&root, &db, &options).unwrap_or_else(|e| panic!("{e}"));
        summarize(
            &root,
            &db,
            &mut Described::default(),
            &DeterministicProvider::default(),
            &SummarizeOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }

    fn agent(&mut self) -> &mut Agent {
        if self.agent.is_none() {
            if self.copy.is_none() {
                self.copy_fixture();
            }
            self.agent = Some(Agent::start(self.root()));
        }
        self.agent.as_mut().expect("the agent")
    }

    fn call(&mut self, tool: &str, arguments: Value) {
        let result = self.agent().call(tool, arguments);
        self.result = Some(result);
    }

    fn result(&self) -> &ToolResult {
        self.result.as_ref().expect("no tool was called")
    }

    /// The text of the last result, which must not be an error.
    fn text(&self) -> &str {
        let result = self.result();
        assert!(!result.is_error, "the tool gave an error: {}", result.text);
        &result.text
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("make a folder");
    for entry in fs::read_dir(from).expect("read the fixture") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy a file");
        }
    }
}

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success() || args.first() == Some(&"check-ignore"),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn kairos(args: &[&str], root: &Path) -> String {
    let out = Command::new(KAIROS)
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .expect("run kairos");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "kairos {args:?} failed with {}:\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// The number after `label` at the start of a line of `text`.
fn count_after(text: &str, label: &str) -> usize {
    let line = text
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("no line starts with {label:?}:\n{text}"));
    line[label.len()..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no count after {label:?}: {line}"))
}

/// The list lines of a result (`- ...`).
fn items(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with("- ")).collect()
}

/// The numbered lines of a result (`1. ...`).
fn numbered(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| {
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            digits > 0 && l[digits..].starts_with(". ")
        })
        .collect()
}

// --- Given --------------------------------------------------------------------

#[given("the polyglot fixture with no index")]
fn fixture_with_no_index(world: &mut CliWorld) {
    world.copy_fixture();
    assert!(!world.db().exists());
}

#[given("an index of the polyglot fixture")]
fn an_index(world: &mut CliWorld) {
    world.copy_fixture();
    fs::write(world.root().join("src/main.rs"), MAIN_RS).expect("write src/main.rs");
    world.build_index();
}

#[given("an index where a Python function has 1 certain caller and 1 possible caller")]
fn an_index_with_two_callers(world: &mut CliWorld) {
    world.copy_fixture();
    let csv = world.root().join("python/polyglot/csv_loader.py");
    let mut text = fs::read_to_string(&csv).expect("read csv_loader.py");
    text.push_str(LOAD_FIRST_PY);
    fs::write(&csv, text).expect("write csv_loader.py");
    world.build_index();
}

// --- When ---------------------------------------------------------------------

#[when(expr = "I run {string} and then {string}")]
fn run_build_and_status(world: &mut CliWorld, build: String, status: String) {
    assert_eq!(
        (build.as_str(), status.as_str()),
        ("kairos index build", "kairos index status")
    );
    let root = world.root().to_path_buf();
    world.build_out = kairos(&["index", "build"], &root);
    world.status_out = kairos(&["index", "status"], &root);
}

#[when("an agent calls code_search with a description of a Rust function")]
fn search_for_pop(world: &mut CliWorld) {
    world.call("code_search", json!({"query": POP_QUERY}));
}

#[when("an agent calls callers for that function")]
fn callers_of_load(world: &mut CliWorld) {
    world.call(
        "callers",
        json!({"symbol": "load", "file": "python/polyglot/csv_loader.py"}),
    );
}

#[when("the agent calls callers with possible edges on")]
fn callers_of_load_with_possible(world: &mut CliWorld) {
    world.call(
        "callers",
        json!({"symbol": "load", "file": "python/polyglot/csv_loader.py", "possible": true}),
    );
}

#[when("an agent calls path from the Rust entry point to a Rust function 3 calls deep")]
fn path_from_main(world: &mut CliWorld) {
    world.call("path", json!({"from": "main", "to": "Queue::push"}));
}

#[when("an agent calls symbol with an argument that the tool does not have")]
fn symbol_with_unknown_argument(world: &mut CliWorld) {
    world.call("symbol", json!({"symbol": "pop", "verbose": true}));
}

#[when("an agent calls duplicates")]
fn duplicates(world: &mut CliWorld) {
    world.call("duplicates", json!({}));
}

#[when("an agent calls duplicates with an argument that the tool does not have")]
fn duplicates_with_unknown_argument(world: &mut CliWorld) {
    world.call("duplicates", json!({"kind": "exact", "verbose": true}));
}

#[when("an agent calls module_map")]
fn module_map(world: &mut CliWorld) {
    world.call("module_map", json!({}));
}

// --- Then ---------------------------------------------------------------------

#[then("the status shows the count of files, symbols, edges and summaries")]
fn status_counts(world: &mut CliWorld) {
    let index = world.index();
    let files = index.files().expect("files");
    let symbols = index.symbols().expect("symbols");
    let edges = index.edges().expect("edges");
    let summaries = symbols.iter().filter(|s| s.summary_key.is_some()).count()
        + index
            .file_summary_keys()
            .expect("file keys")
            .iter()
            .filter(|(_, k)| k.is_some())
            .count()
        + index.modules().expect("modules").len();
    let out = &world.status_out;
    assert_eq!(count_after(out, "Files:"), files.len(), "{out}");
    assert_eq!(count_after(out, "Symbols:"), symbols.len(), "{out}");
    assert_eq!(count_after(out, "Edges:"), edges.len(), "{out}");
    assert_eq!(count_after(out, "Summaries:"), summaries, "{out}");
    assert!(!symbols.is_empty() && !edges.is_empty(), "{out}");
    // This binary has no summarizer: the build says so and why.
    assert!(
        world.build_out.contains("Summaries: not made."),
        "{}",
        world.build_out
    );
}

#[then("the index is in .kairos/index.db and git ignores it")]
fn the_index_is_ignored(world: &mut CliWorld) {
    assert!(world.db().is_file());
    let out = git(world.root(), &["check-ignore", "-q", INDEX_DB]);
    assert!(out.status.success(), "git does not ignore {INDEX_DB}");
    // No tracked file was changed for it.
    assert!(!world.root().join(".gitignore").exists());
    // The index did not index itself.
    assert!(
        world
            .index()
            .files()
            .expect("files")
            .iter()
            .all(|f| !f.path.starts_with(".kairos/index.db"))
    );
}

#[then("that function is in the first 3 results")]
fn pop_in_top_three(world: &mut CliWorld) {
    let text = world.text();
    let first: Vec<_> = numbered(text).into_iter().take(3).collect();
    assert!(
        first
            .iter()
            .any(|l| l.contains("Stack::pop") && l.contains("src/stack.rs")),
        "Stack::pop is not in the first 3 results:\n{text}"
    );
}

#[then("the result has the certain caller only")]
fn only_the_certain_caller(world: &mut CliWorld) {
    let text = world.text();
    let callers = items(text);
    assert_eq!(callers.len(), 1, "{text}");
    assert!(callers[0].contains("load_first"), "{text}");
    assert!(!text.contains("build_report"), "{text}");
}

#[then("the result has the 2 callers, and it marks the possible one")]
fn both_callers(world: &mut CliWorld) {
    let text = world.text();
    let callers = items(text);
    assert_eq!(callers.len(), 2, "{text}");
    let certain = callers
        .iter()
        .find(|l| l.contains("load_first"))
        .expect("load_first");
    let possible = callers
        .iter()
        .find(|l| l.contains("build_report"))
        .expect("build_report");
    assert!(
        certain.contains("certain") && !certain.contains("possible"),
        "{text}"
    );
    assert!(possible.contains("possible"), "{text}");
}

#[then("the result is the 3 calls in order")]
fn three_calls(world: &mut CliWorld) {
    let text = world.text();
    let steps = numbered(text);
    let expected = [
        ("main", "run"),
        ("run", "enqueue_all"),
        ("enqueue_all", "Queue::push"),
    ];
    assert_eq!(steps.len(), expected.len(), "{text}");
    for (step, (caller, callee)) in steps.iter().zip(expected) {
        assert!(
            step.contains(&format!(" {caller} calls {callee} ")),
            "the step {step:?} is not {caller} calls {callee}:\n{text}"
        );
    }
}

#[then("the call is refused, and the error names the argument")]
fn refused(world: &mut CliWorld) {
    let result = world.result();
    assert!(result.is_error, "the call was not refused: {}", result.text);
    assert!(
        result.text.starts_with("VALIDATION: "),
        "the refusal is not a tool error: {}",
        result.text
    );
    assert!(result.text.contains("\"verbose\""), "{}", result.text);
}

#[then("the result has the exact copy of checksum, with the files, the lines and a score")]
fn the_exact_copy(world: &mut CliWorld) {
    let text = world.text().to_string();
    let groups = numbered(&text);
    let exact = groups
        .iter()
        .position(|l| l.contains("exact") && l.contains("score 1.00"))
        .unwrap_or_else(|| panic!("no exact group with a score:\n{text}"));
    // The symbols of the group are the list lines after its numbered line.
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| *l == groups[exact])
        .expect("the group line");
    let symbols: Vec<&str> = lines[start + 1..]
        .iter()
        .take_while(|l| l.starts_with("- "))
        .copied()
        .collect();
    assert_eq!(
        symbols,
        [
            "- checksum (src/checksum.rs:16-22)",
            "- checksum (src/legacy.rs:13-24)"
        ],
        "{text}"
    );
    // Test code and small symbols are left out by default, and the result
    // says so.
    assert!(text.contains("Test code is left out."), "{text}");
    assert!(!text.contains("value (src/checksum.rs"), "{text}");
}

#[then(expr = "{string} gives the same groups")]
fn the_cli_gives_the_same(world: &mut CliWorld, command: String) {
    assert_eq!(command, "kairos index duplicates");
    let root = world.root().to_path_buf();
    let out = kairos(&["index", "duplicates"], &root);
    assert_eq!(out.trim_end(), world.text().trim_end());
}

#[then("the result has each module with its summary and its files")]
fn each_module(world: &mut CliWorld) {
    let index = world.index();
    let modules = index.modules().expect("modules");
    assert!(modules.len() >= 3, "{modules:?}");
    let mut files_of: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (path, key) in index.file_summary_keys().expect("file keys") {
        let Some(key) = key else { continue };
        let folder = match path.rsplit_once('/') {
            Some((folder, _)) => folder.to_string(),
            None => ".".to_string(),
        };
        let summary = index
            .summary(&key)
            .expect("read")
            .expect("a summary")
            .summary;
        files_of.entry(folder).or_default().push((path, summary));
    }
    let text = world.text().to_string();
    for (module, key) in &modules {
        let summary = index
            .summary(key)
            .expect("read")
            .expect("a summary")
            .summary;
        let header = format!("## {module}");
        let start = text
            .lines()
            .position(|l| l == header)
            .unwrap_or_else(|| panic!("no {header:?} in:\n{text}"));
        let section: Vec<&str> = text
            .lines()
            .skip(start + 1)
            .take_while(|l| !l.starts_with("## "))
            .collect();
        assert!(
            section.contains(&summary.as_str()),
            "{module} has no summary line:\n{text}"
        );
        for (file, file_summary) in files_of.get(module).into_iter().flatten() {
            let line = format!("- {file}: {file_summary}");
            assert!(
                section.contains(&line.as_str()),
                "{module} does not list {file}:\n{text}"
            );
        }
    }
}

#[tokio::main]
async fn main() {
    if !rust_analyzer::binary_path().is_some_and(|p| p.is_file()) {
        eprintln!(
            "The pinned {} is not in the cache, so each scenario that builds a Rust index fails. \
             Run `angreal dev fetch-rust-analyzer`.",
            rust_analyzer::VERSION
        );
    }
    if !rust_analyzer::std_source_archive_path().is_some_and(|p| p.is_file()) {
        eprintln!(
            "The pinned std source of Rust {} is not in the cache, so each scenario that builds \
             a Rust index fails. Run `angreal dev fetch-rust-analyzer`.",
            rust_analyzer::RUST_SRC_VERSION
        );
    }
    let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/features");
    CliWorld::cucumber()
        // Each index build runs rust-analyzer, which uses about 0.6 GB.
        .max_concurrent_scenarios(3)
        .fail_on_skipped()
        .run_and_exit(features)
        .await;
}
