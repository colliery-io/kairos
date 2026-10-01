//! The BDD scenarios of kairos-index (COLLIERY-I-0264, "The tests: behaviour
//! first"). The `.feature` files are in `tests/features/`; the steps are here.
//! They run on the fixture repository in `tests/fixtures/polyglot/`, whose
//! `expected-symbols.toml` and `expected-edges.toml` state what the index
//! must hold.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cucumber::{World, given, then, when};
use kairos_embed::DeterministicProvider;
use kairos_index::{
    BuildReport, EdgeRecord, FakeSummarizer, FileRecord, Index, Level, SUMMARIZED_KINDS,
    SummarizeOptions, SummaryReport, SummaryRequest, SymbolRecord, SymbolRef, build_structure,
    summarize,
};
use serde::Deserialize;
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/polyglot")
}

#[derive(Debug, Default, World)]
struct IndexWorld {
    /// The repository that the scenario indexes.
    root: PathBuf,
    /// Holds the copy of the fixture when a scenario changes it.
    repo_copy: Option<TempDir>,
    /// Holds the index files of the scenario.
    out: Option<TempDir>,
    /// The index that each build wrote, in order.
    indexes: Vec<PathBuf>,
    /// The report of the last build.
    report: Option<BuildReport>,
    /// The files of the repository before the build, for the scenarios that
    /// check that a build writes nothing into it.
    files_before: Option<BTreeSet<String>>,
    /// The call that a call-graph scenario looks at.
    focus: Option<Focus>,
    /// The requests that the summarizer got in the last summary run.
    requests: Vec<SummaryRequest>,
    /// The report of the last summary run.
    summary_report: Option<SummaryReport>,
    /// The symbols before a change, for the scenarios that compare keys.
    symbols_before: Option<Vec<SymbolRecord>>,
    /// The symbol that a summary scenario looks at: file, name, container.
    changed: Option<(String, String, String)>,
    /// The model file, when it is on disk.
    model: Option<PathBuf>,
}

/// A call that a scenario looks at: the calling function and the called
/// name, with the target that the scenario expects.
#[derive(Debug, Clone, Default)]
struct Focus {
    caller_file: String,
    caller_name: String,
    called: String,
    callee: Option<(String, String, String)>,
    candidates: BTreeSet<(String, String)>,
}

impl IndexWorld {
    fn index(&self) -> Index {
        let db = self.indexes.last().expect("no index was built");
        Index::open(db).expect("open the index")
    }

    fn files(&self) -> Vec<FileRecord> {
        self.index().files().expect("read the files")
    }

    fn symbols(&self) -> Vec<SymbolRecord> {
        self.index().symbols().expect("read the symbols")
    }

    fn build(&mut self) {
        let out = self
            .out
            .get_or_insert_with(|| TempDir::new().expect("make a folder for the index"));
        let db = out
            .path()
            .join(format!("index-{}.sqlite", self.indexes.len()));
        let report = build_structure(&self.root, &db).unwrap_or_else(|e| panic!("{e}"));
        self.report = Some(report);
        self.indexes.push(db);
    }

    /// Summarize the last index (build one first if there is none) with the
    /// fake summarizer and the deterministic vectors.
    fn summarize_with_fake(&mut self) {
        if self.indexes.is_empty() {
            self.build();
        }
        let db = self.indexes.last().expect("no index was built").clone();
        let mut fake = FakeSummarizer::default();
        let report = summarize(
            &self.root,
            &db,
            &mut fake,
            &DeterministicProvider::default(),
            &SummarizeOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.requests = fake.requests;
        self.summary_report = Some(report);
    }

    fn edges(&self) -> Vec<EdgeRecord> {
        self.index().edges().expect("read the edges")
    }

    /// The edges of the focused call: from the focused function, to the
    /// focused name.
    fn focus_edges(&self) -> Vec<EdgeRecord> {
        let focus = self.focus.as_ref().expect("no call is in focus");
        let found: Vec<_> = self
            .edges()
            .into_iter()
            .filter(|e| {
                e.caller.file == focus.caller_file
                    && e.caller.name == focus.caller_name
                    && calls_name(e, &focus.called)
            })
            .collect();
        assert!(
            !found.is_empty(),
            "no edge from {} in {} calls {}",
            focus.caller_name,
            focus.caller_file,
            focus.called
        );
        found
    }
}

/// Whether an edge calls `name`: its callee has that name, or the called
/// name is `name` or ends with it after `::` or `.`.
fn calls_name(edge: &EdgeRecord, name: &str) -> bool {
    edge.callee.as_ref().is_some_and(|c| c.name == name)
        || edge.callee_name == name
        || edge.callee_name.ends_with(&format!("::{name}"))
        || edge.callee_name.ends_with(&format!(".{name}"))
}

fn files_under(root: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).expect("read a folder") {
            let entry = entry.expect("read a folder entry");
            let path = entry.path();
            out.insert(
                path.strip_prefix(root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned(),
            );
            if entry.file_type().expect("read a file type").is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out);
    out
}

// --- expected-symbols.toml ----------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    file: Vec<ExpectedFile>,
    symbol: Vec<ExpectedSymbol>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedFile {
    path: String,
    decision: String,
    rule: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
#[serde(deny_unknown_fields)]
struct ExpectedSymbol {
    file: String,
    name: String,
    container: String,
    kind: String,
    language: String,
    lines: [u32; 2],
    is_test: bool,
}

impl From<&SymbolRecord> for ExpectedSymbol {
    fn from(s: &SymbolRecord) -> Self {
        ExpectedSymbol {
            file: s.file.clone(),
            name: s.name.clone(),
            container: s.container.clone().unwrap_or_default(),
            kind: s.kind.clone(),
            language: s.language.clone(),
            lines: [s.start_line, s.end_line],
            is_test: s.is_test,
        }
    }
}

fn expected() -> Expected {
    let path = fixture_root().join("expected-symbols.toml");
    let text = fs::read_to_string(&path).expect("read expected-symbols.toml");
    toml::from_str(&text).expect("parse expected-symbols.toml")
}

fn expected_file(decision: &str) -> ExpectedFile {
    let mut matching: Vec<_> = expected()
        .file
        .into_iter()
        .filter(|f| f.decision == decision)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected-symbols.toml must name 1 file with the decision {decision}"
    );
    matching.remove(0)
}

// --- expected-edges.toml ------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedEdges {
    edge: Vec<ExpectedEdge>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
#[serde(deny_unknown_fields)]
struct EdgeEnd {
    file: String,
    name: String,
    line: u32,
}

impl From<&SymbolRef> for EdgeEnd {
    fn from(s: &SymbolRef) -> Self {
        EdgeEnd {
            file: s.file.clone(),
            name: s.name.clone(),
            line: s.start_line,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedEdge {
    caller: EdgeEnd,
    at: u32,
    class: String,
    callee: Option<EdgeEnd>,
    candidates: Option<Vec<EdgeEnd>>,
    external: Option<String>,
    /// "scip" or "name". The default: "scip" for a Rust caller, "name" for
    /// the other languages.
    origin: Option<String>,
}

/// An edge in a form that 2 edge lists can compare: the caller, the line of
/// the call, the class, the origin and the target (the callee, the
/// candidates or the external name).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct EdgeKey {
    caller: EdgeEnd,
    at: u32,
    class: String,
    origin: String,
    callee: Option<EdgeEnd>,
    candidates: Vec<EdgeEnd>,
    external: Option<String>,
}

impl From<ExpectedEdge> for EdgeKey {
    fn from(e: ExpectedEdge) -> Self {
        let mut candidates = e.candidates.unwrap_or_default();
        candidates.sort();
        let origin = e.origin.unwrap_or_else(|| {
            if e.caller.file.ends_with(".rs") {
                "scip".into()
            } else {
                "name".into()
            }
        });
        EdgeKey {
            caller: e.caller,
            at: e.at,
            class: e.class,
            origin,
            callee: e.callee,
            candidates,
            external: e.external,
        }
    }
}

impl From<&EdgeRecord> for EdgeKey {
    fn from(e: &EdgeRecord) -> Self {
        let mut candidates: Vec<EdgeEnd> = e.candidates.iter().map(EdgeEnd::from).collect();
        candidates.sort();
        EdgeKey {
            caller: EdgeEnd::from(&e.caller),
            at: e.line,
            class: e.class.clone(),
            origin: e.origin.clone(),
            callee: e.callee.as_ref().map(EdgeEnd::from),
            candidates,
            external: (e.class == "external").then(|| e.callee_name.clone()),
        }
    }
}

fn expected_edges() -> Vec<EdgeKey> {
    let path = fixture_root().join("expected-edges.toml");
    let text = fs::read_to_string(&path).expect("read expected-edges.toml");
    let parsed: ExpectedEdges = toml::from_str(&text).expect("parse expected-edges.toml");
    let mut keys: Vec<EdgeKey> = parsed.edge.into_iter().map(EdgeKey::from).collect();
    keys.sort();
    keys
}

/// The symbols of expected-symbols.toml with this name and language.
fn expected_named(name: &str, language: &str) -> Vec<ExpectedSymbol> {
    expected()
        .symbol
        .into_iter()
        .filter(|s| s.name == name && s.language == language)
        .collect()
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("make a folder");
    for entry in fs::read_dir(from).expect("read a folder") {
        let entry = entry.expect("read a folder entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("read a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy a file");
        }
    }
}

// --- Given --------------------------------------------------------------------

#[given("the polyglot fixture repository")]
fn the_fixture(world: &mut IndexWorld) {
    world.root = fixture_root();
}

#[given(expr = "the polyglot fixture repository with a rule that excludes {string}")]
fn the_fixture_with_an_exclusion(world: &mut IndexWorld, prefix: String) {
    let copy = TempDir::new().expect("make a folder for the copy");
    copy_tree(&fixture_root(), copy.path());
    let rules_dir = copy.path().join(".kairos");
    fs::create_dir_all(&rules_dir).expect("make .kairos");
    let rules =
        format!("[[rule]]\nid = \"no-scripts\"\npath = \"{prefix}**\"\ndecision = \"excluded\"\n");
    fs::write(rules_dir.join("index-rules.toml"), rules).expect("write the rules");
    world.root = copy.path().to_path_buf();
    world.repo_copy = Some(copy);
}

// --- Given: the call graph ----------------------------------------------------

#[given(expr = "the polyglot fixture, where 2 Rust types each have a method named {string}")]
fn two_rust_methods(world: &mut IndexWorld, name: String) {
    world.root = fixture_root();
    let found = expected_named(&name, "rust");
    let containers: BTreeSet<_> = found.iter().map(|s| s.container.clone()).collect();
    assert_eq!(
        (found.len(), containers.len()),
        (2, 2),
        "the fixture must have 2 Rust types with a method {name}: {found:#?}"
    );
    world.focus = Some(Focus {
        called: name,
        ..Focus::default()
    });
}

#[given("a function that calls push on a value of the second type")]
fn calls_the_second_push(world: &mut IndexWorld) {
    let focus = world.focus.as_mut().expect("no method is named");
    focus.caller_file = "src/lib.rs".into();
    focus.caller_name = "enqueue_all".into();
    focus.callee = Some(("src/queue.rs".into(), "push".into(), "Queue".into()));
}

#[given("the polyglot fixture, where a function calls a trait method on a generic value")]
fn calls_a_trait_method(world: &mut IndexWorld) {
    world.root = fixture_root();
    world.focus = Some(Focus {
        caller_file: "src/lib.rs".into(),
        caller_name: "describe_all".into(),
        called: "describe".into(),
        callee: Some(("src/shapes.rs".into(), "describe".into(), "Shape".into())),
        ..Focus::default()
    });
}

#[given(
    "the polyglot fixture, where a function calls a trait method with no body on a generic value"
)]
fn calls_a_bodiless_trait_method(world: &mut IndexWorld) {
    world.root = fixture_root();
    let area = expected_named("area", "rust");
    assert!(
        area.iter()
            .any(|s| s.container == "Shape" && s.lines[0] == s.lines[1]),
        "the fixture must declare Shape::area with no body: {area:#?}"
    );
    world.focus = Some(Focus {
        caller_file: "src/lib.rs".into(),
        caller_name: "total_area".into(),
        called: "area".into(),
        callee: Some(("src/shapes.rs".into(), "area".into(), "Shape".into())),
        ..Focus::default()
    });
}

#[given("the polyglot fixture with no target folder")]
fn the_fixture_with_no_target(world: &mut IndexWorld) {
    let copy = TempDir::new().expect("make a folder for the copy");
    copy_tree(&fixture_root(), copy.path());
    assert!(!copy.path().join("target").exists());
    world.files_before = Some(files_under(copy.path()));
    world.root = copy.path().to_path_buf();
    world.repo_copy = Some(copy);
}

#[given(expr = "the polyglot fixture, where 2 Python modules each define {string}")]
fn two_python_functions(world: &mut IndexWorld, name: String) {
    world.root = fixture_root();
    let found = expected_named(&name, "python");
    let files: BTreeSet<_> = found.iter().map(|s| s.file.clone()).collect();
    assert_eq!(
        (found.len(), files.len()),
        (2, 2),
        "the fixture must have 2 Python modules that define {name}: {found:#?}"
    );
    world.focus = Some(Focus {
        called: name,
        candidates: found.into_iter().map(|s| (s.file, s.name)).collect(),
        ..Focus::default()
    });
}

#[given("a function that calls load with no import that decides it")]
fn calls_an_ambiguous_load(world: &mut IndexWorld) {
    let focus = world.focus.as_mut().expect("no function is named");
    focus.caller_file = "python/polyglot/report.py".into();
    focus.caller_name = "build_report".into();
}

#[given("a Go function that calls fmt.Println")]
fn a_go_function_calls_println(world: &mut IndexWorld) {
    world.root = fixture_root();
    world.focus = Some(Focus {
        caller_file: "go/greet/greet.go".into(),
        caller_name: "Greet".into(),
        called: "fmt.Println".into(),
        ..Focus::default()
    });
}

#[given(expr = "the polyglot fixture, where 2 Rust test crates share the module {string}")]
fn two_test_crates_share_a_module(world: &mut IndexWorld, module: String) {
    world.root = fixture_root();
    assert!(
        world.root.join(&module).is_file(),
        "{module} is not in the fixture"
    );
    let users: Vec<_> = fs::read_dir(world.root.join("tests"))
        .expect("read tests/")
        .map(|e| e.expect("read an entry").path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "rs")
                && fs::read_to_string(p).is_ok_and(|t| t.contains("mod common;"))
        })
        .collect();
    assert_eq!(users.len(), 2, "2 test crates must declare `mod common;`");
}

#[given(expr = "the polyglot fixture, where 2 Rust test crates each define a function {string}")]
fn two_test_crates_define(world: &mut IndexWorld, name: String) {
    world.root = fixture_root();
    let found = expected_named(&name, "rust");
    let files: BTreeSet<_> = found.iter().map(|s| s.file.clone()).collect();
    assert!(
        files.len() == 2 && files.iter().all(|f| f.starts_with("tests/")),
        "2 test crates must define {name}: {found:#?}"
    );
    world.focus = Some(Focus {
        called: name,
        ..Focus::default()
    });
}

// --- When ---------------------------------------------------------------------

#[when("I build the structure")]
fn build_once(world: &mut IndexWorld) {
    world.build();
}

#[when("I build the index")]
fn build_the_index(world: &mut IndexWorld) {
    world.build();
}

#[when("I build the structure two times")]
fn build_twice(world: &mut IndexWorld) {
    world.build();
    world.build();
}

// --- Then ---------------------------------------------------------------------

#[then("the index has the symbols listed in the fixture's expected-symbols file")]
fn has_the_expected_symbols(world: &mut IndexWorld) {
    let want: BTreeSet<ExpectedSymbol> = expected().symbol.into_iter().collect();
    let got: BTreeSet<ExpectedSymbol> = world.symbols().iter().map(ExpectedSymbol::from).collect();
    let missing: Vec<_> = want.difference(&got).collect();
    let extra: Vec<_> = got.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "symbols differ from expected-symbols.toml\nmissing: {missing:#?}\nnot expected: {extra:#?}"
    );
}

#[then("each symbol has its file, its span, its kind and its language")]
fn each_symbol_is_complete(world: &mut IndexWorld) {
    let files: BTreeSet<String> = world.files().into_iter().map(|f| f.path).collect();
    let symbols = world.symbols();
    assert!(!symbols.is_empty(), "the index has no symbols");
    for s in &symbols {
        assert!(
            files.contains(&s.file),
            "{s:?}: its file is not in the index"
        );
        assert!(
            s.start_line >= 1 && s.end_line >= s.start_line && s.end_byte > s.start_byte,
            "{s:?}: the span is not valid"
        );
        assert!(!s.kind.is_empty() && s.kind != "unknown", "{s:?}: no kind");
        assert!(
            ["rust", "python", "typescript", "tsx", "go"].contains(&s.language.as_str()),
            "{s:?}: no language"
        );
        assert_eq!(s.tree_hash.len(), 64, "{s:?}: no tree hash");
    }
}

#[then("the vendored file, the generated file and the fixture file have no symbols")]
fn special_files_have_no_symbols(world: &mut IndexWorld) {
    let symbols = world.symbols();
    let files = world.files();
    for decision in ["vendored", "generated", "fixture"] {
        let want = expected_file(decision);
        assert!(
            files.iter().any(|f| f.path == want.path),
            "{} is not in the index",
            want.path
        );
        let found: Vec<_> = symbols.iter().filter(|s| s.file == want.path).collect();
        assert!(found.is_empty(), "{} has symbols: {found:#?}", want.path);
    }
}

#[then("the decision for each of them is recorded with its rule")]
fn special_files_have_their_decision(world: &mut IndexWorld) {
    let files = world.files();
    for decision in ["vendored", "generated", "fixture"] {
        let want = expected_file(decision);
        let got = files
            .iter()
            .find(|f| f.path == want.path)
            .unwrap_or_else(|| panic!("{} is not in the index", want.path));
        assert_eq!(
            (
                got.decision.as_str(),
                got.rule.as_str(),
                got.rule_origin.as_str()
            ),
            (want.decision.as_str(), want.rule.as_str(), "default"),
            "the decision for {}",
            want.path
        );
    }
}

#[then("the symbols of the test file are in the index")]
fn test_symbols_are_indexed(world: &mut IndexWorld) {
    let test_file = expected_file("test");
    let files = world.files();
    let got = files
        .iter()
        .find(|f| f.path == test_file.path)
        .unwrap_or_else(|| panic!("{} is not in the index", test_file.path));
    assert_eq!(
        (got.decision.as_str(), got.rule.as_str()),
        ("test", test_file.rule.as_str())
    );

    let want: BTreeSet<ExpectedSymbol> = expected()
        .symbol
        .into_iter()
        .filter(|s| s.file == test_file.path)
        .collect();
    assert!(
        !want.is_empty(),
        "expected-symbols.toml lists no symbol of the test file"
    );
    let got: BTreeSet<ExpectedSymbol> = world
        .symbols()
        .iter()
        .filter(|s| s.file == test_file.path)
        .map(ExpectedSymbol::from)
        .collect();
    assert_eq!(got, want);
}

#[then("they are marked as test code")]
fn test_symbols_are_marked(world: &mut IndexWorld) {
    let test_file = expected_file("test");
    let symbols = world.symbols();
    let in_test_file: Vec<_> = symbols
        .iter()
        .filter(|s| s.file == test_file.path)
        .collect();
    assert!(!in_test_file.is_empty());
    for s in in_test_file {
        assert!(s.is_test, "{s:?} is not marked as test code");
    }
    // Code outside the test file stays unmarked unless the fixture says so.
    let want_test: BTreeSet<(String, String)> = expected()
        .symbol
        .into_iter()
        .filter(|s| s.is_test)
        .map(|s| (s.file, s.name))
        .collect();
    for s in &symbols {
        assert_eq!(
            s.is_test,
            want_test.contains(&(s.file.clone(), s.name.clone())),
            "{s:?}: the test mark is wrong"
        );
    }
}

#[then(expr = "no file under {string} has symbols")]
fn no_symbols_under(world: &mut IndexWorld, prefix: String) {
    let files = world.files();
    let under: Vec<_> = files
        .iter()
        .filter(|f| f.path.starts_with(&prefix))
        .collect();
    assert!(!under.is_empty(), "no file under {prefix} is in the index");
    for f in &under {
        assert_eq!(
            (f.decision.as_str(), f.rule.as_str(), f.rule_origin.as_str()),
            ("excluded", "no-scripts", "repository"),
            "the decision for {}",
            f.path
        );
    }
    let found: Vec<_> = world
        .symbols()
        .into_iter()
        .filter(|s| s.file.starts_with(&prefix))
        .collect();
    assert!(
        found.is_empty(),
        "files under {prefix} have symbols: {found:#?}"
    );
}

#[then("the 2 indexes have the same symbols and the same hashes")]
fn the_two_indexes_agree(world: &mut IndexWorld) {
    assert_eq!(world.indexes.len(), 2, "2 builds were expected");
    let read = |db: &Path| {
        let index = Index::open(db).expect("open an index");
        (
            index.files().expect("read the files"),
            index.symbols().expect("read the symbols"),
        )
    };
    let (files_a, symbols_a) = read(&world.indexes[0]);
    let (files_b, symbols_b) = read(&world.indexes[1]);
    assert!(!symbols_a.is_empty());
    assert_eq!(files_a, files_b, "the files differ");
    assert_eq!(symbols_a, symbols_b, "the symbols or their hashes differ");
}

// --- Then: the call graph -----------------------------------------------------

#[then("the edge from that function goes to the second type's push")]
fn the_edge_goes_to_the_second_push(world: &mut IndexWorld) {
    the_edge_goes_to_the_callee(world);
}

#[then("the edge goes to the trait method")]
fn the_edge_goes_to_the_callee(world: &mut IndexWorld) {
    let focus = world.focus.clone().expect("no call is in focus");
    let (file, name, container) = focus.callee.expect("the scenario names no callee");
    let edges = world.focus_edges();
    assert_eq!(edges.len(), 1, "1 edge was expected: {edges:#?}");
    let callee = edges[0]
        .callee
        .as_ref()
        .unwrap_or_else(|| panic!("the edge has no callee: {:#?}", edges[0]));
    assert_eq!(
        (
            callee.file.as_str(),
            callee.name.as_str(),
            callee.container.as_deref()
        ),
        (file.as_str(), name.as_str(), Some(container.as_str())),
        "the callee of {:#?}",
        edges[0]
    );
    assert_eq!(edges[0].origin, "scip", "a Rust edge comes from SCIP");
}

#[then(expr = "the edge is {string}")]
fn the_edge_has_the_class(world: &mut IndexWorld, class: String) {
    for edge in world.focus_edges() {
        assert_eq!(edge.class, class, "the class of {edge:#?}");
    }
}

#[then("it names the 2 candidates")]
fn the_edge_names_the_candidates(world: &mut IndexWorld) {
    let focus = world.focus.clone().expect("no call is in focus");
    let edges = world.focus_edges();
    assert_eq!(edges.len(), 1, "1 edge was expected: {edges:#?}");
    let got: BTreeSet<(String, String)> = edges[0]
        .candidates
        .iter()
        .map(|c| (c.file.clone(), c.name.clone()))
        .collect();
    assert_eq!(got, focus.candidates);
    assert!(edges[0].callee.is_none(), "a possible edge has no callee");
}

#[then("the fixture still has no target folder")]
fn still_no_target(world: &mut IndexWorld) {
    assert!(
        !world.root.join("target").exists(),
        "the build made a target folder"
    );
    let before = world.files_before.as_ref().expect("no file list was kept");
    let after = files_under(&world.root);
    let added: Vec<_> = after.difference(before).collect();
    let removed: Vec<_> = before.difference(&after).collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "the build changed the repository\nadded: {added:#?}\nremoved: {removed:#?}"
    );
}

#[then("no temporary build folder is left after the run")]
fn no_temporary_folder(world: &mut IndexWorld) {
    let report = world.report.as_ref().expect("no build ran");
    let scip = report.scip.as_ref().expect("the build did not run SCIP");
    assert!(
        !scip.work_dir.exists(),
        "the temporary folder {} is still there",
        scip.work_dir.display()
    );
    assert_eq!(
        scip.build_script_command.as_deref(),
        Some("true"),
        "rust-analyzer must run no build script"
    );
    assert_eq!(
        scip.proc_macro_server_started,
        Some(false),
        "rust-analyzer must start no proc-macro server"
    );
}

#[then("the call of setup in each test crate goes to the setup of that crate")]
fn each_setup_in_its_crate(world: &mut IndexWorld) {
    let called = world.focus.clone().expect("no function is named").called;
    let edges: Vec<_> = world
        .edges()
        .into_iter()
        .filter(|e| calls_name(e, &called))
        .collect();
    assert_eq!(
        edges.len(),
        2,
        "2 calls of {called} were expected: {edges:#?}"
    );
    for edge in &edges {
        let callee = edge
            .callee
            .as_ref()
            .unwrap_or_else(|| panic!("no callee: {edge:#?}"));
        assert_eq!(callee.file, edge.caller.file, "{edge:#?}");
        assert_eq!(
            (edge.class.as_str(), edge.origin.as_str()),
            ("certain", "scip")
        );
    }
}

#[then(expr = "the SCIP run leaves out the test crate {string}")]
fn scip_leaves_out(world: &mut IndexWorld, target: String) {
    let report = world.report.as_ref().expect("no build ran");
    let scip = report.scip.as_ref().expect("the build did not run SCIP");
    assert_eq!(scip.left_out_targets, [target]);
}

#[then(expr = "each call from {string} has a name class")]
fn calls_from_have_name_classes(world: &mut IndexWorld, file: String) {
    let edges: Vec<_> = world
        .edges()
        .into_iter()
        .filter(|e| e.caller.file == file)
        .collect();
    assert!(!edges.is_empty(), "no edge starts in {file}");
    for edge in edges {
        assert_eq!(edge.origin, "name", "{edge:#?}");
    }
}

#[then("each edge in the fixture's expected-edges file is in the index with its class")]
fn has_the_expected_edges(world: &mut IndexWorld) {
    let got: Vec<EdgeRecord> = world.edges();
    let mut got_keys: Vec<EdgeKey> = got.iter().map(EdgeKey::from).collect();
    got_keys.sort();
    let mut missing = Vec::new();
    for want in expected_edges() {
        match got_keys.iter().position(|g| *g == want) {
            Some(i) => {
                got_keys.remove(i);
            }
            None => missing.push(want),
        }
    }
    assert!(
        missing.is_empty(),
        "edges of expected-edges.toml that are not in the index: {missing:#?}"
    );
}

#[then("the index has no other edge")]
fn has_no_other_edge(world: &mut IndexWorld) {
    let mut extra: Vec<EdgeKey> = world.edges().iter().map(EdgeKey::from).collect();
    extra.sort();
    for want in expected_edges() {
        if let Some(i) = extra.iter().position(|g| *g == want) {
            extra.remove(i);
        }
    }
    assert!(
        extra.is_empty(),
        "edges in the index that expected-edges.toml does not list: {extra:#?}"
    );
}

// --- Summaries ------------------------------------------------------------------

/// The symbols that get a summary: not test code, and of a summarized kind.
fn summarized(s: &SymbolRecord) -> bool {
    !s.is_test && SUMMARIZED_KINDS.contains(&s.kind.as_str())
}

fn find_symbol<'a>(
    symbols: &'a [SymbolRecord],
    (file, name, container): &(String, String, String),
) -> &'a SymbolRecord {
    let found: Vec<_> = symbols
        .iter()
        .filter(|s| {
            &s.file == file && &s.name == name && s.container.as_deref() == Some(container.as_str())
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "1 symbol {container}::{name} in {file} was expected"
    );
    found[0]
}

#[given("the polyglot fixture and the fake summarizer")]
fn the_fixture_and_the_fake(world: &mut IndexWorld) {
    world.root = fixture_root();
}

#[given("2 copies of a Rust function that differ only in whitespace and comments")]
fn two_copies(world: &mut IndexWorld) {
    let copy = TempDir::new().expect("make a folder for the repository");
    let src = copy.path().join("src");
    fs::create_dir_all(&src).expect("make src/");
    fs::write(
        src.join("a.rs"),
        "pub fn add_one(x: u32) -> u32 { x + 1 }\n",
    )
    .expect("write a.rs");
    fs::write(
        src.join("b.rs"),
        "/// The same code as in a.rs.\npub fn add_one(x: u32) -> u32 {\n    // Add one.\n    x  +  1\n}\n",
    )
    .expect("write b.rs");
    world.root = copy.path().to_path_buf();
    world.repo_copy = Some(copy);
}

#[given("a summarized index")]
fn a_summarized_index(world: &mut IndexWorld) {
    let copy = TempDir::new().expect("make a folder for the copy");
    copy_tree(&fixture_root(), copy.path());
    world.root = copy.path().to_path_buf();
    world.repo_copy = Some(copy);
    world.summarize_with_fake();
    world.symbols_before = Some(world.symbols());
}

#[when("I summarize the index")]
fn summarize_the_index(world: &mut IndexWorld) {
    world.summarize_with_fake();
}

#[when("the body of a function changes and its signature stays the same")]
fn change_a_body(world: &mut IndexWorld) {
    let path = world.root.join("src/queue.rs");
    let before = fs::read_to_string(&path).expect("read src/queue.rs");
    let signature = "    pub fn push(&mut self, item: u32) {\n        self.items.push_back(item);";
    assert!(
        before.contains(signature),
        "src/queue.rs has no push to change"
    );
    let after = before.replace(
        signature,
        "    pub fn push(&mut self, item: u32) {\n        self.items.push_front(item);",
    );
    fs::write(&path, after).expect("write src/queue.rs");
    world.changed = Some(("src/queue.rs".into(), "push".into(), "Queue".into()));

    // Build again into the same index, so that its summary pool stays.
    let db = world.indexes.last().expect("no index was built").clone();
    world.report = Some(build_structure(&world.root, &db).unwrap_or_else(|e| panic!("{e}")));
    world.summarize_with_fake();
}

#[then("each symbol that is not test code has a summary and a vector")]
fn each_symbol_has_a_summary(world: &mut IndexWorld) {
    let index = world.index();
    let symbols = world.symbols();
    let mut count = 0;
    for s in symbols.iter().filter(|s| summarized(s)) {
        let summary = index
            .summary(&s.tree_hash)
            .expect("read a summary")
            .unwrap_or_else(|| panic!("{} in {} has no summary", s.name, s.file));
        assert_eq!(summary.level, "symbol", "{s:?}");
        assert!(!summary.summary.is_empty(), "{s:?}: the summary is empty");
        let vector = summary
            .vector
            .unwrap_or_else(|| panic!("{} in {} has no vector", s.name, s.file));
        assert_eq!(vector.len(), 384, "{s:?}: the vector width");
        count += 1;
    }
    assert!(count >= 20, "only {count} symbols have a summary");

    // The other kinds (a `mod x;` line, an impl block, a constant) get none.
    for s in symbols.iter().filter(|s| !s.is_test && !summarized(s)) {
        assert!(
            !world
                .requests
                .iter()
                .any(|r| r.level == Level::Symbol && r.key == s.tree_hash),
            "{s:?} is not a function or a type, and it got a summary"
        );
    }

    // Each file and module summary has a vector too.
    for (path, key) in index.file_summary_keys().expect("read the files") {
        if let Some(key) = key {
            let summary = index.summary(&key).expect("read").expect("a file summary");
            assert!(
                summary.vector.is_some(),
                "the summary of {path} has no vector"
            );
        }
    }
    let modules = index.modules().expect("read the modules");
    assert!(!modules.is_empty(), "no module has a summary");
    for (path, key) in modules {
        let summary = index
            .summary(&key)
            .expect("read")
            .expect("a module summary");
        assert_eq!(summary.level, "module");
        assert!(
            summary.vector.is_some(),
            "the summary of {path} has no vector"
        );
    }
}

#[then("no test symbol has a summary")]
fn no_test_summary(world: &mut IndexWorld) {
    let index = world.index();
    let tests: Vec<_> = world.symbols().into_iter().filter(|s| s.is_test).collect();
    assert!(!tests.is_empty(), "the fixture has no test symbols");
    for s in &tests {
        assert!(
            !world.requests.iter().any(|r| r.key == s.tree_hash),
            "the summarizer got the test symbol {} in {}",
            s.name,
            s.file
        );
        assert!(
            index.summary(&s.tree_hash).expect("read").is_none(),
            "the test symbol {} in {} has a summary",
            s.name,
            s.file
        );
    }
}

#[then("the 2 symbols have the same key")]
fn the_same_key(world: &mut IndexWorld) {
    let symbols: Vec<_> = world
        .symbols()
        .into_iter()
        .filter(|s| s.name == "add_one")
        .collect();
    assert_eq!(symbols.len(), 2, "2 copies were expected: {symbols:#?}");
    assert_eq!(symbols[0].tree_hash, symbols[1].tree_hash);
    assert!(
        world
            .index()
            .summary(&symbols[0].tree_hash)
            .expect("read")
            .is_some(),
        "the key has no summary"
    );
}

#[then("the summarizer ran one time for them")]
fn ran_one_time(world: &mut IndexWorld) {
    let key = world
        .symbols()
        .into_iter()
        .find(|s| s.name == "add_one")
        .expect("add_one")
        .tree_hash;
    let runs = world
        .requests
        .iter()
        .filter(|r| r.level == Level::Symbol && r.key == key)
        .count();
    assert_eq!(runs, 1, "the summarizer ran {runs} times for the 2 copies");
    let report = world.summary_report.as_ref().expect("no summary run");
    assert_eq!((report.symbols.summarized, report.symbols.reused), (1, 1));
}

#[then("the input to each file summary is the summaries of its symbols, not the code of the file")]
fn file_input_is_symbol_summaries(world: &mut IndexWorld) {
    let index = world.index();
    let symbols = world.symbols();
    let files = world.files();
    let keys: std::collections::BTreeMap<String, Option<String>> = index
        .file_summary_keys()
        .expect("read the files")
        .into_iter()
        .collect();
    let mut checked = 0;
    for f in files.iter().filter(|f| f.decision == "source") {
        let parts: Vec<_> = symbols
            .iter()
            .filter(|s| s.file == f.path && summarized(s))
            .collect();
        if parts.is_empty() {
            continue;
        }
        let requests: Vec<_> = world
            .requests
            .iter()
            .filter(|r| r.level == Level::File && r.path == f.path)
            .collect();
        assert_eq!(
            requests.len(),
            1,
            "1 file summary of {} was expected",
            f.path
        );
        let request = requests[0];
        assert_eq!(keys[&f.path].as_deref(), Some(request.key.as_str()));
        assert!(request.code.is_none(), "{}: the input has code", f.path);
        assert_eq!(
            request.children.len(),
            parts.len(),
            "{}: 1 line for each symbol was expected: {:#?}",
            f.path,
            request.children
        );
        for (line, s) in request.children.iter().zip(&parts) {
            let summary = index
                .summary(&s.tree_hash)
                .expect("read")
                .expect("a symbol summary")
                .summary;
            assert!(
                line.ends_with(&summary) && line.contains(&s.name),
                "{}: the line {line:?} is not the summary of {}",
                f.path,
                s.name
            );
        }
        let text = fs::read_to_string(world.root.join(&f.path)).expect("read the file");
        let prompt = request.prompt();
        for code_line in text.lines().map(str::trim).filter(|l| l.len() > 12) {
            assert!(
                !prompt.contains(code_line),
                "{}: the input has the code line {code_line:?}",
                f.path
            );
        }
        checked += 1;
    }
    assert!(checked >= 8, "only {checked} files have a summary");
}

#[then("the key of the function changes")]
fn the_key_changes(world: &mut IndexWorld) {
    let changed = world.changed.clone().expect("no function changed");
    let before = world.symbols_before.clone().expect("no keys before");
    let after = world.symbols();
    let old = find_symbol(&before, &changed);
    let new = find_symbol(&after, &changed);
    assert_eq!(old.signature, new.signature, "the signature changed");
    assert_ne!(old.tree_hash, new.tree_hash, "the key did not change");
    let ran: Vec<_> = world
        .requests
        .iter()
        .filter(|r| r.level == Level::Symbol)
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(
        ran,
        [new.tree_hash.as_str()],
        "only the changed function needs a new summary"
    );
}

#[then("the keys of its callers do not change")]
fn caller_keys_stay(world: &mut IndexWorld) {
    let changed = world.changed.clone().expect("no function changed");
    let before = world.symbols_before.clone().expect("no keys before");
    let after = world.symbols();
    let callers: Vec<_> = world
        .edges()
        .into_iter()
        .filter(|e| {
            e.callee.as_ref().is_some_and(|c| {
                c.file == changed.0
                    && c.name == changed.1
                    && c.container.as_deref() == Some(changed.2.as_str())
            })
        })
        .map(|e| {
            (
                e.caller.file.clone(),
                e.caller.name.clone(),
                e.caller.container.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert!(!callers.is_empty(), "the function has no callers");
    for caller in &callers {
        let find = |symbols: &[SymbolRecord]| {
            symbols
                .iter()
                .find(|s| {
                    s.file == caller.0
                        && s.name == caller.1
                        && s.container.clone().unwrap_or_default() == caller.2
                })
                .unwrap_or_else(|| panic!("no symbol {caller:?}"))
                .tree_hash
                .clone()
        };
        assert_eq!(find(&before), find(&after), "the key of {caller:?} changed");
    }
}

/// The count of sentences in `text`: the ends `.`, `!` and `?` before a space
/// or the end, outside code in backticks, with no `e.g.` or `i.e.`.
fn sentences(text: &str) -> usize {
    let mut plain = String::new();
    let mut in_code = false;
    for c in text.chars() {
        if c == '`' {
            in_code = !in_code;
        } else if !in_code {
            plain.push(c);
        }
    }
    let plain = plain.replace("e.g.", "eg").replace("i.e.", "ie");
    let chars: Vec<char> = plain.trim().chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace())
        })
        .count()
}

#[cfg(feature = "llama")]
#[when("I summarize 3 symbols of the polyglot fixture with the real model")]
fn summarize_with_the_model(world: &mut IndexWorld) {
    let path = world.model.clone().expect("the model file is not on disk");
    world.root = fixture_root();
    world.build();
    let db = world.indexes.last().expect("no index").clone();
    let model = kairos_index::LlamaModelFile::load(&path).unwrap_or_else(|e| panic!("{e}"));
    let mut summarizer = model.summarizer().unwrap_or_else(|e| panic!("{e}"));
    let report = summarize(
        &world.root,
        &db,
        &mut summarizer,
        &DeterministicProvider::default(),
        &SummarizeOptions {
            under: None,
            max_new_symbols: Some(3),
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for call in &report.calls {
        eprintln!(
            "{} {} ({:.1} s): {}",
            call.level.as_str(),
            call.name,
            call.elapsed.as_secs_f64(),
            call.summary
        );
    }
    world.summary_report = Some(report);
}

#[then("each summary has 1 to 3 sentences")]
fn one_to_three_sentences(world: &mut IndexWorld) {
    let report = world.summary_report.as_ref().expect("no summary run");
    let symbols = report
        .calls
        .iter()
        .filter(|c| c.level == Level::Symbol)
        .count();
    assert_eq!(symbols, 3, "3 symbol summaries were expected");
    for call in &report.calls {
        let n = sentences(&call.summary);
        assert!(
            (1..=3).contains(&n),
            "{}: {n} sentences: {:?}",
            call.name,
            call.summary
        );
    }
}

#[then("no summary is empty")]
fn no_empty_summary(world: &mut IndexWorld) {
    let report = world.summary_report.as_ref().expect("no summary run");
    assert!(!report.calls.is_empty(), "the model wrote no summary");
    for call in &report.calls {
        assert!(
            !call.summary.trim().is_empty(),
            "{}: the summary is empty",
            call.name
        );
    }
}

/// The step "Given the Qwen3-4B model file is on disk". It is added only
/// when the model file is on disk and the test has the `llama` feature.
/// Otherwise no step matches, and cucumber shows the scenario as skipped.
fn the_model_is_on_disk(
    world: &mut IndexWorld,
    _: cucumber::step::Context,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
    world.model = kairos_index::model_path().filter(|p| p.is_file());
    assert!(world.model.is_some(), "the model file is not on disk");
    Box::pin(async {})
}

/// Why the `@model` scenario cannot run here, or `None` if it can.
fn why_no_model() -> Option<String> {
    let Some(path) = kairos_index::model_path() else {
        return Some("HOME and KAIROS_INDEX_MODEL are not set".into());
    };
    if !path.is_file() {
        return Some(format!(
            "the model file {} is not on disk. Set KAIROS_INDEX_MODEL, or put {} there",
            path.display(),
            kairos_index::MODEL_FILE_NAME
        ));
    }
    if !cfg!(feature = "llama") {
        return Some("the test is built without the feature `llama`".into());
    }
    None
}

#[tokio::main]
async fn main() {
    let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/features");
    let mut cucumber = IndexWorld::cucumber();
    match why_no_model() {
        None => {
            cucumber = cucumber.given(
                cucumber::codegen::Regex::new("^the Qwen3-4B model file is on disk$")
                    .expect("a valid pattern"),
                the_model_is_on_disk,
            );
        }
        Some(reason) => {
            eprintln!("The @model scenario does not run: {reason}.");
        }
    }
    cucumber
        // Each build runs rust-analyzer, which uses about 0.6 GB. The model
        // scenario uses about 4 GB more.
        .max_concurrent_scenarios(4)
        // A skipped step fails the run, but for a scenario tagged
        // @allow.skipped: the @model scenario when the model is not here.
        .fail_on_skipped()
        .run_and_exit(features)
        .await;
}
