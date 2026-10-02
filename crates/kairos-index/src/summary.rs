//! The summaries and their vectors (COLLIERY-I-0264, "The two parts of an
//! index"; COLLIERY-T-1850).
//!
//! - **A symbol summary** comes from the code of the symbol and the signatures
//!   of the functions that it calls (its `certain` edges). It does not use
//!   their summaries: if it did, one change would change the input of each
//!   caller above it. Test symbols get no summary.
//! - **A file summary** comes from the summaries of its symbols, and **a
//!   module summary** from the summaries of the files in its folder. Neither
//!   reads code: the benchmark showed that file summaries from code cost as
//!   much as all the symbol summaries.
//! - **The key** of a symbol summary is the hash of the tree hash of the
//!   symbol and of the signatures of its callees (COLLIERY-T-1851). So a
//!   changed signature gives new summaries to the direct callers only, and a
//!   changed body to none. The key of a file summary is the hash of its path
//!   and of the list of child summaries; the key of a module summary is the
//!   hash of that list. A key that is in the pool is not summarized again.
//! - **The vectors** come from an [`EmbeddingProvider`]: in Kairos, the local
//!   `bge-small-en-v1.5`, the same model and space as the Kairos items.
//!
//! The model is a [`Summarizer`]. The scenarios use [`FakeSummarizer`]; the
//! real one is `LlamaSummarizer` (feature `llama`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kairos_embed::EmbeddingProvider;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{IndexError, schema};

/// The symbol kinds that get a summary: functions and types. A `mod x;`
/// line, an `impl` block, a constant or a field gets none. The same kinds as
/// the benchmark (`~/code-index-eval/bench/select_symbols.py`).
pub const SUMMARIZED_KINDS: &[&str] = &[
    "function",
    "method",
    "constructor",
    "struct",
    "enum",
    "trait",
    "type_alias",
    "class",
    "interface",
];

/// The system prompt of the model comparison (`~/code-index-eval`).
pub const SYSTEM_PROMPT: &str = "You write short summaries of source code for a code search index.";

/// The file name of the summary model, as bartowski publishes it on Hugging
/// Face (`bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF`).
pub const MODEL_FILE_NAME: &str = "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

/// Where the summary model file is: `KAIROS_INDEX_MODEL` if it is set, else
/// `~/.cache/kairos-index/models/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf`.
/// Nothing downloads the file. Get it one time, by hand.
pub fn model_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KAIROS_INDEX_MODEL").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".cache/kairos-index/models")
            .join(MODEL_FILE_NAME),
    )
}

/// The most callee signatures that go into one symbol prompt.
const MAX_CALLEES: usize = 24;

/// How many summaries go to the embedding provider at once.
const EMBED_BATCH: usize = 64;

/// The level of a summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Symbol,
    File,
    Module,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Symbol => "symbol",
            Level::File => "file",
            Level::Module => "module",
        }
    }
}

/// The input for one summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRequest {
    pub level: Level,
    /// The key that the summary gets in the pool.
    pub key: String,
    /// The language of a symbol or a file. A module has none.
    pub language: Option<String>,
    /// The symbol kind, `file` or `module`.
    pub kind: String,
    /// The symbol name, the file path or the folder path.
    pub name: String,
    /// The file of a symbol or a file, the folder of a module.
    pub path: String,
    /// The code of a symbol. A file or a module has none.
    pub code: Option<String>,
    /// The signatures of the functions that a symbol calls.
    pub callees: Vec<String>,
    /// The child summaries of a file or a module, one line each. This list is
    /// the whole input of the summary, and its hash is the key.
    pub children: Vec<String>,
}

impl SummaryRequest {
    /// The user prompt for the model.
    pub fn prompt(&self) -> String {
        self.prompt_with_code(self.code.as_deref().unwrap_or(""))
    }

    /// The user prompt with `code` in place of the code of the symbol. A
    /// runtime with a small context gives a shortened code here.
    pub fn prompt_with_code(&self, code: &str) -> String {
        let language = self.language.as_deref().unwrap_or("");
        match self.level {
            // The prompt of the model comparison, with the callee signatures
            // added after the code.
            Level::Symbol => {
                let noun = match self.kind.as_str() {
                    "function" | "method" | "constructor" => "function",
                    "type_alias" => "type alias",
                    other => other,
                };
                let mut prompt = format!(
                    "Summarize the {language} {noun} `{}` from `{}` in 1 to 3 sentences. \
                     Say what it does, its inputs and outputs, and its side effects \
                     (I/O, network, state changes, errors raised). \
                     Do not restate the name. Output only the summary.\n\n\
                     ```{language}\n{code}\n```",
                    self.name, self.path
                );
                if !self.callees.is_empty() {
                    prompt.push_str(&format!(
                        "\n\nIt calls functions with these signatures:\n\n```{language}\n{}\n```",
                        self.callees.join("\n")
                    ));
                }
                prompt
            }
            Level::File => format!(
                "Summarize the {language} source file `{}` in 1 to 3 sentences, from the \
                 summaries of its parts below. Say what the file is for and what it gives to \
                 other code. Do not list each part. Output only the summary.\n\n{}",
                self.path,
                self.children.join("\n")
            ),
            Level::Module => format!(
                "Summarize a folder of source code in 1 to 3 sentences, from the summaries \
                 of its files below. Say what the folder is for. \
                 Do not list each file. Output only the summary.\n\n{}",
                self.children.join("\n")
            ),
        }
    }
}

/// Writes one summary. The real one runs a local model; the scenarios use
/// [`FakeSummarizer`].
pub trait Summarizer {
    /// The summary for `request`: 1 to 3 sentences of plain text. The error
    /// text says why the model gave no summary.
    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String>;
}

/// A summarizer for tests: a fixed text for each input, with no model. It
/// keeps each request, so that a test can see what the model would get.
#[derive(Debug, Default)]
pub struct FakeSummarizer {
    pub requests: Vec<SummaryRequest>,
}

impl Summarizer for FakeSummarizer {
    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        let digest = hex(&Sha256::digest(request.prompt().as_bytes()));
        self.requests.push(request.clone());
        Ok(format!(
            "A {} summary, {}.",
            request.level.as_str(),
            &digest[..12]
        ))
    }
}

/// Which part of the index to summarize.
#[derive(Debug, Clone, Default)]
pub struct SummarizeOptions {
    /// Only the files whose path starts with one of these texts. Empty:
    /// each file.
    pub under: Vec<String>,
    /// Stop after this number of new symbol summaries. A file whose symbols
    /// do not all have a summary gets no summary.
    pub max_new_symbols: Option<usize>,
}

/// One call of the summarizer.
#[derive(Debug, Clone)]
pub struct SummaryCall {
    pub level: Level,
    pub key: String,
    /// The file and the symbol name, the file path or the folder path.
    pub name: String,
    pub elapsed: Duration,
    pub summary: String,
}

/// The counts of one level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LevelCount {
    /// Items that got a new summary from the summarizer.
    pub summarized: usize,
    /// Items whose key was in the pool already, or that had the key of an
    /// earlier item of the run.
    pub reused: usize,
    /// Items that got no summary: a limit stopped the run, or a child has
    /// no summary.
    pub left: usize,
}

/// What a summary run did.
#[derive(Debug, Clone, Default)]
pub struct SummaryReport {
    pub symbols: LevelCount,
    pub files: LevelCount,
    pub modules: LevelCount,
    /// The vectors written in this run.
    pub vectors: usize,
    pub embed_elapsed: Duration,
    /// Each call of the summarizer, in order.
    pub calls: Vec<SummaryCall>,
}

/// One symbol row, as the summarizer needs it.
struct Sym {
    id: i64,
    file_id: i64,
    name: String,
    container: Option<String>,
    kind: String,
    language: String,
    start_byte: usize,
    end_byte: usize,
    tree_hash: String,
    is_test: bool,
}

impl Sym {
    fn summarized(&self) -> bool {
        !self.is_test && SUMMARIZED_KINDS.contains(&self.kind.as_str())
    }

    /// The name with its container, as a child line shows it.
    fn qualified(&self) -> String {
        match &self.container {
            Some(c) if self.language == "rust" => format!("{c}::{}", self.name),
            Some(c) => format!("{c}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

struct FileRow {
    id: i64,
    path: String,
    language: Option<String>,
    decision: String,
    content_hash: String,
}

/// Summarize the index at `db`, built from the tree at `root`, and write the
/// vectors of the new summaries.
///
/// The code of each symbol is read from `root`. If a file changed after the
/// build of the structure, the run stops and names the file. The summaries
/// are written one by one, so a stopped run keeps its work.
pub fn summarize(
    root: &Path,
    db: &Path,
    summarizer: &mut dyn Summarizer,
    embedder: &dyn EmbeddingProvider,
    options: &SummarizeOptions,
) -> Result<SummaryReport, IndexError> {
    run(root, db, Some((summarizer, embedder)), options)
}

/// Link each symbol, file and module of the index at `db` whose key is in
/// the pool to its summary, with no model (COLLIERY-T-1854).
///
/// It is [`summarize`] with no summarizer: a key that the pool does not have
/// is counted as `left`, and its file and module get no summary. No vector
/// is made. So a binary with no model shows the summaries of a base index
/// that it downloaded. The `summarized` counts of the report are 0.
pub fn link(
    root: &Path,
    db: &Path,
    options: &SummarizeOptions,
) -> Result<SummaryReport, IndexError> {
    run(root, db, None, options)
}

/// The summarizer and the vector provider of a run.
type Model<'a> = (&'a mut dyn Summarizer, &'a dyn EmbeddingProvider);

fn run(
    root: &Path,
    db: &Path,
    mut model: Option<Model<'_>>,
    options: &SummarizeOptions,
) -> Result<SummaryReport, IndexError> {
    let mut conn = Connection::open(db)?;
    schema::prepare(&conn)?;
    let vector_model = match &model {
        Some((_, embedder)) => {
            let name = model_name(*embedder);
            check_vector_model(&conn, &name)?;
            Some(name)
        }
        None => None,
    };

    let in_scope =
        |path: &str| options.under.is_empty() || options.under.iter().any(|u| path.starts_with(u));
    let files = read_files(&conn)?;
    let symbols = read_symbols(&conn)?;
    let calls = read_callees(&conn)?;
    let by_id: HashMap<i64, &Sym> = symbols.iter().map(|s| (s.id, s)).collect();
    let file_by_id: HashMap<i64, &FileRow> = files.iter().map(|f| (f.id, f)).collect();
    let mut texts = FileTexts::new(root, &file_by_id);
    let mut report = SummaryReport::default();

    // 1. The symbols.
    let mut new_symbols = 0usize;
    let mut seen: HashSet<String> = HashSet::new();
    // The key of each symbol whose summary is in the pool.
    let mut symbol_keys: HashMap<i64, String> = HashMap::new();
    for s in &symbols {
        let file = file_by_id[&s.file_id];
        if !s.summarized() || !in_scope(&file.path) {
            continue;
        }
        let mut callees = Vec::new();
        for callee_id in calls.get(&s.id).map(Vec::as_slice).unwrap_or_default() {
            let Some(callee) = by_id.get(callee_id) else {
                continue;
            };
            // Not narsil's signature: that is the first line, which can hold
            // a one-line body or only a part of a long signature.
            let signature = declaration(
                &texts.slice(callee.file_id, callee.start_byte, callee.end_byte)?,
                &callee.language,
            );
            if !callees.contains(&signature) && callees.len() < MAX_CALLEES {
                callees.push(signature);
            }
        }
        let key = symbol_key(&s.tree_hash, &callees);
        if seen.contains(&key) || in_pool(&conn, &key)? {
            seen.insert(key.clone());
            symbol_keys.insert(s.id, key);
            report.symbols.reused += 1;
            continue;
        }
        let Some((summarizer, _)) = model.as_mut() else {
            report.symbols.left += 1;
            continue;
        };
        if options
            .max_new_symbols
            .is_some_and(|max| new_symbols >= max)
        {
            report.symbols.left += 1;
            continue;
        }
        let code = texts.slice(s.file_id, s.start_byte, s.end_byte)?;
        let request = SummaryRequest {
            level: Level::Symbol,
            key: key.clone(),
            language: Some(s.language.clone()),
            kind: s.kind.clone(),
            name: s.name.clone(),
            path: file.path.clone(),
            code: Some(code),
            callees,
            children: Vec::new(),
        };
        run_one(
            &conn,
            *summarizer,
            &request,
            format!("{}:{}", file.path, s.qualified()),
            &mut report,
        )?;
        seen.insert(key.clone());
        symbol_keys.insert(s.id, key);
        new_symbols += 1;
        report.symbols.summarized += 1;
    }

    // 2. The files, from the summaries of their symbols.
    let mut by_file: BTreeMap<i64, Vec<&Sym>> = BTreeMap::new();
    for s in symbols.iter().filter(|s| s.summarized()) {
        by_file.entry(s.file_id).or_default().push(s);
    }
    let mut file_keys: BTreeMap<String, Option<String>> = BTreeMap::new();
    for f in files
        .iter()
        .filter(|f| f.decision == "source" && in_scope(&f.path))
    {
        let Some(children) = by_file.get(&f.id) else {
            continue;
        };
        let mut lines = Vec::with_capacity(children.len());
        for s in children {
            let Some(key) = symbol_keys.get(&s.id) else {
                break;
            };
            match summary_of(&conn, key)? {
                Some(text) => lines.push(format!("- `{}` ({}): {text}", s.qualified(), s.kind)),
                None => break,
            }
        }
        if lines.len() < children.len() {
            report.files.left += 1;
            file_keys.insert(f.path.clone(), None);
            continue;
        }
        let language = f.language.clone().unwrap_or_default();
        let key = children_key(Level::File, &language, &f.path, &lines);
        let request = SummaryRequest {
            level: Level::File,
            key: key.clone(),
            language: Some(language),
            kind: "file".into(),
            name: f.path.clone(),
            path: f.path.clone(),
            code: None,
            callees: Vec::new(),
            children: lines,
        };
        if in_pool(&conn, &key)? {
            report.files.reused += 1;
        } else if let Some((summarizer, _)) = model.as_mut() {
            run_one(&conn, *summarizer, &request, f.path.clone(), &mut report)?;
            report.files.summarized += 1;
        } else {
            report.files.left += 1;
            file_keys.insert(f.path.clone(), None);
            continue;
        }
        file_keys.insert(f.path.clone(), Some(key));
    }

    // 3. The modules: each folder of summarized files, from their summaries.
    let mut folders: BTreeMap<String, Vec<(&str, Option<&str>)>> = BTreeMap::new();
    for (path, key) in &file_keys {
        let (folder, name) = match path.rsplit_once('/') {
            Some((folder, name)) => (folder.to_string(), name),
            None => (".".to_string(), path.as_str()),
        };
        folders
            .entry(folder)
            .or_default()
            .push((name, key.as_deref()));
    }
    let mut module_keys = Vec::new();
    for (folder, entries) in &folders {
        let mut lines = Vec::with_capacity(entries.len());
        for (name, key) in entries {
            match key {
                Some(key) => match summary_of(&conn, key)? {
                    Some(text) => lines.push(format!("- `{name}`: {text}")),
                    None => break,
                },
                None => break,
            }
        }
        if lines.len() < entries.len() {
            report.modules.left += 1;
            continue;
        }
        let key = children_key(Level::Module, "", "", &lines);
        if in_pool(&conn, &key)? {
            report.modules.reused += 1;
        } else if let Some((summarizer, _)) = model.as_mut() {
            let request = SummaryRequest {
                level: Level::Module,
                key: key.clone(),
                language: None,
                kind: "module".into(),
                name: folder.clone(),
                path: folder.clone(),
                code: None,
                callees: Vec::new(),
                children: lines,
            };
            run_one(&conn, *summarizer, &request, folder.clone(), &mut report)?;
            report.modules.summarized += 1;
        } else {
            report.modules.left += 1;
            continue;
        }
        module_keys.push((folder.clone(), key));
    }

    // 4. Link the files and the modules in scope to their summaries.
    {
        let tx = conn.transaction()?;
        {
            let mut set_file = tx.prepare("UPDATE files SET summary_key = ?2 WHERE path = ?1")?;
            for (path, key) in &file_keys {
                set_file.execute(params![path, key])?;
            }
            let mut set_symbol = tx.prepare("UPDATE symbols SET summary_key = ?2 WHERE id = ?1")?;
            for (id, key) in &symbol_keys {
                set_symbol.execute(params![id, key])?;
            }
            // The modules in scope are written again. With no scope, the
            // prefix is empty and each module goes.
            let all = [String::new()];
            let prefixes = if options.under.is_empty() {
                &all[..]
            } else {
                &options.under[..]
            };
            for prefix in prefixes {
                tx.execute(
                    "DELETE FROM modules WHERE substr(path, 1, length(?1)) = ?1",
                    params![prefix],
                )?;
            }
            let mut set_module =
                tx.prepare("INSERT OR REPLACE INTO modules (path, summary_key) VALUES (?1, ?2)")?;
            for (path, key) in &module_keys {
                set_module.execute(params![path, key])?;
            }
        }
        tx.commit()?;
    }

    // 5. The vectors of each summary that has none. A link run makes none.
    if let (Some((_, embedder)), Some(vector_model)) = (model, vector_model) {
        let started = Instant::now();
        let names: HashMap<&str, &str> = symbols
            .iter()
            .filter_map(|s| Some((symbol_keys.get(&s.id)?.as_str(), s.name.as_str())))
            .collect();
        report.vectors = embed_missing(&mut conn, embedder, &vector_model, &names)?;
        report.embed_elapsed = started.elapsed();
    }
    Ok(report)
}

/// Call the summarizer for `request` and put its summary in the pool.
fn run_one(
    conn: &Connection,
    summarizer: &mut dyn Summarizer,
    request: &SummaryRequest,
    name: String,
    report: &mut SummaryReport,
) -> Result<(), IndexError> {
    let started = Instant::now();
    let text = summarizer
        .summarize(request)
        .map_err(|message| IndexError::Summary {
            name: name.clone(),
            message,
        })?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(IndexError::Summary {
            name,
            message: "the summary is empty".into(),
        });
    }
    conn.execute(
        "INSERT INTO summaries (key, level, summary) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO NOTHING",
        params![request.key, request.level.as_str(), text],
    )?;
    report.calls.push(SummaryCall {
        level: request.level,
        key: request.key.clone(),
        name,
        elapsed: started.elapsed(),
        summary: text,
    });
    Ok(())
}

/// The key of a symbol summary: the hash of the tree hash of the symbol and
/// of the signatures of its callees, in the order of the calls. The
/// signatures are compared with no formatting (see [`signature_key`]), so a
/// formatting change gives the same key.
fn symbol_key(tree_hash: &str, callees: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"symbol");
    hasher.update([0]);
    hasher.update(tree_hash.as_bytes());
    for callee in callees {
        hasher.update([0]);
        hasher.update(signature_key(callee).as_bytes());
    }
    hex(&hasher.finalize())
}

/// A signature with no formatting: no whitespace, except one space between
/// 2 word characters, and no comma before a closing bracket.
fn signature_key(signature: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(signature.len());
    let mut space = false;
    for c in signature.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && out.chars().last().is_some_and(word) && word(c) {
            out.push(' ');
        }
        space = false;
        if matches!(c, ')' | ']' | '>' | '}') && out.ends_with(',') {
            out.pop();
        }
        out.push(c);
    }
    out
}

/// The key of a file or a module summary: the hash of its level, its
/// language, its path (a file only; empty for a module) and its child lines,
/// in order.
fn children_key(level: Level, language: &str, path: &str, lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(level.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(language.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_bytes());
    for line in lines {
        hasher.update([0]);
        hasher.update(line.as_bytes());
    }
    hex(&hasher.finalize())
}

fn in_pool(conn: &Connection, key: &str) -> Result<bool, IndexError> {
    Ok(conn
        .query_row("SELECT 1 FROM summaries WHERE key = ?1", [key], |_| Ok(()))
        .optional()?
        .is_some())
}

fn summary_of(conn: &Connection, key: &str) -> Result<Option<String>, IndexError> {
    Ok(conn
        .query_row("SELECT summary FROM summaries WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

/// The model of the vectors, as `pool_meta` keeps it.
pub(crate) fn model_name(embedder: &dyn EmbeddingProvider) -> String {
    let id = embedder.model_id();
    format!("{}/{}/{}", id.provider, id.model, id.dimension)
}

/// Refuse a provider whose model is not the model of the vectors in the pool.
fn check_vector_model(conn: &Connection, model: &str) -> Result<(), IndexError> {
    let found: Option<String> = conn
        .query_row(
            "SELECT value FROM pool_meta WHERE name = 'vector_model'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match found {
        Some(found) if found != model => Err(IndexError::VectorModel {
            pool: found,
            run: model.to_string(),
        }),
        _ => Ok(()),
    }
}

/// Embed each summary of the pool that has no vector. A symbol summary is
/// embedded as `name: summary` (the name is part of its key); a file or a
/// module summary as its text.
fn embed_missing(
    conn: &mut Connection,
    embedder: &dyn EmbeddingProvider,
    model: &str,
    names: &HashMap<&str, &str>,
) -> Result<usize, IndexError> {
    let missing: Vec<(String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT key, level, summary FROM summaries WHERE vector IS NULL ORDER BY key",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut written = 0;
    for batch in missing.chunks(EMBED_BATCH) {
        let texts: Vec<String> = batch
            .iter()
            .map(|(key, level, summary)| match names.get(key.as_str()) {
                Some(name) if level == "symbol" => format!("{name}: {summary}"),
                _ => summary.clone(),
            })
            .collect();
        let vectors = embedder
            .embed(&texts)
            .map_err(|e| IndexError::Embed(e.to_string()))?;
        let tx = conn.transaction()?;
        {
            let mut set = tx.prepare("UPDATE summaries SET vector = ?2 WHERE key = ?1")?;
            for ((key, _, _), vector) in batch.iter().zip(&vectors) {
                let blob: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
                set.execute(params![key, blob])?;
            }
            tx.execute(
                "INSERT OR IGNORE INTO pool_meta (name, value) VALUES ('vector_model', ?1)",
                [model],
            )?;
        }
        tx.commit()?;
        written += batch.len();
    }
    Ok(written)
}

/// Copy the summary pool of the index at `from` into the index at `to`. A
/// key that `to` has already keeps its summary; it gets the vector of `from`
/// if it has none. The 2 pools must have the vectors of one model.
pub(crate) fn copy_pool(from: &Connection, to: &mut Connection) -> Result<(), IndexError> {
    let model: Option<String> = from
        .query_row(
            "SELECT value FROM pool_meta WHERE name = 'vector_model'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(model) = &model {
        check_vector_model(to, model)?;
    }
    let tx = to.transaction()?;
    {
        if let Some(model) = &model {
            tx.execute(
                "INSERT OR IGNORE INTO pool_meta (name, value) VALUES ('vector_model', ?1)",
                [model],
            )?;
        }
        let mut insert = tx.prepare(
            "INSERT INTO summaries (key, level, summary, vector) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (key) DO UPDATE SET vector = excluded.vector
             WHERE summaries.vector IS NULL",
        )?;
        let mut stmt = from.prepare("SELECT key, level, summary, vector FROM summaries")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            insert.execute(params![
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn read_files(conn: &Connection) -> Result<Vec<FileRow>, IndexError> {
    let mut stmt =
        conn.prepare("SELECT id, path, language, decision, content_hash FROM files ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        Ok(FileRow {
            id: r.get(0)?,
            path: r.get(1)?,
            language: r.get(2)?,
            decision: r.get(3)?,
            content_hash: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn read_symbols(conn: &Connection) -> Result<Vec<Sym>, IndexError> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.file_id, s.name, s.container, s.kind, s.language, s.start_byte,
                s.end_byte, s.tree_hash, s.is_test
         FROM symbols s JOIN files f ON f.id = s.file_id
         ORDER BY f.path, s.start_byte, s.end_byte DESC, s.kind, s.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Sym {
            id: r.get(0)?,
            file_id: r.get(1)?,
            name: r.get(2)?,
            container: r.get(3)?,
            kind: r.get(4)?,
            language: r.get(5)?,
            start_byte: r.get::<_, i64>(6)? as usize,
            end_byte: r.get::<_, i64>(7)? as usize,
            tree_hash: r.get(8)?,
            is_test: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The callees of each caller, from its `certain` call edges, in the order
/// of the calls.
fn read_callees(conn: &Connection) -> Result<HashMap<i64, Vec<i64>>, IndexError> {
    let mut stmt = conn.prepare(
        "SELECT caller_id, callee_id FROM edges
         WHERE class = 'certain' AND callee_id IS NOT NULL AND callee_id != caller_id
         ORDER BY caller_id, line, col",
    )?;
    let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (caller, callee) = row?;
        let list = out.entry(caller).or_default();
        if !list.contains(&callee) {
            list.push(callee);
        }
    }
    Ok(out)
}

/// The text of the files, read from the tree when first needed, and checked
/// against the content hash of the build.
struct FileTexts<'a> {
    root: &'a Path,
    files: &'a HashMap<i64, &'a FileRow>,
    texts: HashMap<i64, String>,
}

impl<'a> FileTexts<'a> {
    fn new(root: &'a Path, files: &'a HashMap<i64, &'a FileRow>) -> Self {
        FileTexts {
            root,
            files,
            texts: HashMap::new(),
        }
    }

    fn slice(&mut self, file_id: i64, start: usize, end: usize) -> Result<String, IndexError> {
        if !self.texts.contains_key(&file_id) {
            let file = self.files[&file_id];
            let path = self.root.join(&file.path);
            let bytes = fs::read(&path).map_err(|source| IndexError::Io {
                path: path.clone(),
                source,
            })?;
            if hex(&Sha256::digest(&bytes)) != file.content_hash {
                return Err(IndexError::Changed(file.path.clone()));
            }
            self.texts
                .insert(file_id, String::from_utf8_lossy(&bytes).into_owned());
        }
        let text = &self.texts[&file_id];
        Ok(text.get(start..end).unwrap_or_default().to_string())
    }
}

/// The signature of a function from its code: the text before its body, on
/// one line. The body starts at the first `{` (Rust, Go, TypeScript) or `:`
/// (Python) outside brackets and strings. Lines before the `def` of a Python
/// function (its decorators) are dropped.
fn declaration(code: &str, language: &str) -> String {
    let python = language == "python";
    let code = if python {
        code.lines()
            .position(|l| {
                let l = l.trim_start();
                l.starts_with("def ") || l.starts_with("async def ") || l.starts_with("class ")
            })
            .map_or(code, |i| {
                let start: usize = code.lines().take(i).map(|l| l.len() + 1).sum();
                code.get(start..).unwrap_or(code)
            })
    } else {
        code
    };
    let body = if python { ':' } else { '{' };
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut end = code.len();
    let mut chars = code.char_indices();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '`' => quote = Some(c),
            '\'' if python => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            c if c == body && depth <= 0 => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    code[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(level: Level) -> SummaryRequest {
        SummaryRequest {
            level,
            key: "k".into(),
            language: Some("rust".into()),
            kind: "function".into(),
            name: "add".into(),
            path: "src/lib.rs".into(),
            code: Some("fn add(a: u32, b: u32) -> u32 { a + b }".into()),
            callees: Vec::new(),
            children: vec!["- `add` (function): Adds.".into()],
        }
    }

    #[test]
    fn a_file_prompt_has_the_path() {
        let prompt = request(Level::File).prompt();
        assert!(
            prompt.starts_with("Summarize the rust source file `src/lib.rs` in 1 to 3 sentences"),
            "{prompt}"
        );
    }

    #[test]
    fn the_symbol_prompt_is_the_prompt_of_the_model_comparison() {
        assert_eq!(
            request(Level::Symbol).prompt(),
            "Summarize the rust function `add` from `src/lib.rs` in 1 to 3 sentences. \
             Say what it does, its inputs and outputs, and its side effects \
             (I/O, network, state changes, errors raised). \
             Do not restate the name. Output only the summary.\n\n\
             ```rust\nfn add(a: u32, b: u32) -> u32 { a + b }\n```"
        );
    }

    #[test]
    fn the_callee_signatures_follow_the_code() {
        let mut r = request(Level::Symbol);
        r.callees = vec!["fn helper(x: u32) -> u32".into()];
        assert!(
            r.prompt()
                .ends_with("```\n\nIt calls functions with these signatures:\n\n```rust\nfn helper(x: u32) -> u32\n```")
        );
    }

    #[test]
    fn a_file_prompt_has_no_code() {
        let prompt = request(Level::File).prompt();
        assert!(!prompt.contains("a + b"), "{prompt}");
        assert!(prompt.ends_with("- `add` (function): Adds."));
    }

    #[test]
    fn the_key_of_a_file_depends_on_the_order_of_its_children() {
        let a = children_key(Level::File, "rust", "a.rs", &["x".into(), "y".into()]);
        let b = children_key(Level::File, "rust", "a.rs", &["y".into(), "x".into()]);
        let c = children_key(Level::Module, "rust", "a.rs", &["x".into(), "y".into()]);
        let d = children_key(Level::File, "rust", "b.rs", &["x".into(), "y".into()]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d, "the path is in the key of a file");
        assert_eq!(
            a,
            children_key(Level::File, "rust", "a.rs", &["x".into(), "y".into()])
        );
    }

    #[test]
    fn a_signature_key_has_no_formatting() {
        let want = "def load(path: str, opts: dict[str, int]) -> list";
        for formatted in [
            "def load(path: str, opts: dict[str, int]) -> list",
            "def load( path: str,\n    opts: dict[str,int], ) -> list",
            "def  load(path:str, opts: dict[ str, int ])->list",
        ] {
            assert_eq!(signature_key(formatted), signature_key(want), "{formatted}");
        }
        assert_ne!(signature_key("fn f(a: u32)"), signature_key("fn f(a: u64)"));
        assert_eq!(signature_key("pub fn  f( a : u32 , )"), "pub fn f(a:u32)");
    }

    #[test]
    fn the_key_of_a_symbol_has_the_signatures_of_its_callees() {
        let plain = symbol_key("t", &[]);
        let one = symbol_key("t", &["fn g(x: u32)".into()]);
        let reformatted = symbol_key("t", &["fn g( x: u32 )".into()]);
        let changed = symbol_key("t", &["fn g(x: u64)".into()]);
        assert_ne!(plain, one);
        assert_eq!(one, reformatted);
        assert_ne!(one, changed);
    }

    #[test]
    fn the_fake_gives_the_same_text_for_the_same_input() {
        let mut fake = FakeSummarizer::default();
        let a = fake.summarize(&request(Level::Symbol)).unwrap();
        let b = fake.summarize(&request(Level::Symbol)).unwrap();
        let c = fake.summarize(&request(Level::File)).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(fake.requests.len(), 3);
    }

    /// A repository with 2 Rust functions in one file, and its structure.
    fn small_index() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(
            repo.join("src/lib.rs"),
            "pub fn one() -> u32 { 1 }\n\npub fn two() -> u32 { one() + 1 }\n",
        )
        .unwrap();
        let db = dir.path().join("index.sqlite");
        crate::build_structure(&repo, &db).unwrap();
        (dir, repo, db)
    }

    #[test]
    fn a_file_waits_for_the_summaries_of_all_its_symbols() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut fake = FakeSummarizer::default();
        let limited = SummarizeOptions {
            under: Vec::new(),
            max_new_symbols: Some(1),
        };
        let report = summarize(&repo, &db, &mut fake, &embed, &limited).unwrap();
        assert_eq!(
            report.symbols,
            LevelCount {
                summarized: 1,
                reused: 0,
                left: 1
            }
        );
        assert_eq!(
            report.files,
            LevelCount {
                summarized: 0,
                reused: 0,
                left: 1
            }
        );

        let report = summarize(&repo, &db, &mut fake, &embed, &limited).unwrap();
        assert_eq!(
            report.symbols,
            LevelCount {
                summarized: 1,
                reused: 1,
                left: 0
            }
        );
        assert_eq!(report.files.summarized, 1);
        assert_eq!(report.modules.summarized, 1);

        // A third run finds each key in the pool and calls nothing.
        let report = summarize(&repo, &db, &mut fake, &embed, &limited).unwrap();
        assert!(report.calls.is_empty());
        assert_eq!(report.vectors, 0);
    }

    #[test]
    fn the_callee_signature_is_in_the_prompt_and_not_its_summary() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut fake = FakeSummarizer::default();
        summarize(&repo, &db, &mut fake, &embed, &SummarizeOptions::default()).unwrap();
        let two = fake.requests.iter().find(|r| r.name == "two").unwrap();
        assert_eq!(two.callees, ["pub fn one() -> u32"]);
        assert!(!two.prompt().contains("A symbol summary"));
    }

    #[test]
    fn a_pool_refuses_vectors_of_another_model() {
        let (_dir, repo, db) = small_index();
        let mut fake = FakeSummarizer::default();
        let options = SummarizeOptions::default();
        summarize(
            &repo,
            &db,
            &mut fake,
            &kairos_embed::DeterministicProvider::new(384),
            &options,
        )
        .unwrap();
        let err = summarize(
            &repo,
            &db,
            &mut fake,
            &kairos_embed::DeterministicProvider::new(8),
            &options,
        )
        .unwrap_err();
        assert!(matches!(err, IndexError::VectorModel { .. }), "{err}");
    }

    #[test]
    fn a_file_changed_after_the_build_stops_the_run() {
        let (_dir, repo, db) = small_index();
        fs::write(repo.join("src/lib.rs"), "pub fn one() -> u32 { 2 }\n").unwrap();
        let err = summarize(
            &repo,
            &db,
            &mut FakeSummarizer::default(),
            &kairos_embed::DeterministicProvider::default(),
            &SummarizeOptions::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&err, IndexError::Changed(path) if path == "src/lib.rs"),
            "{err}"
        );
    }

    #[test]
    fn a_declaration_is_the_text_before_the_body() {
        let cases = [
            (
                "pub fn f(x: u32) -> u32 { x }",
                "rust",
                "pub fn f(x: u32) -> u32",
            ),
            (
                "pub fn long(\n    a: &str,\n    b: Vec<u8>,\n) -> Result<(), E>\nwhere\n    E: Error,\n{\n    a\n}",
                "rust",
                "pub fn long( a: &str, b: Vec<u8>, ) -> Result<(), E> where E: Error,",
            ),
            ("fn area(&self) -> f64;", "rust", "fn area(&self) -> f64"),
            (
                "func (g *Greeter) Hi(name string) (string, error) {\n\treturn \"{\", nil\n}",
                "go",
                "func (g *Greeter) Hi(name string) (string, error)",
            ),
            (
                "@cached\ndef load(path: str, opts: dict[str, int]) -> list:\n    return []",
                "python",
                "def load(path: str, opts: dict[str, int]) -> list",
            ),
            (
                "export function total(items: Item[], sep = \"{\"): number {\n  return 0;\n}",
                "typescript",
                "export function total(items: Item[], sep = \"{\"): number",
            ),
        ];
        for (code, language, want) in cases {
            assert_eq!(declaration(code, language), want, "{code}");
        }
    }
}
