//! The scenarios of `kairos index` and of the local code tools
//! (COLLIERY-T-1852). The `.feature` file is in `tests/features/`.
//!
//! Each scenario runs on a copy of the polyglot fixture of kairos-index,
//! with a git repository in it. The test is the agent: it starts the built
//! `kairos index mcp` and sends it JSON-RPC lines on stdin, as an MCP client
//! does. The indexes of the tool scenarios are built with the library and a
//! fake summarizer that gives fixed texts, and with the deterministic
//! vectors, so that each result is the same on each run.
//!
//! The scenarios of the base index in Kairos (COLLIERY-T-1854) are in
//! [`base`].

#[path = "index_bdd/base.rs"]
mod base;

use std::collections::{BTreeMap, BTreeSet};
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

/// The temp home of each `kairos` that a scenario runs (KAIROS-T-0712).
static HOME: std::sync::LazyLock<TempDir> =
    std::sync::LazyLock::new(|| tempfile::tempdir().expect("a temp home"));

/// The folder of the indexes of each `kairos` of a scenario
/// (KAIROS-A-0072): `KAIROS_INDEX_DIR` in the temp home.
fn index_home() -> PathBuf {
    HOME.path().join("index")
}

/// The index file of the checkout at `root`, by the rule of KAIROS-A-0072:
/// the folder name, `-`, and 16 hex digits of the SHA-256 of the canonical
/// path.
fn index_db(root: &Path) -> PathBuf {
    use sha2::Digest;
    let root = root.canonicalize().expect("the checkout");
    let digest = sha2::Sha256::digest(root.as_os_str().as_encoded_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let name: String = root
        .file_name()
        .expect("a folder name")
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    index_home().join(format!("{name}-{hex}")).join("index.db")
}

/// A clean environment for a `kairos` of a scenario (KAIROS-T-0712): no
/// `KAIROS_*` variable of the person (the deployment, the key), no session
/// folder, and a temp home, config folder and cache folder. A scenario
/// that needs a variable sets it after this.
fn clean_env(command: &mut Command) -> &mut Command {
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("KAIROS_") {
            command.env_remove(&name);
        }
    }
    let home = HOME.path();
    command
        .env_remove("CLAUDE_PROJECT_DIR")
        .env("KAIROS_INDEX_DIR", index_home())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("KAIROS_CONFIG_DIR", home.join(".config/kairos"))
}

/// The index in a checkout.
/// The old place of the index in a checkout (before KAIROS-T-0709).
const OLD_INDEX_DB: &str = ".kairos/index.db";

/// How long the agent waits for one answer of the server.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../kairos-index/tests/fixtures/polyglot")
}

/// The entry point that the path scenario starts from. The fixture has no
/// binary, so each tool scenario adds this one: `main` calls `run`, `run`
/// calls `enqueue_all`, and `enqueue_all` calls `Queue::push`. `main` also
/// calls `routes::dispatch`, which reaches `routes::target` by 2 routes
/// (COLLIERY-T-2531).
const MAIN_RS: &str = r#"//! The entry point of the polyglot fixture (COLLIERY-T-1852).

fn main() {
    println!("{} {}", run(), polyglot::routes::dispatch(true));
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
    // KAIROS-T-0421: test code gets a summary too.
    (
        "python/tests/test_report.py",
        "test_render_joins_rows",
        "Checks that a report puts each row on its own line.",
    ),
];

/// A search for what a test checks, with no word of its name. It has the
/// word "test", so test code ranks with the other code (KAIROS-T-0422).
const TEST_QUERY: &str = "which test puts each row on its own line";

/// The description that the agent searches with: the words of the summary
/// of `Stack::pop`, written as a question.
const POP_QUERY: &str = "which function removes the newest number, or None when empty";

/// A fake summarizer with fixed texts for some symbols.
#[derive(Default)]
struct Described {
    fake: FakeSummarizer,
}

impl Summarizer for Described {
    fn model(&self) -> String {
        "fake/described".to_string()
    }

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
        Self::start_with_env(root, &[])
    }

    /// [`Self::start`] with these environment variables for the server
    /// (the Kairos deployment of a base-index scenario).
    fn start_with_env(root: &Path, env: &[(&str, &str)]) -> Agent {
        let mut command = Command::new(KAIROS);
        clean_env(&mut command)
            .args(["index", "mcp", "--root"])
            .arg(root);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command
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
    /// The texts of some calls of one step.
    texts: Vec<String>,
    /// The scenarios of the base index.
    base: base::BaseWorld,
    /// The folder above the checkouts of a scenario with more than one
    /// (#272), and the folder of its config.
    board: Option<(TempDir, TempDir)>,
}

impl CliWorld {
    fn root(&self) -> &Path {
        self.copy.as_ref().expect("no copy of the fixture").path()
    }

    fn db(&self) -> PathBuf {
        index_db(self.root())
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
        build_index_at(self.root());
    }
}

/// Build and summarize the index of the checkout at `root` with the library.
fn build_index_at(root: &Path) {
    {
        let root = root.to_path_buf();
        let db = index_db(&root);
        fs::create_dir_all(db.parent().expect("a folder")).expect("make the index folder");
        let options = BuildOptions {
            rust_analyzer: None,
            std_source: None,
            sysroot: None,
            download: false,
            ..Default::default()
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
}

impl CliWorld {
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
    let out = clean_env(&mut Command::new(KAIROS))
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

/// The deterministic vectors are a hash of the text: their rank is noise.
/// A scenario that checks the text rank removes them (KAIROS-T-0423).
#[given("an index of the polyglot fixture whose summaries have no vectors")]
fn an_index_with_no_vectors(world: &mut CliWorld) {
    an_index(world);
    let conn = rusqlite::Connection::open(world.db()).expect("open the index");
    conn.execute_batch(
        "UPDATE summaries SET vector = NULL;
         DELETE FROM pool_meta WHERE name = 'vector_model';",
    )
    .expect("remove the vectors");
}

#[given("the polyglot fixture, where main reaches target by 2 different routes")]
fn an_index_with_two_routes(world: &mut CliWorld) {
    world.copy_fixture();
    let routes = fs::read_to_string(world.root().join("src/routes.rs")).expect("read routes.rs");
    assert!(routes.contains("direct()") && routes.contains("indirect()"));
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

// --- More than one open checkout (#272) ---------------------------------------

/// The 2 checkouts of the scenario, under the board folder.
fn board_checkout(world: &CliWorld, name: &str) -> PathBuf {
    let (board, _) = world.board.as_ref().expect("no board folder");
    board
        .path()
        .canonicalize()
        .expect("the board folder")
        .join(name)
}

#[given("a folder with 2 indexed checkouts, service and engine, where only engine has engine_only")]
fn two_indexed_checkouts(world: &mut CliWorld) {
    let board = TempDir::new().expect("a board folder");
    let config = TempDir::new().expect("a config folder");
    for name in ["service", "engine"] {
        let root = board.path().join(name);
        fs::create_dir_all(&root).expect("the checkout");
        copy_tree(&fixture_root(), &root);
        if name == "engine" {
            fs::write(
                root.join("src/engine_only.rs"),
                "/// Only engine has this.\npub fn engine_only() -> u32 {\n    7\n}\n",
            )
            .expect("write src/engine_only.rs");
        }
        git(&root, &["init", "-q"]);
        build_index_at(&root);
    }
    world.board = Some((board, config));
}

#[when("an agent started in that folder calls fetch_index for service, then for engine")]
fn fetch_both(world: &mut CliWorld) {
    let (board, config) = world.board.as_ref().expect("no board folder");
    // No Kairos: `fetch_index` opens the local index of each checkout.
    let agent = Agent::start_with_env(
        board.path(),
        &[
            ("KAIROS_CONFIG_DIR", config.path().to_str().expect("utf-8")),
            ("KAIROS_URL", "http://127.0.0.1:9"),
            ("KAIROS_KEY", ""),
            ("KAIROS_MCP_KEY", ""),
        ],
    );
    world.agent = Some(agent);
    for name in ["service", "engine"] {
        let root = board_checkout(world, name);
        world.call("fetch_index", json!({"root": root}));
        let text = world.text().to_string();
        assert!(
            text.starts_with(&format!(
                "The server opened the index of {}.",
                root.display()
            )),
            "{text}"
        );
        world.texts.push(text);
    }
}

#[then("the second fetch_index says that 2 checkouts are open: service, engine")]
fn two_are_open(world: &mut CliWorld) {
    let first = &world.texts[0];
    assert!(!first.contains("checkouts are open"), "{first}");
    let second = &world.texts[1];
    assert!(
        second.contains(
            "2 checkouts are open: service, engine. Give `checkout` to read another one."
        ),
        "{second}"
    );
}

#[when(expr = "the agent calls code_search for {string}")]
fn code_search_for(world: &mut CliWorld, query: String) {
    world.call("code_search", json!({"query": query, "limit": 50}));
}

#[then("the results come from both checkouts, each marked with its name")]
fn results_from_both(world: &mut CliWorld) {
    let text = world.text();
    assert!(text.starts_with("service: The search used "), "{text}");
    assert!(text.contains("\nengine: The search used "), "{text}");
    let results = numbered(text);
    assert!(!results.is_empty(), "{text}");
    for line in &results {
        assert!(
            line.contains(". [service] ") || line.contains(". [engine] "),
            "{line}"
        );
    }
    assert!(results.iter().any(|l| l.contains("[service]")), "{text}");
    assert!(results.iter().any(|l| l.contains("[engine]")), "{text}");
}

#[when("the agent calls symbol for \"engine_only\" with no checkout")]
fn engine_only_default(world: &mut CliWorld) {
    world.call("symbol", json!({"symbol": "engine_only"}));
}

#[then("the result is the function of engine")]
fn is_engine_only(world: &mut CliWorld) {
    let text = world.text();
    assert!(text.starts_with("engine_only (function, rust)"), "{text}");
    assert!(text.contains("File: src/engine_only.rs:2-4"), "{text}");
}

#[when(expr = "the agent calls symbol for \"engine_only\" in the checkout {string}")]
fn engine_only_in(world: &mut CliWorld, checkout: String) {
    world.call(
        "symbol",
        json!({"symbol": "engine_only", "checkout": checkout}),
    );
}

#[then("the server says that no symbol has that name")]
fn no_engine_only(world: &mut CliWorld) {
    let r = world.result();
    assert!(r.is_error, "{}", r.text);
    assert_eq!(r.text, "NOT_FOUND: No symbol has the name \"engine_only\".");
}

#[then("the call is refused, and the error names the 2 open checkouts")]
fn refused_unknown_checkout(world: &mut CliWorld) {
    let r = world.result();
    assert!(r.is_error, "{}", r.text);
    let service = board_checkout(world, "service");
    let engine = board_checkout(world, "engine");
    assert!(
        r.text.starts_with(&format!(
            "VALIDATION: No open checkout is \"nowhere\". The open checkouts are: {}, {}.",
            service.display(),
            engine.display()
        )),
        "{}",
        r.text
    );
    assert!(r.text.contains("\"field\":\"checkout\""), "{}", r.text);
}

// --- When ---------------------------------------------------------------------

/// Two commands, one after the other. The outputs are kept in `build_out`
/// and `status_out`.
#[when(expr = "I run {string} and then {string}")]
fn run_build_and_status(world: &mut CliWorld, first: String, second: String) {
    assert!(
        [
            ("kairos index build", "kairos index status"),
            ("kairos index build --json", "kairos index status --json"),
            ("kairos index update --link-only", "kairos index status"),
            (
                "kairos index duplicates --json",
                "kairos index references Queue --file src/queue.rs --json"
            ),
        ]
        .contains(&(first.as_str(), second.as_str())),
        "no step runs {first:?} and {second:?}"
    );
    let root = world.root().to_path_buf();
    let args = |command: &str| -> Vec<String> {
        command
            .strip_prefix("kairos ")
            .expect("a kairos command")
            .split(' ')
            .map(str::to_string)
            .collect()
    };
    let first = args(&first);
    let second = args(&second);
    world.build_out = kairos(&first.iter().map(String::as_str).collect::<Vec<_>>(), &root);
    world.status_out = kairos(
        &second.iter().map(String::as_str).collect::<Vec<_>>(),
        &root,
    );
}

/// The output of a command: one JSON object, and nothing else.
fn one_object(out: &str) -> Value {
    let value: Value = serde_json::from_str(out)
        .unwrap_or_else(|e| panic!("the output is not one JSON value ({e}):\n{out}"));
    assert!(value.is_object(), "{out}");
    value
}

/// KAIROS-T-0654.
#[then("each output is one JSON object with the counts of the index")]
fn json_counts(world: &mut CliWorld) {
    let index = world.index();
    let files = index.files().expect("files").len();
    let symbols = index.symbols().expect("symbols").len();
    let edges = index.edges().expect("edges").len();
    let build = one_object(&world.build_out);
    let out = &world.build_out;
    assert_eq!(build["command"], "build", "{out}");
    assert_eq!(build["start"], Value::Null, "{out}");
    assert!(
        build["index"]
            .as_str()
            .is_some_and(|p| Path::new(p) == world.db()),
        "{out}"
    );
    assert_eq!(build["moved_from"], Value::Null, "{out}");
    assert_eq!(build["structure"]["files"], files, "{out}");
    assert_eq!(build["structure"]["symbols"], symbols, "{out}");
    assert_eq!(build["structure"]["edges"]["total"], edges, "{out}");
    // This binary has no summarizer: the object says so and why.
    assert_eq!(build["summaries"]["result"], "not_made", "{out}");
    assert!(
        build["summaries"]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("This kairos binary has no summarizer.")),
        "{out}"
    );
    let status = one_object(&world.status_out);
    let out = &world.status_out;
    assert_eq!(status["index"], build["index"], "{out}");
    assert_eq!(status["files"], files, "{out}");
    assert_eq!(status["symbols"], symbols, "{out}");
    assert_eq!(status["edges"]["total"], edges, "{out}");
}

/// KAIROS-T-0654: the envelope of KAIROS-A-0057, with the rows of the text.
#[then("each output is the list envelope, with the rows of the text")]
fn json_lists(world: &mut CliWorld) {
    let root = world.root().to_path_buf();
    let groups = one_object(&world.build_out);
    let out = &world.build_out;
    let text = kairos(&["index", "duplicates"], &root);
    let items = groups["items"].as_array().expect("items");
    assert_eq!(items.len(), numbered(&text).len(), "{out}\n{text}");
    assert_eq!(groups["total"], items.len(), "{out}");
    assert_eq!(groups["next_offset"], Value::Null, "{out}");
    assert!(
        items.iter().any(|g| g["kind"] == "exact"
            && g["symbols"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s["name"] == "checksum"))),
        "no exact copy of checksum: {out}"
    );
    let uses = one_object(&world.status_out);
    let out = &world.status_out;
    let items = uses["items"].as_array().expect("items");
    assert_eq!(items.len(), 8, "{out}");
    assert_eq!(uses["total"], 8, "{out}");
    assert_eq!(uses["next_offset"], Value::Null, "{out}");
    assert_eq!(uses["symbol"]["qualified"], "Queue", "{out}");
    assert_eq!(uses["symbol"]["file"], "src/queue.rs", "{out}");
    let kinds = |kind: &str| items.iter().filter(|u| u["kind"] == kind).count();
    assert_eq!(
        (kinds("type"), kinds("field"), kinds("import")),
        (4, 3, 1),
        "{out}"
    );
    assert_eq!(items[0]["file"], "src/lib.rs", "{out}");
    assert_eq!(items[0]["enclosing"], Value::Null, "{out}");
    assert_eq!(items[1]["enclosing"]["name"], "enqueue_all", "{out}");
}

#[when("an agent calls code_search with a description of a Rust function")]
fn search_for_pop(world: &mut CliWorld) {
    world.call("code_search", json!({"query": POP_QUERY}));
}

#[when("an agent calls code_search with what a test checks")]
fn search_for_a_test(world: &mut CliWorld) {
    world.call("code_search", json!({"query": TEST_QUERY}));
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

#[when("an agent calls path from main to target")]
fn path_from_main_to_target(world: &mut CliWorld) {
    world.call("path", json!({"from": "main", "to": "target"}));
}

#[when("an agent calls symbol with an argument that the tool does not have")]
fn symbol_with_unknown_argument(world: &mut CliWorld) {
    world.call("symbol", json!({"symbol": "pop", "verbose": true}));
}

// KAIROS-T-0419: each use of a symbol.

#[when(expr = "an agent calls references for {string} in {string}")]
fn references_in_file(world: &mut CliWorld, name: String, file: String) {
    world.call("references", json!({"symbol": name, "file": file}));
}

// KAIROS-T-0703: the places that cite an ADR.

#[given(expr = "an index of the polyglot fixture, where 2 files cite {string}")]
fn an_index_with_citations(world: &mut CliWorld, code: String) {
    world.copy_fixture();
    fs::write(world.root().join("src/main.rs"), MAIN_RS).expect("write src/main.rs");
    fs::write(
        world.root().join("src/decisions.rs"),
        format!(
            "/// A retried write is safe: {code}.\n\
             pub fn retry_write() -> u32 {{\n\
             \x20   // The key is kept for 24 hours ({code}).\n\
             \x20   24\n\
             }}\n"
        ),
    )
    .expect("write src/decisions.rs");
    fs::write(
        world.root().join("migrations/002_keys.sql"),
        format!(
            "-- {code}: one row for each caller and key.\nCREATE TABLE request_keys (key TEXT);\n"
        ),
    )
    .expect("write the migration");
    world.build_index();
}

#[when(expr = "an agent calls references for {string}")]
fn references_of(world: &mut CliWorld, name: String) {
    world.call("references", json!({"symbol": name}));
}

#[then(
    expr = "the result has the {int} places that cite {string} by file, each with its line, its context and its symbol"
)]
fn the_citing_places(world: &mut CliWorld, count: usize, code: String) {
    let text = world.text();
    assert!(
        text.starts_with(&format!(
            "{count} places cite {code} in 2 files: 1 doc, 2 comment."
        )),
        "{text}"
    );
    assert!(
        text.contains("\nmigrations/002_keys.sql:\n- line 1: comment, at the top of the file"),
        "{text}"
    );
    assert!(
        text.contains("\nsrc/decisions.rs:\n- line 1: doc, about retry_write\n- line 3: comment, about retry_write"),
        "{text}"
    );
}

#[then("the call is refused, because an ADR short code takes no file")]
fn refused_for_a_code(world: &mut CliWorld) {
    let result = world.result();
    assert!(result.is_error, "the call was not refused: {}", result.text);
    assert!(result.text.starts_with("VALIDATION: "), "{}", result.text);
    assert!(
        result.text.contains("an ADR short code takes no `file`"),
        "{}",
        result.text
    );
}

#[then(expr = "the result says that no place cites {string}")]
fn no_place_cites(world: &mut CliWorld, code: String) {
    let text = world.text();
    assert!(
        text.starts_with(&format!(
            "No place in the code of this checkout cites {code}."
        )),
        "{text}"
    );
}

#[when("an agent calls references with an argument that the tool does not have")]
fn references_with_unknown_argument(world: &mut CliWorld) {
    world.call("references", json!({"symbol": "Queue", "verbose": true}));
}

#[then(
    "the result has the uses of the type by file, each with its line, its kind and the symbol that holds it"
)]
fn uses_of_the_type(world: &mut CliWorld) {
    let text = world.text();
    let expected = "8 uses of Queue (src/queue.rs:4) in 2 files: 4 type, 3 field, 1 import.\n\
         src/lib.rs:\n\
         - line 8: import, at the top of the file\n\
         - line 12: type, in enqueue_all\n\
         - line 13: type, in enqueue_all\n\
         src/queue.rs:\n\
         - line 8: type, in Queue\n\
         - line 10: type, in Queue::new\n\
         - line 11: field, in Queue::new\n\
         - line 16: field, in Queue::push\n\
         - line 20: field, in Queue::len";
    assert_eq!(text, expected);
}

#[then("the result has the 2 calls, and it says that it has the calls only for python code")]
fn python_calls_only(world: &mut CliWorld) {
    let text = world.text();
    assert!(
        text.starts_with(
            "2 uses of Report (python/polyglot/report.py:9) in 2 files: 2 call. For python \
             code, the index has the calls only:"
        ),
        "{text}"
    );
    assert!(
        text.contains("python/polyglot/report.py:\n- line 19: call, in build_report, by name"),
        "{text}"
    );
}

#[then(expr = "{string} gives the same text")]
fn the_cli_gives_the_same_text(world: &mut CliWorld, command: String) {
    let args: Vec<&str> = command
        .strip_prefix("kairos ")
        .expect("a kairos command")
        .split(' ')
        .collect();
    let root = world.root().to_path_buf();
    let out = kairos(&args, &root);
    assert_eq!(out.trim_end(), world.text().trim_end());
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

// KAIROS-T-0472: the Terraform view of the module map. A folder with no
// Terraform code has its summary and its files only.
#[then("the Terraform module shows its resources and the root shows its module calls")]
fn terraform_module_map(world: &mut CliWorld) {
    let text = world.text();
    let section = |path: &str| -> String {
        let start = text
            .find(&format!("## {path}\n"))
            .unwrap_or_else(|| panic!("no section {path}: {text}"));
        let rest = &text[start + 3..];
        rest[..rest.find("\n## ").unwrap_or(rest.len())].to_string()
    };
    let bucket = section("infra/modules/bucket");
    for line in [
        "resources: aws_s3_bucket (this [count]), aws_sns_topic (events [for_each])",
        "variables: name, owner",
        "outputs: arn",
        "called by: infra (module.store)",
    ] {
        assert!(bucket.contains(&format!("\n{line}\n")), "{line}: {bucket}");
    }
    let root = section("infra");
    for line in [
        "module calls: store = ./modules/bucket, network = terraform-aws-modules/vpc/aws 5.0.0",
        "calls: module.store > infra/modules/bucket, module.network > terraform-aws-modules/vpc/aws (remote)",
    ] {
        assert!(root.contains(&format!("\n{line}\n")), "{line}: {root}");
    }
    let src = section("src");
    let terraform = [
        "resources:",
        "data:",
        "module calls:",
        "variables:",
        "outputs:",
        "called by:",
        "calls:",
    ];
    assert!(
        !src.lines()
            .any(|l| terraform.iter().any(|t| l.starts_with(t))),
        "a folder with no Terraform code: {src}"
    );
}

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
        world
            .build_out
            .contains("This kairos binary has no summarizer."),
        "{}",
        world.build_out
    );
}

#[then("the index is in the user cache and no file of it is in the checkout")]
fn the_index_is_in_the_cache(world: &mut CliWorld) {
    assert!(world.db().is_file(), "{}", world.db().display());
    assert!(world.db().starts_with(index_home()));
    // KAIROS-A-0072: no file of the index in the checkout, no tracked file
    // changed, and no line in the excludes of git.
    assert!(!world.root().join(".kairos").exists());
    assert!(!world.root().join(".gitignore").exists());
    let exclude =
        std::fs::read_to_string(world.root().join(".git/info/exclude")).unwrap_or_default();
    assert!(!exclude.contains("index.db"), "{exclude}");
    let status = git(world.root(), &["status", "--porcelain", "--ignored"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(!status.contains(".kairos"), "{status}");
}

#[given("the polyglot fixture with an index of an older CLI in .kairos/index.db")]
fn an_old_index(world: &mut CliWorld) {
    world.copy_fixture();
    let old = world.root().join(OLD_INDEX_DB);
    fs::create_dir_all(old.parent().expect("a folder")).expect("make .kairos");
    kairos_index::build_structure(world.root(), &old).expect("an old index");
}

#[then("the old index is moved to the user cache")]
fn the_old_index_moved(world: &mut CliWorld) {
    assert!(!world.root().join(OLD_INDEX_DB).exists());
    assert!(!world.root().join(".kairos").exists());
    assert!(world.db().is_file());
    assert!(world.index().counts().expect("counts").files > 0);
    assert!(
        world.status_out.contains(&world.db().display().to_string()),
        "{}",
        world.status_out
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

#[then("the first result is that test, marked as test code, with its summary")]
fn the_test_first(world: &mut CliWorld) {
    let text = world.text();
    let first = numbered(text).into_iter().next().expect("no result");
    assert!(
        first.contains("test_render_joins_rows (python/tests/test_report.py:")
            && first.contains(", function, test code: Checks that a report puts each row"),
        "the test is not the first result:\n{text}"
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

#[then("the result has the 2 routes, shortest first")]
fn two_routes(world: &mut CliWorld) {
    let text = world.text();
    // Each chain starts with a line "Chain N, ..." and has its calls as
    // numbered lines.
    let mut chains: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Chain ") {
            chains.push(Vec::new());
        } else if let Some(chain) = chains.last_mut()
            && !numbered(line).is_empty()
        {
            chain.push(line);
        }
    }
    let expected: [&[(&str, &str)]; 2] = [
        &[
            ("main", "dispatch"),
            ("dispatch", "direct"),
            ("direct", "target"),
        ],
        &[
            ("main", "dispatch"),
            ("dispatch", "indirect"),
            ("indirect", "relay"),
            ("relay", "target"),
        ],
    ];
    assert_eq!(chains.len(), expected.len(), "{text}");
    for (chain, want) in chains.iter().zip(expected) {
        assert_eq!(chain.len(), want.len(), "{text}");
        for (step, (caller, callee)) in chain.iter().zip(want) {
            assert!(
                step.contains(&format!(" {caller} calls {callee} ")),
                "the step {step:?} is not {caller} calls {callee}:\n{text}"
            );
        }
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
    // A tool that takes `symbol` lists `line` too (KAIROS-T-0417).
    // `duplicates` has neither.
    if result.text.contains("\"symbol\"") {
        assert!(result.text.contains("\"line\""), "{}", result.text);
    }
    // `symbol` lists `source` too (KAIROS-T-0420). `callers` and
    // `references` list `possible` in its place.
    if result.text.contains("\"symbol\"") && !result.text.contains("\"possible\"") {
        assert!(result.text.contains("\"source\""), "{}", result.text);
    }
}

// KAIROS-T-0417: the choice of one symbol.

#[when(expr = "an agent calls symbol and callers for {string} in {string}")]
fn symbol_and_callers_in_file(world: &mut CliWorld, name: String, file: String) {
    world.call("symbol", json!({"symbol": name, "file": file}));
    let symbol = world.text().to_string();
    world.call("callers", json!({"symbol": name, "file": file}));
    let callers = world.text().to_string();
    world.texts = vec![symbol, callers];
}

#[then("the 2 results are about the struct")]
fn about_the_struct(world: &mut CliWorld) {
    let [symbol, callers] = world.texts.as_slice() else {
        panic!("not 2 results: {:?}", world.texts);
    };
    assert!(
        symbol.starts_with("Checksum (struct, rust)\nFile: src/checksum.rs:6-"),
        "{symbol}"
    );
    assert!(
        callers.contains(" of Checksum (src/checksum.rs:6):"),
        "{callers}"
    );
}

#[when(expr = "an agent calls symbol for {string} with no file")]
fn symbol_with_no_file(world: &mut CliWorld, name: String) {
    world.call("symbol", json!({"symbol": name}));
}

#[then("the error lists the 2 structs and tells the agent to give the file")]
fn lists_the_structs(world: &mut CliWorld) {
    let result = world.result();
    assert!(result.is_error, "{}", result.text);
    assert_eq!(
        result.text,
        "AMBIGUOUS: 2 symbols have the name \"Checksum\". Give the argument `file` to choose \
         one. Or give a candidate from the list as `symbol`.\ndetails: {\"candidates\":\
         [\"Checksum (src/checksum.rs:6)\",\"Checksum (src/legacy.rs:4)\"]}"
    );
}

#[then("each candidate in the list, given back as the symbol, gives that symbol")]
fn candidates_given_back(world: &mut CliWorld) {
    for (candidate, file, line) in [
        ("Checksum (src/checksum.rs:6)", "src/checksum.rs", 6),
        ("Checksum (src/legacy.rs:4)", "src/legacy.rs", 4),
    ] {
        world.call("symbol", json!({ "symbol": candidate }));
        let text = world.text();
        assert!(
            text.starts_with(&format!("Checksum (struct, rust)\nFile: {file}:{line}-")),
            "{candidate}: {text}"
        );
    }
    // The impl block, by its line.
    world.call(
        "symbol",
        json!({"symbol": "Checksum", "file": "src/checksum.rs", "line": 10}),
    );
    let text = world.text();
    assert!(
        text.starts_with("Checksum (implementation, rust)\nFile: src/checksum.rs:10-"),
        "{text}"
    );
}

// KAIROS-T-0422: a helper in a Rust tests/ folder is test code.

#[when(expr = "an agent calls symbol, code_search and module_map for {string} in {string}")]
fn symbol_search_and_map(world: &mut CliWorld, name: String, file: String) {
    world.call("symbol", json!({"symbol": name, "file": file}));
    let symbol = world.text().to_string();
    world.call("code_search", json!({"query": name, "limit": 50}));
    let search = world.text().to_string();
    world.call("module_map", json!({}));
    let map = world.text().to_string();
    world.texts = vec![symbol, search, map];
}

#[then("each of the 3 results marks tests/common/mod.rs and its helper as test code")]
fn the_helper_is_test_code(world: &mut CliWorld) {
    let [symbol, search, map] = world.texts.as_slice() else {
        panic!("not 3 results: {:?}", world.texts);
    };
    assert!(
        symbol.starts_with("helper (function, rust)\nFile: tests/common/mod.rs:")
            && symbol.contains("\nThis is test code."),
        "{symbol}"
    );
    assert!(
        numbered(search)
            .iter()
            .any(|l| l.contains("helper (tests/common/mod.rs:")
                && l.contains(", function, test code")),
        "{search}"
    );
    assert!(
        map.lines()
            .any(|l| l.starts_with("- tests/common/mod.rs") && l.contains("test code")),
        "{map}"
    );
}

// KAIROS-T-0420: the source of a symbol.

#[when(expr = "an agent calls symbol for {string} in {string} with source")]
fn symbol_with_source(world: &mut CliWorld, name: String, file: String) {
    world.call(
        "symbol",
        json!({"symbol": name, "file": file, "source": true}),
    );
}

#[then("the result has the source of the struct, from the checkout")]
fn the_source_of_the_struct(world: &mut CliWorld) {
    let text = world.text();
    assert!(
        text.ends_with(
            "\nSource: lines 6-8, 37 bytes.\n```rust\npub struct Checksum {\n    sum: u32,\n}\n```"
        ),
        "{text}"
    );
    assert!(!text.contains("Warning:"), "{text}");
}

#[when(expr = "the file {string} changes and the agent calls symbol with source again")]
fn the_file_changes(world: &mut CliWorld, file: String) {
    let path = world.root().join(&file);
    let old = fs::read_to_string(&path).expect("read the file");
    fs::write(&path, format!("// A new first line.\n{old}")).expect("write the file");
    world.call(
        "symbol",
        json!({"symbol": "Checksum", "file": file, "source": true}),
    );
}

#[then("the result says that the source can be different from the symbol")]
fn the_source_can_differ(world: &mut CliWorld) {
    let text = world.text();
    assert!(
        text.contains(
            "Source: lines 6-8, 37 bytes. Warning: the file changed after the last index \
             update. The source can be different from the symbol. Run `kairos index update`."
        ),
        "{text}"
    );
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
    let tests: BTreeSet<String> = index
        .files()
        .expect("files")
        .into_iter()
        .filter(|f| f.decision == "test")
        .map(|f| f.path)
        .collect();
    let mut summarized_tests = 0;
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
            // A test file has a summary too, and the mark (KAIROS-T-0421).
            let line = if tests.contains(file) {
                summarized_tests += 1;
                format!("- {file} (test code): {file_summary}")
            } else {
                format!("- {file}: {file_summary}")
            };
            assert!(
                section.contains(&line.as_str()),
                "{module} does not list {file}:\n{text}"
            );
        }
    }
    assert!(summarized_tests > 0, "no test file has a summary:\n{text}");
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
