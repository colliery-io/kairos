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
    BuildOptions, BuildReport, EdgeRecord, FakeSummarizer, FileRecord, Index, Level,
    SUMMARIZED_KINDS, SummarizeOptions, SummaryReport, SummaryRequest, SymbolRecord, SymbolRef,
    UpdateOptions, UpdateReport, build_structure_with, merge, rust_analyzer, summarize, update,
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
    /// The index before an update, for the scenarios that compare.
    before: Option<Snapshot>,
    /// The report of the last update or merge.
    update_report: Option<UpdateReport>,
    /// The other folders of a scenario: branch trees and their indexes.
    extra: Vec<TempDir>,
    /// The indexes of the 2 branches of a merge, and the symbol that each
    /// branch changed: file, name, container.
    branches: Vec<(PathBuf, (String, String, String))>,
    /// A file that a scenario moved: the path before and after.
    moved: Option<(String, String)>,
    /// A rust-analyzer binary in place of the pinned one in the cache.
    rust_analyzer: Option<PathBuf>,
    /// A std source archive in place of the pinned one in the cache.
    std_source: Option<PathBuf>,
    /// The error of the last build, for a scenario where the build fails.
    build_error: Option<String>,
}

/// The rust-analyzer and the std source of the scenarios: those pinned in
/// the cache, or `rust_analyzer` and `std_source`. The scenarios never
/// download them.
fn build_options(rust_analyzer: Option<PathBuf>, std_source: Option<PathBuf>) -> BuildOptions {
    BuildOptions {
        rust_analyzer,
        std_source,
        download: false,
    }
}

/// What an index holds, to compare before and after an update.
#[derive(Debug, Clone)]
struct Snapshot {
    symbols: Vec<SymbolRecord>,
    edges: Vec<EdgeRecord>,
    file_keys: std::collections::BTreeMap<String, Option<String>>,
    modules: std::collections::BTreeMap<String, String>,
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
        let options = build_options(self.rust_analyzer.clone(), self.std_source.clone());
        match build_structure_with(&self.root, &db, &options) {
            Ok(report) => {
                self.report = Some(report);
                self.indexes.push(db);
            }
            // A scenario with another binary or std source checks the error.
            Err(e) if self.rust_analyzer.is_some() || self.std_source.is_some() => {
                self.build_error = Some(e.to_string())
            }
            Err(e) => panic!("{e}"),
        }
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

    fn snapshot(&self) -> Snapshot {
        let index = self.index();
        Snapshot {
            symbols: index.symbols().expect("read the symbols"),
            edges: index.edges().expect("read the edges"),
            file_keys: index
                .file_summary_keys()
                .expect("read the files")
                .into_iter()
                .collect(),
            modules: index
                .modules()
                .expect("read the modules")
                .into_iter()
                .collect(),
        }
    }

    /// Update the last index from the tree, with the fake summarizer.
    fn update(&mut self, rust_edges: bool) {
        let db = self.indexes.last().expect("no index was built").clone();
        let mut fake = FakeSummarizer::default();
        let report = update(
            &self.root,
            &db,
            &mut fake,
            &DeterministicProvider::default(),
            &UpdateOptions {
                rust_edges,
                build: build_options(None, None),
                ..UpdateOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.requests = fake.requests;
        self.update_report = Some(report);
    }

    /// The symbol requests of the last run, as `file:name`.
    fn symbol_requests(&self) -> Vec<String> {
        self.requests
            .iter()
            .filter(|r| r.level == Level::Symbol)
            .map(|r| format!("{}:{}", r.path, r.name))
            .collect()
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

#[then(expr = "the log shows the build-script command {string} and no proc-macro server")]
fn the_log_shows_no_build(world: &mut IndexWorld, command: String) {
    let report = world.report.as_ref().expect("no build ran");
    let scip = report.scip.as_ref().expect("the build did not run SCIP");
    assert!(
        !scip.work_dir.exists(),
        "the temporary folder {} is still there",
        scip.work_dir.display()
    );
    assert_eq!(
        scip.build_script_command.as_deref(),
        Some(command.as_str()),
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

// --- The pinned rust-analyzer (COLLIERY-T-1858) -------------------------------

/// The 3 test crates of the fixture that declare `mod common;`.
const SHARED_MODULE_USERS: [&str; 3] = ["tests/first.rs", "tests/second.rs", "tests/third.rs"];

#[given("the polyglot fixture, where 3 test crates share tests/common/mod.rs")]
fn three_test_crates_share_a_module(world: &mut IndexWorld) {
    world.root = fixture_root();
    assert!(world.root.join("tests/common/mod.rs").is_file());
    let users: BTreeSet<String> = fs::read_dir(world.root.join("tests"))
        .expect("read tests/")
        .map(|e| e.expect("read an entry").path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "rs")
                && fs::read_to_string(p).is_ok_and(|t| t.contains("mod common;"))
        })
        .map(|p| format!("tests/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    assert_eq!(
        users,
        SHARED_MODULE_USERS.map(String::from).into(),
        "3 test crates must declare `mod common;`"
    );
}

#[when("I build the index with the pinned rust-analyzer")]
fn build_with_the_pin(world: &mut IndexWorld) {
    world.build();
    let scip = world
        .report
        .as_ref()
        .and_then(|r| r.scip.as_ref())
        .expect("the build did not run SCIP");
    assert_eq!(scip.rust_analyzer, rust_analyzer::VERSION);
}

#[then("no target is left out")]
fn no_target_left_out(world: &mut IndexWorld) {
    let report = world.report.as_ref().expect("no build ran");
    let scip = report.scip.as_ref().expect("the build did not run SCIP");
    assert_eq!(scip.left_out_targets, Vec::<String>::new());
}

#[then(expr = "each call from the 3 test crates to the shared module is a {string} edge from SCIP")]
fn shared_module_calls_are_scip(world: &mut IndexWorld, class: String) {
    let edges = world.edges();
    for file in SHARED_MODULE_USERS {
        let calls: Vec<_> = edges
            .iter()
            .filter(|e| e.caller.file == file && calls_name(e, "helper"))
            .collect();
        assert!(!calls.is_empty(), "{file} has no call of helper");
        for e in calls {
            assert_eq!(
                (e.class.as_str(), e.origin.as_str()),
                (class.as_str(), "scip"),
                "{e:#?}"
            );
            assert_eq!(
                e.callee.as_ref().map(|c| c.file.as_str()),
                Some("tests/common/mod.rs"),
                "{e:#?}"
            );
        }
    }
}

#[given("a rust-analyzer binary whose sha256 is not the pinned value")]
fn a_wrong_binary(world: &mut IndexWorld) {
    world.root = fixture_root();
    let dir = TempDir::new().expect("make a folder for the binary");
    let path = dir.path().join("rust-analyzer");
    fs::write(&path, "#!/bin/sh\necho 'rust-analyzer 0.0.0'\n").expect("write the binary");
    world.rust_analyzer = Some(path);
    world.extra.push(dir);
}

#[then("the build stops, and the error names the binary and the expected checksum")]
fn the_build_stops(world: &mut IndexWorld) {
    assert!(world.report.is_none(), "the build did not stop");
    let error = world
        .build_error
        .as_deref()
        .expect("the build gave no error");
    let path = world.rust_analyzer.as_ref().expect("no binary");
    let expected = rust_analyzer::pin()
        .expect("a pin for this platform")
        .binary_sha256;
    assert!(
        error.contains(&path.display().to_string()),
        "the error does not name the binary: {error}"
    );
    assert!(
        error.contains(expected),
        "the error does not name the checksum: {error}"
    );
}

// --- The pinned std source (COLLIERY-T-1860) ----------------------------------

/// The function of the fixture that calls `fixture_name` inside 4 std
/// macros, and the macros, in the order of the code.
const STD_MACRO_CALLER: &str = "std_macro_calls";
const STD_MACROS: [&str; 4] = ["assert_eq!", "format!", "println!", "vec!"];

/// The checked `library/` folder of the pinned std source in the cache.
fn pinned_std_source() -> PathBuf {
    let archive = rust_analyzer::std_source_archive_path().expect("HOME is set");
    let library = archive.parent().expect("a folder").join(format!(
        "rust-src-{}/library",
        rust_analyzer::RUST_SRC_VERSION
    ));
    fs::canonicalize(&library).unwrap_or_else(|e| {
        panic!(
            "the pinned std source is not at {}: {e}. Run `angreal dev fetch-rust-analyzer`",
            library.display()
        )
    })
}

/// The std source that the last SCIP run logged.
fn logged_std_source(world: &IndexWorld) -> PathBuf {
    let report = world.report.as_ref().expect("no build ran");
    let scip = report.scip.as_ref().expect("the build did not run SCIP");
    let logged = scip
        .std_source
        .as_ref()
        .expect("the log of rust-analyzer names no std source");
    fs::canonicalize(logged).unwrap_or_else(|e| panic!("{}: {e}", logged.display()))
}

#[given(
    "the polyglot fixture, where a Rust function calls a fixture function inside assert_eq!, format!, vec! and println!"
)]
fn std_macro_calls(world: &mut IndexWorld) {
    world.root = fixture_root();
    let text = fs::read_to_string(world.root.join("src/labels.rs")).expect("read src/labels.rs");
    let body = text
        .split_once(&format!("fn {STD_MACRO_CALLER}("))
        .map(|(_, b)| b)
        .expect("src/labels.rs has no std_macro_calls");
    for name in STD_MACROS {
        let call = format!("{name}[");
        let paren = format!("{name}(");
        assert!(
            body.lines()
                .any(|l| (l.contains(&call) || l.contains(&paren)) && l.contains("fixture_name()")),
            "{STD_MACRO_CALLER} does not call fixture_name inside {name}"
        );
    }
}

#[when("I build the index with the pinned rust-analyzer and the pinned std source")]
fn build_with_the_pins(world: &mut IndexWorld) {
    world.build();
    let scip = world
        .report
        .as_ref()
        .and_then(|r| r.scip.as_ref())
        .expect("the build did not run SCIP");
    assert_eq!(scip.rust_analyzer, rust_analyzer::VERSION);
    assert_eq!(logged_std_source(world), pinned_std_source());
}

#[then(expr = "each of the 4 calls is a {string} edge from SCIP")]
fn std_macro_calls_are_scip(world: &mut IndexWorld, class: String) {
    let calls: Vec<_> = world
        .edges()
        .into_iter()
        .filter(|e| {
            e.caller.file == "src/labels.rs"
                && e.caller.name == STD_MACRO_CALLER
                && calls_name(e, "fixture_name")
        })
        .collect();
    let lines: BTreeSet<_> = calls.iter().map(|e| e.line).collect();
    assert_eq!(
        (calls.len(), lines.len()),
        (4, 4),
        "4 calls of fixture_name on 4 lines were expected: {calls:#?}"
    );
    for e in &calls {
        assert_eq!(
            (e.class.as_str(), e.origin.as_str()),
            (class.as_str(), "scip"),
            "{e:#?}"
        );
        assert_eq!(
            e.callee
                .as_ref()
                .map(|c| (c.file.as_str(), c.name.as_str())),
            Some(("src/labels.rs", "fixture_name")),
            "{e:#?}"
        );
    }
}

#[given("a fixture whose rust-toolchain.toml pins Rust 1.93")]
fn a_fixture_on_rust_1_93(world: &mut IndexWorld) {
    let copy = TempDir::new().expect("make a folder for the copy");
    copy_tree(&fixture_root(), copy.path());
    fs::write(
        copy.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.93.0\"\n",
    )
    .expect("write rust-toolchain.toml");
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .current_dir(copy.path())
        .output()
        .expect("run rustc");
    let version = String::from_utf8_lossy(&rustc.stdout);
    assert!(
        version.starts_with("rustc 1.93."),
        "the copy does not use Rust 1.93: {version}"
    );
    world.root = copy.path().to_path_buf();
    world.repo_copy = Some(copy);
}

#[then("rust-analyzer reads the pinned std source, and the log shows its path")]
fn reads_the_pinned_std_source(world: &mut IndexWorld) {
    let logged = logged_std_source(world);
    assert_eq!(logged, pinned_std_source());
    assert!(
        !logged.starts_with(Path::new(&std::env::var("HOME").unwrap_or_default()).join(".rustup")),
        "rust-analyzer read the std source of a toolchain: {}",
        logged.display()
    );
}

#[given("a std source archive whose sha256 is not the pinned value")]
fn a_wrong_std_source(world: &mut IndexWorld) {
    world.root = fixture_root();
    let dir = TempDir::new().expect("make a folder for the archive");
    let path = dir.path().join(format!(
        "rust-src-{}.tar.gz",
        rust_analyzer::RUST_SRC_VERSION
    ));
    fs::write(&path, "not a rust-src archive").expect("write the archive");
    world.std_source = Some(path);
    world.extra.push(dir);
}

#[then("the build stops, and the error names the archive and the expected checksum")]
fn the_build_stops_for_the_std_source(world: &mut IndexWorld) {
    assert!(world.report.is_none(), "the build did not stop");
    let error = world
        .build_error
        .as_deref()
        .expect("the build gave no error");
    let path = world.std_source.as_ref().expect("no archive");
    assert!(
        error.contains(&path.display().to_string()),
        "the error does not name the archive: {error}"
    );
    assert!(
        error.contains(rust_analyzer::RUST_SRC_SHA256),
        "the error does not name the checksum: {error}"
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
    world.report = Some(
        build_structure_with(&world.root, &db, &build_options(None, None))
            .unwrap_or_else(|e| panic!("{e}")),
    );
    world.summarize_with_fake();
}

#[then("each symbol that is not test code has a summary and a vector")]
fn each_symbol_has_a_summary(world: &mut IndexWorld) {
    let index = world.index();
    let symbols = world.symbols();
    let mut count = 0;
    for s in symbols.iter().filter(|s| summarized(s)) {
        let key = s
            .summary_key
            .as_deref()
            .unwrap_or_else(|| panic!("{} in {} has no summary key", s.name, s.file));
        let summary = index
            .summary(key)
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
            s.summary_key.is_none()
                && !world.requests.iter().any(|r| r.level == Level::Symbol
                    && r.path == s.file
                    && r.name == s.name
                    && r.kind == s.kind),
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
    let tests: Vec<_> = world.symbols().into_iter().filter(|s| s.is_test).collect();
    assert!(!tests.is_empty(), "the fixture has no test symbols");
    for s in &tests {
        assert!(
            !world
                .requests
                .iter()
                .any(|r| r.level == Level::Symbol && r.path == s.file && r.name == s.name),
            "the summarizer got the test symbol {} in {}",
            s.name,
            s.file
        );
        assert!(
            s.summary_key.is_none(),
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
    assert_eq!(symbols[0].summary_key, symbols[1].summary_key);
    let key = symbols[0].summary_key.as_deref().expect("a summary key");
    assert!(
        world.index().summary(key).expect("read").is_some(),
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
        .summary_key
        .expect("a summary key");
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
                .summary(s.summary_key.as_deref().expect("a summary key"))
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
    assert_ne!(old.tree_hash, new.tree_hash, "the tree did not change");
    assert_ne!(old.summary_key, new.summary_key, "the key did not change");
    let ran: Vec<_> = world
        .requests
        .iter()
        .filter(|r| r.level == Level::Symbol)
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(
        ran,
        [new.summary_key.as_deref().expect("a summary key")],
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
                .summary_key
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
            under: Vec::new(),
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

#[then("each summary has 1 to 5 sentences")]
fn one_to_five_sentences(world: &mut IndexWorld) {
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
            (1..=5).contains(&n),
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

// --- Updates and merges (COLLIERY-T-1851) -----------------------------------------

/// Replace `from` with `to` in the file `path` of the tree at `root`. The
/// text must be in the file one time.
fn edit(root: &Path, path: &str, from: &str, to: &str) {
    let file = root.join(path);
    let text = fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {path}: {e}"));
    assert_eq!(
        text.matches(from).count(),
        1,
        "{path} must hold {from:?} one time"
    );
    fs::write(&file, text.replacen(from, to, 1)).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

/// The body change of the Rust scenarios: `Stack::pop` gets a filter.
const POP_BEFORE: &str = "        self.items.pop()\n";
const POP_AFTER: &str = "        self.items.pop().filter(|item| *item > 0)\n";

/// The summary key of the symbol `(file, name, container)`; an empty
/// container is a symbol at the top of its file.
fn symbol_key(
    symbols: &[SymbolRecord],
    (file, name, container): &(String, String, String),
) -> Option<String> {
    let found: Vec<_> = symbols
        .iter()
        .filter(|s| {
            &s.file == file && &s.name == name && s.container.as_deref().unwrap_or("") == container
        })
        .collect();
    assert_eq!(found.len(), 1, "1 symbol {name} in {file} was expected");
    found[0].summary_key.clone()
}

/// The identity of a symbol across 2 builds: its file, container, name and
/// kind. Its lines can change.
fn identity(s: &SymbolRecord) -> (String, String, String, String) {
    (
        s.file.clone(),
        s.container.clone().unwrap_or_default(),
        s.name.clone(),
        s.kind.clone(),
    )
}

#[given("a summarized index of the polyglot fixture")]
fn a_summarized_index_of_the_fixture(world: &mut IndexWorld) {
    a_summarized_index(world);
    world.before = Some(world.snapshot());
}

#[given("a summarized index with Rust edges from SCIP")]
fn a_summarized_index_with_scip(world: &mut IndexWorld) {
    a_summarized_index_of_the_fixture(world);
    let report = world.report.as_ref().expect("no build ran");
    assert!(report.scip.is_some(), "the build did not run SCIP");
    let before = world.before.as_ref().expect("no snapshot");
    assert!(
        before
            .edges
            .iter()
            .any(|e| e.caller.file == "src/stack.rs" && e.origin == "scip"),
        "src/stack.rs has no SCIP edge"
    );
}

#[when("I change the body of one Rust function and update the index")]
fn change_one_rust_body(world: &mut IndexWorld) {
    edit(&world.root, "src/stack.rs", POP_BEFORE, POP_AFTER);
    world.changed = Some(("src/stack.rs".into(), "pop".into(), "Stack".into()));
    world.update(false);
}

#[when("I change a Rust function and update the index with no options")]
fn change_rust_no_options(world: &mut IndexWorld) {
    change_one_rust_body(world);
}

#[when(expr = "I update the index with {string}")]
fn update_with(world: &mut IndexWorld, option: String) {
    assert_eq!(option, "--rust-edges", "an unknown option");
    world.update(true);
}

#[then("the summarizer ran for that function and for its file and module only")]
fn ran_for_function_file_module(world: &mut IndexWorld) {
    let (file, name, _) = world.changed.clone().expect("no function changed");
    assert_eq!(world.symbol_requests(), [format!("{file}:{name}")]);
    let files: Vec<_> = world
        .requests
        .iter()
        .filter(|r| r.level == Level::File)
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(files, [file.as_str()], "the file summaries");
    let modules: Vec<_> = world
        .requests
        .iter()
        .filter(|r| r.level == Level::Module)
        .map(|r| r.path.clone())
        .collect();
    let folder = file.rsplit_once('/').map_or(".", |(f, _)| f).to_string();
    assert_eq!(modules, [folder], "the module summaries");
}

#[then("each other summary is the same as before")]
fn other_summaries_stay(world: &mut IndexWorld) {
    let changed = world.changed.clone().expect("no function changed");
    let before = world.before.clone().expect("no snapshot");
    let after = world.snapshot();
    let folder = changed
        .0
        .rsplit_once('/')
        .map_or(".", |(f, _)| f)
        .to_string();
    let mut compared = 0;
    for s in after.symbols.iter().filter(|s| summarized(s)) {
        if (s.file.as_str(), s.name.as_str(), s.container.as_deref())
            == (
                changed.0.as_str(),
                changed.1.as_str(),
                Some(changed.2.as_str()),
            )
        {
            continue;
        }
        let old = before
            .symbols
            .iter()
            .find(|o| identity(o) == identity(s))
            .unwrap_or_else(|| panic!("{s:?} is new"));
        assert_eq!(old.summary_key, s.summary_key, "the key of {s:?}");
        assert!(s.summary_key.is_some(), "{s:?} has no summary");
        compared += 1;
    }
    assert!(compared >= 20, "only {compared} symbols compared");
    for (path, key) in &after.file_keys {
        if *path != changed.0 {
            assert_eq!(
                before.file_keys.get(path),
                Some(key),
                "the summary of {path}"
            );
        }
    }
    for (path, key) in &after.modules {
        if *path != folder {
            assert_eq!(before.modules.get(path), Some(key), "the summary of {path}");
        }
    }
}

#[when("I reformat a Python file and update the index")]
fn reformat_python(world: &mut IndexWorld) {
    let path = "python/polyglot/report.py";
    let text = fs::read_to_string(world.root.join(path)).expect("read report.py");
    let reformatted = text
        .replace("    def __init__(self, rows):\n", "    def __init__( self,rows ):\n\n")
        .replace(
            "        return \"\\n\".join(str(row) for row in self.rows)\n",
            "        return \"\\n\".join(\n            str(row)\n            for row in self.rows\n        )\n",
        )
        .replace("def build_report(path):\n", "def build_report(\n    path,\n):\n");
    assert_ne!(text, reformatted, "the reformat changed nothing");
    fs::write(world.root.join(path), reformatted).expect("write report.py");
    world.update(false);
}

#[then("the summarizer did not run")]
fn the_summarizer_did_not_run(world: &mut IndexWorld) {
    let ran: Vec<_> = world
        .requests
        .iter()
        .map(|r| format!("{} {}:{}", r.level.as_str(), r.path, r.name))
        .collect();
    assert!(ran.is_empty(), "the summarizer ran for {ran:#?}");
}

#[when("I delete a TypeScript function and update the index")]
fn delete_typescript(world: &mut IndexWorld) {
    let before = world.before.as_ref().expect("no snapshot");
    assert!(
        before.edges.iter().any(|e| e.caller.name == "formatPrice")
            && before
                .edges
                .iter()
                .any(|e| e.callee.as_ref().is_some_and(|c| c.name == "formatPrice")),
        "formatPrice must have edges in and out before the change"
    );
    edit(
        &world.root,
        "web/src/format.ts",
        "\nexport function formatPrice(money: Money): string {\n  return `${(money.cents / 100).toFixed(2)} ${money.currency}`;\n}\n",
        "",
    );
    world.update(false);
}

#[then("the function and its edges are not in the structure")]
fn function_and_edges_gone(world: &mut IndexWorld) {
    let snapshot = world.snapshot();
    let found: Vec<_> = snapshot
        .symbols
        .iter()
        .filter(|s| s.name == "formatPrice")
        .collect();
    assert!(found.is_empty(), "the function is still there: {found:#?}");
    for e in &snapshot.edges {
        let names = std::iter::once(&e.caller)
            .chain(e.callee.as_ref())
            .chain(&e.candidates);
        for end in names {
            assert!(
                end.name != "formatPrice",
                "an edge of the function is left: {e:#?}"
            );
        }
    }
}

#[given("an index of branch A, where one Go function changed")]
fn branch_a(world: &mut IndexWorld) {
    if world.indexes.is_empty() {
        a_summarized_index_of_the_fixture(world);
    }
    let changed = (
        "go/greet/greet.go".to_string(),
        "Title".to_string(),
        String::new(),
    );
    make_branch(world, changed, |root| {
        edit(
            root,
            "go/greet/greet.go",
            "name[0] == ' '",
            "name[0] == '\\t'",
        )
    });
}

#[given("an index of branch B, where one Rust function changed")]
fn branch_b(world: &mut IndexWorld) {
    let changed = (
        "src/stack.rs".to_string(),
        "pop".to_string(),
        "Stack".to_string(),
    );
    make_branch(world, changed, |root| {
        edit(root, "src/stack.rs", POP_BEFORE, POP_AFTER)
    });
}

/// Copy the base tree and its index, change the copy, and update the copied
/// index from it.
fn make_branch(world: &mut IndexWorld, changed: (String, String, String), change: impl Fn(&Path)) {
    let base_root = world.root.clone();
    let base_db = world.indexes[0].clone();
    let dir = TempDir::new().expect("make a folder for the branch");
    let root = dir.path().join("tree");
    copy_tree(&base_root, &root);
    let db = dir.path().join("index.sqlite");
    fs::copy(&base_db, &db).expect("copy the base index");
    change(&root);
    let mut fake = FakeSummarizer::default();
    update(
        &root,
        &db,
        &mut fake,
        &DeterministicProvider::default(),
        &UpdateOptions {
            build: build_options(None, None),
            ..UpdateOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        fake.requests
            .iter()
            .any(|r| r.level == Level::Symbol && r.path == changed.0 && r.name == changed.1),
        "the branch did not summarize {changed:?}"
    );
    world.branches.push((db, changed));
    world.extra.push(dir);
}

#[when("I merge the 2 indexes for the merged tree")]
fn merge_the_branches(world: &mut IndexWorld) {
    assert_eq!(world.branches.len(), 2, "2 branches were expected");
    let dir = TempDir::new().expect("make a folder for the merge");
    let root = dir.path().join("tree");
    copy_tree(&world.root, &root);
    edit(
        &root,
        "go/greet/greet.go",
        "name[0] == ' '",
        "name[0] == '\\t'",
    );
    edit(&root, "src/stack.rs", POP_BEFORE, POP_AFTER);
    let out = dir.path().join("merged.sqlite");
    let mut fake = FakeSummarizer::default();
    let report = merge(
        &root,
        &[&world.branches[0].0, &world.branches[1].0],
        &out,
        &mut fake,
        &DeterministicProvider::default(),
        &UpdateOptions {
            build: build_options(None, None),
            ..UpdateOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    world.requests = fake.requests;
    world.update_report = Some(report);
    world.root = root;
    world.indexes.push(out);
    world.extra.push(dir);
}

#[then("the merged index has both changed functions with their new summaries")]
fn merged_has_both(world: &mut IndexWorld) {
    let merged = world.symbols();
    let before = world.before.clone().expect("no snapshot");
    for (db, changed) in world.branches.clone() {
        let branch = Index::open(&db).expect("open a branch index");
        let symbols = branch.symbols().expect("read the symbols");
        let branch_key = symbol_key(&symbols, &changed).expect("the branch has a key");
        let merged_key = symbol_key(&merged, &changed).expect("the merged index has a key");
        assert_eq!(branch_key, merged_key, "the key of {changed:?}");
        assert_ne!(
            symbol_key(&before.symbols, &changed),
            Some(merged_key.clone()),
            "the key of {changed:?} did not change on its branch"
        );
        let summary = world.index().summary(&merged_key).expect("read");
        assert!(
            summary.is_some_and(|s| s.vector.is_some()),
            "{changed:?} has no summary and vector"
        );
    }
}

#[given("a summarized index, where function A calls B, and B calls C")]
fn a_chain(world: &mut IndexWorld) {
    a_summarized_index_of_the_fixture(world);
    let edges = world.edges();
    let calls = |from: &str, to: &str| {
        edges.iter().any(|e| {
            e.caller.file == "go/greet/greet.go"
                && e.caller.name == from
                && e.class == "certain"
                && e.callee.as_ref().is_some_and(|c| c.name == to)
        })
    };
    assert!(
        calls("Greet", "Message") && calls("Message", "Title"),
        "the fixture has no chain"
    );
}

#[when("the signature of C changes and I update the index")]
fn change_c_signature(world: &mut IndexWorld) {
    edit(
        &world.root,
        "go/greet/greet.go",
        "func Title(name string) string {",
        "func Title(name string, rest ...string) string {",
    );
    world.update(false);
}

#[then("C and B are summarized again")]
fn c_and_b_again(world: &mut IndexWorld) {
    let ran = world.symbol_requests();
    for name in ["Title", "Message"] {
        let want = format!("go/greet/greet.go:{name}");
        assert!(
            ran.contains(&want),
            "{name} was not summarized again: {ran:?}"
        );
    }
}

#[then("A is not summarized again")]
fn a_not_again(world: &mut IndexWorld) {
    let ran = world.symbol_requests();
    assert_eq!(ran.len(), 2, "only C and B were expected: {ran:?}");
    assert!(
        !ran.contains(&"go/greet/greet.go:Greet".to_string()),
        "{ran:?}"
    );
}

#[when("I move a Python file to another folder and update the index")]
fn move_python(world: &mut IndexWorld) {
    let from = "python/polyglot/json_loader.py";
    let to = "python/polyglot/loaders/json_loader.py";
    fs::create_dir_all(world.root.join("python/polyglot/loaders")).expect("make the folder");
    fs::rename(world.root.join(from), world.root.join(to)).expect("move the file");
    world.moved = Some((from.into(), to.into()));
    world.update(false);
}

#[then("the file is summarized again")]
fn moved_file_again(world: &mut IndexWorld) {
    let (_, to) = world.moved.clone().expect("no file moved");
    let files: Vec<_> = world
        .requests
        .iter()
        .filter(|r| r.level == Level::File)
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(files, [to.as_str()], "the file summaries");
    let request = world
        .requests
        .iter()
        .find(|r| r.level == Level::File)
        .expect("a file request");
    assert!(
        request.prompt().contains(&to),
        "the prompt has no path: {}",
        request.prompt()
    );
}

#[then("its symbols are not summarized again")]
fn moved_symbols_stay(world: &mut IndexWorld) {
    let (from, to) = world.moved.clone().expect("no file moved");
    assert_eq!(world.symbol_requests(), Vec::<String>::new());
    let before = world.before.clone().expect("no snapshot");
    let after = world.symbols();
    let moved: Vec<_> = after
        .iter()
        .filter(|s| s.file == to && summarized(s))
        .collect();
    assert!(!moved.is_empty(), "the moved file has no symbols");
    for s in moved {
        let old = before
            .symbols
            .iter()
            .find(|o| o.file == from && o.name == s.name && o.kind == s.kind)
            .unwrap_or_else(|| panic!("{s:?} was not in the old file"));
        assert_eq!(old.summary_key, s.summary_key, "the key of {s:?}");
    }
}

#[then("SCIP did not run")]
fn scip_did_not_run(world: &mut IndexWorld) {
    let report = world.update_report.as_ref().expect("no update ran");
    assert!(
        report.build.scip.is_none(),
        "SCIP ran: {:?}",
        report.build.scip
    );
}

#[then("the edges of the changed file have name classes, marked to be replaced")]
fn changed_edges_are_marked(world: &mut IndexWorld) {
    let (file, name, _) = world.changed.clone().expect("no function changed");
    let edges = world.edges();
    // The edges of the changed function. The other functions of the file
    // keep their SCIP edges (the scenario "An unchanged symbol in a changed
    // file keeps its SCIP edges").
    let changed: Vec<_> = edges
        .iter()
        .filter(|e| e.caller.file == file && e.caller.name == name)
        .collect();
    assert!(!changed.is_empty(), "{name} in {file} has no edges");
    for e in &changed {
        assert!(
            e.origin != "scip" && e.scip_pending,
            "an edge of {name} is not a marked name class: {e:#?}"
        );
    }
    // The edges of the other Rust files are the SCIP edges of the base.
    let before = world.before.clone().expect("no snapshot");
    let key = |e: &EdgeRecord| {
        (
            e.caller.file.clone(),
            e.caller.name.clone(),
            e.line,
            e.col,
            e.callee_name.clone(),
            e.class.clone(),
            e.origin.clone(),
        )
    };
    let kept: BTreeSet<_> = edges
        .iter()
        .filter(|e| e.caller.file.ends_with(".rs") && e.caller.file != file)
        .map(key)
        .collect();
    let base: BTreeSet<_> = before
        .edges
        .iter()
        .filter(|e| e.caller.file.ends_with(".rs") && e.caller.file != file)
        .map(key)
        .collect();
    assert_eq!(kept, base, "the other Rust edges changed");
    assert!(
        edges
            .iter()
            .filter(|e| e.caller.file != file)
            .all(|e| !e.scip_pending),
        "an edge of an unchanged file is marked"
    );
}

/// `Stack::new` gets 1 more line, so that `push` and `pop` move down and
/// their code stays the same.
#[when("I change one function in a Rust file and update the index with no options")]
fn change_one_of_a_file(world: &mut IndexWorld) {
    edit(
        &world.root,
        "src/stack.rs",
        "        Stack {\n            items: Vec::with_capacity(DEFAULT_CAPACITY),\n        }\n",
        "        let capacity = DEFAULT_CAPACITY;\n        let items = Vec::with_capacity(capacity);\n\n        Stack { items }\n",
    );
    world.changed = Some(("src/stack.rs".into(), "new".into(), "Stack".into()));
    world.update(false);
}

/// An edge with its line from the start of its caller: the line, the
/// column, the called name, the class, the origin and the callee.
type MovedEdge = (u32, u32, String, String, String, Option<(String, String)>);

/// The edges from `name` in `file`, with the line from the start of the
/// caller, so that 2 builds compare when the caller moved.
fn edges_from(edges: &[EdgeRecord], file: &str, name: &str) -> BTreeSet<MovedEdge> {
    edges
        .iter()
        .filter(|e| e.caller.file == file && e.caller.name == name)
        .map(|e| {
            (
                e.line - e.caller.start_line,
                e.col,
                e.callee_name.clone(),
                e.class.clone(),
                e.origin.clone(),
                e.callee.as_ref().map(|c| (c.file.clone(), c.name.clone())),
            )
        })
        .collect()
}

#[then("the other functions of that file keep their SCIP edges and their keys")]
fn others_keep_scip(world: &mut IndexWorld) {
    let (file, changed, _) = world.changed.clone().expect("no function changed");
    let before = world.before.clone().expect("no snapshot");
    let after = world.snapshot();
    let others: BTreeSet<&str> = after
        .symbols
        .iter()
        .filter(|s| {
            s.file == file && matches!(s.kind.as_str(), "function" | "method") && s.name != changed
        })
        .map(|s| s.name.as_str())
        .collect();
    assert!(others.len() >= 2, "{file} has too few other functions");
    for name in others {
        let old = before
            .symbols
            .iter()
            .find(|s| s.file == file && s.name == name)
            .expect("the function was in the base");
        let new = after
            .symbols
            .iter()
            .find(|s| s.file == file && s.name == name)
            .expect("the function is in the update");
        assert_ne!(old.start_line, new.start_line, "{name} did not move");
        let was = edges_from(&before.edges, &file, name);
        let is = edges_from(&after.edges, &file, name);
        assert!(!was.is_empty(), "{name} had no edges");
        assert!(
            was.iter().all(|e| e.4 == "scip"),
            "{name} had an edge that is not from SCIP: {was:#?}"
        );
        assert_eq!(is, was, "the edges of {name}");
        assert!(
            after
                .edges
                .iter()
                .filter(|e| e.caller.file == file && e.caller.name == name)
                .all(|e| !e.scip_pending),
            "an edge of {name} is marked"
        );
        assert_eq!(new.summary_key, old.summary_key, "the key of {name}");
    }
}

#[then("only the changed function has name-class edges, marked to be replaced")]
fn only_changed_is_marked(world: &mut IndexWorld) {
    let (file, changed, _) = world.changed.clone().expect("no function changed");
    let edges = world.edges();
    let marked: BTreeSet<_> = edges
        .iter()
        .filter(|e| e.scip_pending)
        .map(|e| (e.caller.file.clone(), e.caller.name.clone()))
        .collect();
    assert_eq!(
        marked,
        BTreeSet::from([(file.clone(), changed.clone())]),
        "the marked edges"
    );
    let own: Vec<_> = edges
        .iter()
        .filter(|e| e.caller.file == file && e.caller.name == changed)
        .collect();
    assert!(!own.is_empty(), "{changed} has no edges");
    for e in own {
        assert!(e.origin != "scip" && e.scip_pending, "{e:#?}");
    }
}

#[then("the summarizer did not run for the unchanged functions")]
fn ran_for_the_changed_only(world: &mut IndexWorld) {
    let (file, changed, _) = world.changed.clone().expect("no function changed");
    assert_eq!(world.symbol_requests(), [format!("{file}:{changed}")]);
}

#[then("SCIP ran, and the marked edges are SCIP edges again")]
fn marked_edges_are_scip(world: &mut IndexWorld) {
    let report = world.update_report.as_ref().expect("no update ran");
    assert!(report.build.scip.is_some(), "SCIP did not run");
    let (file, _, _) = world.changed.clone().expect("no function changed");
    let edges = world.edges();
    assert!(
        edges.iter().all(|e| !e.scip_pending),
        "an edge is still marked"
    );
    let changed: Vec<_> = edges.iter().filter(|e| e.caller.file == file).collect();
    assert!(!changed.is_empty(), "{file} has no edges");
    for e in changed {
        assert_eq!(e.origin, "scip", "{e:#?}");
    }
}

#[given(
    "the polyglot fixture, where a Rust function calls a fixture function inside a format! argument and inside a custom macro that keeps its input as text"
)]
fn macro_calls(world: &mut IndexWorld) {
    world.root = fixture_root();
    let text = fs::read_to_string(world.root.join("src/labels.rs")).expect("read src/labels.rs");
    assert!(
        text.contains("format!(\"{} ({source})\", count_text(count))")
            && text.contains("quoted!(fixture_name())")
            && text.contains("macro_rules! quoted"),
        "src/labels.rs has not the 2 macro calls"
    );
}

/// The edge from `label` in src/labels.rs to the fixture function `name`.
fn label_edge(world: &IndexWorld, name: &str) -> EdgeRecord {
    let edges = world.edges();
    let found: Vec<_> = edges
        .iter()
        .filter(|e| {
            e.caller.file == "src/labels.rs" && e.caller.name == "label" && calls_name(e, name)
        })
        .collect();
    assert_eq!(found.len(), 1, "1 edge to {name} was expected: {found:#?}");
    let e = found[0].clone();
    assert_eq!(
        e.callee
            .as_ref()
            .map(|c| (c.file.as_str(), c.name.as_str())),
        Some(("src/labels.rs", name)),
        "{e:#?}"
    );
    assert_eq!(e.class, "certain", "{e:#?}");
    e
}

#[then(
    expr = "the call inside the custom macro is an edge with a name class and the source {string}"
)]
fn macro_edge(world: &mut IndexWorld, origin: String) {
    let e = label_edge(world, "fixture_name");
    assert_eq!(e.origin, origin, "{e:#?}");
    assert!(!e.scip_pending, "{e:#?}");
}

#[then("the call inside the format! argument is a SCIP edge")]
fn format_edge(world: &mut IndexWorld) {
    let e = label_edge(world, "count_text");
    assert_eq!(e.origin, "scip", "{e:#?}");
}

#[then("no call that SCIP resolved is also an edge from the macro text")]
fn no_double_macro_edges(world: &mut IndexWorld) {
    let edges = world.edges();
    let scip: BTreeSet<_> = edges
        .iter()
        .filter(|e| e.origin == "scip")
        .map(|e| (e.caller.clone(), e.line, e.col))
        .map(|(c, l, k)| (c.file, c.name, c.start_line, l, k))
        .collect();
    let macro_text: Vec<_> = edges.iter().filter(|e| e.origin == "macro-text").collect();
    assert!(!macro_text.is_empty(), "no edge comes from the macro text");
    for e in macro_text {
        let at = (
            e.caller.file.clone(),
            e.caller.name.clone(),
            e.caller.start_line,
            e.line,
            e.col,
        );
        assert!(!scip.contains(&at), "SCIP resolved this call too: {e:#?}");
    }
}

#[when("I change a file and do not commit it, and update the index")]
fn change_uncommitted(world: &mut IndexWorld) {
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=kairos",
                "-c",
                "user.email=kairos@example.invalid",
            ])
            .args(args)
            .current_dir(&world.root)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    let path = world.root.join("go/greet/greet.go");
    let mut text = fs::read_to_string(&path).expect("read greet.go");
    text.push_str("\n// Farewell builds the text of a farewell.\nfunc Farewell(name string) string {\n\treturn \"Bye, \" + Title(name)\n}\n");
    fs::write(&path, text).expect("write greet.go");
    let status = git(&["status", "--porcelain"]);
    assert_eq!(
        status.trim(),
        "M go/greet/greet.go",
        "the change must be uncommitted"
    );
    world.update(false);
}

#[then("the index describes the changed file")]
fn index_has_the_change(world: &mut IndexWorld) {
    let index = world.index();
    let symbols = world.symbols();
    let farewell = symbols
        .iter()
        .find(|s| s.file == "go/greet/greet.go" && s.name == "Farewell")
        .expect("the new function is not in the index");
    let key = farewell
        .summary_key
        .as_deref()
        .expect("the new function has no summary key");
    assert!(
        index.summary(key).expect("read").is_some(),
        "the new function has no summary"
    );
    assert!(
        world
            .edges()
            .iter()
            .any(|e| e.caller.name == "Farewell"
                && e.callee.as_ref().is_some_and(|c| c.name == "Title")),
        "the new call is not an edge"
    );
    let bytes = fs::read(world.root.join("go/greet/greet.go")).expect("read greet.go");
    let file = world
        .files()
        .into_iter()
        .find(|f| f.path == "go/greet/greet.go")
        .expect("the file is in the index");
    assert_eq!(
        file.size,
        bytes.len() as u64,
        "the index has the committed file"
    );
    assert_eq!(world.symbol_requests(), ["go/greet/greet.go:Farewell"]);
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
