//! The BDD scenarios of kairos-index (COLLIERY-I-0264, "The tests: behaviour
//! first"). The `.feature` files are in `tests/features/`; the steps are here.
//! They run on the fixture repository in `tests/fixtures/polyglot/`, whose
//! `expected-symbols.toml` states what the index must hold.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cucumber::{World, given, then, when};
use kairos_index::{FileRecord, Index, SymbolRecord, build_structure};
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
        build_structure(&self.root, &db).expect("build the structure");
        self.indexes.push(db);
    }
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

// --- When ---------------------------------------------------------------------

#[when("I build the structure")]
fn build_once(world: &mut IndexWorld) {
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

#[tokio::main]
async fn main() {
    let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/features");
    IndexWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(features)
        .await;
}
