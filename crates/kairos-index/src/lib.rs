//! The code index (COLLIERY-I-0264).
//!
//! An index has two parts, in one SQLite file:
//!
//! 1. **The structure**: the files with the decision of the file rules, the
//!    symbols that the vendored narsil code extracts, and the edges. It is
//!    made again from the tree on each build.
//! 2. **The summary pool**: each summary and its vector, keyed by a hash of
//!    what made it. A build of the structure keeps it.
//!
//! This crate builds the structure ([`build_structure`]) and reads it back
//! ([`Index`]). The Rust call edges come from `rust-analyzer scip` with the
//! build turned off (see `scip`), with a pinned rust-analyzer release (see
//! [`rust_analyzer`]); the edges of the other languages come from
//! their names (see `edges`). [`summarize`] writes the summaries and their
//! vectors (COLLIERY-T-1850), with a [`Summarizer`]: the fake one for tests,
//! or `LlamaSummarizer` with the feature `llama`.
//!
//! [`update`] builds the structure again from the tree (with the changes
//! that are not committed) and runs the summarizer only for the keys that
//! the pool does not have. It runs no SCIP unless it is told to: a Rust
//! function whose code did not change keeps its SCIP edges. [`merge`] is an update of the
//! merged tree with the pools of 2 indexes (COLLIERY-T-1851).

mod calls;
mod duplicates;
mod edges;
mod extract;
#[cfg(feature = "llama")]
mod llama;
mod parse_cache;
mod query;
pub mod rules;
pub mod rust_analyzer;
mod schema;
mod scip;
pub mod store;
mod summary;
mod tokens;
pub mod tools;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use kairos_narsil::parser::LanguageParser;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

pub use duplicates::{
    DEFAULT_MIN_LINES, DuplicateGroup, DuplicateKind, DuplicateOptions, Duplicates,
    MIN_NEAR_TOKENS, NEAR_THRESHOLD, SAME_IDEA_THRESHOLD,
};
#[cfg(feature = "llama")]
pub use llama::{LlamaModelFile, LlamaSummarizer, MAX_NEW_TOKENS};
pub use query::{
    CallEdge, Callees, Counts, External, FileInfo, Lookup, ModuleInfo, NoSymbolCallee, PathStep,
    SearchHit, SearchMode, SearchResult, SymbolInfo,
};
pub use rules::{Decision, Origin, RuleSet};
pub use rust_analyzer::BuildOptions;
pub use schema::SCHEMA_VERSION;
pub use summary::{
    FakeSummarizer, Level, LevelCount, MODEL_FILE_NAME, SUMMARIZED_KINDS, SYSTEM_PROMPT,
    SummarizeOptions, Summarizer, SummaryCall, SummaryReport, SummaryRequest, link, model_path,
    summarize,
};

/// The index of a checkout, from its root (COLLIERY-T-1852). A build does not
/// read it, or its journal, as a file of the tree.
pub const INDEX_FILE: &str = ".kairos/index.db";

/// A file larger than this is read only in part: enough for the header and
/// the rules. The default rules exclude it (`too-large`), so it is never
/// parsed.
const READ_LIMIT: u64 = 16 * 1024 * 1024;

/// What a build of the structure did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub files: usize,
    pub parsed_files: usize,
    /// The parsed files whose parse came from the parse cache of the index
    /// (KAIROS-T-0297): their content did not change.
    pub cached_files: usize,
    pub symbols: usize,
    pub edges: EdgeStats,
    /// The `rust-analyzer scip` run, if the tree has a Cargo workspace at
    /// its root with Rust files to index.
    pub scip: Option<ScipRun>,
    /// The token vectors of the functions (COLLIERY-T-1857).
    pub token_vectors: TokenVectorStats,
}

/// The token vectors of a build: made from the code, or reused from the
/// index for a function whose tree hash the index had already.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenVectorStats {
    pub made: usize,
    pub reused: usize,
}

/// The count of the edges of a build, by class and origin, and the Rust
/// calls that SCIP did not resolve.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdgeStats {
    pub certain_scip: usize,
    pub external_scip: usize,
    pub certain_name: usize,
    pub possible_name: usize,
    pub external_name: usize,
    /// Rust call sites with no SCIP reference at the called name. They got
    /// a name class.
    pub rust_unresolved: usize,
    /// SCIP references to a function of an indexed file that has no narsil
    /// symbol at that place. They got a name class.
    pub scip_join_misses: usize,
    /// SCIP references to a symbol with more than one definition, where
    /// the crate of the reference does not choose one. They got a name class.
    pub scip_ambiguous: usize,
    /// Calls in the text of a Rust macro that SCIP did not resolve. They got
    /// a name class, with the origin `macro-text`.
    pub macro_text: usize,
    /// SCIP edges kept from a base index, for the Rust functions whose code
    /// did not change. They are not in `certain_scip` or `external_scip`.
    /// A kept edge whose callee is gone is a name class, counted there.
    pub kept_scip: usize,
    /// Name classes that wait for a SCIP run (`scip_pending`).
    pub pending: usize,
}

impl EdgeStats {
    /// Each edge of the build: from the SCIP run, by name, and kept from
    /// the base (COLLIERY-T-1854).
    pub fn total(&self) -> usize {
        self.certain_scip
            + self.external_scip
            + self.certain_name
            + self.possible_name
            + self.external_name
            + self.kept_scip
    }
}

/// One `rust-analyzer scip` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipRun {
    /// The temporary folder of the run. The run deletes it at its end.
    pub work_dir: PathBuf,
    pub elapsed: std::time::Duration,
    /// The build-script command that rust-analyzer logged. `true` runs
    /// nothing.
    pub build_script_command: Option<String>,
    /// Whether rust-analyzer logged that it started a proc-macro server.
    pub proc_macro_server_started: Option<bool>,
    /// What `--version` of the rust-analyzer of the run printed.
    pub rust_analyzer: String,
    /// The std source that rust-analyzer logged: the `library/` folder of
    /// the pinned `rust-src` archive. The run is refused if it is not.
    pub std_source: Option<PathBuf>,
    pub documents: usize,
    pub occurrences: usize,
    /// The targets that the run leaves out, by root file from the
    /// repository root: each shares a module file with an earlier target.
    /// Only a rust-analyzer with the defect of 1.93.0 needs this (see
    /// `scip`). Their calls get name classes.
    pub left_out_targets: Vec<String>,
}

/// One file of the tree and the decision for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub path: String,
    pub language: Option<String>,
    pub decision: String,
    pub rule: String,
    pub rule_origin: String,
    pub size: u64,
    pub longest_line: u64,
    pub content_hash: String,
    pub parse_error: Option<String>,
}

/// One symbol of the structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRecord {
    pub file: String,
    pub name: String,
    pub container: Option<String>,
    pub kind: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub signature: Option<String>,
    pub tree_hash: String,
    pub is_test: bool,
    /// The key of its summary in the pool, when it has one.
    pub summary_key: Option<String>,
    /// The token vector of a function or a method (COLLIERY-T-1857): its
    /// MinHash signature.
    pub token_vector: Option<Vec<u32>>,
    /// The count of the tokens of the token vector.
    pub token_count: Option<u32>,
}

/// A symbol, as an edge names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRef {
    pub file: String,
    pub name: String,
    pub container: Option<String>,
    pub start_line: u32,
}

/// One edge of the call graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRecord {
    pub caller: SymbolRef,
    /// The called symbol, for a `certain` edge.
    pub callee: Option<SymbolRef>,
    /// The called name: as SCIP names it (`Vec::push`) or as the code
    /// writes it (`fmt.Println`).
    pub callee_name: String,
    pub kind: String,
    pub class: String,
    pub origin: String,
    /// The place of the called name, from 1.
    pub line: u32,
    pub col: u32,
    /// The symbols that the name matches, for a `possible` edge.
    pub candidates: Vec<SymbolRef>,
    /// A name class in a changed Rust file, until a SCIP run replaces it.
    pub scip_pending: bool,
    /// The place of the definition, for a `certain` edge with no callee: a
    /// function that a macro makes (COLLIERY-T-2531).
    pub target: Option<EdgeTarget>,
}

/// The place of a definition with no symbol (COLLIERY-T-2531).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeTarget {
    pub file: String,
    /// From 1.
    pub line: u32,
    /// A macro invocation holds the place: the macro makes the function.
    pub macro_made: bool,
}

/// An error of the index. The texts follow ASD-STE100: a user can see them.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("The repository root {0} is not a folder.")]
    NotAFolder(PathBuf),
    #[error("Cannot read {path}: {source}.")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read the tree: {0}.")]
    Walk(#[from] ignore::Error),
    #[error("The rules file {path} is not valid: {message}.")]
    Rules { path: PathBuf, message: String },
    #[error(
        "The index has schema version {found}. This version of Kairos reads version {}.",
        SCHEMA_VERSION
    )]
    SchemaVersion { found: i64 },
    #[error("The index database has an error: {0}.")]
    Db(#[from] rusqlite::Error),
    #[error("The parser did not start: {0}.")]
    Parser(String),
    #[error("The SCIP run of rust-analyzer failed: {0}.")]
    Scip(String),
    #[error(
        "The rust-analyzer binary {path} has the sha256 {found}. The pinned value is {expected}."
    )]
    RustAnalyzerChecksum {
        path: PathBuf,
        found: String,
        expected: String,
    },
    #[error(
        "The pinned rust-analyzer is not at {0}. Run `angreal dev fetch-rust-analyzer` to download it."
    )]
    RustAnalyzerMissing(PathBuf),
    #[error("The index has no pinned rust-analyzer release for {0}.")]
    RustAnalyzerPlatform(String),
    #[error("The download of rust-analyzer failed: {0}.")]
    RustAnalyzerDownload(String),
    #[error(
        "The std source archive {path} has the sha256 {found}. The pinned value is {expected}."
    )]
    StdSourceChecksum {
        path: PathBuf,
        found: String,
        expected: String,
    },
    #[error(
        "The pinned std source is not at {0}. Run `angreal dev fetch-rust-analyzer` to download it."
    )]
    StdSourceMissing(PathBuf),
    #[error("The download of the std source failed: {0}.")]
    StdSourceDownload(String),
    #[error("The tool file {path} has the sha256 {found}. The pinned value is {expected}.")]
    ToolChecksum {
        path: PathBuf,
        found: String,
        expected: String,
    },
    #[error("The download of a tool failed: {0}.")]
    ToolDownload(String),
    #[error("The SCIP run did not turn off the build: {0}.")]
    BuildNotOff(String),
    #[error("The summarizer gave no summary for {name}: {message}.")]
    Summary { name: String, message: String },
    #[error("The file {0} changed after the build of the structure. Build the structure again.")]
    Changed(String),
    #[error("The summary pool has vectors of the model {pool}. This run uses {run}.")]
    VectorModel { pool: String, run: String },
    #[error("The vectors failed: {0}.")]
    Embed(String),
    #[error("The summary model failed: {0}.")]
    Model(String),
    #[error("The index {0} is there already. Give the name of a new file.")]
    Exists(PathBuf),
    #[error("The parse cache of the index has an error: {0}.")]
    ParseCache(String),
}

/// Build the structure of the tree at `root` into the index at `db`.
///
/// The index file is made if it is not there. Its structure is deleted and
/// written again; its summary pool is kept. The repository's rules are read
/// from `.kairos/index-rules.toml` under `root`.
///
/// If `root` has a `Cargo.toml` and Rust files to index, the Rust edges
/// come from `rust-analyzer scip`, with the build turned off. That needs the
/// pinned rust-analyzer release and the pinned std source, which this
/// function downloads on first need (see [`rust_analyzer`]).
pub fn build_structure(root: &Path, db: &Path) -> Result<BuildReport, IndexError> {
    build_structure_with(root, db, &BuildOptions::default())
}

/// [`build_structure`], with the choice of the rust-analyzer binary and of
/// its download.
pub fn build_structure_with(
    root: &Path,
    db: &Path,
    options: &BuildOptions,
) -> Result<BuildReport, IndexError> {
    let known = known_vectors_at(db)?;
    build(root, db, Mode::Scip(options), &known, Parse::All)
}

/// The token vectors of the index at `db`, by tree hash, for a build that
/// reuses them (COLLIERY-T-1857). None if `db` is not there, or if it has
/// another schema version (the build then refuses it).
fn known_vectors_at(db: &Path) -> Result<HashMap<String, (Vec<u32>, u32)>, IndexError> {
    if !db.is_file() {
        return Ok(HashMap::new());
    }
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Ok(HashMap::new());
    }
    known_vectors(&conn)
}

fn known_vectors(conn: &Connection) -> Result<HashMap<String, (Vec<u32>, u32)>, IndexError> {
    let mut stmt = conn.prepare(
        "SELECT tree_hash, token_vector, token_count FROM symbols
         WHERE token_vector IS NOT NULL AND token_count IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (tokens::from_blob(&r.get::<_, Vec<u8>>(1)?), r.get(2)?),
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Which files a build parses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parse {
    /// Each file: a build of the whole index.
    All,
    /// The files that the parse cache of the index does not have: an update.
    Changed,
}

/// Where a build gets the Rust edges.
enum Mode<'a> {
    /// A `rust-analyzer scip` run.
    Scip(&'a BuildOptions),
    /// The SCIP edges of these indexes, for the Rust functions whose code
    /// did not change.
    Keep(&'a [edges::BaseIndex]),
}

/// Build the structure of the tree at `root` into the index at `db`, which
/// is the base: its summary pool stays, and a Rust function whose code did
/// not change keeps its SCIP edges, unless `options.rust_edges`.
pub fn update_structure(
    root: &Path,
    db: &Path,
    options: &UpdateOptions,
) -> Result<BuildReport, IndexError> {
    let known = known_vectors_at(db)?;
    if options.rust_edges {
        return build(root, db, Mode::Scip(&options.build), &known, Parse::Changed);
    }
    let conn = Connection::open(db)?;
    schema::prepare(&conn)?;
    let base = edges::BaseIndex::read(&conn)?;
    drop(conn);
    build(
        root,
        db,
        Mode::Keep(std::slice::from_ref(&base)),
        &known,
        Parse::Changed,
    )
}

/// The files of the tree at `root` that differ from the index at `db`
/// (COLLIERY-T-1854): a file with another content, a file that the index
/// does not have, and a file of the index that the tree does not have.
/// Sorted. An index of another schema version is refused.
pub fn changed_files(root: &Path, db: &Path) -> Result<Vec<String>, IndexError> {
    let mut known: HashMap<String, String> = Index::open(db)?
        .files()?
        .into_iter()
        .map(|f| (f.path, f.content_hash))
        .collect();
    let mut changed = Vec::new();
    for (rel, abs) in walk(root)? {
        let hash = read_file(&abs)?.content_hash;
        if known.remove(&rel).is_none_or(|known| known != hash) {
            changed.push(rel);
        }
    }
    changed.extend(known.into_keys());
    changed.sort();
    Ok(changed)
}

fn build(
    root: &Path,
    db: &Path,
    mode: Mode<'_>,
    known: &HashMap<String, (Vec<u32>, u32)>,
    parse: Parse,
) -> Result<BuildReport, IndexError> {
    if !root.is_dir() {
        return Err(IndexError::NotAFolder(root.to_path_buf()));
    }
    let rules = RuleSet::for_repository(root)?;
    let parser = LanguageParser::new().map_err(|e| IndexError::Parser(e.to_string()))?;
    let paths = walk(root)?;

    // 1. Read, decide and parse each file.
    let mut report = BuildReport {
        files: 0,
        parsed_files: 0,
        cached_files: 0,
        symbols: 0,
        edges: EdgeStats::default(),
        scip: None,
        token_vectors: TokenVectorStats::default(),
    };
    let cache = match parse {
        Parse::Changed => parse_cache::Reader::open(db)?,
        Parse::All => None,
    };
    let mut used_keys = HashSet::new();
    let mut fresh = Vec::new();
    let mut prepared = Vec::with_capacity(paths.len());
    for (rel, abs) in &paths {
        let file = read_file(abs)?;
        let header: String = file
            .text
            .lines()
            .take(rules::HEADER_LINES)
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        let first_line = header.lines().next().unwrap_or("");
        let verdict = rules.decide(&rules::FileFacts {
            path: rel,
            size: file.size,
            longest_line: file.longest_line,
            first_line,
            header: &header,
        });
        let language = extract::language_of(rel);

        let mut found = None;
        let mut parse_error = None;
        if let (true, Some(language)) = (verdict.decision.is_parsed(), language) {
            if !file.complete || !file.utf8 {
                parse_error = Some("the file is not complete UTF-8 text".to_string());
            } else {
                let test_file = verdict.decision == Decision::Test;
                let key = parse_cache::key(rel, language, test_file, &file.content_hash);
                let cached = match &cache {
                    Some(cache) => cache
                        .get(&key)?
                        .and_then(|bytes| parse_cache::decode(&bytes, known)),
                    None => None,
                };
                if let Some(extracted) = cached {
                    found = Some(extracted);
                    report.cached_files += 1;
                } else {
                    match extract::extract(&parser, rel, language, &file.text, test_file, known) {
                        Ok(extracted) => {
                            fresh.push((key.clone(), parse_cache::encode(&extracted)?));
                            found = Some(extracted);
                        }
                        Err(e) => parse_error = Some(e),
                    }
                }
                used_keys.insert(key);
                report.parsed_files += 1;
            }
        }
        prepared.push(Prepared {
            rel,
            language,
            verdict,
            file,
            found,
            parse_error,
        });
    }

    drop(cache);

    // 2. The SCIP index of the Cargo workspace at the root, or the SCIP
    // edges of the base indexes.
    let has_rust = prepared
        .iter()
        .any(|p| p.language == Some("rust") && p.found.is_some());
    let workspace = has_rust && root.join("Cargo.toml").is_file();
    let scip_index = if let (true, Mode::Scip(options)) = (workspace, &mode) {
        let (run, index, crates) = scip::run(root, options)?;
        report.scip = Some(run);
        Some((index, crates))
    } else {
        None
    };
    let rust = match (&scip_index, &mode) {
        (Some((index, crates)), _) => edges::RustEdges::Scip(index, crates),
        (None, Mode::Keep(bases)) if workspace => edges::RustEdges::Base { bases, mark: true },
        _ => edges::RustEdges::Names,
    };

    // 3. Write the structure.
    let mut conn = Connection::open(db)?;
    schema::prepare(&conn)?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "DELETE FROM edge_candidates; DELETE FROM edges; DELETE FROM scip_covered;
         DELETE FROM symbols; DELETE FROM files; DELETE FROM modules;",
    )?;
    {
        let mut insert_file = tx.prepare(
            "INSERT INTO files (path, language, decision, rule, rule_origin, size, longest_line, content_hash, parse_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        let mut insert_symbol = tx.prepare(
            "INSERT INTO symbols (file_id, name, container, kind, language, start_line, end_line,
                                  start_byte, end_byte, signature, tree_hash, is_test, token_vector,
                                  token_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;

        let mut parsed = Vec::new();
        let mut file_ids = Vec::new();
        for p in &prepared {
            insert_file.execute(params![
                p.rel,
                p.language,
                p.verdict.decision.as_str(),
                p.verdict.rule,
                p.verdict.origin.as_str(),
                p.file.size as i64,
                p.file.longest_line as i64,
                p.file.content_hash,
                p.parse_error,
            ])?;
            let file_id = tx.last_insert_rowid();
            report.files += 1;

            let (Some(found), Some(language)) = (&p.found, p.language) else {
                continue;
            };
            let mut symbols = Vec::with_capacity(found.symbols.len());
            for s in &found.symbols {
                insert_symbol.execute(params![
                    file_id,
                    s.name,
                    s.container,
                    s.kind,
                    language,
                    s.start_line as i64,
                    s.end_line as i64,
                    s.start_byte as i64,
                    s.end_byte as i64,
                    s.signature,
                    s.tree_hash,
                    s.is_test,
                    s.token_vector.as_deref().map(tokens::to_blob),
                    s.token_count,
                ])?;
                match (&s.token_vector, s.token_vector_made) {
                    (Some(_), true) => report.token_vectors.made += 1,
                    (Some(_), false) => report.token_vectors.reused += 1,
                    (None, _) => {}
                }
                symbols.push(edges::Sym {
                    id: tx.last_insert_rowid(),
                    name: s.name.clone(),
                    container: s.container.clone(),
                    kind: s.kind,
                    start_line: s.start_line,
                    end_line: s.end_line,
                    start_byte: s.start_byte,
                    end_byte: s.end_byte,
                    tree_hash: s.tree_hash.clone(),
                });
            }
            report.symbols += symbols.len();
            parsed.push(edges::ParsedFile {
                path: p.rel,
                language,
                text: &p.file.text,
                symbols,
                calls: &found.calls,
                use_ranges: &found.use_ranges,
                macro_calls: &found.macro_calls,
                macro_ranges: &found.macro_ranges,
            });
            file_ids.push(file_id);
        }

        let resolved = edges::resolve(&parsed, rust);
        report.edges = resolved.stats;
        let new_edges = resolved.edges;
        let mut insert_covered =
            tx.prepare("INSERT OR IGNORE INTO scip_covered (file_id, start_byte) VALUES (?1, ?2)")?;
        for (file, byte) in &resolved.covered {
            insert_covered.execute(params![file_ids[*file], *byte as i64])?;
        }
        let mut insert_edge = tx.prepare(
            "INSERT INTO edges (caller_id, callee_id, callee_name, edge_kind, class, origin, line, col,
                                scip_pending, target_file_id, target_line, target_macro)
             VALUES (?1, ?2, ?3, 'call', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        let mut insert_candidate = tx.prepare(
            "INSERT OR IGNORE INTO edge_candidates (edge_id, symbol_id) VALUES (?1, ?2)",
        )?;
        for e in &new_edges {
            insert_edge.execute(params![
                e.caller_id,
                e.callee_id,
                e.callee_name,
                e.class,
                e.origin,
                e.line as i64,
                e.col as i64,
                e.scip_pending,
                e.target.as_ref().map(|t| file_ids[t.file]),
                e.target.as_ref().map(|t| t.line as i64),
                e.target.as_ref().is_some_and(|t| t.macro_made),
            ])?;
            let edge_id = tx.last_insert_rowid();
            for c in &e.candidates {
                insert_candidate.execute(params![edge_id, c])?;
            }
        }
    }
    parse_cache::write(&tx, &used_keys, &fresh)?;
    tx.commit()?;
    Ok(report)
}

/// One file of the tree after the rules and the parser, before the write.
struct Prepared<'a> {
    rel: &'a str,
    language: Option<&'static str>,
    verdict: rules::Verdict,
    file: FileContent,
    found: Option<extract::FileExtract>,
    parse_error: Option<String>,
}

/// The files under `root`, sorted by path. `.gitignore` is honoured, so
/// `target/` and `node_modules/` are not read; `.git/` is never read. Hidden
/// files are read: `.angreal/` and `.kairos/` hold code and rules.
fn walk(root: &Path) -> Result<Vec<(String, PathBuf)>, IndexError> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    let index_file = root.join(INDEX_FILE);
    let mut files = Vec::new();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let abs = entry.into_path();
        // The index and its SQLite journal files (`-journal`, `-wal`).
        if abs
            .as_os_str()
            .as_encoded_bytes()
            .starts_with(index_file.as_os_str().as_encoded_bytes())
        {
            continue;
        }
        let rel = abs
            .strip_prefix(root)
            .expect("the walk stays under the root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        files.push((rel, abs));
    }
    files.sort();
    Ok(files)
}

struct FileContent {
    size: u64,
    longest_line: u64,
    text: String,
    utf8: bool,
    complete: bool,
    content_hash: String,
}

fn read_file(path: &Path) -> Result<FileContent, IndexError> {
    let io = |source| IndexError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    let mut bytes = Vec::with_capacity(size.min(READ_LIMIT) as usize);
    file.by_ref()
        .take(READ_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    let complete = (bytes.len() as u64) == size;
    let content_hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let (text, utf8) = match String::from_utf8(bytes) {
        Ok(text) => (text, true),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), false),
    };
    let longest_line = text
        .lines()
        .map(|line| line.chars().count() as u64)
        .max()
        .unwrap_or(0);
    Ok(FileContent {
        size,
        longest_line,
        text,
        utf8,
        complete,
        content_hash,
    })
}

/// An index on disk, open to read.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open the index at `db`. It must exist.
    pub fn open(db: &Path) -> Result<Self, IndexError> {
        if !db.is_file() {
            return Err(IndexError::Io {
                path: db.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
        let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(IndexError::SchemaVersion { found: version });
        }
        Ok(Index { conn })
    }

    /// Each file, by path.
    pub fn files(&self) -> Result<Vec<FileRecord>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, language, decision, rule, rule_origin, size, longest_line, content_hash, parse_error
             FROM files ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(FileRecord {
                path: r.get(0)?,
                language: r.get(1)?,
                decision: r.get(2)?,
                rule: r.get(3)?,
                rule_origin: r.get(4)?,
                size: r.get::<_, i64>(5)? as u64,
                longest_line: r.get::<_, i64>(6)? as u64,
                content_hash: r.get(7)?,
                parse_error: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Each symbol, by file and place.
    pub fn symbols(&self) -> Result<Vec<SymbolRecord>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, s.name, s.container, s.kind, s.language, s.start_line, s.end_line,
                    s.start_byte, s.end_byte, s.signature, s.tree_hash, s.is_test, s.summary_key,
                    s.token_vector, s.token_count
             FROM symbols s JOIN files f ON f.id = s.file_id
             ORDER BY f.path, s.start_byte, s.end_byte DESC, s.kind, s.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SymbolRecord {
                file: r.get(0)?,
                name: r.get(1)?,
                container: r.get(2)?,
                kind: r.get(3)?,
                language: r.get(4)?,
                start_line: r.get(5)?,
                end_line: r.get(6)?,
                start_byte: r.get(7)?,
                end_byte: r.get(8)?,
                signature: r.get(9)?,
                tree_hash: r.get(10)?,
                is_test: r.get(11)?,
                summary_key: r.get(12)?,
                token_vector: r
                    .get::<_, Option<Vec<u8>>>(13)?
                    .map(|b| tokens::from_blob(&b)),
                token_count: r.get(14)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Each edge, by caller and place, with the candidates of each
    /// `possible` edge.
    pub fn edges(&self) -> Result<Vec<EdgeRecord>, IndexError> {
        fn symbol_ref(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Option<SymbolRef>> {
            let file: Option<String> = r.get(at)?;
            Ok(match file {
                None => None,
                Some(file) => Some(SymbolRef {
                    file,
                    name: r.get(at + 1)?,
                    container: r.get(at + 2)?,
                    start_line: r.get(at + 3)?,
                }),
            })
        }
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.callee_name, e.edge_kind, e.class, e.origin, e.line, e.col,
                    cf.path, c.name, c.container, c.start_line,
                    tf.path, t.name, t.container, t.start_line, e.scip_pending,
                    gf.path, e.target_line, e.target_macro
             FROM edges e
             JOIN symbols c ON c.id = e.caller_id JOIN files cf ON cf.id = c.file_id
             LEFT JOIN symbols t ON t.id = e.callee_id LEFT JOIN files tf ON tf.id = t.file_id
             LEFT JOIN files gf ON gf.id = e.target_file_id
             ORDER BY cf.path, c.start_byte, e.line, e.col, e.callee_name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                EdgeRecord {
                    caller: symbol_ref(r, 7)?.expect("an edge has a caller"),
                    callee: symbol_ref(r, 11)?,
                    callee_name: r.get(1)?,
                    kind: r.get(2)?,
                    class: r.get(3)?,
                    origin: r.get(4)?,
                    line: r.get(5)?,
                    col: r.get(6)?,
                    candidates: Vec::new(),
                    scip_pending: r.get(15)?,
                    target: match (
                        r.get::<_, Option<String>>(16)?,
                        r.get::<_, Option<u32>>(17)?,
                    ) {
                        (Some(file), Some(line)) => Some(EdgeTarget {
                            file,
                            line,
                            macro_made: r.get(18)?,
                        }),
                        _ => None,
                    },
                },
            ))
        })?;
        let mut edges: Vec<(i64, EdgeRecord)> = rows.collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT ec.edge_id, f.path, s.name, s.container, s.start_line
             FROM edge_candidates ec
             JOIN symbols s ON s.id = ec.symbol_id JOIN files f ON f.id = s.file_id
             ORDER BY ec.edge_id, f.path, s.start_line",
        )?;
        let mut candidates: std::collections::HashMap<i64, Vec<SymbolRef>> =
            std::collections::HashMap::new();
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, symbol_ref(r, 1)?.expect("a candidate")))
        })?;
        for row in rows {
            let (edge_id, symbol) = row?;
            candidates.entry(edge_id).or_default().push(symbol);
        }
        for (id, edge) in &mut edges {
            edge.candidates = candidates.remove(id).unwrap_or_default();
        }
        Ok(edges.into_iter().map(|(_, e)| e).collect())
    }

    /// The summary with this key in the pool, with its vector.
    pub fn summary(&self, key: &str) -> Result<Option<SummaryRecord>, IndexError> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT key, level, summary, vector FROM summaries WHERE key = ?1",
                [key],
                |r| {
                    let vector: Option<Vec<u8>> = r.get(3)?;
                    Ok(SummaryRecord {
                        key: r.get(0)?,
                        level: r.get(1)?,
                        summary: r.get(2)?,
                        vector: vector.map(|bytes| {
                            bytes
                                .chunks_exact(4)
                                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                                .collect()
                        }),
                    })
                },
            )
            .optional()?)
    }

    /// Each file with the key of its summary, by path.
    pub fn file_summary_keys(&self) -> Result<Vec<(String, Option<String>)>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, summary_key FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Each module (a folder of summarized files) with the key of its
    /// summary, by path.
    pub fn modules(&self) -> Result<Vec<(String, String)>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, summary_key FROM modules ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// How an update or a merge builds the index.
#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    /// Run `rust-analyzer scip` for the Rust edges (`--rust-edges`). Without
    /// it, a Rust function whose code did not change keeps the SCIP edges of
    /// the base, also in a changed file, and the edges of a changed Rust
    /// function are name classes that wait for the next run.
    pub rust_edges: bool,
    /// Which part of the index to summarize.
    pub summarize: SummarizeOptions,
    /// The rust-analyzer of a SCIP run.
    pub build: BuildOptions,
}

/// What an update or a merge did.
#[derive(Debug, Clone)]
pub struct UpdateReport {
    pub build: BuildReport,
    pub summary: SummaryReport,
}

/// Update the index at `db` for the tree at `root`, with the changes that
/// are not committed.
///
/// The structure is built again from the tree. The summaries come from the
/// pool by key; the summarizer runs only for the keys that the pool does not
/// have, and only the new summaries get vectors. If `db` is not there, the
/// update makes it with an empty pool.
pub fn update(
    root: &Path,
    db: &Path,
    summarizer: &mut dyn Summarizer,
    embedder: &dyn kairos_embed::EmbeddingProvider,
    options: &UpdateOptions,
) -> Result<UpdateReport, IndexError> {
    let build = update_structure(root, db, options)?;
    let summary = summarize(root, db, summarizer, embedder, &options.summarize)?;
    Ok(UpdateReport { build, summary })
}

/// Merge the indexes `bases` into the new index `out`, for the merged tree
/// at `root`: an update of that tree with the pools of all of them.
///
/// The same key always means the same input, so the pools merge with no
/// conflict. A Rust function keeps the SCIP edges of the first base that has
/// its code and no edge of it that waits for SCIP. `out` must not exist. The pools
/// must have the vectors of one model.
pub fn merge(
    root: &Path,
    bases: &[&Path],
    out: &Path,
    summarizer: &mut dyn Summarizer,
    embedder: &dyn kairos_embed::EmbeddingProvider,
    options: &UpdateOptions,
) -> Result<UpdateReport, IndexError> {
    if out.exists() {
        return Err(IndexError::Exists(out.to_path_buf()));
    }
    let mut kept = Vec::with_capacity(bases.len());
    let mut known = HashMap::new();
    let mut conn = Connection::open(out)?;
    schema::prepare(&conn)?;
    for base in bases {
        // Index::open refuses a missing file or another schema version.
        let index = Index::open(base)?;
        kept.push(edges::BaseIndex::read(&index.conn)?);
        known.extend(known_vectors(&index.conn)?);
        summary::copy_pool(&index.conn, &mut conn)?;
    }
    drop(conn);
    let mode = if options.rust_edges {
        Mode::Scip(&options.build)
    } else {
        Mode::Keep(&kept)
    };
    let build = build(root, out, mode, &known, Parse::All)?;
    let summary = summarize(root, out, summarizer, embedder, &options.summarize)?;
    Ok(UpdateReport { build, summary })
}

/// One summary of the pool.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryRecord {
    pub key: String,
    pub level: String,
    pub summary: String,
    /// Little-endian f32 in the index. `None` until it is embedded.
    pub vector: Option<Vec<f32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rebuild_keeps_the_summary_pool() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let db = dir.path().join("index.sqlite");

        build_structure(&repo, &db).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO summaries (key, level, summary) VALUES ('k', 'symbol', 'Does f.')",
            [],
        )
        .unwrap();
        drop(conn);

        let report = build_structure(&repo, &db).unwrap();
        assert_eq!(report.symbols, 1);
        let conn = Connection::open(&db).unwrap();
        let kept: i64 = conn
            .query_row("SELECT count(*) FROM summaries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1);
    }

    #[test]
    fn an_index_of_another_schema_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let err = build_structure(dir.path(), &db).unwrap_err();
        assert!(
            matches!(err, IndexError::SchemaVersion { found: 99 }),
            "{err}"
        );
    }

    #[test]
    fn gitignored_folders_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::write(repo.join(".gitignore"), "/target/\n").unwrap();
        std::fs::create_dir_all(repo.join("target/debug")).unwrap();
        std::fs::write(repo.join("target/debug/x.rs"), "fn x() {}\n").unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(repo.join("main.go"), "package main\nfunc main() {}\n").unwrap();
        let paths: Vec<String> = walk(repo).unwrap().into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [".gitignore", "main.go"]);
    }

    #[test]
    fn the_index_of_the_checkout_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join(".kairos")).unwrap();
        std::fs::write(repo.join(".kairos/index.db"), "x").unwrap();
        std::fs::write(repo.join(".kairos/index.db-journal"), "x").unwrap();
        std::fs::write(repo.join(".kairos/index-rules.toml"), "").unwrap();
        let paths: Vec<String> = walk(repo).unwrap().into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [".kairos/index-rules.toml"]);
    }

    #[test]
    fn changed_files_compare_the_tree_with_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("a.py"), "def a():\n    return 1\n").unwrap();
        std::fs::write(repo.join("b.py"), "def b():\n    return 2\n").unwrap();
        std::fs::write(repo.join("c.py"), "def c():\n    return 3\n").unwrap();
        let db = dir.path().join("index.sqlite");
        build_structure(&repo, &db).unwrap();
        assert!(changed_files(&repo, &db).unwrap().is_empty());

        std::fs::write(repo.join("a.py"), "def a():\n    return 10\n").unwrap();
        std::fs::remove_file(repo.join("b.py")).unwrap();
        std::fs::write(repo.join("d.py"), "def d():\n    return 4\n").unwrap();
        assert_eq!(changed_files(&repo, &db).unwrap(), ["a.py", "b.py", "d.py"]);
    }

    /// COLLIERY-T-1854: with no model, each key that the pool has is linked,
    /// and each other key is counted as left.
    #[test]
    fn link_needs_no_model() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("pkg")).unwrap();
        std::fs::write(
            repo.join("pkg/a.py"),
            "def a():\n    return 1\n\n\ndef b():\n    return a()\n",
        )
        .unwrap();
        let db = dir.path().join("index.sqlite");
        let embedder = kairos_embed::DeterministicProvider::default();
        let mut fake = FakeSummarizer::default();
        update(&repo, &db, &mut fake, &embedder, &UpdateOptions::default()).unwrap();
        let summarized = Index::open(&db).unwrap().symbols().unwrap();

        // The same tree: the structure is made again, and each key links.
        update_structure(&repo, &db, &UpdateOptions::default()).unwrap();
        let report = link(&repo, &db, &SummarizeOptions::default()).unwrap();
        assert_eq!(
            (
                report.symbols.reused,
                report.files.reused,
                report.modules.reused
            ),
            (2, 1, 1)
        );
        assert_eq!(
            report.symbols.left + report.files.left + report.modules.left,
            0
        );
        assert!(report.calls.is_empty() && report.vectors == 0);
        let index = Index::open(&db).unwrap();
        assert_eq!(index.symbols().unwrap(), summarized);
        assert_eq!(index.modules().unwrap().len(), 1);

        // A changed function: it, its file and its module are left.
        std::fs::write(
            repo.join("pkg/a.py"),
            "def a():\n    return 2\n\n\ndef b():\n    return a()\n",
        )
        .unwrap();
        update_structure(&repo, &db, &UpdateOptions::default()).unwrap();
        let report = link(&repo, &db, &SummarizeOptions::default()).unwrap();
        assert_eq!(
            (report.symbols.reused, report.symbols.left),
            (1, 1),
            "{report:?}"
        );
        assert_eq!((report.files.left, report.modules.left), (1, 1));
        let index = Index::open(&db).unwrap();
        let keys: Vec<(String, bool)> = index
            .symbols()
            .unwrap()
            .into_iter()
            .map(|s| (s.name, s.summary_key.is_some()))
            .collect();
        assert_eq!(keys, [("a".to_string(), false), ("b".to_string(), true)]);
        assert!(index.modules().unwrap().is_empty());
        assert_eq!(
            index.file_summary_keys().unwrap(),
            [("pkg/a.py".to_string(), None)]
        );
    }
}
