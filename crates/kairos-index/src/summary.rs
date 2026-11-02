//! The summaries and their vectors (COLLIERY-I-0264, "The two parts of an
//! index"; COLLIERY-T-1850).
//!
//! - **A symbol summary** comes from the code of the symbol and the signatures
//!   of the functions that it calls (its `certain` edges). It does not use
//!   their summaries: if it did, one change would change the input of each
//!   caller above it. Test symbols get a summary too (KAIROS-T-0421).
//! - **A file summary** comes from the summaries of its symbols, and **a
//!   module summary** from the summaries of the files in its folder. Neither
//!   reads code: the benchmark showed that file summaries from code cost as
//!   much as all the symbol summaries.
//! - **A file of declarations** has no symbol that gets a summary: for
//!   example a `lib.rs` of `mod` and `pub use` lines. Its summary comes from
//!   its doc comment and its declarations ([`declaration_lines`],
//!   KAIROS-T-0425). A file with none of these gets no summary.
//! - **The key** of a symbol summary is the hash of the tree hash of the
//!   symbol and of the signatures of its callees (COLLIERY-T-1851). So a
//!   changed signature gives new summaries to the direct callers only, and a
//!   changed body to none. The key of a file summary is the hash of its path
//!   and of the list of child summaries (or of its declaration lines); the
//!   key of a module summary is the hash of that list. A key that is in
//!   the pool is not summarized again.
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

/// The symbol kinds that get a summary: functions, types, `impl` blocks and
/// constants (KAIROS-T-0349). A `mod x;` line or a field gets none. The input
/// of an `impl` block is its header and the declarations of its functions,
/// not their bodies: see [`impl_text`]. Test code gets a summary as source
/// does (KAIROS-T-0421).
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
    "implementation",
    "constant",
    // Terraform: a managed and a data resource (KAIROS-T-0470). A file of
    // variables, outputs and module calls is a file of declarations.
    "resource",
    "data",
    // SQL (KAIROS-T-0350).
    "table",
    "index",
    "view",
    "trigger",
];

/// The system prompt of the model comparison (`~/code-index-eval`).
pub(crate) const SYSTEM_PROMPT: &str =
    "You write short summaries of source code for a code search index.";

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

/// The kind of a file request whose input is the declarations of the file
/// (KAIROS-T-0425), not the summaries of its symbols.
pub const DECLARATIONS_KIND: &str = "declarations";

/// The most lines of declarations that go into one file prompt.
const MAX_DECLARATION_LINES: usize = 200;

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
    /// The symbol kind, `file`, [`DECLARATIONS_KIND`] or `module`.
    pub kind: String,
    /// The symbol name, the file path or the folder path.
    pub name: String,
    /// The file of a symbol or a file, the folder of a module.
    pub path: String,
    /// The code of a symbol, or the declaration lines of a file of
    /// declarations. Another file or a module has none.
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
            Level::Symbol if self.kind == "implementation" => format!(
                "Summarize the {language} impl block `{}` from `{}` in 1 to 3 sentences, \
                 from its header and the declarations of its functions below. Say which \
                 type or trait it implements and what its functions are for. \
                 Do not restate the name. Output only the summary.\n\n\
                 ```{language}\n{code}\n```",
                self.name, self.path
            ),
            Level::Symbol if self.kind == "constant" => format!(
                "Summarize the {language} constant `{}` from `{}` in 1 or 2 sentences. \
                 Say what value it holds and what the code uses it for. \
                 Do not restate the name. Output only the summary.\n\n\
                 ```{language}\n{code}\n```",
                self.name, self.path
            ),
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
            // A file of declarations (KAIROS-T-0425).
            Level::File if self.kind == DECLARATIONS_KIND => format!(
                "Summarize the {language} source file `{}` in 1 to 3 sentences, from its \
                 doc comment and its declarations below: the modules that it declares and \
                 the names that it imports or exports. The file has no functions or types \
                 of its own. Say what the file is for and what it gives to other code. \
                 Do not list each name. Output only the summary.\n\n\
                 ```{language}\n{code}\n```",
                self.path
            ),
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
    /// The model, as `provider/model` (KAIROS-T-0338): for example
    /// `embedded/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M` or `ollama-cloud/gemma4:31b`.
    /// It is part of each summary key, so a summary is reused only for the
    /// same model, and the pool records it with each summary.
    fn model(&self) -> String;

    /// The summary for `request`: 1 to 3 sentences of plain text. The error
    /// text says why the model gave no summary.
    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String>;

    /// How many requests the summarizer takes at once (KAIROS-T-0341): 1
    /// for a model on the CPU, more for a hosted provider.
    fn concurrency(&self) -> usize {
        1
    }

    /// The summaries of `requests`, one result for each, in the same order.
    /// The default is one call after the other; a hosted summarizer sends
    /// them at the same time.
    fn summarize_many(&mut self, requests: &[SummaryRequest]) -> Vec<Result<String, String>> {
        requests.iter().map(|r| self.summarize(r)).collect()
    }

    /// Give the summarizer a function to call while a request waits for
    /// the provider: a permit or a back-off after a 429 (KAIROS-T-0495).
    /// The builder reports the progress of its build there. A summarizer
    /// that never waits does nothing with it.
    fn heartbeat(&mut self, beat: crate::rate::Beat) {
        let _ = beat;
    }

    /// Why the run must stop now, when it must (KAIROS-T-0495): the build
    /// was stopped, or it gives its slot. A failed summary does not stop
    /// the run; this does.
    fn halted(&self) -> Option<String> {
        None
    }
}

/// A summarizer for tests: a fixed text for each input, with no model. It
/// keeps each request, so that a test can see what the model would get.
#[derive(Debug)]
pub struct FakeSummarizer {
    /// The model name that the keys and the pool get: `fake/fixed` by
    /// default. A scenario with 2 models gives each fake its own name.
    pub model: String,
    pub requests: Vec<SummaryRequest>,
    /// The requests that the fake takes at once (KAIROS-T-0341): 1 by
    /// default.
    pub concurrency: usize,
    /// The size of each batch that [`Summarizer::summarize_many`] got.
    pub batches: Vec<usize>,
}

impl Default for FakeSummarizer {
    fn default() -> Self {
        FakeSummarizer::named("fake/fixed")
    }
}

impl FakeSummarizer {
    /// A fake summarizer with the model name `model`.
    pub fn named(model: &str) -> Self {
        FakeSummarizer {
            model: model.to_string(),
            requests: Vec::new(),
            concurrency: 1,
            batches: Vec::new(),
        }
    }

    /// A fake summarizer that takes `concurrency` requests at once.
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }
}

impl Summarizer for FakeSummarizer {
    fn model(&self) -> String {
        self.model.clone()
    }

    fn concurrency(&self) -> usize {
        self.concurrency
    }

    fn summarize_many(&mut self, requests: &[SummaryRequest]) -> Vec<Result<String, String>> {
        self.batches.push(requests.len());
        requests.iter().map(|r| self.summarize(r)).collect()
    }

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
    /// Items whose summary call failed (KAIROS-T-0495): the summary is
    /// missing, and a later run makes it.
    pub failed: usize,
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
    /// The first summary call that failed (KAIROS-T-0495), when one did.
    pub failure: Option<SummaryFailure>,
    /// The failed calls that the provider refused for the rate.
    pub rate_limited: usize,
}

/// A summary call that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryFailure {
    /// The file and the symbol name, the file path or the folder path.
    pub name: String,
    pub message: String,
}

impl SummaryReport {
    fn level_mut(&mut self, level: Level) -> &mut LevelCount {
        match level {
            Level::Symbol => &mut self.symbols,
            Level::File => &mut self.files,
            Level::Module => &mut self.modules,
        }
    }

    /// The summaries that the index does not have (KAIROS-T-0495): the
    /// failed calls, and the items that got no summary because a child has
    /// none. A later run makes them, as the keys are not in the pool.
    pub fn missing(&self) -> usize {
        [self.symbols, self.files, self.modules]
            .iter()
            .map(|c| c.failed + c.left)
            .sum()
    }
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
}

impl Sym {
    fn summarized(&self) -> bool {
        SUMMARIZED_KINDS.contains(&self.kind.as_str())
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

/// [`summarize`], and when a file of the tree changed after the build of the
/// structure ([`IndexError::Changed`]; for example a dependency step wrote a
/// lock file), build the structure again from the tree, once, and summarize
/// again (KAIROS-T-0667). `build` is the report of the first build of the
/// structure: the report of the second replaces it, and the path of the file
/// that changed is the second part of the result, for the log. A second
/// `Changed` is the error.
pub fn summarize_or_rebuild(
    root: &Path,
    db: &Path,
    summarizer: &mut dyn Summarizer,
    embedder: &dyn EmbeddingProvider,
    update: &crate::UpdateOptions,
    build: &crate::BuildReport,
) -> Result<(crate::UpdateReport, Option<String>), IndexError> {
    match summarize(root, db, summarizer, embedder, &update.summarize) {
        Ok(summary) => Ok((
            crate::UpdateReport {
                build: build.clone(),
                summary,
            },
            None,
        )),
        Err(IndexError::Changed(path)) => {
            let build = crate::update_structure(root, db, update)?;
            let summary = summarize(root, db, summarizer, embedder, &update.summarize)?;
            Ok((crate::UpdateReport { build, summary }, Some(path)))
        }
        Err(e) => Err(e),
    }
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
    conn.busy_timeout(crate::BUSY_TIMEOUT)?;
    schema::prepare(&conn)?;
    let vector_model = match &model {
        Some((_, embedder)) => {
            let name = model_name(*embedder);
            check_vector_model(&conn, &name)?;
            Some(name)
        }
        None => None,
    };
    // The model of the keys (KAIROS-T-0338): the summarizer of this run,
    // or, for a link run, the model of the last run that the pool records.
    // A pool with no record (an index from before this version) links
    // nothing: each key is new.
    let key_model = match &model {
        Some((summarizer, _)) => {
            let name = summarizer.model();
            conn.execute(
                "INSERT OR REPLACE INTO pool_meta (name, value) VALUES ('summary_model', ?1)",
                [&name],
            )?;
            name
        }
        None => summary_model_of(&conn)?.unwrap_or_default(),
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

    // 1. The symbols. The requests go to the summarizer in batches of its
    // concurrency (KAIROS-T-0341); a batch is written before the next.
    let mut pending: Vec<(SummaryRequest, String)> = Vec::new();
    // The keys whose summary call failed (KAIROS-T-0495): their items get
    // no summary key, and a later run makes them.
    let mut failed: HashSet<String> = HashSet::new();
    let mut new_symbols = 0usize;
    let mut seen: HashSet<String> = HashSet::new();
    // The key of each symbol whose summary is in the pool.
    let mut symbol_keys: HashMap<i64, String> = HashMap::new();
    // The symbols of each file, for the functions inside an `impl` block.
    let mut per_file: HashMap<i64, Vec<&Sym>> = HashMap::new();
    for s in &symbols {
        per_file.entry(s.file_id).or_default().push(s);
    }
    for s in &symbols {
        let file = file_by_id[&s.file_id];
        if !s.summarized() || !in_scope(&file.path) {
            continue;
        }
        // An `impl` block (KAIROS-T-0349): its input is its header and the
        // declarations of its functions, so its key comes from that text,
        // not from its tree hash, and a change in a body leaves it. Its
        // calls are those of its functions, so it gets no callee list.
        if s.kind == "implementation" {
            let text = impl_text(
                &mut texts,
                s,
                per_file.get(&s.file_id).map_or(&[], Vec::as_slice),
            )?;
            let key = symbol_key(&key_model, &hex(&Sha256::digest(text.as_bytes())), &[]);
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
            let request = SummaryRequest {
                level: Level::Symbol,
                key: key.clone(),
                language: Some(s.language.clone()),
                kind: s.kind.clone(),
                name: s.name.clone(),
                path: file.path.clone(),
                code: Some(text),
                callees: Vec::new(),
                children: Vec::new(),
            };
            pending.push((request, format!("{}:{}", file.path, s.qualified())));
            report.symbols.summarized += 1;
            if pending.len() >= summarizer.concurrency().max(1) {
                flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
            }
            seen.insert(key.clone());
            symbol_keys.insert(s.id, key);
            new_symbols += 1;
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
        let key = symbol_key(&key_model, &s.tree_hash, &callees);
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
        pending.push((request, format!("{}:{}", file.path, s.qualified())));
        report.symbols.summarized += 1;
        if pending.len() >= summarizer.concurrency().max(1) {
            flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
        }
        seen.insert(key.clone());
        symbol_keys.insert(s.id, key);
        new_symbols += 1;
    }
    if let Some((summarizer, _)) = model.as_mut() {
        flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
    }
    symbol_keys.retain(|_, key| !failed.contains(key));

    // 2. The files, from the summaries of their symbols. A file with no
    // symbol of a summarized kind, from its declarations (KAIROS-T-0425).
    let mut by_file: BTreeMap<i64, Vec<&Sym>> = BTreeMap::new();
    for s in symbols.iter().filter(|s| s.summarized()) {
        by_file.entry(s.file_id).or_default().push(s);
    }
    let mut file_keys: BTreeMap<String, Option<String>> = BTreeMap::new();
    for f in files
        .iter()
        .filter(|f| matches!(f.decision.as_str(), "source" | "test") && in_scope(&f.path))
    {
        let language = f.language.clone().unwrap_or_default();
        let request = match by_file.get(&f.id) {
            Some(children) => {
                let mut lines = Vec::with_capacity(children.len());
                for s in children {
                    let Some(key) = symbol_keys.get(&s.id) else {
                        break;
                    };
                    match summary_of(&conn, key)? {
                        Some(text) => {
                            lines.push(format!("- `{}` ({}): {text}", s.qualified(), s.kind))
                        }
                        None => break,
                    }
                }
                if lines.len() < children.len() {
                    report.files.left += 1;
                    file_keys.insert(f.path.clone(), None);
                    continue;
                }
                SummaryRequest {
                    level: Level::File,
                    key: children_key(&key_model, Level::File, &language, &f.path, &lines),
                    language: Some(language),
                    kind: "file".into(),
                    name: f.path.clone(),
                    path: f.path.clone(),
                    code: None,
                    callees: Vec::new(),
                    children: lines,
                }
            }
            None => {
                // The key has the kind first, so it is never the key of a
                // file of symbols.
                let Some(text) = declaration_lines(&texts.whole(f.id)?, &language) else {
                    continue;
                };
                let key_lines = [DECLARATIONS_KIND.to_string(), text.clone()];
                SummaryRequest {
                    level: Level::File,
                    key: children_key(&key_model, Level::File, &language, &f.path, &key_lines),
                    language: Some(language),
                    kind: DECLARATIONS_KIND.into(),
                    name: f.path.clone(),
                    path: f.path.clone(),
                    code: Some(text),
                    callees: Vec::new(),
                    children: Vec::new(),
                }
            }
        };
        let key = request.key.clone();
        if in_pool(&conn, &key)? || pending.iter().any(|(r, _)| r.key == key) {
            report.files.reused += 1;
        } else if let Some((summarizer, _)) = model.as_mut() {
            pending.push((request, f.path.clone()));
            report.files.summarized += 1;
            if pending.len() >= summarizer.concurrency().max(1) {
                flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
            }
        } else {
            report.files.left += 1;
            // A file of declarations with no summary in the pool does not
            // hold back the summary of its folder: a pool from before
            // KAIROS-T-0425 has the folder summary without the file.
            if request.kind != DECLARATIONS_KIND {
                file_keys.insert(f.path.clone(), None);
            }
            continue;
        }
        file_keys.insert(f.path.clone(), Some(key));
    }
    if let Some((summarizer, _)) = model.as_mut() {
        flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
    }
    for key in file_keys.values_mut() {
        if key.as_ref().is_some_and(|k| failed.contains(k)) {
            *key = None;
        }
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
        let key = children_key(&key_model, Level::Module, "", "", &lines);
        if in_pool(&conn, &key)? || pending.iter().any(|(r, _)| r.key == key) {
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
            pending.push((request, folder.clone()));
            report.modules.summarized += 1;
            if pending.len() >= summarizer.concurrency().max(1) {
                flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
            }
        } else {
            report.modules.left += 1;
            continue;
        }
        module_keys.push((folder.clone(), key));
    }
    if let Some((summarizer, _)) = model.as_mut() {
        flush(&conn, *summarizer, &mut pending, &mut report, &mut failed)?;
    }
    module_keys.retain(|(_, key)| !failed.contains(key));

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
            // The summaries of the symbols are in the text rank now
            // (KAIROS-T-0423).
            crate::text::refresh(&tx)?;
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

/// Send the pending requests to the summarizer as one batch, and put each
/// summary in the pool (KAIROS-T-0341). The summaries that came back are
/// written before an error is returned, so a stopped run keeps its work.
/// The batch is empty afterwards.
///
/// A failed call does not stop the run (KAIROS-T-0495): it is counted as
/// `failed`, its key goes into `failed`, and the item has no summary. The
/// run stops when the summarizer is halted, and when no call of the run has
/// given a summary yet and none of the failed calls was refused for the
/// rate: a wrong key or a wrong model fails each call, so the run fails at
/// once, as before.
fn flush(
    conn: &Connection,
    summarizer: &mut dyn Summarizer,
    pending: &mut Vec<(SummaryRequest, String)>,
    report: &mut SummaryReport,
    failed: &mut HashSet<String>,
) -> Result<(), IndexError> {
    if pending.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let requests: Vec<SummaryRequest> = pending.iter().map(|(r, _)| r.clone()).collect();
    let results = summarizer.summarize_many(&requests);
    let elapsed = started.elapsed();
    let model = summarizer.model();
    let batch: Vec<(SummaryRequest, String)> = std::mem::take(pending);
    for ((request, name), result) in batch.into_iter().zip(results) {
        let text = match result.map(|text| text.trim().to_string()) {
            Ok(text) if !text.is_empty() => text,
            failure => {
                let message = failure
                    .err()
                    .unwrap_or_else(|| "the summary is empty".to_string());
                let count = report.level_mut(request.level);
                count.summarized = count.summarized.saturating_sub(1);
                count.failed += 1;
                if crate::rate::is_rate_limited(&message) {
                    report.rate_limited += 1;
                }
                report
                    .failure
                    .get_or_insert(SummaryFailure { name, message });
                failed.insert(request.key);
                continue;
            }
        };
        conn.execute(
            "INSERT INTO summaries (key, level, summary, model) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (key) DO NOTHING",
            params![request.key, request.level.as_str(), text, model],
        )?;
        report.calls.push(SummaryCall {
            level: request.level,
            key: request.key.clone(),
            name,
            elapsed,
            summary: text,
        });
    }
    if let Some(why) = summarizer.halted() {
        return Err(IndexError::Stopped(why));
    }
    if report.calls.is_empty()
        && report.rate_limited == 0
        && let Some(SummaryFailure { name, message }) = report.failure.clone()
    {
        return Err(IndexError::Summary { name, message });
    }
    Ok(())
}

/// The key of a symbol summary: the hash of the model, of the tree hash of
/// the symbol and of the signatures of its callees, in the order of the
/// calls. The signatures are compared with no formatting (see
/// [`signature_key`]), so a formatting change gives the same key. The model
/// is in the key (KAIROS-T-0338), so the same code with another model is
/// another key.
fn symbol_key(model: &str, tree_hash: &str, callees: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"symbol");
    hasher.update([0]);
    hasher.update(model.as_bytes());
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

/// The key of a file or a module summary: the hash of the model, its level,
/// its language, its path (a file only; empty for a module) and its child
/// lines, in order.
fn children_key(model: &str, level: Level, language: &str, path: &str, lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(level.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(model.as_bytes());
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

/// The model of the last summary run, as `pool_meta` keeps it
/// (KAIROS-T-0338). None for a pool that no summary run wrote.
pub(crate) fn summary_model_of(conn: &Connection) -> Result<Option<String>, IndexError> {
    Ok(conn
        .query_row(
            "SELECT value FROM pool_meta WHERE name = 'summary_model'",
            [],
            |r| r.get(0),
        )
        .optional()?)
}

/// The model of the vectors, as `pool_meta` keeps it.
pub(crate) fn model_name(embedder: &dyn EmbeddingProvider) -> String {
    vector_model_name(embedder)
}

/// The model of the vectors of `embedder`, as an index records it
/// (`provider/model/dimension`): the `vector_model` of a stored index.
pub fn vector_model_name(embedder: &dyn EmbeddingProvider) -> String {
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
        // The model of the last run of `from` is the record of `to` when
        // `to` has none (KAIROS-T-0338).
        if let Some(summary_model) = summary_model_of(from)? {
            tx.execute(
                "INSERT OR IGNORE INTO pool_meta (name, value) VALUES ('summary_model', ?1)",
                [summary_model],
            )?;
        }
        let mut insert = tx.prepare(
            "INSERT INTO summaries (key, level, summary, vector, model) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET vector = excluded.vector
             WHERE summaries.vector IS NULL",
        )?;
        let mut stmt = from.prepare("SELECT key, level, summary, vector, model FROM summaries")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            insert.execute(params![
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
                row.get::<_, String>(4)?,
            ])?;
        }
        // A symbol of `to` can be linked to a summary that it did not have.
        crate::text::refresh(&tx)?;
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
                s.end_byte, s.tree_hash
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

    /// The whole text of a file.
    fn whole(&mut self, file_id: i64) -> Result<String, IndexError> {
        self.slice(file_id, 0, 0)?;
        Ok(self.texts[&file_id].clone())
    }
}

/// The input of the summary of a file of declarations (KAIROS-T-0425): its
/// doc comment, then its declarations, as lines of the file. None when the
/// file has neither, or when the language is not one of these:
///
/// - Rust: the `//!` and `/*! */` comments; the `mod x;`, `use` and
///   `extern crate` items, with any visibility.
/// - Python: the module docstring; the `import` and `from` lines and
///   `__all__` at the top level.
/// - TypeScript and JavaScript: the comment at the start of the file; the
///   `import` and `export` statements and the `require` lines at the top
///   level.
/// - Go: the package comment; the `import` declarations. The `package` line
///   comes first when the file has one of these.
/// - Terraform: the comment at the start of the file; the first line of each
///   top-level block, and its `source`, `version`, `type` and `description`
///   lines (KAIROS-T-0470).
///
/// At most [`MAX_DECLARATION_LINES`] lines are kept.
pub(crate) fn declaration_lines(text: &str, language: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (doc, declarations) = match language {
        "rust" => rust_declarations(&lines),
        "python" => python_declarations(&lines),
        "typescript" | "tsx" | "javascript" => script_declarations(&lines),
        "go" => go_declarations(&lines),
        "terraform" => terraform_declarations(&lines),
        _ => return None,
    };
    if doc.is_empty() && declarations.is_empty() {
        return None;
    }
    let mut out = doc;
    if !out.is_empty() && !declarations.is_empty() {
        out.push(String::new());
    }
    out.extend(declarations);
    out.truncate(MAX_DECLARATION_LINES);
    Some(out.join("\n"))
}

/// The lines of the statement that starts at line `start`: up to the end of
/// the line where its brackets close (and, for Python, with no `\` at the
/// end). Also the index of the next line.
fn statement(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let line = lines[i].trim_end();
        for c in line.chars() {
            match c {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        out.push(line.to_string());
        i += 1;
        if depth <= 0 && !line.ends_with('\\') {
            break;
        }
    }
    (out, i)
}

/// A Rust item with no visibility: `pub`, `pub(crate)` and `pub(in x)` go.
fn rust_item(line: &str) -> &str {
    let Some(rest) = line.strip_prefix("pub") else {
        return line;
    };
    let rest = match rest.strip_prefix('(') {
        Some(inner) => inner.split_once(')').map_or(inner, |(_, after)| after),
        None if rest.starts_with(char::is_whitespace) => rest,
        None => return line,
    };
    rest.trim_start()
}

fn rust_declarations(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut doc, mut declarations) = (Vec::new(), Vec::new());
    let mut in_doc_block = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if in_doc_block || line.starts_with("/*!") {
            doc.push(line.to_string());
            in_doc_block = !line.contains("*/");
            i += 1;
            continue;
        }
        if line.starts_with("//!") {
            doc.push(line.to_string());
            i += 1;
            continue;
        }
        let item = rust_item(line);
        if ["mod ", "use ", "extern crate "]
            .iter()
            .any(|p| item.starts_with(p))
        {
            // A `mod x { ... }` with a body is skipped whole.
            let (lines_of, next) = statement(lines, i);
            if lines_of.last().is_some_and(|l| l.ends_with(';')) {
                declarations.extend(lines_of);
            }
            i = next;
            continue;
        }
        i += 1;
    }
    (doc, declarations)
}

fn python_declarations(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut doc, mut declarations) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() && (lines[i].trim().is_empty() || lines[i].trim().starts_with('#')) {
        i += 1;
    }
    if let Some(first) = lines.get(i) {
        let text = first.trim();
        let body = text.trim_start_matches(['r', 'R', 'u', 'U', 'b', 'B']);
        if let Some(quote) = ["\"\"\"", "'''"].into_iter().find(|q| body.starts_with(q)) {
            doc.push(text.to_string());
            i += 1;
            if !body[3..].contains(quote) {
                while i < lines.len() {
                    doc.push(lines[i].trim_end().to_string());
                    i += 1;
                    if lines[i - 1].contains(quote) {
                        break;
                    }
                }
            }
        } else if body.starts_with(['"', '\'']) {
            doc.push(text.to_string());
            i += 1;
        }
    }
    while i < lines.len() {
        let line = lines[i];
        if ["import ", "from ", "__all__"]
            .iter()
            .any(|p| line.starts_with(p))
        {
            let (lines_of, next) = statement(lines, i);
            declarations.extend(lines_of);
            i = next;
            continue;
        }
        i += 1;
    }
    (doc, declarations)
}

fn script_declarations(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut doc, mut declarations) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with("#!")) {
        i += 1;
    }
    if lines
        .get(i)
        .is_some_and(|l| l.trim_start().starts_with("/*"))
    {
        while i < lines.len() {
            doc.push(lines[i].trim_end().to_string());
            i += 1;
            if lines[i - 1].contains("*/") {
                break;
            }
        }
    } else {
        while i < lines.len() && lines[i].trim_start().starts_with("//") {
            doc.push(lines[i].trim_end().to_string());
            i += 1;
        }
    }
    while i < lines.len() {
        let line = lines[i];
        let module = ["import ", "import{", "export ", "export{", "export*"]
            .iter()
            .any(|p| line.starts_with(p));
        let require = ["const ", "let ", "var "]
            .iter()
            .any(|p| line.starts_with(p))
            && line.contains("require(");
        if module || require || line.starts_with("module.exports") || line.starts_with("exports.") {
            let (lines_of, next) = statement(lines, i);
            declarations.extend(lines_of);
            i = next;
            continue;
        }
        i += 1;
    }
    (doc, declarations)
}

fn go_declarations(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut doc, mut declarations) = (Vec::new(), Vec::new());
    let Some(package) = lines.iter().position(|l| l.starts_with("package ")) else {
        return (doc, declarations);
    };
    // The package comment: the comment just above the package line.
    let mut start = package;
    if package > 0 && lines[package - 1].trim_end().ends_with("*/") {
        while start > 0 {
            start -= 1;
            if lines[start].trim_start().starts_with("/*") {
                break;
            }
        }
    } else {
        while start > 0 && lines[start - 1].trim_start().starts_with("//") {
            start -= 1;
        }
    }
    doc.extend(
        lines[start..package]
            .iter()
            .map(|l| l.trim_end().to_string()),
    );
    let mut i = package + 1;
    while i < lines.len() {
        if lines[i].starts_with("import ") || lines[i].starts_with("import(") {
            let (lines_of, next) = statement(lines, i);
            declarations.extend(lines_of);
            i = next;
            continue;
        }
        i += 1;
    }
    if !doc.is_empty() || !declarations.is_empty() {
        declarations.insert(0, lines[package].trim_end().to_string());
    }
    (doc, declarations)
}

fn terraform_declarations(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut doc, mut declarations) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with('#') || line.starts_with("//") {
            doc.push(lines[i].trim_end().to_string());
            i += 1;
        } else {
            break;
        }
    }
    let mut depth = 0i32;
    for line in &lines[i..] {
        let trimmed = line.trim();
        let before = depth;
        let mut quote = false;
        for c in line.chars() {
            match c {
                '"' => quote = !quote,
                '{' if !quote => depth += 1,
                '}' if !quote => depth -= 1,
                '#' if !quote => break,
                _ => {}
            }
        }
        let kept = match before {
            // The first line of a top-level block, and its last line.
            0 => depth > 0 || trimmed.ends_with('}') && !trimmed.starts_with('#'),
            1 if depth == 0 => true,
            1 => {
                let key = trimmed.split('=').next().unwrap_or_default().trim();
                trimmed.contains('=')
                    && matches!(key, "source" | "version" | "type" | "description")
            }
            _ => false,
        };
        if kept {
            declarations.push(line.trim_end().to_string());
        }
    }
    (doc, declarations)
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

/// The text of an `impl` block for its summary (KAIROS-T-0349): its header
/// up to the opening brace, then the declaration of each function in it, no
/// bodies. `inner` is the symbols of its file.
fn impl_text(texts: &mut FileTexts, block: &Sym, inner: &[&Sym]) -> Result<String, IndexError> {
    let code = texts.slice(block.file_id, block.start_byte, block.end_byte)?;
    let header = code.split('{').next().unwrap_or(&code).trim_end();
    let mut out = format!("{header} {{");
    for f in inner.iter().filter(|f| {
        f.id != block.id
            && f.start_byte >= block.start_byte
            && f.end_byte <= block.end_byte
            && matches!(f.kind.as_str(), "function" | "method" | "constructor")
    }) {
        let signature = declaration(
            &texts.slice(f.file_id, f.start_byte, f.end_byte)?,
            &f.language,
        );
        out.push_str("\n    ");
        out.push_str(&signature);
        out.push(';');
    }
    out.push_str("\n}");
    Ok(out)
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
        let a = children_key(
            "fake/fixed",
            Level::File,
            "rust",
            "a.rs",
            &["x".into(), "y".into()],
        );
        let b = children_key(
            "fake/fixed",
            Level::File,
            "rust",
            "a.rs",
            &["y".into(), "x".into()],
        );
        let c = children_key(
            "fake/fixed",
            Level::Module,
            "rust",
            "a.rs",
            &["x".into(), "y".into()],
        );
        let d = children_key(
            "fake/fixed",
            Level::File,
            "rust",
            "b.rs",
            &["x".into(), "y".into()],
        );
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d, "the path is in the key of a file");
        assert_eq!(
            a,
            children_key(
                "fake/fixed",
                Level::File,
                "rust",
                "a.rs",
                &["x".into(), "y".into()]
            )
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
        let plain = symbol_key("fake/fixed", "t", &[]);
        let one = symbol_key("fake/fixed", "t", &["fn g(x: u32)".into()]);
        let reformatted = symbol_key("fake/fixed", "t", &["fn g( x: u32 )".into()]);
        let changed = symbol_key("fake/fixed", "t", &["fn g(x: u64)".into()]);
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
                left: 1,
                failed: 0
            }
        );
        assert_eq!(
            report.files,
            LevelCount {
                summarized: 0,
                reused: 0,
                left: 1,
                failed: 0
            }
        );

        let report = summarize(&repo, &db, &mut fake, &embed, &limited).unwrap();
        assert_eq!(
            report.symbols,
            LevelCount {
                summarized: 1,
                reused: 1,
                left: 0,
                failed: 0
            }
        );
        assert_eq!(report.files.summarized, 1);
        assert_eq!(report.modules.summarized, 1);

        // A third run finds each key in the pool and calls nothing.
        let report = summarize(&repo, &db, &mut fake, &embed, &limited).unwrap();
        assert!(report.calls.is_empty());
        assert_eq!(report.vectors, 0);
    }

    /// A summarizer whose answer for each request is given by `answer`
    /// (KAIROS-T-0495).
    struct Scripted<F: FnMut(&SummaryRequest) -> Result<String, String>> {
        answer: F,
        halted: Option<String>,
    }

    impl<F: FnMut(&SummaryRequest) -> Result<String, String>> Summarizer for Scripted<F> {
        fn model(&self) -> String {
            "fake/fixed".into()
        }

        fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
            (self.answer)(request)
        }

        fn halted(&self) -> Option<String> {
            self.halted.clone()
        }
    }

    fn rate_error() -> String {
        format!("x answered 429 (6 tries; {})", crate::rate::RATE_LIMITED)
    }

    #[test]
    fn a_failed_summary_is_missing_and_the_next_run_makes_it() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        // The summary of `two` is refused for the rate; the others come.
        let mut first = Scripted {
            answer: |r: &SummaryRequest| {
                if r.name == "two" {
                    Err(rate_error())
                } else {
                    Ok(format!("About {}.", r.name))
                }
            },
            halted: None,
        };
        let options = SummarizeOptions::default();
        let report = summarize(&repo, &db, &mut first, &embed, &options).unwrap();
        assert_eq!(
            report.symbols,
            LevelCount {
                summarized: 1,
                reused: 0,
                left: 0,
                failed: 1
            }
        );
        assert_eq!(report.files.left, 1, "the file waits for `two`");
        assert_eq!(report.modules.left, 1);
        assert_eq!(report.missing(), 3);
        assert_eq!(report.rate_limited, 1);
        assert_eq!(report.failure.as_ref().unwrap().name, "src/lib.rs:two");
        let conn = Connection::open(&db).unwrap();
        let without: i64 = conn
            .query_row(
                "SELECT count(*) FROM symbols WHERE name = 'two' AND summary_key IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(without, 1, "the symbol has no key of a missing summary");

        // The next run makes the missing summaries only.
        let mut fake = FakeSummarizer::default();
        let report = summarize(&repo, &db, &mut fake, &embed, &options).unwrap();
        assert_eq!(report.symbols.summarized, 1);
        assert_eq!(report.symbols.reused, 1);
        assert_eq!(report.files.summarized, 1);
        assert_eq!(report.modules.summarized, 1);
        assert_eq!(report.missing(), 0);
    }

    #[test]
    fn a_run_refused_for_the_rate_at_each_call_ends_with_all_missing() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut refused = Scripted {
            answer: |_: &SummaryRequest| Err(rate_error()),
            halted: None,
        };
        let report = summarize(
            &repo,
            &db,
            &mut refused,
            &embed,
            &SummarizeOptions::default(),
        )
        .unwrap();
        assert!(report.calls.is_empty());
        assert_eq!(report.symbols.failed, 2);
        assert_eq!(report.missing(), 4);
    }

    #[test]
    fn a_run_whose_each_call_fails_for_another_reason_fails() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut wrong_key = Scripted {
            answer: |_: &SummaryRequest| Err("x answered 401 Unauthorized".to_string()),
            halted: None,
        };
        let error = summarize(
            &repo,
            &db,
            &mut wrong_key,
            &embed,
            &SummarizeOptions::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&error, IndexError::Summary { message, .. } if message.contains("401")),
            "{error}"
        );
    }

    #[test]
    fn a_halted_summarizer_stops_the_run() {
        let (_dir, repo, db) = small_index();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut stopped = Scripted {
            answer: |_: &SummaryRequest| Err("stopped".to_string()),
            halted: Some("the build was stopped".to_string()),
        };
        let error = summarize(
            &repo,
            &db,
            &mut stopped,
            &embed,
            &SummarizeOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(error, IndexError::Stopped(_)), "{error}");
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
    fn a_file_changed_after_the_build_makes_the_structure_again_once() {
        let (_dir, repo, db) = small_index();
        let update = crate::UpdateOptions::default();
        let first = crate::update_structure(&repo, &db, &update).unwrap();
        let embedder = kairos_embed::DeterministicProvider::default();
        // Nothing changed: no second build.
        let (report, again) = summarize_or_rebuild(
            &repo,
            &db,
            &mut FakeSummarizer::default(),
            &embedder,
            &update,
            &first,
        )
        .unwrap();
        assert_eq!(again, None);
        assert_eq!(report.build.symbols, first.symbols);
        // A file written after the build: the structure is made again, and
        // the summary run goes on.
        fs::write(
            repo.join("src/lib.rs"),
            "pub fn one() -> u32 { 2 }\n\npub fn three() -> u32 { 3 }\n",
        )
        .unwrap();
        let (report, again) = summarize_or_rebuild(
            &repo,
            &db,
            &mut FakeSummarizer::default(),
            &embedder,
            &update,
            &first,
        )
        .unwrap();
        assert_eq!(again.as_deref(), Some("src/lib.rs"));
        assert_eq!(report.build.symbols, 2);
        assert!(report.summary.failure.is_none());
    }

    #[test]
    fn a_rust_file_of_declarations_gives_its_doc_and_its_mod_and_use_lines() {
        let text = "//! Policy documents.\n\
                    \n\
                    #[cfg(test)]\n\
                    mod tests {\n    use super::*;\n}\n\
                    mod document;\n\
                    pub(crate) mod rules;\n\
                    pub use document::{\n    PolicyDocument,\n    parse_document,\n};\n\
                    extern crate alloc;\n\
                    // A plain comment.\n\
                    macro_rules! m { () => {} }\n";
        assert_eq!(
            declaration_lines(text, "rust").as_deref(),
            Some(
                "//! Policy documents.\n\
                 \n\
                 mod document;\n\
                 pub(crate) mod rules;\n\
                 pub use document::{\n    PolicyDocument,\n    parse_document,\n};\n\
                 extern crate alloc;"
            )
        );
        assert_eq!(
            declaration_lines("// A plain comment.\nmacro_rules! m { () => {} }\n", "rust"),
            None
        );
        assert_eq!(declaration_lines("", "rust"), None);
        assert_eq!(declaration_lines("mod a;\n", "sql"), None);
    }

    #[test]
    fn a_python_file_gives_its_docstring_and_its_imports() {
        let text = "#!/usr/bin/env python3\n\
                    \"\"\"The polyglot package.\n\nIt loads data.\n\"\"\"\n\
                    from .stats import (\n    mean,\n    median,\n)\n\
                    import os\n\
                    __all__ = [\"mean\"]\n\
                    def later():\n    import sys\n";
        assert_eq!(
            declaration_lines(text, "python").as_deref(),
            Some(
                "\"\"\"The polyglot package.\n\nIt loads data.\n\"\"\"\n\
                 \n\
                 from .stats import (\n    mean,\n    median,\n)\n\
                 import os\n\
                 __all__ = [\"mean\"]"
            )
        );
        assert_eq!(
            declaration_lines("'''One line.'''\n", "python").as_deref(),
            Some("'''One line.'''")
        );
        assert_eq!(declaration_lines("# a comment\n\n", "python"), None);
    }

    #[test]
    fn a_script_file_gives_its_first_comment_and_its_imports_and_exports() {
        let text = "/**\n * The web part.\n */\n\
                    import { cents } from \"./cents\";\n\
                    export {\n  total,\n} from \"./totals\";\n\
                    export * from \"./format\";\n\
                    const fs = require(\"fs\");\n\
                    module.exports = { cents };\n";
        assert_eq!(
            declaration_lines(text, "typescript").as_deref(),
            Some(
                "/**\n * The web part.\n */\n\
                 \n\
                 import { cents } from \"./cents\";\n\
                 export {\n  total,\n} from \"./totals\";\n\
                 export * from \"./format\";\n\
                 const fs = require(\"fs\");\n\
                 module.exports = { cents };"
            )
        );
        assert_eq!(
            declaration_lines("// Barrel.\nexport * from './a'\n", "javascript").as_deref(),
            Some("// Barrel.\n\nexport * from './a'")
        );
    }

    #[test]
    fn a_go_file_gives_its_package_comment_and_its_imports() {
        let text = "// Package greet says hello.\n\
                    package greet\n\
                    \n\
                    import (\n\t\"fmt\"\n\t\"strings\"\n)\n";
        assert_eq!(
            declaration_lines(text, "go").as_deref(),
            Some(
                "// Package greet says hello.\n\
                 \n\
                 package greet\n\
                 import (\n\t\"fmt\"\n\t\"strings\"\n)"
            )
        );
        assert_eq!(declaration_lines("package greet\n", "go"), None);
    }

    /// KAIROS-T-0470: a Terraform file of variables, outputs and module calls.
    #[test]
    fn a_terraform_file_gives_its_comment_and_its_blocks() {
        let text = "# The inputs of the root module.\n\
                    \n\
                    variable \"region\" {\n  type    = string\n  default = \"us-east-1\"\n}\n\
                    \n\
                    variable \"name\" {}\n\
                    \n\
                    module \"store\" {\n  source = \"./modules/bucket\"\n  name   = \"x\"\n  tags   = { a = \"b\" }\n}\n";
        assert_eq!(
            declaration_lines(text, "terraform").as_deref(),
            Some(
                "# The inputs of the root module.\n\
                 \n\
                 variable \"region\" {\n  type    = string\n}\n\
                 variable \"name\" {}\n\
                 module \"store\" {\n  source = \"./modules/bucket\"\n}"
            )
        );
        assert_eq!(declaration_lines("region = \"x\"\n", "terraform"), None);
    }

    #[test]
    fn a_file_of_declarations_has_its_own_prompt() {
        let mut r = request(Level::File);
        r.kind = DECLARATIONS_KIND.into();
        r.children = Vec::new();
        r.code = Some("mod document;".into());
        let prompt = r.prompt();
        assert!(
            prompt.starts_with(
                "Summarize the rust source file `src/lib.rs` in 1 to 3 sentences, from its \
                 doc comment and its declarations below"
            ),
            "{prompt}"
        );
        assert!(prompt.ends_with("```rust\nmod document;\n```"), "{prompt}");
        // A runtime with a small context gives a shortened text.
        assert!(
            r.prompt_with_code("mod a;")
                .ends_with("```rust\nmod a;\n```")
        );
    }

    /// KAIROS-T-0425: a lib.rs of declarations gets a file summary, and the
    /// next run reuses it from the pool.
    #[test]
    fn a_file_of_declarations_is_summarized_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(
            repo.join("src/lib.rs"),
            "//! Numbers.\n\nmod one;\n\npub use one::one;\n",
        )
        .unwrap();
        fs::write(repo.join("src/one.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
        fs::write(repo.join("src/empty.rs"), "// Nothing here.\n").unwrap();
        let db = dir.path().join("index.sqlite");
        crate::build_structure(&repo, &db).unwrap();
        let embed = kairos_embed::DeterministicProvider::default();
        let mut fake = FakeSummarizer::default();
        let options = SummarizeOptions::default();
        let report = summarize(&repo, &db, &mut fake, &embed, &options).unwrap();
        assert_eq!(report.files.summarized, 2, "lib.rs and one.rs");
        let lib: Vec<_> = fake
            .requests
            .iter()
            .filter(|r| r.level == Level::File && r.path == "src/lib.rs")
            .collect();
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].kind, DECLARATIONS_KIND);
        assert_eq!(
            lib[0].code.as_deref(),
            Some("//! Numbers.\n\nmod one;\npub use one::one;")
        );
        assert!(
            !fake
                .requests
                .iter()
                .any(|r| r.level == Level::File && r.path == "src/empty.rs")
        );
        // The folder summary has the summary of lib.rs.
        let module = fake
            .requests
            .iter()
            .find(|r| r.level == Level::Module)
            .unwrap();
        assert!(module.children.iter().any(|l| l.starts_with("- `lib.rs`")));

        let report = summarize(&repo, &db, &mut fake, &embed, &options).unwrap();
        assert_eq!(report.files.reused, 2);
        assert!(report.calls.is_empty());

        let index = crate::Index::open(&db).unwrap();
        let map = index.module_map(None).unwrap();
        let files: Vec<_> = map.iter().flat_map(|m| &m.files).collect();
        let lib = files.iter().find(|f| f.path == "src/lib.rs").unwrap();
        assert!(lib.declarations_only && lib.summary.is_some(), "{lib:?}");
        let empty = files.iter().find(|f| f.path == "src/empty.rs").unwrap();
        assert!(
            empty.declarations_only && empty.summary.is_none(),
            "{empty:?}"
        );
        let one = files.iter().find(|f| f.path == "src/one.rs").unwrap();
        assert!(!one.declarations_only, "{one:?}");
    }

    /// A link run with no summary of a file of declarations in the pool (a
    /// pool from before KAIROS-T-0425) still links the folder summary.
    #[test]
    fn a_file_of_declarations_with_no_summary_does_not_hold_back_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/one.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
        let db = dir.path().join("index.sqlite");
        crate::build_structure(&repo, &db).unwrap();
        let embed = kairos_embed::DeterministicProvider::default();
        let options = SummarizeOptions::default();
        summarize(&repo, &db, &mut FakeSummarizer::default(), &embed, &options).unwrap();

        fs::write(repo.join("src/lib.rs"), "mod one;\n").unwrap();
        crate::build_structure(&repo, &db).unwrap();
        let report = link(&repo, &db, &options).unwrap();
        assert_eq!(
            report.files,
            LevelCount {
                summarized: 0,
                reused: 1,
                left: 1,
                failed: 0
            }
        );
        assert_eq!(report.modules.reused, 1);
        assert_eq!(report.modules.left, 0);
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
            (
                "resource \"aws_s3_bucket\" \"b\" {\n  bucket = \"${var.name}-{x}\"\n}",
                "terraform",
                "resource \"aws_s3_bucket\" \"b\"",
            ),
        ];
        for (code, language, want) in cases {
            assert_eq!(declaration(code, language), want, "{code}");
        }
    }
}
