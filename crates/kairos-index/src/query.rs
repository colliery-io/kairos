//! The reads of an index that the code tools give to an agent
//! (COLLIERY-I-0264, "What an agent gets"; COLLIERY-T-1852).
//!
//! - [`Index::search`]: a search of the summaries. A text rank and a vector
//!   rank are fused by reciprocal rank, as the search of the Kairos items
//!   does (`kairos-core::retrieval`): a cosine and a text score have no common
//!   scale, but 2 ranks do. The text rank is the FTS5 table of `text`, with
//!   the Porter stemmer and BM25 (KAIROS-T-0423). With no summaries, the
//!   search reads the names, the signatures and the paths. Test code ranks
//!   below the other symbols, unless the query is about tests
//!   (COLLIERY-T-1857, KAIROS-T-0422).
//! - [`Index::lookup`]: a symbol from the name that an agent gives.
//! - [`Index::callers`] and [`Index::callees`]: `certain` edges, and the
//!   `possible` ones on request.
//! - [`Index::references`]: each use of a symbol: the SCIP uses of a Rust
//!   symbol (a type, a field, an import, a call), and the calls by name of
//!   the other languages (KAIROS-T-0419).
//! - [`Index::adr_citations`]: each place that cites an ADR, and
//!   [`Index::cited_adrs`]: each ADR that the code cites (KAIROS-T-0703).
//! - [`Index::paths`]: the 3 shortest chains of `certain` calls from one
//!   symbol to another, with other steps (COLLIERY-T-2531).
//! - [`Index::module_map`]: the folders of source files with their
//!   summaries. It replaces the Metis index.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use kairos_embed::EmbeddingProvider;
use rusqlite::{OptionalExtension, Row, params};

use crate::citations::CitedAdr;
use crate::summary::model_name;
use crate::{Index, IndexError, SUMMARIZED_KINDS};

/// The constant of reciprocal-rank fusion: a rank `r` scores `1 / (K + r)`.
/// 60 is the value of the paper and of the Kairos item search.
const RRF_K: f64 = 60.0;

/// The most edges that a search for a path follows from one symbol.
const MAX_PATH_DEPTH: usize = 16;

/// The words that make a query about tests (KAIROS-T-0422): with one of
/// them, test code ranks with the other code; with none, it ranks below it.
/// A word of the query is compared after [`crate::text::words`] cuts it, so
/// `test_parse` has "test".
const TEST_WORDS: &[&str] = &[
    "test",
    "tests",
    "testing",
    "tested",
    "spec",
    "specs",
    "fixture",
    "fixtures",
    "mock",
    "mocks",
    "mocked",
    "mocking",
    "stub",
    "stubs",
    "assert",
    "asserts",
    "assertion",
    "assertions",
];

/// True when `query` is about tests: it has a word of [`TEST_WORDS`].
pub(crate) fn about_tests(query: &str) -> bool {
    crate::text::words(query)
        .iter()
        .any(|w| TEST_WORDS.contains(&w.as_str()))
}

/// One symbol, as the tools show it.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolInfo {
    pub id: i64,
    pub file: String,
    pub name: String,
    pub container: Option<String>,
    pub kind: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub is_test: bool,
    /// The summary from the pool, when the symbol has one.
    pub summary: Option<String>,
}

impl SymbolInfo {
    /// The name with its container: `Stack::pop` in Rust, `Cart.total` in
    /// the other languages.
    pub fn qualified(&self) -> String {
        match &self.container {
            Some(c) if self.language == "rust" => format!("{c}::{}", self.name),
            Some(c) => format!("{c}.{}", self.name),
            None => self.name.clone(),
        }
    }

    /// `file:line`.
    pub fn place(&self) -> String {
        format!("{}:{}", self.file, self.start_line)
    }
}

/// What a name gives.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Found(SymbolInfo),
    /// More than one symbol has the name. The agent must give the file.
    Ambiguous(Vec<SymbolInfo>),
    Missing,
}

/// One edge as `callers` or `callees` shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallEdge {
    /// The caller for `callers`, the callee for `callees`. A `possible`
    /// edge of `callees` has one entry for each candidate.
    pub other: SymbolInfo,
    pub class: String,
    pub origin: String,
    /// The file and the line of the call.
    pub file: String,
    pub line: u32,
    /// The count of the candidates of a `possible` edge.
    pub candidates: usize,
    /// A name class that waits for a SCIP run.
    pub scip_pending: bool,
}

/// A call that SCIP resolved to a definition with no symbol: a function that
/// a macro makes (COLLIERY-T-2531). The edge keeps the place of the
/// definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoSymbolCallee {
    /// The called name, as SCIP names it (`boards::transition_task`).
    pub name: String,
    /// The place of the definition.
    pub file: String,
    pub line: u32,
    /// The definition is in the invocation of a macro.
    pub macro_made: bool,
    pub class: String,
    /// The line of the call, in the file of the caller.
    pub call_line: u32,
}

/// One use of a symbol, as `references` shows it (KAIROS-T-0419).
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// The file and the place of the use. The column is from 1.
    pub file: String,
    pub line: u32,
    pub col: u32,
    /// `call`, `type`, `field`, `import` or `other`.
    pub kind: String,
    /// The smallest symbol that holds the use. `None` at the top of a file.
    pub enclosing: Option<SymbolInfo>,
    /// `scip` for a use that SCIP gave; else the origin of the call edge:
    /// `name` or `macro-text`.
    pub origin: String,
    /// `certain`, or `possible` for a call by a name that more than one
    /// function has.
    pub class: String,
    /// The count of the candidates of a `possible` call.
    pub candidates: usize,
    /// A call by name that waits for a SCIP run.
    pub scip_pending: bool,
}

/// What the index has for the uses of a symbol (KAIROS-T-0419).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Each use that SCIP gives: a language with a SCIP index (Rust, Go,
    /// TypeScript, JavaScript, Python, KAIROS-T-0473), after a SCIP run.
    Full,
    /// A language with a SCIP index, but this count of its files changed
    /// after the last SCIP run: they have their calls only.
    Pending(usize),
    /// The calls only: the language has no SCIP run, or the index has no
    /// SCIP data for it.
    CallsOnly,
}

/// One place that cites an ADR (KAIROS-T-0703).
#[derive(Debug, Clone, PartialEq)]
pub struct AdrCitation {
    pub file: String,
    /// From 1.
    pub line: u32,
    /// From 1, in characters.
    pub col: u32,
    /// `comment`, `doc` or `test`.
    pub context: String,
    /// The symbol that the citation is about. `None` at the top of a file.
    pub symbol: Option<SymbolInfo>,
}

/// What `references` gives.
#[derive(Debug, Clone, PartialEq)]
pub struct References {
    pub target: SymbolInfo,
    /// By file, line and column.
    pub uses: Vec<Reference>,
    pub coverage: Coverage,
}

/// What `callees` gives./// What `callees` gives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Callees {
    /// The callees with a symbol.
    pub edges: Vec<CallEdge>,
    /// The callees with no symbol, at the place of their definition.
    pub no_symbol: Vec<NoSymbolCallee>,
    /// The calls to names outside the repository.
    pub externals: Vec<External>,
}

/// A call to a name outside the repository.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct External {
    pub name: String,
    pub line: u32,
}

/// One call of a path.
#[derive(Debug, Clone, PartialEq)]
pub struct PathStep {
    pub caller: SymbolInfo,
    pub callee: SymbolInfo,
    /// The line of the call, in the file of the caller.
    pub line: u32,
    pub scip_pending: bool,
}

/// A folder of source files.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleInfo {
    /// From the root; `.` for the root.
    pub path: String,
    pub summary: Option<String>,
    pub files: Vec<FileInfo>,
    /// The Terraform blocks of the folder, when it has Terraform code
    /// (KAIROS-T-0472).
    pub terraform: Option<TerraformFolder>,
}

/// The Terraform view of a folder (KAIROS-T-0472): the field set of
/// terraform-config-inspect. Terraform reads one folder as one module.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerraformFolder {
    /// The managed resources: (type, names). A name has ` [count]` or
    /// ` [for_each]` when the block has it.
    pub resources: Vec<(String, Vec<String>)>,
    /// The data resources: (type, names), marked as the resources.
    pub data: Vec<(String, Vec<String>)>,
    /// The module calls.
    pub module_calls: Vec<TerraformModuleCall>,
    pub variables: Vec<String>,
    pub outputs: Vec<String>,
    /// The module calls of the repository that call this folder:
    /// `infra (module.store)`.
    pub called_by: Vec<String>,
    /// For a root folder (no module call of the repository calls it): each
    /// chain of module calls from it, `module.store > infra/modules/bucket`.
    pub calls: Vec<String>,
}

/// A `module` block.
#[derive(Debug, Clone, PartialEq)]
pub struct TerraformModuleCall {
    pub name: String,
    pub source: Option<String>,
    pub version: Option<String>,
    /// The folder of a local source, from the root.
    pub folder: Option<String>,
    /// `count` or `for_each`, when the block has it.
    pub marker: Option<String>,
}

impl TerraformFolder {
    /// The lines of the folder in the module map, with no `- `.
    pub fn lines(&self) -> Vec<String> {
        let by_type = |groups: &[(String, Vec<String>)]| {
            groups
                .iter()
                .map(|(ty, names)| format!("{ty} ({})", names.join(", ")))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut out = Vec::new();
        if !self.resources.is_empty() {
            out.push(format!("resources: {}", by_type(&self.resources)));
        }
        if !self.data.is_empty() {
            out.push(format!("data: {}", by_type(&self.data)));
        }
        if !self.module_calls.is_empty() {
            let calls: Vec<String> = self
                .module_calls
                .iter()
                .map(|m| {
                    let mut text = format!(
                        "{} = {}",
                        m.name,
                        m.source.as_deref().unwrap_or("(no source)")
                    );
                    if let Some(version) = &m.version {
                        text.push_str(&format!(" {version}"));
                    }
                    if let Some(marker) = &m.marker {
                        text.push_str(&format!(" [{marker}]"));
                    }
                    text
                })
                .collect();
            out.push(format!("module calls: {}", calls.join(", ")));
        }
        if !self.variables.is_empty() {
            out.push(format!("variables: {}", self.variables.join(", ")));
        }
        if !self.outputs.is_empty() {
            out.push(format!("outputs: {}", self.outputs.join(", ")));
        }
        if !self.called_by.is_empty() {
            out.push(format!("called by: {}", self.called_by.join(", ")));
        }
        if !self.calls.is_empty() {
            out.push(format!("calls: {}", self.calls.join(", ")));
        }
        out
    }
}

/// A Terraform symbol for the module map: its file, kind, container, name
/// and signature.
type TerraformRow = (String, String, Option<String>, String, Option<String>);

/// The Terraform view of each folder with Terraform code, by folder (`.`
/// for the root).
fn terraform_folders(blocks: Vec<TerraformRow>) -> BTreeMap<String, TerraformFolder> {
    // The value of `key = "…"` and the markers in a signature that
    // `extract` wrote: `module "z" { source = "./m"; count }`.
    fn facts(signature: Option<&str>) -> Vec<&str> {
        signature
            .and_then(|s| s.split_once("{ "))
            .and_then(|(_, rest)| rest.strip_suffix(" }"))
            .map(|rest| rest.split("; ").collect())
            .unwrap_or_default()
    }
    fn value<'a>(facts: &[&'a str], key: &str) -> Option<&'a str> {
        facts.iter().find_map(|f| {
            f.strip_prefix(key)?
                .strip_prefix(" = \"")?
                .strip_suffix('"')
        })
    }
    fn marker(facts: &[&str]) -> Option<String> {
        facts
            .iter()
            .find(|f| matches!(**f, "count" | "for_each"))
            .map(|f| f.to_string())
    }
    let mut folders: BTreeMap<String, TerraformFolder> = BTreeMap::new();
    let mut groups: BTreeMap<(String, bool, String), Vec<String>> = BTreeMap::new();
    for (path, kind, container, name, signature) in blocks {
        let folder = match path.rsplit_once('/') {
            Some((folder, _)) => folder.to_string(),
            None => ".".to_string(),
        };
        let facts = facts(signature.as_deref());
        let entry = folders.entry(folder.clone()).or_default();
        let marked = |name: &str| match marker(&facts) {
            Some(m) => format!("{name} [{m}]"),
            None => name.to_string(),
        };
        match kind.as_str() {
            "resource" | "data" => {
                let ty = container.unwrap_or_default();
                let ty = ty.strip_prefix("data.").unwrap_or(&ty).to_string();
                groups
                    .entry((folder, kind == "data", ty))
                    .or_default()
                    .push(marked(&name));
            }
            "module" => {
                let source = value(&facts, "source").map(str::to_string);
                let here = if folder == "." { "" } else { folder.as_str() };
                entry.module_calls.push(TerraformModuleCall {
                    folder: source
                        .as_deref()
                        .and_then(|s| crate::edges::terraform_source_folder(here, s))
                        .map(|f| if f.is_empty() { ".".to_string() } else { f }),
                    source,
                    version: value(&facts, "version").map(str::to_string),
                    marker: marker(&facts),
                    name,
                });
            }
            "variable" => entry.variables.push(name),
            "output" => entry.outputs.push(name),
            _ => {}
        }
    }
    for ((folder, data, ty), names) in groups {
        let entry = folders.entry(folder).or_default();
        if data {
            entry.data.push((ty, names));
        } else {
            entry.resources.push((ty, names));
        }
    }
    // The callers of each folder, and the chains of calls from each root.
    let calls: Vec<(String, TerraformModuleCall)> = folders
        .iter()
        .flat_map(|(f, t)| t.module_calls.iter().map(move |m| (f.clone(), m.clone())))
        .collect();
    for (from, call) in &calls {
        if let Some(entry) = call.folder.as_ref().and_then(|f| folders.get_mut(f)) {
            entry
                .called_by
                .push(format!("{from} (module.{})", call.name));
        }
    }
    fn chains(
        folder: &str,
        folders: &BTreeMap<String, TerraformFolder>,
        prefix: &str,
        seen: &mut Vec<String>,
        out: &mut Vec<String>,
    ) {
        let Some(t) = folders.get(folder) else {
            return;
        };
        for call in &t.module_calls {
            let target = match (&call.folder, &call.source) {
                (Some(f), _) => f.clone(),
                (None, Some(s)) => format!("{s} (remote)"),
                (None, None) => "(no source)".to_string(),
            };
            let chain = format!("{prefix}module.{} > {target}", call.name);
            out.push(chain.clone());
            if let Some(f) = call.folder.as_ref().filter(|f| !seen.contains(f)) {
                seen.push(f.clone());
                chains(f, folders, &format!("{chain} > "), seen, out);
                seen.pop();
            }
        }
    }
    let roots: Vec<String> = folders
        .iter()
        .filter(|(_, t)| t.called_by.is_empty() && !t.module_calls.is_empty())
        .map(|(f, _)| f.clone())
        .collect();
    for root in roots {
        let mut out = Vec::new();
        chains(&root, &folders, "", &mut vec![root.clone()], &mut out);
        if let Some(entry) = folders.get_mut(&root) {
            entry.calls = out;
        }
    }
    folders
}

/// A source file of a module.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub summary: Option<String>,
    pub symbols: usize,
    /// Test code (KAIROS-T-0352). It is summarized too (KAIROS-T-0421).
    pub is_test: bool,
    /// The file has no symbol of a summarized kind (KAIROS-T-0425): only
    /// declarations, such as `mod` and `use` lines. Its summary, if it has
    /// one, comes from these and from its doc comment.
    pub declarations_only: bool,
}

/// The counts of an index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: usize,
    /// Files that the parser read: source and test files of a known language.
    pub parsed_files: usize,
    pub symbols: usize,
    pub edges: usize,
    pub certain: usize,
    pub possible: usize,
    pub external: usize,
    /// Name classes that wait for a SCIP run.
    pub pending: usize,
    /// Symbols, files and modules with a summary.
    pub symbol_summaries: usize,
    pub file_summaries: usize,
    pub module_summaries: usize,
    /// The symbols that can get a summary: functions and types, test code
    /// too (KAIROS-T-0421).
    pub summarizable: usize,
    /// All the summaries in the pool, also those of code that is gone.
    pub pool: usize,
    /// The model of the vectors, as `provider/model/dimension`.
    pub vector_model: Option<String>,
    /// The model of the last summary run, as `provider/model`
    /// (KAIROS-T-0338).
    pub summary_model: Option<String>,
}

impl Counts {
    /// The symbols, files and modules with a summary.
    pub fn summaries(&self) -> usize {
        self.symbol_summaries + self.file_summaries + self.module_summaries
    }
}

/// What a search read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// The text and the vectors of the summaries.
    Vectors,
    /// The text of the summaries only: no provider for the model of the
    /// vectors.
    SummaryText,
    /// The names, the signatures and the paths: the index has no summaries.
    Names,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub symbol: SymbolInfo,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub mode: SearchMode,
    pub hits: Vec<SearchHit>,
}

/// The columns of [`SymbolInfo`], for `s` (symbols), `f` (files) and `m`
/// (summaries).
pub(crate) const SYMBOL_COLUMNS: &str = "s.id, f.path, s.name, s.container, s.kind, s.language, \
     s.start_line, s.end_line, s.signature, s.is_test, m.summary";
pub(crate) const SYMBOL_FROM: &str = "symbols s JOIN files f ON f.id = s.file_id \
     LEFT JOIN summaries m ON m.key = s.summary_key";

pub(crate) fn symbol_info(r: &Row<'_>, at: usize) -> rusqlite::Result<SymbolInfo> {
    Ok(SymbolInfo {
        id: r.get(at)?,
        file: r.get(at + 1)?,
        name: r.get(at + 2)?,
        container: r.get(at + 3)?,
        kind: r.get(at + 4)?,
        language: r.get(at + 5)?,
        start_line: r.get(at + 6)?,
        end_line: r.get(at + 7)?,
        signature: r.get(at + 8)?,
        is_test: r.get(at + 9)?,
        summary: r.get(at + 10)?,
    })
}

impl Index {
    /// The counts of the index.
    pub fn counts(&self) -> Result<Counts, IndexError> {
        let count = |sql: &str| -> Result<usize, IndexError> {
            Ok(self.conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as usize)
        };
        let kinds = SUMMARIZED_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Counts {
            files: count("SELECT count(*) FROM files")?,
            parsed_files: count(
                "SELECT count(*) FROM files WHERE language IS NOT NULL
                   AND decision IN ('source', 'test') AND parse_error IS NULL",
            )?,
            symbols: count("SELECT count(*) FROM symbols")?,
            edges: count("SELECT count(*) FROM edges")?,
            certain: count("SELECT count(*) FROM edges WHERE class = 'certain'")?,
            possible: count("SELECT count(*) FROM edges WHERE class = 'possible'")?,
            external: count("SELECT count(*) FROM edges WHERE class = 'external'")?,
            pending: count("SELECT count(*) FROM edges WHERE scip_pending = 1")?,
            symbol_summaries: count(
                "SELECT count(*) FROM symbols s JOIN summaries m ON m.key = s.summary_key",
            )?,
            file_summaries: count(
                "SELECT count(*) FROM files f JOIN summaries m ON m.key = f.summary_key",
            )?,
            module_summaries: count(
                "SELECT count(*) FROM modules d JOIN summaries m ON m.key = d.summary_key",
            )?,
            summarizable: count(&format!(
                "SELECT count(*) FROM symbols s JOIN files f ON f.id = s.file_id
                 WHERE f.decision IN ('source', 'test') AND s.kind IN ({kinds})"
            ))?,
            pool: count("SELECT count(*) FROM summaries")?,
            vector_model: self.vector_model()?,
            summary_model: self.summary_model()?,
        })
    }

    /// The model of the last summary run of the pool, as `provider/model`
    /// (KAIROS-T-0338).
    pub fn summary_model(&self) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM pool_meta WHERE name = 'summary_model'",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The model of the vectors of the pool, as `provider/model/dimension`.
    pub fn vector_model(&self) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM pool_meta WHERE name = 'vector_model'",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The symbol that `symbol` names: `name`, `Container::name` or
    /// `Container.name`, or a candidate as an ambiguity lists it,
    /// `name (file:line)`. `file` is a path from the root, or its end; it
    /// chooses among symbols of the same name. `line` chooses the symbol that
    /// starts on that line, or else the smallest one that holds it. `choose`
    /// makes the choice among the symbols that are left (KAIROS-T-0417).
    pub fn lookup(
        &self,
        symbol: &str,
        file: Option<&str>,
        line: Option<u32>,
    ) -> Result<Lookup, IndexError> {
        let (symbol, file, line) = match candidate_parts(symbol) {
            Some((name, at_file, at_line)) => {
                (name, file.or(Some(at_file)), line.or(Some(at_line)))
            }
            None => (symbol.trim(), file, line),
        };
        let (container, name) = match symbol.rfind("::").map(|at| (at, 2)).or_else(|| {
            symbol
                .rfind('.')
                .filter(|&at| at > 0 && at + 1 < symbol.len())
                .map(|at| (at, 1))
        }) {
            Some((at, width)) => (Some(&symbol[..at]), &symbol[at + width..]),
            None => (None, symbol),
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SYMBOL_COLUMNS} FROM {SYMBOL_FROM} WHERE s.name = ?1
             ORDER BY f.path, s.start_byte"
        ))?;
        let all: Vec<SymbolInfo> = stmt
            .query_map([name], |r| symbol_info(r, 0))?
            .collect::<Result<_, _>>()?;
        let found: Vec<SymbolInfo> = all
            .into_iter()
            .filter(|s| match container {
                // The last part of the container: `a::Stack` and `Stack` match.
                Some(c) => s.container.as_deref().is_some_and(|own| {
                    own == c || c.ends_with(&format!("::{own}")) || c.ends_with(&format!(".{own}"))
                }),
                None => true,
            })
            .filter(|s| match file {
                Some(f) => s.file == f || s.file.ends_with(&format!("/{f}")),
                None => true,
            })
            .collect();
        Ok(choose(found, line))
    }

    fn symbol_by_id(&self, id: i64) -> Result<SymbolInfo, IndexError> {
        Ok(self.conn.query_row(
            &format!("SELECT {SYMBOL_COLUMNS} FROM {SYMBOL_FROM} WHERE s.id = ?1"),
            [id],
            |r| symbol_info(r, 0),
        )?)
    }

    /// The callers of the symbol `id`: the `certain` edges to it, and with
    /// `possible`, the `possible` edges that have it as a candidate.
    pub fn callers(&self, id: i64, possible: bool) -> Result<Vec<CallEdge>, IndexError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SYMBOL_COLUMNS}, e.class, e.origin, e.line, e.scip_pending,
                    (SELECT count(*) FROM edge_candidates c WHERE c.edge_id = e.id)
             FROM edges e JOIN symbols s ON s.id = e.caller_id
             JOIN files f ON f.id = s.file_id LEFT JOIN summaries m ON m.key = s.summary_key
             WHERE (e.callee_id = ?1 AND e.class = 'certain')
                OR (?2 AND e.class = 'possible'
                    AND e.id IN (SELECT edge_id FROM edge_candidates WHERE symbol_id = ?1))
             ORDER BY e.class, f.path, e.line"
        ))?;
        let rows = stmt.query_map(params![id, possible], |r| {
            let other = symbol_info(r, 0)?;
            Ok(CallEdge {
                file: other.file.clone(),
                other,
                class: r.get(11)?,
                origin: r.get(12)?,
                line: r.get(13)?,
                scip_pending: r.get(14)?,
                candidates: r.get::<_, i64>(15)? as usize,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The callees of the symbol `id`: the `certain` edges from it, and with
    /// `possible`, each candidate of its `possible` edges. The calls to
    /// names outside the repository (`external`) are the second list.
    pub fn callees(&self, id: i64, possible: bool) -> Result<Callees, IndexError> {
        let caller = self.symbol_by_id(id)?;
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.callee_id, e.callee_name, e.class, e.origin, e.line, e.scip_pending,
                    t.path, e.target_line, e.target_macro
             FROM edges e LEFT JOIN files t ON t.id = e.target_file_id
             WHERE e.caller_id = ?1 ORDER BY e.line, e.col",
        )?;
        type Raw = (
            i64,
            Option<i64>,
            String,
            String,
            String,
            u32,
            bool,
            Option<String>,
            Option<u32>,
            bool,
        );
        let raw: Vec<Raw> = stmt
            .query_map([id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut edges = Vec::new();
        let mut no_symbol = Vec::new();
        let mut externals = Vec::new();
        let mut candidates = self
            .conn
            .prepare("SELECT symbol_id FROM edge_candidates WHERE edge_id = ?1")?;
        for (
            edge_id,
            callee_id,
            name,
            class,
            origin,
            line,
            scip_pending,
            target_file,
            target_line,
            target_macro,
        ) in raw
        {
            let edge = |other: SymbolInfo, count: usize| CallEdge {
                other,
                class: class.clone(),
                origin: origin.clone(),
                file: caller.file.clone(),
                line,
                candidates: count,
                scip_pending,
            };
            match (class.as_str(), callee_id) {
                ("certain", Some(callee)) => edges.push(edge(self.symbol_by_id(callee)?, 0)),
                ("certain", None) => {
                    if let (Some(file), Some(at)) = (target_file, target_line) {
                        no_symbol.push(NoSymbolCallee {
                            name,
                            file,
                            line: at,
                            macro_made: target_macro,
                            class: class.clone(),
                            call_line: line,
                        });
                    }
                }
                ("possible", _) if possible => {
                    let ids: Vec<i64> = candidates
                        .query_map([edge_id], |r| r.get(0))?
                        .collect::<Result<_, _>>()?;
                    for c in &ids {
                        edges.push(edge(self.symbol_by_id(*c)?, ids.len()));
                    }
                }
                ("external", _) => externals.push(External { name, line }),
                _ => {}
            }
        }
        Ok(Callees {
            edges,
            no_symbol,
            externals,
        })
    }

    /// The uses of the symbol `id` (KAIROS-T-0419): the SCIP uses of
    /// `refs`, and the calls of the edges that `refs` does not have (by
    /// name, in the text of a macro, or in a Rust file that changed after the
    /// SCIP run). With `possible`, also the `possible` calls that have it as
    /// a candidate. By file, line and column.
    pub fn references(&self, id: i64, possible: bool) -> Result<References, IndexError> {
        let target = self.symbol_by_id(id)?;
        let enclosing_columns = SYMBOL_COLUMNS.replace("f.path", "sf.path");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.path, r.line, r.col, r.use_kind, {enclosing_columns}
             FROM refs r JOIN files f ON f.id = r.file_id
             LEFT JOIN symbols s ON s.id = r.enclosing_id
             LEFT JOIN files sf ON sf.id = s.file_id
             LEFT JOIN summaries m ON m.key = s.summary_key
             WHERE r.target_id = ?1"
        ))?;
        let rows = stmt.query_map([id], |r| {
            Ok(Reference {
                file: r.get(0)?,
                line: r.get(1)?,
                col: r.get(2)?,
                kind: r.get(3)?,
                enclosing: match r.get::<_, Option<i64>>(4)? {
                    Some(_) => Some(symbol_info(r, 4)?),
                    None => None,
                },
                origin: "scip".to_string(),
                class: "certain".to_string(),
                candidates: 0,
                scip_pending: false,
            })
        })?;
        let mut uses: Vec<Reference> = rows.collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SYMBOL_COLUMNS}, e.class, e.origin, e.line, e.col, e.scip_pending,
                    (SELECT count(*) FROM edge_candidates c WHERE c.edge_id = e.id)
             FROM edges e JOIN symbols s ON s.id = e.caller_id
             JOIN files f ON f.id = s.file_id LEFT JOIN summaries m ON m.key = s.summary_key
             WHERE ((e.callee_id = ?1 AND e.class = 'certain')
                OR (?2 AND e.class = 'possible'
                    AND e.id IN (SELECT edge_id FROM edge_candidates WHERE symbol_id = ?1)))
               AND NOT (e.origin = 'scip'
                    AND s.file_id IN (SELECT file_id FROM refs_files WHERE pending = 0))"
        ))?;
        let rows = stmt.query_map(params![id, possible], |r| {
            let caller = symbol_info(r, 0)?;
            Ok(Reference {
                file: caller.file.clone(),
                line: r.get(13)?,
                col: r.get(14)?,
                kind: "call".to_string(),
                enclosing: Some(caller),
                class: r.get(11)?,
                origin: r.get(12)?,
                scip_pending: r.get(15)?,
                candidates: r.get::<_, i64>(16)? as usize,
            })
        })?;
        for row in rows {
            uses.push(row?);
        }
        uses.sort_by(|a, b| (&a.file, a.line, a.col).cmp(&(&b.file, b.line, b.col)));

        // The files of the language family of the target with SCIP uses
        // (KAIROS-T-0473): TypeScript, TSX and JavaScript are one family.
        let one = [target.language.as_str()];
        let family: &[&str] = match target.language.as_str() {
            "typescript" | "tsx" | "javascript" => &["typescript", "tsx", "javascript"],
            _ => &one,
        };
        let count = |pending: bool| -> Result<usize, IndexError> {
            let mut n = 0;
            for language in family {
                n += self.conn.query_row(
                    "SELECT count(*) FROM refs_files r JOIN files f ON f.id = r.file_id
                     WHERE r.pending = ?1 AND f.language = ?2",
                    params![pending, language],
                    |r| r.get::<_, i64>(0),
                )? as usize;
            }
            Ok(n)
        };
        let coverage = if count(false)? == 0 {
            Coverage::CallsOnly
        } else {
            match count(true)? {
                0 => Coverage::Full,
                n => Coverage::Pending(n),
            }
        };
        Ok(References {
            target,
            uses,
            coverage,
        })
    }

    /// The places that cite the ADR `code` (KAIROS-T-0703), by file, line
    /// and column. The code is compared as it is written: `KAIROS-A-0065`.
    pub fn adr_citations(&self, code: &str) -> Result<Vec<AdrCitation>, IndexError> {
        let enclosing_columns = SYMBOL_COLUMNS.replace("f.path", "sf.path");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.path, c.line, c.col, c.context, {enclosing_columns}
             FROM adr_citations c JOIN files f ON f.id = c.file_id
             LEFT JOIN symbols s ON s.id = c.symbol_id
             LEFT JOIN files sf ON sf.id = s.file_id
             LEFT JOIN summaries m ON m.key = s.summary_key
             WHERE c.code = ?1
             ORDER BY f.path, c.line, c.col"
        ))?;
        let rows = stmt.query_map([code], |r| {
            Ok(AdrCitation {
                file: r.get(0)?,
                line: r.get(1)?,
                col: r.get(2)?,
                context: r.get(3)?,
                symbol: match r.get::<_, Option<i64>>(4)? {
                    Some(_) => Some(symbol_info(r, 4)?),
                    None => None,
                },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Each ADR that the code cites (KAIROS-T-0703), with the count of its
    /// places and of its files, by code.
    pub fn cited_adrs(&self) -> Result<Vec<CitedAdr>, IndexError> {
        crate::citations::cited(&self.conn)
    }

    /// The shortest chains of `certain` calls from the symbol `from` to the
    /// symbol `to`, at most `max`, shortest first, each with other steps
    /// (COLLIERY-T-2531). Each chain has at most 16 calls. Empty if no chain
    /// joins them.
    pub fn paths(&self, from: i64, to: i64, max: usize) -> Result<Vec<Vec<PathStep>>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT caller_id, callee_id, line, scip_pending FROM edges
             WHERE class = 'certain' AND callee_id IS NOT NULL
             ORDER BY caller_id, line, col",
        )?;
        let mut next: HashMap<i64, Vec<(i64, u32, bool)>> = HashMap::new();
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get(2)?,
                r.get(3)?,
            ))
        })?;
        for row in rows {
            let (caller, callee, line, pending) = row?;
            let list = next.entry(caller).or_default();
            // The first call of each callee is the step of a chain.
            if !list.iter().any(|(c, _, _)| *c == callee) {
                list.push((callee, line, pending));
            }
        }
        let chains = shortest_chains(&next, from, to, max);
        let mut out = Vec::with_capacity(chains.len());
        for chain in chains {
            let mut steps = Vec::with_capacity(chain.len() - 1);
            for pair in chain.windows(2) {
                let (_, line, scip_pending) = next[&pair[0]]
                    .iter()
                    .find(|(c, _, _)| *c == pair[1])
                    .copied()
                    .expect("a step of a chain is an edge");
                steps.push(PathStep {
                    caller: self.symbol_by_id(pair[0])?,
                    callee: self.symbol_by_id(pair[1])?,
                    line,
                    scip_pending,
                });
            }
            out.push(steps);
        }
        Ok(out)
    }

    /// The folders of the parsed files, with their summaries, by path: the
    /// source files and the test files (KAIROS-T-0352), each with its
    /// summary (KAIROS-T-0421). `under` keeps the folders whose path starts
    /// with it. A file with no symbol of a summarized kind is marked
    /// `declarations_only` (KAIROS-T-0425).
    pub fn module_map(&self, under: Option<&str>) -> Result<Vec<ModuleInfo>, IndexError> {
        let kinds = SUMMARIZED_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.path, m.summary, (SELECT count(*) FROM symbols s WHERE s.file_id = f.id),
                    f.decision = 'test',
                    NOT EXISTS (SELECT 1 FROM symbols s
                                WHERE s.file_id = f.id AND s.kind IN ({kinds}))
             FROM files f LEFT JOIN summaries m ON m.key = f.summary_key
             WHERE f.decision IN ('source', 'test') AND f.language IS NOT NULL
             ORDER BY f.path"
        ))?;
        type Row = (String, Option<String>, i64, bool, bool);
        let files: Vec<Row> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT d.path, m.summary FROM modules d JOIN summaries m ON m.key = d.summary_key",
        )?;
        let summaries: HashMap<String, String> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        // The Terraform blocks of the source and test files (KAIROS-T-0472).
        let mut stmt = self.conn.prepare(
            "SELECT f.path, s.kind, s.container, s.name, s.signature
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.language = 'terraform' AND f.decision IN ('source', 'test')
             ORDER BY f.path, s.start_byte",
        )?;
        let blocks = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut terraform = terraform_folders(blocks);
        let mut modules: BTreeMap<String, Vec<FileInfo>> = BTreeMap::new();
        for (path, summary, symbols, is_test, declarations_only) in files {
            let folder = match path.rsplit_once('/') {
                Some((folder, _)) => folder.to_string(),
                None => ".".to_string(),
            };
            if under.is_some_and(|u| !folder.starts_with(u) && !path.starts_with(u)) {
                continue;
            }
            modules.entry(folder).or_default().push(FileInfo {
                path,
                summary,
                symbols: symbols as usize,
                is_test,
                declarations_only,
            });
        }
        Ok(modules
            .into_iter()
            .map(|(path, files)| ModuleInfo {
                summary: summaries.get(&path).cloned(),
                terraform: terraform.remove(&path),
                path,
                files,
            })
            .collect())
    }

    /// Search the summaries for `query`, at most `limit` symbols.
    ///
    /// The text rank is the BM25 of the FTS5 table `symbol_text` on the
    /// name with its container, the signature and the summary, with the
    /// Porter stemmer; a word of the name counts more (KAIROS-T-0423). The
    /// vector rank is the cosine of the vector of the query against each
    /// summary vector; it needs `embedder` to have the model of
    /// the pool. A symbol with no summary (code that waits for its summary)
    /// is ranked by its name, its signature and its path
    /// (KAIROS-T-0352); with no summary in the index, each symbol is.
    ///
    /// Test code ranks below each other symbol, in each mode, unless the
    /// query is about tests ([`about_tests`], KAIROS-T-0422).
    ///
    /// A type and its impl blocks give one result: the type, with the best
    /// score of the group ([`impl_owners`], KAIROS-T-0424). The methods
    /// of the impl blocks stay results of their own. `limit` counts these
    /// results.
    pub fn search(
        &self,
        query: &str,
        embedder: Option<&dyn EmbeddingProvider>,
        limit: usize,
    ) -> Result<SearchResult, IndexError> {
        let kinds = SUMMARIZED_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SYMBOL_COLUMNS}, m.vector FROM {SYMBOL_FROM}
             WHERE s.kind IN ({kinds}) ORDER BY f.path, s.start_byte"
        ))?;
        let rows: Vec<(SymbolInfo, Option<Vec<u8>>)> = stmt
            .query_map([], |r| Ok((symbol_info(r, 0)?, r.get(11)?)))?
            .collect::<Result<_, _>>()?;
        let summarized = rows.iter().any(|(s, _)| s.summary.is_some());
        let docs = rows;

        let at: HashMap<i64, usize> = docs
            .iter()
            .enumerate()
            .map(|(i, (s, _))| (s.id, i))
            .collect();
        let text_scores: Vec<(usize, f64)> = crate::text::rank(&self.conn, query)?
            .into_iter()
            .filter_map(|(id, score)| Some((*at.get(&id)?, score)))
            .collect();
        let mut fused: HashMap<usize, f64> = HashMap::new();
        let text_ranks = ranked(&text_scores);
        for (rank, (doc, _)) in text_ranks.iter().enumerate() {
            *fused.entry(*doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        }

        let pool_model = self.vector_model()?;
        let embedder =
            embedder.filter(|e| summarized && pool_model.as_deref() == Some(&model_name(*e)));
        let mode = match (summarized, embedder) {
            (false, _) => SearchMode::Names,
            (true, None) => SearchMode::SummaryText,
            (true, Some(_)) => SearchMode::Vectors,
        };
        if let Some(embedder) = embedder {
            let q = embedder
                .embed_one(query)
                .map_err(|e| IndexError::Embed(e.to_string()))?;
            let cosines: Vec<(usize, f64)> = docs
                .iter()
                .enumerate()
                .filter_map(|(i, (_, v))| {
                    let v: Vec<f32> = v
                        .as_ref()?
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    kairos_embed::cosine(&q, &v).map(|c| (i, c as f64))
                })
                .collect();
            for (rank, (doc, _)) in ranked(&cosines).into_iter().enumerate() {
                *fused.entry(doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
            }
            // A symbol with no vector (no summary: code that waits for its
            // summary) is in the text rank only. Its text rank
            // counts for the vector rank too, so a good name match is not
            // below each symbol that is in both ranks (KAIROS-T-0352).
            for (rank, (doc, _)) in text_ranks.iter().enumerate() {
                if docs[*doc].1.is_none() {
                    *fused.entry(*doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
                }
            }
        }

        // A type and its impl blocks are one result (KAIROS-T-0424): the
        // place of the type, the best score of the group. An impl block
        // with no type in the index stays a result of its own.
        let owners = impl_owners(&docs.iter().map(|(s, _)| s).collect::<Vec<_>>());
        let mut folded: HashMap<usize, f64> = HashMap::new();
        for (doc, score) in fused {
            let at = owners.get(&doc).copied().unwrap_or(doc);
            let best = folded.entry(at).or_insert(score);
            *best = best.max(score);
        }

        // Test code ranks below each other symbol (COLLIERY-T-1857,
        // KAIROS-T-0422): the names of test functions and the summaries of
        // tests ("Tests that a policy document is parsed") often have the
        // words of the query. It is a tier, not a factor on the score, so it
        // holds in each mode, with and without summaries. A query about
        // tests ranks all the code by its score, so the search finds a test
        // by what it checks (KAIROS-T-0421).
        let tests_last = !about_tests(query);
        let mut order: Vec<(usize, f64)> = folded.into_iter().collect();
        order.sort_by(|a, b| {
            let (test_a, test_b) = if tests_last {
                (docs[a.0].0.is_test, docs[b.0].0.is_test)
            } else {
                (false, false)
            };
            test_a
                .cmp(&test_b)
                .then(b.1.total_cmp(&a.1))
                .then(a.0.cmp(&b.0))
        });
        let mut docs: Vec<Option<SymbolInfo>> = docs.into_iter().map(|(s, _)| Some(s)).collect();
        let hits = order
            .into_iter()
            .take(limit)
            .filter_map(|(i, score)| {
                Some(SearchHit {
                    symbol: docs[i].take()?,
                    score,
                })
            })
            .collect();
        Ok(SearchResult { mode, hits })
    }
}

/// The shortest chain of symbols from `from` to `to` over `next`, breadth
/// first, that uses no symbol of `banned` and no call of `cut`. At most
/// [`MAX_PATH_DEPTH`] calls.
fn shortest_chain(
    next: &HashMap<i64, Vec<(i64, u32, bool)>>,
    from: i64,
    to: i64,
    banned: &HashSet<i64>,
    cut: &HashSet<(i64, i64)>,
) -> Option<Vec<i64>> {
    let mut came_from: HashMap<i64, i64> = HashMap::new();
    let mut depth: HashMap<i64, usize> = HashMap::from([(from, 0)]);
    let mut queue = VecDeque::from([from]);
    while let Some(at) = queue.pop_front() {
        if at == to {
            break;
        }
        if depth[&at] >= MAX_PATH_DEPTH {
            continue;
        }
        for &(callee, _, _) in next.get(&at).map(Vec::as_slice).unwrap_or_default() {
            if depth.contains_key(&callee)
                || banned.contains(&callee)
                || cut.contains(&(at, callee))
            {
                continue;
            }
            depth.insert(callee, depth[&at] + 1);
            came_from.insert(callee, at);
            queue.push_back(callee);
        }
    }
    if from == to || !came_from.contains_key(&to) {
        return None;
    }
    let mut chain = vec![to];
    let mut at = to;
    while at != from {
        at = came_from[&at];
        chain.push(at);
    }
    chain.reverse();
    Some(chain)
}

/// The `max` shortest chains with other steps from `from` to `to`, shortest
/// first: Yen's algorithm over [`shortest_chain`]. Each chain has no symbol
/// twice and at most [`MAX_PATH_DEPTH`] calls.
fn shortest_chains(
    next: &HashMap<i64, Vec<(i64, u32, bool)>>,
    from: i64,
    to: i64,
    max: usize,
) -> Vec<Vec<i64>> {
    let Some(first) = shortest_chain(next, from, to, &HashSet::new(), &HashSet::new()) else {
        return Vec::new();
    };
    let mut found = vec![first];
    // The candidates, by length and then in the order found.
    let mut candidates: Vec<Vec<i64>> = Vec::new();
    while found.len() < max {
        let last = found.last().expect("a chain").clone();
        for i in 0..last.len() - 1 {
            let root = &last[..=i];
            let cut: HashSet<(i64, i64)> = found
                .iter()
                .filter(|c| c.len() > i + 1 && c[..=i] == *root)
                .map(|c| (c[i], c[i + 1]))
                .collect();
            let banned: HashSet<i64> = root[..i].iter().copied().collect();
            let Some(spur) = shortest_chain(next, last[i], to, &banned, &cut) else {
                continue;
            };
            let mut chain = root[..i].to_vec();
            chain.extend(spur);
            if chain.len() - 1 <= MAX_PATH_DEPTH
                && !found.contains(&chain)
                && !candidates.contains(&chain)
            {
                candidates.push(chain);
            }
        }
        let Some(best) = candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| c.len())
            .map(|(i, _)| i)
        else {
            break;
        };
        found.push(candidates.remove(best));
    }
    found
}

/// The kinds of a type that an impl block folds into (KAIROS-T-0424).
const TYPE_KINDS: &[&str] = &["struct", "enum", "trait", "type_alias"];

/// The type of each impl block of `symbols`, as positions: impl block →
/// type (KAIROS-T-0424). A Rust `impl` block has the name of its type
/// (`impl Shape for Square` is `Square`). The type has the same name, the
/// same container and the same language:
///
/// 1. The one type in the file of the impl block.
/// 2. Else the one type in the index: an impl block can be in a different
///    file from its type.
///
/// With no type (an impl of a type from a different crate) or more than one
/// (two types of one name in different files, and none in the file of the
/// impl block), the impl block has no entry.
pub(crate) fn impl_owners(symbols: &[&SymbolInfo]) -> HashMap<usize, usize> {
    fn key(s: &SymbolInfo) -> (&str, Option<&str>, &str) {
        (&s.name, s.container.as_deref(), &s.language)
    }
    let mut types: HashMap<(&str, Option<&str>, &str), Vec<usize>> = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        if TYPE_KINDS.contains(&s.kind.as_str()) {
            types.entry(key(s)).or_default().push(i);
        }
    }
    let mut owners = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        if s.kind != "implementation" {
            continue;
        }
        let Some(all) = types.get(&key(s)) else {
            continue;
        };
        let here: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&t| symbols[t].file == s.file)
            .collect();
        let owner = match (here.as_slice(), all.as_slice()) {
            ([t], _) | ([], [t]) => Some(*t),
            _ => None,
        };
        if let Some(t) = owner {
            owners.insert(i, t);
        }
    }
    owners
}

/// The documents with a score above 0, best first. Equal scores keep the
/// order of the documents.
fn ranked(scores: &[(usize, f64)]) -> Vec<(usize, f64)> {
    let mut out: Vec<(usize, f64)> = scores.iter().copied().filter(|(_, s)| *s > 0.0).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// `name (file:line)`, the text of a candidate of an ambiguity, as its
/// parts. `None` for any other text.
fn candidate_parts(text: &str) -> Option<(&str, &str, u32)> {
    let inner = text.trim().strip_suffix(')')?;
    let (name, place) = inner.rsplit_once(" (")?;
    let (file, line) = place.rsplit_once(':')?;
    let line = line.parse().ok()?;
    let name = name.trim();
    (!name.is_empty() && !file.is_empty()).then_some((name, file, line))
}

/// The choice among the symbols that match a name (KAIROS-T-0417):
///
/// 1. With `line`, the symbols that start on that line, or else the
///    smallest symbols that hold it.
/// 2. A Rust `impl` block has the name of its type. When the name matches a
///    type and impl blocks, the impl blocks are left out: the agent asks for
///    the type, and `line` chooses an impl block.
/// 3. When only one of the symbols left is a function or a type, that one.
pub(crate) fn choose(found: Vec<SymbolInfo>, line: Option<u32>) -> Lookup {
    let mut found = found;
    if let Some(line) = line {
        found.retain(|s| s.start_line <= line && line <= s.end_line);
        if found.iter().any(|s| s.start_line == line) {
            found.retain(|s| s.start_line == line);
        } else if let Some(least) = found
            .iter()
            .map(|s| s.end_line.saturating_sub(s.start_line))
            .min()
        {
            found.retain(|s| s.end_line.saturating_sub(s.start_line) == least);
        }
    }
    if found.len() > 1
        && found.iter().any(|s| s.kind == "implementation")
        && found.iter().any(|s| s.kind != "implementation")
    {
        found.retain(|s| s.kind != "implementation");
    }
    match found.len() {
        0 => Lookup::Missing,
        1 => Lookup::Found(found.into_iter().next().expect("one")),
        _ => {
            let typed: Vec<&SymbolInfo> = found
                .iter()
                .filter(|s| SUMMARIZED_KINDS.contains(&s.kind.as_str()))
                .collect();
            if typed.len() == 1 {
                Lookup::Found(typed[0].clone())
            } else {
                Lookup::Ambiguous(found)
            }
        }
    }
}

/// The source text of a symbol, from the file in the checkout
/// (KAIROS-T-0420).
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolSource {
    /// The text of the symbol, cut at the limit. A cut keeps whole lines
    /// when it can.
    pub text: String,
    /// The length of the symbol in bytes, as the index has it.
    pub bytes: usize,
    /// True when `text` has fewer bytes than the symbol.
    pub cut: bool,
    /// True when the file is not the file that the index read: the text
    /// can be different from the symbol.
    pub changed: bool,
}

impl Index {
    /// The source text of `symbol`: its byte range in its file under `root`,
    /// at most `limit` bytes. `None` when the file is not in the checkout.
    /// The hash of the file is compared with the hash that the index keeps,
    /// so a file that changed after the index is marked.
    pub fn source(
        &self,
        root: &std::path::Path,
        symbol: &SymbolInfo,
        limit: usize,
    ) -> Result<Option<SymbolSource>, IndexError> {
        let (start, end, hash): (i64, i64, String) = self.conn.query_row(
            "SELECT s.start_byte, s.end_byte, f.content_hash
             FROM symbols s JOIN files f ON f.id = s.file_id WHERE s.id = ?1",
            params![symbol.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let Some(file) = crate::read_indexed_bytes(&root.join(&symbol.file))? else {
            return Ok(None);
        };
        let changed = crate::content_hash(&file) != hash;
        let bytes = (end - start).max(0) as usize;
        let start = (start.max(0) as usize).min(file.len());
        let end = (end.max(0) as usize).clamp(start, file.len());
        let mut range = &file[start..end];
        let cut = range.len() > limit;
        if cut {
            range = &range[..limit];
            // Whole lines: stop after the last line end, when there is one.
            if let Some(last) = range.iter().rposition(|b| *b == b'\n') {
                range = &range[..=last];
            }
        }
        Ok(Some(SymbolSource {
            text: String::from_utf8_lossy(range).into_owned(),
            bytes,
            cut,
            changed,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0472: the chains of module calls from a root folder, past
    /// a nested call, with no loop.
    #[test]
    fn a_root_terraform_folder_has_its_chains_of_calls() {
        let row = |path: &str, kind: &str, container: Option<&str>, name: &str, sig: &str| {
            (
                path.to_string(),
                kind.to_string(),
                container.map(str::to_string),
                name.to_string(),
                Some(sig.to_string()),
            )
        };
        let folders = terraform_folders(vec![
            row(
                "main.tf",
                "module",
                Some("module"),
                "app",
                "module \"app\" { source = \"./app\"; for_each }",
            ),
            row(
                "app/main.tf",
                "module",
                Some("module"),
                "db",
                "module \"db\" { source = \"../db\" }",
            ),
            row(
                "db/main.tf",
                "module",
                Some("module"),
                "back",
                "module \"back\" { source = \"../app\" }",
            ),
            row(
                "db/main.tf",
                "data",
                Some("data.aws_ami"),
                "ubuntu",
                "data \"aws_ami\" \"ubuntu\" {",
            ),
        ]);
        let root = &folders["."];
        assert_eq!(
            root.calls,
            [
                "module.app > app",
                "module.app > app > module.db > db",
                "module.app > app > module.db > db > module.back > app",
            ]
        );
        assert_eq!(root.lines()[0], "module calls: app = ./app [for_each]");
        assert!(folders["app"].calls.is_empty());
        assert_eq!(
            folders["app"].called_by,
            [". (module.app)", "db (module.back)"]
        );
        assert_eq!(folders["db"].lines()[0], "data: aws_ami (ubuntu)");
    }

    /// KAIROS-T-0420: the source of a symbol, cut at the limit, and marked
    /// when the file changed after the index.
    #[test]
    fn the_source_of_a_symbol_comes_from_the_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let text = "pub fn f() -> u32 {\n    1\n}\n\npub fn g() {}\n";
        std::fs::write(repo.join("src/lib.rs"), text).unwrap();
        let db = dir.path().join("index.sqlite");
        crate::build_structure(&repo, &db).unwrap();
        let index = Index::open(&db).unwrap();
        let Lookup::Found(f) = index.lookup("f", None, None).unwrap() else {
            panic!("no f");
        };

        let all = index.source(&repo, &f, 1000).unwrap().unwrap();
        assert_eq!(all.text, "pub fn f() -> u32 {\n    1\n}");
        assert_eq!(all.bytes, all.text.len());
        assert!(!all.cut && !all.changed);

        // A cut keeps whole lines.
        let cut = index.source(&repo, &f, 24).unwrap().unwrap();
        assert_eq!(cut.text, "pub fn f() -> u32 {\n");
        assert!(cut.cut);
        assert_eq!(cut.bytes, all.bytes);

        std::fs::write(repo.join("src/lib.rs"), format!("// new\n{text}")).unwrap();
        let changed = index.source(&repo, &f, 1000).unwrap().unwrap();
        assert!(changed.changed);
        assert_ne!(changed.text, all.text);

        std::fs::remove_file(repo.join("src/lib.rs")).unwrap();
        assert_eq!(index.source(&repo, &f, 1000).unwrap(), None);
    }

    #[test]
    fn shortest_chains_are_distinct_and_shortest_first() {
        // 1 -> 2 -> 5, 1 -> 3 -> 4 -> 5, 1 -> 3 -> 5, and 2 -> 3.
        let mut next: HashMap<i64, Vec<(i64, u32, bool)>> = HashMap::new();
        for (a, b) in [(1, 2), (1, 3), (2, 5), (2, 3), (3, 4), (3, 5), (4, 5)] {
            next.entry(a).or_default().push((b, 1, false));
        }
        let chains = shortest_chains(&next, 1, 5, 3);
        assert_eq!(chains, [vec![1, 2, 5], vec![1, 3, 5], vec![1, 2, 3, 5]]);
        assert_eq!(shortest_chains(&next, 1, 5, 1), [vec![1, 2, 5]]);
        assert!(shortest_chains(&next, 5, 1, 3).is_empty());
    }

    fn sym(id: i64, file: &str, kind: &str, lines: (u32, u32)) -> SymbolInfo {
        SymbolInfo {
            id,
            file: file.into(),
            name: "ContentHash".into(),
            container: None,
            kind: kind.into(),
            language: "rust".into(),
            start_line: lines.0,
            end_line: lines.1,
            signature: None,
            is_test: false,
            summary: None,
        }
    }

    fn found_id(lookup: Lookup) -> i64 {
        match lookup {
            Lookup::Found(s) => s.id,
            other => panic!("not one symbol: {other:?}"),
        }
    }

    /// The struct and its 2 impl blocks in one file (KAIROS-T-0417).
    fn struct_and_impls() -> Vec<SymbolInfo> {
        vec![
            sym(1, "src/document.rs", "struct", (121, 124)),
            sym(2, "src/document.rs", "implementation", (126, 160)),
            sym(3, "src/document.rs", "implementation", (168, 175)),
        ]
    }

    #[test]
    fn a_type_is_chosen_over_its_impl_blocks() {
        assert_eq!(found_id(choose(struct_and_impls(), None)), 1);
    }

    #[test]
    fn a_line_chooses_one_symbol_of_a_file() {
        assert_eq!(found_id(choose(struct_and_impls(), Some(126))), 2);
        assert_eq!(found_id(choose(struct_and_impls(), Some(168))), 3);
        // A line inside a symbol chooses the smallest symbol that holds it.
        assert_eq!(found_id(choose(struct_and_impls(), Some(130))), 2);
        assert_eq!(choose(struct_and_impls(), Some(165)), Lookup::Missing);
    }

    #[test]
    fn impl_blocks_with_no_type_stay_ambiguous() {
        let impls = struct_and_impls().split_off(1);
        match choose(impls, None) {
            Lookup::Ambiguous(all) => assert_eq!(all.len(), 2),
            other => panic!("not ambiguous: {other:?}"),
        }
    }

    /// `sym` with a name and a container.
    fn named(id: i64, file: &str, name: &str, container: Option<&str>, kind: &str) -> SymbolInfo {
        SymbolInfo {
            name: name.into(),
            container: container.map(Into::into),
            ..sym(id, file, kind, (1, 1))
        }
    }

    fn owners_of(all: &[SymbolInfo]) -> Vec<(i64, i64)> {
        let refs: Vec<&SymbolInfo> = all.iter().collect();
        let mut out: Vec<(i64, i64)> = impl_owners(&refs)
            .into_iter()
            .map(|(i, t)| (all[i].id, all[t].id))
            .collect();
        out.sort();
        out
    }

    /// KAIROS-T-0424: the impl blocks of a type fold into it.
    #[test]
    fn impl_blocks_fold_into_their_type() {
        assert_eq!(owners_of(&struct_and_impls()), [(2, 1), (3, 1)]);
    }

    #[test]
    fn two_types_of_one_name_keep_their_own_impl_blocks() {
        let all = vec![
            named(1, "src/checksum.rs", "Checksum", None, "struct"),
            named(2, "src/checksum.rs", "Checksum", None, "implementation"),
            named(3, "src/legacy.rs", "Checksum", None, "struct"),
            named(4, "src/legacy.rs", "Checksum", None, "implementation"),
            // An impl block in a third file: two types, none in its file.
            named(5, "src/other.rs", "Checksum", None, "implementation"),
        ];
        assert_eq!(owners_of(&all), [(2, 1), (4, 3)]);
    }

    #[test]
    fn an_impl_block_in_a_different_file_folds_into_the_one_type() {
        let all = vec![
            named(1, "src/a.rs", "Policy", None, "struct"),
            named(2, "src/b.rs", "Policy", None, "implementation"),
        ];
        assert_eq!(owners_of(&all), [(2, 1)]);
    }

    #[test]
    fn an_impl_block_with_no_type_and_a_method_do_not_fold() {
        let all = vec![
            named(1, "src/a.rs", "Policy", None, "struct"),
            // `impl Display for String`: a type from a different crate.
            named(2, "src/a.rs", "String", None, "implementation"),
            // A method has the type as its container, not as its name.
            named(3, "src/a.rs", "load", Some("Policy"), "method"),
            // A type and an impl block in two different modules of one file.
            named(4, "src/a.rs", "Inner", Some("one"), "struct"),
            named(5, "src/a.rs", "Inner", Some("two"), "implementation"),
            // A function of the name of a type is not a type.
            named(6, "src/a.rs", "make", None, "function"),
            named(7, "src/a.rs", "make", None, "implementation"),
        ];
        assert!(owners_of(&all).is_empty(), "{:?}", owners_of(&all));
    }

    #[test]
    fn two_types_of_one_name_stay_ambiguous() {
        let all = vec![
            sym(1, "src/a.rs", "struct", (1, 3)),
            sym(2, "src/a.rs", "implementation", (5, 9)),
            sym(3, "src/b.rs", "struct", (1, 3)),
        ];
        match choose(all, None) {
            Lookup::Ambiguous(left) => {
                assert_eq!(left.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 3])
            }
            other => panic!("not ambiguous: {other:?}"),
        }
    }

    #[test]
    fn a_query_with_a_test_word_is_about_tests() {
        for query in [
            "how is parsing tested",
            "the tests of the parser",
            "Testing the policy parser",
            "which fixture has a sample policy",
            "mock the clock",
            "where do we assert the rules",
            "test_parse_document",
            "the spec of validate",
        ] {
            assert!(about_tests(query), "{query}");
        }
        for query in [
            "how are policy documents defined, parsed and validated",
            "latest contest results",
            "attest the signature",
            "",
        ] {
            assert!(!about_tests(query), "{query}");
        }
    }

    #[test]
    fn the_text_of_a_candidate_has_its_parts() {
        assert_eq!(
            candidate_parts("ContentHash (crates/a/src/document.rs:121)"),
            Some(("ContentHash", "crates/a/src/document.rs", 121))
        );
        assert_eq!(
            candidate_parts("Stack::pop (src/stack.rs:20)"),
            Some(("Stack::pop", "src/stack.rs", 20))
        );
        assert_eq!(candidate_parts("ContentHash"), None);
        assert_eq!(candidate_parts("f (x)"), None);
        assert_eq!(candidate_parts(" (src/a.rs:1)"), None);
    }
}
