//! The reads of an index that the code tools give to an agent
//! (COLLIERY-I-0264, "What an agent gets"; COLLIERY-T-1852).
//!
//! - [`Index::search`]: a search of the summaries. A text rank and a vector
//!   rank are fused by reciprocal rank, as the search of the Kairos items
//!   does (`kairos-core::retrieval`): a cosine and a text score have no common
//!   scale, but 2 ranks do. With no summaries, the search reads the names,
//!   the signatures and the paths. Test code ranks below the other
//!   symbols (COLLIERY-T-1857).
//! - [`Index::lookup`]: a symbol from the name that an agent gives.
//! - [`Index::callers`] and [`Index::callees`]: `certain` edges, and the
//!   `possible` ones on request.
//! - [`Index::paths`]: the 3 shortest chains of `certain` calls from one
//!   symbol to another, with other steps (COLLIERY-T-2531).
//! - [`Index::module_map`]: the folders of source files with their
//!   summaries. It replaces the Metis index.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use kairos_embed::EmbeddingProvider;
use rusqlite::{OptionalExtension, Row, params};

use crate::summary::model_name;
use crate::{Index, IndexError, SUMMARIZED_KINDS};

/// The constant of reciprocal-rank fusion: a rank `r` scores `1 / (K + r)`.
/// 60 is the value of the paper and of the Kairos item search.
const RRF_K: f64 = 60.0;

/// BM25 constants.
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// The most edges that a search for a path follows from one symbol.
const MAX_PATH_DEPTH: usize = 16;

/// Words that tell nothing about code.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "does", "for", "from", "how", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "this", "to", "what", "when", "where", "which",
    "who", "with",
];

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

/// What `callees` gives.
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
}

/// A source file of a module.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub summary: Option<String>,
    pub symbols: usize,
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
    /// The symbols that can get a summary: functions and types, not test code.
    pub summarizable: usize,
    /// All the summaries in the pool, also those of code that is gone.
    pub pool: usize,
    /// The model of the vectors, as `provider/model/dimension`.
    pub vector_model: Option<String>,
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
                 WHERE s.is_test = 0 AND f.decision = 'source' AND s.kind IN ({kinds})"
            ))?,
            pool: count("SELECT count(*) FROM summaries")?,
            vector_model: self.vector_model()?,
        })
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
    /// `Container.name`. `file` is a path from the root, or its end; it
    /// chooses among symbols of the same name. When more than one symbol
    /// matches and only one of them is a function or a type, that one is
    /// found (a Rust struct and its `impl` block have the same name).
    pub fn lookup(&self, symbol: &str, file: Option<&str>) -> Result<Lookup, IndexError> {
        let symbol = symbol.trim();
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
        Ok(match found.len() {
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
        })
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

    /// The folders of the parsed source files, with their summaries, by
    /// path. `under` keeps the folders whose path starts with it.
    pub fn module_map(&self, under: Option<&str>) -> Result<Vec<ModuleInfo>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, m.summary, (SELECT count(*) FROM symbols s WHERE s.file_id = f.id)
             FROM files f LEFT JOIN summaries m ON m.key = f.summary_key
             WHERE f.decision = 'source' AND f.language IS NOT NULL
             ORDER BY f.path",
        )?;
        let files: Vec<(String, Option<String>, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT d.path, m.summary FROM modules d JOIN summaries m ON m.key = d.summary_key",
        )?;
        let summaries: HashMap<String, String> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut modules: BTreeMap<String, Vec<FileInfo>> = BTreeMap::new();
        for (path, summary, symbols) in files {
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
            });
        }
        Ok(modules
            .into_iter()
            .map(|(path, files)| ModuleInfo {
                summary: summaries.get(&path).cloned(),
                path,
                files,
            })
            .collect())
    }

    /// Search the summaries for `query`, at most `limit` symbols.
    ///
    /// The text rank is BM25 on the name, the container, the signature and
    /// the summary. The vector rank is the cosine of the vector of the query
    /// against each summary vector; it needs `embedder` to have the model of
    /// the pool. With no summary in the index, the search ranks the names,
    /// the signatures and the paths of the functions and types.
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
        let docs: Vec<(SymbolInfo, Option<Vec<u8>>)> = if summarized {
            rows.into_iter()
                .filter(|(s, _)| s.summary.is_some())
                .collect()
        } else {
            rows
        };

        let texts: Vec<Vec<String>> = docs
            .iter()
            .map(|(s, _)| {
                let mut text = format!(
                    "{} {} {}",
                    s.qualified(),
                    s.signature.as_deref().unwrap_or(""),
                    s.summary.as_deref().unwrap_or("")
                );
                if !summarized {
                    text.push(' ');
                    text.push_str(&s.file);
                }
                words(&text)
            })
            .collect();
        let text_scores = bm25(&words(query), &texts);
        let mut fused: HashMap<usize, f64> = HashMap::new();
        for (rank, (doc, _)) in ranked(&text_scores).into_iter().enumerate() {
            *fused.entry(doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
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
        }

        // Test code ranks below each other symbol (COLLIERY-T-1857): with no
        // summaries, the names of test functions often have the words of the
        // query.
        let mut order: Vec<(usize, f64)> = fused.into_iter().collect();
        order.sort_by(|a, b| {
            let (test_a, test_b) = (docs[a.0].0.is_test, docs[b.0].0.is_test);
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

/// The documents with a score above 0, best first. Equal scores keep the
/// order of the documents.
fn ranked(scores: &[(usize, f64)]) -> Vec<(usize, f64)> {
    let mut out: Vec<(usize, f64)> = scores.iter().copied().filter(|(_, s)| *s > 0.0).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// BM25 of `query` against each document, as (document, score).
fn bm25(query: &[String], docs: &[Vec<String>]) -> Vec<(usize, f64)> {
    if docs.is_empty() {
        return Vec::new();
    }
    let n = docs.len() as f64;
    let average = docs.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let terms: HashSet<&String> = query.iter().collect();
    let mut frequency: HashMap<&String, f64> = HashMap::new();
    for doc in docs {
        let present: HashSet<&String> = doc.iter().filter(|w| terms.contains(w)).collect();
        for term in present {
            *frequency.entry(term).or_default() += 1.0;
        }
    }
    docs.iter()
        .enumerate()
        .map(|(i, doc)| {
            let length = doc.len() as f64;
            let score = terms
                .iter()
                .map(|term| {
                    let tf = doc.iter().filter(|w| w == term).count() as f64;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let df = frequency.get(term).copied().unwrap_or(0.0);
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    idf * tf * (BM25_K1 + 1.0)
                        / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * length / average.max(1.0)))
                })
                .sum();
            (i, score)
        })
        .collect()
}

/// The words of a text for the text rank: lower case, with each identifier
/// also cut at `_`, `::` and at a change to a capital (`enqueue_all` gives
/// `enqueue_all`, `enqueue` and `all`). Stop words and single letters go.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if token.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut previous_lower = false;
        for c in token.chars() {
            if c == '_' {
                parts.push(std::mem::take(&mut current));
                previous_lower = false;
                continue;
            }
            if c.is_uppercase() && previous_lower {
                parts.push(std::mem::take(&mut current));
            }
            previous_lower = c.is_lowercase() || c.is_ascii_digit();
            current.extend(c.to_lowercase());
        }
        parts.push(current);
        let parts: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let whole = token.to_lowercase();
        if parts.len() > 1 {
            out.push(whole);
        }
        out.extend(parts);
    }
    out.retain(|w| w.chars().count() > 1 && !STOP_WORDS.contains(&w.as_str()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_cut_identifiers() {
        assert_eq!(
            words("enqueue_all(items) -> Queue: the HttpClient"),
            [
                "enqueue_all",
                "enqueue",
                "all",
                "items",
                "queue",
                "httpclient",
                "http",
                "client"
            ]
        );
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

    #[test]
    fn bm25_ranks_the_document_with_the_rare_word_first() {
        let docs: Vec<Vec<String>> = ["adds a number to the stack", "removes the newest number"]
            .iter()
            .map(|d| words(d))
            .collect();
        let scores = bm25(&words("newest number"), &docs);
        let order: Vec<usize> = ranked(&scores).into_iter().map(|(i, _)| i).collect();
        assert_eq!(order, [1, 0]);
        assert!(bm25(&words("queue"), &docs).iter().all(|(_, s)| *s == 0.0));
    }
}
