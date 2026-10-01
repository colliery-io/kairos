//! The call edges (COLLIERY-I-0264, "The call graph").
//!
//! - **Rust, from SCIP:** a reference to a function from inside the body of
//!   another function is an edge. The definition of the function is joined
//!   to the narsil symbol on the file and the span. A function of an indexed
//!   file is `certain`; a function of std, of a dependency or of a file that
//!   the index does not parse is `external`.
//! - **Python, TypeScript, Go, and the Rust calls that SCIP does not
//!   resolve:** the called name gives the class. `certain`: one function of
//!   that name in the same file, or one in the repository. `possible`: more
//!   than one; the edge lists them. `external`: none.
//! - **Calls in the text of a Rust macro** (`macro-text`): a name class,
//!   only where SCIP resolved nothing (COLLIERY-T-1851).
//! - **A local update** runs no SCIP. Each Rust function whose code did not
//!   change keeps the SCIP edges of the base index, also in a changed file
//!   and when its lines moved. A changed function gets name classes, marked
//!   `scip_pending`, until a run with the Rust edges (COLLIERY-T-1851).

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::calls::CallSite;
use crate::scip::{self, proto};
use crate::{EdgeStats, IndexError};

/// A symbol of a parsed file, with its row id.
pub struct Sym {
    pub id: i64,
    pub name: String,
    pub container: Option<String>,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub tree_hash: String,
}

/// A parsed file, as the edges need it.
pub struct ParsedFile<'a> {
    pub path: &'a str,
    pub language: &'static str,
    pub text: &'a str,
    pub symbols: Vec<Sym>,
    pub calls: &'a [CallSite],
    pub use_ranges: &'a [(usize, usize)],
    pub macro_calls: &'a [CallSite],
}

/// An edge, ready to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEdge {
    pub caller_id: i64,
    pub callee_id: Option<i64>,
    pub callee_name: String,
    pub class: &'static str,
    pub origin: &'static str,
    pub line: usize,
    pub col: usize,
    pub candidates: Vec<i64>,
    pub scip_pending: bool,
}

/// Where the Rust edges come from.
pub enum RustEdges<'a> {
    /// Name classes only: the tree has no Cargo workspace at its root.
    Names,
    /// A SCIP run of the workspace, and the crate of each file.
    Scip(&'a proto::Index, &'a scip::Crates),
    /// No SCIP run. A Rust function with the code of a function of a base
    /// index, whose edges wait for no SCIP run, keeps its SCIP edges. The
    /// other Rust functions get name classes, marked `scip_pending` if
    /// `mark`.
    Base { bases: &'a [BaseIndex], mark: bool },
}

/// The edges of a build.
pub struct Resolved {
    pub edges: Vec<NewEdge>,
    pub stats: EdgeStats,
    /// The call sites that SCIP resolved, as (file, byte), for `scip_covered`.
    pub covered: Vec<(usize, usize)>,
}

/// A symbol across 2 builds: its file, its container, its name, its kind,
/// and its place among the symbols of that file with the same 3. Its lines
/// can change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymKey {
    pub file: String,
    pub container: Option<String>,
    pub name: String,
    pub kind: String,
    pub ordinal: usize,
}

/// The keys of the symbols of one file, in the order of the file.
fn sym_keys<'a>(
    file: &str,
    symbols: impl Iterator<Item = (Option<&'a str>, &'a str, &'a str)>,
) -> Vec<SymKey> {
    let mut seen: HashMap<(Option<&str>, &str, &str), usize> = HashMap::new();
    symbols
        .map(|(container, name, kind)| {
            let n = seen.entry((container, name, kind)).or_default();
            let key = SymKey {
                file: file.to_string(),
                container: container.map(str::to_string),
                name: name.to_string(),
                kind: kind.to_string(),
                ordinal: *n,
            };
            *n += 1;
            key
        })
        .collect()
}

/// The Rust edges of an earlier index, for an update that runs no SCIP.
#[derive(Debug, Default)]
pub struct BaseIndex {
    files: HashMap<String, BaseFile>,
}

#[derive(Debug, Default)]
struct BaseFile {
    /// The call sites that SCIP resolved, by byte.
    covered: Vec<usize>,
    symbols: HashMap<SymKey, BaseSym>,
}

/// A symbol of a base index, with its SCIP edges as the caller.
#[derive(Debug)]
struct BaseSym {
    tree_hash: String,
    start_line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
    /// An edge of the symbol waits for a SCIP run.
    pending: bool,
    edges: Vec<BaseEdge>,
}

#[derive(Debug)]
struct BaseEdge {
    callee: Option<SymKey>,
    callee_name: String,
    class: String,
    line: usize,
    col: usize,
}

impl BaseIndex {
    /// Read the Rust files of the index at `conn`: their symbols, their SCIP
    /// edges and the call sites that SCIP resolved.
    pub fn read(conn: &Connection) -> Result<Self, IndexError> {
        let mut files: HashMap<String, BaseFile> = HashMap::new();
        let mut paths: HashMap<i64, String> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT id, path FROM files WHERE language = 'rust'")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, path) = row?;
                files.insert(path.clone(), BaseFile::default());
                paths.insert(id, path);
            }
        }
        // The key of each symbol, from the symbols of each file in order.
        let mut keys: HashMap<i64, SymKey> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT s.id, f.path, s.container, s.name, s.kind, s.tree_hash, s.start_line,
                        s.end_line, s.start_byte, s.end_byte
                 FROM symbols s JOIN files f ON f.id = s.file_id
                 ORDER BY f.path, s.start_byte, s.end_byte DESC, s.kind, s.name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    BaseSym {
                        tree_hash: r.get(5)?,
                        start_line: r.get::<_, i64>(6)? as usize,
                        end_line: r.get::<_, i64>(7)? as usize,
                        start_byte: r.get::<_, i64>(8)? as usize,
                        end_byte: r.get::<_, i64>(9)? as usize,
                        pending: false,
                        edges: Vec::new(),
                    },
                ))
            })?;
            let rows: Vec<_> = rows.collect::<Result<_, _>>()?;
            for group in rows.chunk_by(|a, b| a.1 == b.1) {
                let file_keys = sym_keys(
                    &group[0].1,
                    group
                        .iter()
                        .map(|(_, _, c, n, k, _)| (c.as_deref(), n.as_str(), k.as_str())),
                );
                keys.extend(group.iter().map(|row| row.0).zip(file_keys));
            }
            for (id, path, _, _, _, sym) in rows {
                if let (Some(file), Some(key)) = (files.get_mut(&path), keys.get(&id)) {
                    file.symbols.insert(key.clone(), sym);
                }
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT e.caller_id, e.callee_id, e.callee_name, e.class, e.origin, e.line, e.col,
                        e.scip_pending
                 FROM edges e JOIN symbols c ON c.id = e.caller_id
                 WHERE c.language = 'rust' AND (e.origin = 'scip' OR e.scip_pending = 1)",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })?;
            for row in rows {
                let (caller, callee, callee_name, class, origin, line, col, pending) = row?;
                let Some(caller) = keys.get(&caller) else {
                    continue;
                };
                let Some(sym) = files
                    .get_mut(&caller.file)
                    .and_then(|f| f.symbols.get_mut(caller))
                else {
                    continue;
                };
                sym.pending |= pending;
                if origin == "scip" {
                    sym.edges.push(BaseEdge {
                        callee: callee.and_then(|id| keys.get(&id).cloned()),
                        callee_name,
                        class,
                        line: line as usize,
                        col: col as usize,
                    });
                }
            }
        }
        {
            let mut stmt = conn.prepare("SELECT file_id, start_byte FROM scip_covered")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (file_id, byte) = row?;
                if let Some(file) = paths.get(&file_id).and_then(|p| files.get_mut(p)) {
                    file.covered.push(byte as usize);
                }
            }
        }
        Ok(BaseIndex { files })
    }
}

/// Whether the function `s` of `file` has the code of the base symbol `b`,
/// so that it keeps the SCIP edges of `b`: the same tree, the same length in
/// bytes and lines, no edge that waits for SCIP, and the start of a name at
/// the place of each edge after the move of `s`. The name can be other than
/// the SCIP name: `use a::f as g` calls `f` as `g`.
fn same_code(b: &BaseSym, s: &Sym, file: &ParsedFile<'_>, line_starts: &[usize]) -> bool {
    if b.pending
        || b.tree_hash != s.tree_hash
        || b.end_byte - b.start_byte != s.end_byte - s.start_byte
        || b.end_line - b.start_line != s.end_line - s.start_line
    {
        return false;
    }
    let is_name = |c: char| c.is_alphanumeric() || c == '_';
    b.edges.iter().all(|e| {
        let line = (e.line + s.start_line).checked_sub(b.start_line + 1);
        let at = line.and_then(|line| {
            byte_offset(
                line_starts,
                file.text,
                i32::try_from(line).ok()?,
                i32::try_from(e.col.checked_sub(1)?).ok()?,
                // The column of an edge is as rust-analyzer writes it.
                proto::ENCODING_UTF8,
            )
        });
        at.is_some_and(|at| {
            s.start_byte <= at
                && at < s.end_byte
                && file.text[at..].starts_with(is_name)
                && !file.text[..at].ends_with(is_name)
        })
    })
}

/// The kinds that a call can start in.
fn is_function(kind: &str) -> bool {
    matches!(kind, "function" | "method" | "constructor")
}

/// The kinds that a name can call: in Python and TypeScript, a call of a
/// class makes an object.
fn is_callable(language: &str, kind: &str) -> bool {
    is_function(kind) || (kind == "class" && matches!(language, "python" | "typescript" | "tsx"))
}

/// The languages whose names can call each other.
fn family(language: &str) -> &str {
    match language {
        "tsx" => "typescript",
        other => other,
    }
}

/// The smallest function of `file` whose span holds `byte`.
fn caller_at(file: &ParsedFile<'_>, byte: usize) -> Option<i64> {
    file.symbols
        .iter()
        .filter(|s| is_function(s.kind) && s.start_byte <= byte && byte < s.end_byte)
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| s.id)
}

struct Names<'a> {
    /// (language family, name) to (file index, symbol id).
    by_name: HashMap<(&'a str, &'a str), Vec<(usize, i64)>>,
}

impl<'a> Names<'a> {
    fn new(files: &'a [ParsedFile<'a>]) -> Self {
        let mut by_name: HashMap<(&str, &str), Vec<(usize, i64)>> = HashMap::new();
        for (i, file) in files.iter().enumerate() {
            for s in &file.symbols {
                if is_callable(file.language, s.kind) {
                    by_name
                        .entry((family(file.language), s.name.as_str()))
                        .or_default()
                        .push((i, s.id));
                }
            }
        }
        Names { by_name }
    }

    /// The name class of a call of `name` from file `file`.
    #[allow(clippy::too_many_arguments)]
    fn edge(
        &self,
        language: &str,
        file: usize,
        caller_id: i64,
        name: &str,
        written: &str,
        line: usize,
        col: usize,
        stats: &mut EdgeStats,
    ) -> NewEdge {
        let all = self
            .by_name
            .get(&(family(language), name))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let same_file: Vec<i64> = all
            .iter()
            .filter(|(f, _)| *f == file)
            .map(|(_, id)| *id)
            .collect();
        let chosen: Vec<i64> = if same_file.is_empty() {
            all.iter().map(|(_, id)| *id).collect()
        } else {
            same_file
        };
        let (class, callee_id, candidates) = match chosen.as_slice() {
            [] => {
                stats.external_name += 1;
                ("external", None, Vec::new())
            }
            [one] => {
                stats.certain_name += 1;
                ("certain", Some(*one), Vec::new())
            }
            many => {
                stats.possible_name += 1;
                ("possible", None, many.to_vec())
            }
        };
        NewEdge {
            caller_id,
            callee_id,
            callee_name: written.to_string(),
            class,
            origin: "name",
            line,
            col,
            candidates,
            scip_pending: false,
        }
    }
}

/// The byte offset of a SCIP position (line and column from 0).
fn byte_offset(
    line_starts: &[usize],
    text: &str,
    line: i32,
    col: i32,
    encoding: i32,
) -> Option<usize> {
    let start = *line_starts.get(usize::try_from(line).ok()?)?;
    let col = usize::try_from(col).ok()?;
    let rest = &text[start..];
    let offset = match encoding {
        proto::ENCODING_UTF16 | proto::ENCODING_UTF32 => {
            let mut units = 0;
            let mut bytes = 0;
            for c in rest.chars() {
                if units >= col || c == '\n' {
                    break;
                }
                units += if encoding == proto::ENCODING_UTF16 {
                    c.len_utf16()
                } else {
                    1
                };
                bytes += c.len_utf8();
            }
            bytes
        }
        _ => col,
    };
    (start + offset <= text.len()).then_some(start + offset)
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// Where a SCIP reference goes.
enum Target {
    /// A definition in a parsed file: the file and the byte.
    Indexed(usize, usize),
    /// std, a dependency, or a file that the index does not parse.
    External,
    /// More than one definition, and the crate does not choose one.
    Ambiguous,
}

/// The definition that a reference from `file` goes to. A symbol with more
/// than one definition (the same path in 2 crates of one package) goes to
/// the one in the crate of the reference, else to the one in the library.
fn choose(
    defs: &[(Option<usize>, usize)],
    file: &str,
    files: &[ParsedFile<'_>],
    crates: &scip::Crates,
) -> Target {
    let indexed: Vec<(usize, usize)> = defs.iter().filter_map(|&(f, b)| Some((f?, b))).collect();
    match (defs.len(), indexed.as_slice()) {
        (0, _) | (_, []) => return Target::External,
        (1, [(f, b)]) => return Target::Indexed(*f, *b),
        _ => {}
    }
    let crate_of = |f: usize| crates.file_crate.get(files[f].path).copied();
    let here = crates.file_crate.get(file).copied();
    let same: Vec<_> = indexed
        .iter()
        .filter(|(f, _)| here.is_some() && crate_of(*f) == here)
        .collect();
    if let [(f, b)] = same.as_slice() {
        return Target::Indexed(*f, *b);
    }
    let libs: Vec<_> = indexed
        .iter()
        .filter(|(f, _)| crate_of(*f).is_some_and(|c| crates.libraries.contains(&c)))
        .collect();
    if let [(f, b)] = libs.as_slice() {
        return Target::Indexed(*f, *b);
    }
    Target::Ambiguous
}

/// The edges of all parsed files, with the Rust edges from `rust`.
pub fn resolve(files: &[ParsedFile<'_>], rust: RustEdges<'_>) -> Resolved {
    let no_crates = scip::Crates::default();
    let crates = match &rust {
        RustEdges::Scip(_, c) => *c,
        _ => &no_crates,
    };
    let names = Names::new(files);
    let by_path: HashMap<&str, usize> =
        files.iter().enumerate().map(|(i, f)| (f.path, i)).collect();
    let starts: Vec<Vec<usize>> = files.iter().map(|f| line_starts(f.text)).collect();
    let mut stats = EdgeStats::default();
    let mut edges = Vec::new();
    let mut covered = Vec::new();

    // The Rust documents of the SCIP index, by file, and the definition of
    // each symbol.
    let mut scip_docs: HashMap<usize, &proto::Document> = HashMap::new();
    let mut definitions: HashMap<&str, Vec<(Option<usize>, usize)>> = HashMap::new();
    if let RustEdges::Scip(index, _) = &rust {
        for doc in &index.documents {
            let file = by_path.get(doc.relative_path.as_str()).copied();
            if let Some(f) = file.filter(|&f| files[f].language == "rust") {
                scip_docs.insert(f, doc);
            }
            for occ in &doc.occurrences {
                if occ.symbol_roles & proto::ROLE_DEFINITION == 0 || occ.range.len() < 3 {
                    continue;
                }
                let byte = file.and_then(|f| {
                    byte_offset(
                        &starts[f],
                        files[f].text,
                        occ.range[0],
                        occ.range[1],
                        doc.position_encoding,
                    )
                });
                definitions
                    .entry(occ.symbol.as_str())
                    .or_default()
                    .push((file, byte.unwrap_or(0)));
            }
        }
    }
    // For a base index: the id of each symbol, by its key.
    let mut ids: HashMap<SymKey, i64> = HashMap::new();
    if let RustEdges::Base { .. } = &rust {
        for file in files {
            let keys = sym_keys(
                file.path,
                file.symbols
                    .iter()
                    .map(|s| (s.container.as_deref(), s.name.as_str(), s.kind)),
            );
            for (s, key) in file.symbols.iter().zip(keys) {
                ids.insert(key, s.id);
            }
        }
    }
    let mut joins: HashMap<(usize, usize), Option<i64>> = HashMap::new();

    for (i, file) in files.iter().enumerate() {
        // The places that SCIP resolved: no name class goes there.
        let mut resolved_at: HashSet<usize> = HashSet::new();
        // The places where SCIP gave a name class (a fallback).
        let mut fallback_at: HashSet<usize> = HashSet::new();
        // The Rust functions that kept their SCIP edges from a base index.
        let mut kept: HashSet<i64> = HashSet::new();
        // A name class in this file waits for a SCIP run, but in a kept
        // function.
        let mut pending = false;
        if let Some(doc) = scip_docs.get(&i) {
            for occ in &doc.occurrences {
                if occ.range.len() < 3 {
                    continue;
                }
                let Some(byte) = byte_offset(
                    &starts[i],
                    file.text,
                    occ.range[0],
                    occ.range[1],
                    doc.position_encoding,
                ) else {
                    continue;
                };
                resolved_at.insert(byte);
                if occ.symbol_roles & proto::ROLE_DEFINITION != 0
                    || file.use_ranges.iter().any(|&(s, e)| s <= byte && byte < e)
                {
                    continue;
                }
                let Some((name, short)) = scip::descriptors(&occ.symbol)
                    .as_deref()
                    .and_then(scip::function_name)
                else {
                    continue;
                };
                let Some(caller_id) = caller_at(file, byte) else {
                    continue;
                };
                let (line, col) = (occ.range[0] as usize + 1, occ.range[1] as usize + 1);
                let defs = definitions
                    .get(occ.symbol.as_str())
                    .map_or(&[][..], Vec::as_slice);
                match choose(defs, file.path, files, crates) {
                    Target::Ambiguous => {
                        stats.scip_ambiguous += 1;
                        fallback_at.insert(byte);
                        edges.push(names.edge(
                            file.language,
                            i,
                            caller_id,
                            &name,
                            &short,
                            line,
                            col,
                            &mut stats,
                        ));
                    }
                    Target::Indexed(def_file, def_byte) => {
                        let callee = *joins.entry((def_file, def_byte)).or_insert_with(|| {
                            files[def_file]
                                .symbols
                                .iter()
                                .filter(|s| {
                                    is_function(s.kind)
                                        && s.name == name
                                        && s.start_byte <= def_byte
                                        && def_byte < s.end_byte
                                })
                                .min_by_key(|s| s.end_byte - s.start_byte)
                                .map(|s| s.id)
                        });
                        match callee {
                            Some(callee_id) => {
                                stats.certain_scip += 1;
                                edges.push(NewEdge {
                                    caller_id,
                                    callee_id: Some(callee_id),
                                    callee_name: short,
                                    class: "certain",
                                    origin: "scip",
                                    line,
                                    col,
                                    candidates: Vec::new(),
                                    scip_pending: false,
                                });
                            }
                            None => {
                                stats.scip_join_misses += 1;
                                fallback_at.insert(byte);
                                edges.push(names.edge(
                                    file.language,
                                    i,
                                    caller_id,
                                    &name,
                                    &short,
                                    line,
                                    col,
                                    &mut stats,
                                ));
                            }
                        }
                    }
                    Target::External => {
                        stats.external_scip += 1;
                        edges.push(NewEdge {
                            caller_id,
                            callee_id: None,
                            callee_name: short,
                            class: "external",
                            origin: "scip",
                            line,
                            col,
                            candidates: Vec::new(),
                            scip_pending: false,
                        });
                    }
                }
            }
        } else if let (RustEdges::Base { bases, mark }, "rust") = (&rust, file.language) {
            pending = *mark;
            let base_files: Vec<&BaseFile> = bases
                .iter()
                .filter_map(|b| b.files.get(file.path))
                .collect();
            let keys = sym_keys(
                file.path,
                file.symbols
                    .iter()
                    .map(|s| (s.container.as_deref(), s.name.as_str(), s.kind)),
            );
            for (s, key) in file.symbols.iter().zip(&keys) {
                if !is_function(s.kind) {
                    continue;
                }
                let Some((base, b)) = base_files.iter().find_map(|f| {
                    f.symbols
                        .get(key)
                        .filter(|b| same_code(b, s, file, &starts[i]))
                        .map(|b| (*f, b))
                }) else {
                    continue;
                };
                kept.insert(s.id);
                let caller_id = s.id;
                resolved_at.extend(
                    base.covered
                        .iter()
                        .filter(|&&c| b.start_byte <= c && c < b.end_byte)
                        .map(|&c| c - b.start_byte + s.start_byte),
                );
                for e in &b.edges {
                    let line = e.line + s.start_line - b.start_line;
                    stats.kept_scip += 1;
                    let callee_id = match &e.callee {
                        None => None,
                        Some(key) => match ids.get(key) {
                            Some(id) => Some(*id),
                            // The callee is gone or renamed: a name class
                            // until the next SCIP run.
                            None => {
                                let mut edge = names.edge(
                                    file.language,
                                    i,
                                    caller_id,
                                    &key.name,
                                    &e.callee_name,
                                    line,
                                    e.col,
                                    &mut stats,
                                );
                                edge.scip_pending = *mark;
                                stats.pending += usize::from(*mark);
                                edges.push(edge);
                                continue;
                            }
                        },
                    };
                    edges.push(NewEdge {
                        caller_id,
                        callee_id,
                        callee_name: e.callee_name.clone(),
                        class: if e.class == "certain" {
                            "certain"
                        } else {
                            "external"
                        },
                        origin: "scip",
                        line,
                        col: e.col,
                        candidates: Vec::new(),
                        scip_pending: false,
                    });
                }
            }
        }

        for call in file.calls {
            if resolved_at.contains(&call.start_byte) {
                if !fallback_at.contains(&call.start_byte) {
                    covered.push((i, call.start_byte));
                }
                continue;
            }
            let Some(caller_id) = caller_at(file, call.start_byte) else {
                continue;
            };
            if file.language == "rust" {
                stats.rust_unresolved += 1;
            }
            let mut edge = names.edge(
                file.language,
                i,
                caller_id,
                &call.name,
                &call.written,
                call.line,
                call.col,
                &mut stats,
            );
            edge.scip_pending = pending && !kept.contains(&caller_id);
            stats.pending += usize::from(edge.scip_pending);
            edges.push(edge);
        }
        for call in file.macro_calls {
            if resolved_at.contains(&call.start_byte) {
                if !fallback_at.contains(&call.start_byte) {
                    covered.push((i, call.start_byte));
                }
                continue;
            }
            let Some(caller_id) = caller_at(file, call.start_byte) else {
                continue;
            };
            stats.macro_text += 1;
            let mut edge = names.edge(
                file.language,
                i,
                caller_id,
                &call.name,
                &call.written,
                call.line,
                call.col,
                &mut stats,
            );
            edge.origin = "macro-text";
            edge.scip_pending = pending && !kept.contains(&caller_id);
            stats.pending += usize::from(edge.scip_pending);
            edges.push(edge);
        }
    }
    Resolved {
        edges,
        stats,
        covered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_become_bytes() {
        let text = "fn é() {}\nlet x = 1;\n";
        let starts = line_starts(text);
        // `(` is column 4 in UTF-16 and byte 5.
        assert_eq!(
            byte_offset(&starts, text, 0, 4, proto::ENCODING_UTF16),
            Some(5)
        );
        assert_eq!(
            byte_offset(&starts, text, 0, 5, proto::ENCODING_UTF8),
            Some(5)
        );
        assert_eq!(byte_offset(&starts, text, 1, 4, 1), Some(15));
        assert_eq!(byte_offset(&starts, text, 9, 0, 1), None);
    }

    fn sym(id: i64, name: &str, kind: &'static str, span: (usize, usize)) -> Sym {
        Sym {
            id,
            name: name.into(),
            container: None,
            kind,
            start_line: 1,
            end_line: 1,
            start_byte: span.0,
            end_byte: span.1,
            tree_hash: String::new(),
        }
    }

    fn call(name: &str, written: &str, byte: usize) -> CallSite {
        CallSite {
            name: name.into(),
            written: written.into(),
            start_byte: byte,
            end_byte: byte + name.len(),
            line: 1,
            col: byte + 1,
        }
    }

    #[test]
    fn name_classes() {
        let a_calls = [
            call("helper", "helper", 5),
            call("load", "load", 6),
            call("Println", "fmt.Println", 7),
        ];
        let files = [
            ParsedFile {
                path: "a.go",
                language: "go",
                text: "",
                symbols: vec![
                    sym(1, "main", "function", (0, 50)),
                    sym(2, "helper", "function", (60, 70)),
                ],
                calls: &a_calls,
                use_ranges: &[],
                macro_calls: &[],
            },
            ParsedFile {
                path: "b.go",
                language: "go",
                text: "",
                symbols: vec![
                    sym(3, "helper", "function", (0, 9)),
                    sym(4, "load", "function", (10, 20)),
                ],
                calls: &[],
                use_ranges: &[],
                macro_calls: &[],
            },
            ParsedFile {
                path: "c.go",
                language: "go",
                text: "",
                symbols: vec![sym(5, "load", "function", (0, 9))],
                calls: &[],
                use_ranges: &[],
                macro_calls: &[],
            },
        ];
        let Resolved { edges, stats, .. } = resolve(&files, RustEdges::Names);
        let got: Vec<_> = edges
            .iter()
            .map(|e| {
                (
                    e.callee_name.as_str(),
                    e.class,
                    e.callee_id,
                    e.candidates.clone(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("helper", "certain", Some(2), vec![]),
                ("load", "possible", None, vec![4, 5]),
                ("fmt.Println", "external", None, vec![]),
            ]
        );
        assert_eq!(
            (stats.certain_name, stats.possible_name, stats.external_name),
            (1, 1, 1)
        );
    }

    fn key(name: &str) -> SymKey {
        SymKey {
            file: "src/lib.rs".into(),
            container: None,
            name: name.into(),
            kind: "function".into(),
            ordinal: 0,
        }
    }

    /// A base where `a` (line 1) calls `b` and `Vec::new` (from SCIP).
    fn base() -> BaseIndex {
        let text = "fn a() { b(); Vec::new(); }\nfn b() {}\n";
        let a_end = text.find('\n').unwrap();
        let edge = |callee: Option<SymKey>, name: &str, col: usize| BaseEdge {
            callee,
            callee_name: name.into(),
            class: if name == "b" { "certain" } else { "external" }.into(),
            line: 1,
            col,
        };
        let sym = |start: usize, end: usize, line: usize, edges| BaseSym {
            tree_hash: format!("hash {start}"),
            start_line: line,
            end_line: line,
            start_byte: start,
            end_byte: end,
            pending: false,
            edges,
        };
        let file = BaseFile {
            covered: vec![9, 14],
            symbols: HashMap::from([
                (
                    key("a"),
                    sym(
                        0,
                        a_end,
                        1,
                        vec![edge(Some(key("b")), "b", 10), edge(None, "Vec::new", 20)],
                    ),
                ),
                (key("b"), sym(a_end + 1, a_end + 10, 2, Vec::new())),
            ]),
        };
        BaseIndex {
            files: HashMap::from([("src/lib.rs".to_string(), file)]),
        }
    }

    fn rust_file<'a>(text: &'a str, symbols: Vec<Sym>, calls: &'a [CallSite]) -> ParsedFile<'a> {
        ParsedFile {
            path: "src/lib.rs",
            language: "rust",
            text,
            symbols,
            calls,
            use_ranges: &[],
            macro_calls: &[],
        }
    }

    #[test]
    fn a_moved_function_keeps_its_scip_edges() {
        // A new function before `a` moves `a` down 1 line and 10 bytes.
        let text = "fn c() {}\nfn a() { b(); Vec::new(); }\nfn b() {}\n";
        let mut a = sym(2, "a", "function", (10, 37));
        a.tree_hash = "hash 0".into();
        a.start_line = 2;
        a.end_line = 2;
        let mut b = sym(3, "b", "function", (38, 47));
        b.tree_hash = "hash 28".into();
        b.start_line = 3;
        b.end_line = 3;
        let mut c = sym(1, "c", "function", (0, 9));
        c.tree_hash = "new".into();
        let calls = [call("b", "b", 19), call("new", "Vec::new", 24)];
        let files = [rust_file(text, vec![c, a, b], &calls)];
        let bases = [base()];
        let Resolved { edges, .. } = resolve(
            &files,
            RustEdges::Base {
                bases: &bases,
                mark: true,
            },
        );
        let got: Vec<_> = edges
            .iter()
            .map(|e| (e.caller_id, e.callee_id, e.origin, e.line, e.scip_pending))
            .collect();
        assert_eq!(
            got,
            [(2, Some(3), "scip", 2, false), (2, None, "scip", 2, false)]
        );
    }

    #[test]
    fn a_kept_edge_to_a_removed_callee_becomes_a_marked_name_class() {
        // `b` is gone; `a` did not change.
        let text = "fn a() { b(); Vec::new(); }\n";
        let mut a = sym(1, "a", "function", (0, 27));
        a.tree_hash = "hash 0".into();
        let calls = [call("b", "b", 9), call("new", "Vec::new", 14)];
        let files = [rust_file(text, vec![a], &calls)];
        let bases = [base()];
        let Resolved { edges, .. } = resolve(
            &files,
            RustEdges::Base {
                bases: &bases,
                mark: true,
            },
        );
        let got: Vec<_> = edges
            .iter()
            .map(|e| (e.callee_name.as_str(), e.class, e.origin, e.scip_pending))
            .collect();
        assert_eq!(
            got,
            [
                ("b", "external", "name", true),
                ("Vec::new", "external", "scip", false)
            ]
        );
    }
}
