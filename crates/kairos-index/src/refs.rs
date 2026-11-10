//! The uses of a symbol, not only its calls (KAIROS-T-0419).
//!
//! - **Rust, from SCIP:** each reference of the SCIP index to a symbol of a
//!   parsed file is a use: in a function, in a type, in a `use` line, at the
//!   top of a file. The definition of the SCIP symbol is joined to the narsil
//!   symbol on the file, the span and the name, as for the edges. A struct
//!   field has no narsil symbol: a use of a field goes to its type, with the
//!   kind `field`. A definition, a local symbol, `Self` and an operator are
//!   not uses.
//! - **The kind of a use:** `import` in a `use` line (or with the SCIP role
//!   `Import`), `call` for a function, a method or a macro, `type` for a
//!   type, `field` for a field of a type, `other` for the rest (a constant,
//!   a module, an enum variant).
//! - **An update with no SCIP run** keeps the uses of each Rust file whose
//!   content did not change, while their symbols are there. A changed Rust
//!   file has no uses until the next SCIP run: it is marked `pending`.
//! - **A file that the SCIP run did not cover** (code behind a `cfg` that
//!   the run does not turn on, a folder that the indexer skips) has no row
//!   in `refs_files`, in a language that has rows. An update keeps it so,
//!   changed or not, and does not mark it `pending`: an edit does not make
//!   the next SCIP run cover it. Only a server index can (KAIROS-A-0083).
//! - **The other languages with a SCIP runner** (Go, TypeScript, TSX,
//!   JavaScript, Python, KAIROS-T-0473) get their uses from their SCIP index
//!   the same way, when the runner gave one. A language with no SCIP index
//!   in the build has the calls of the edges as its uses, by name (see
//!   [`crate::query`]).

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::IndexError;
use crate::edges::{
    BaseIndex, ParsedFile, SymKey, Target, byte_offset, choose, is_name_char, line_starts, sym_keys,
};
use crate::runners::{SCIP_LANGUAGES, ScipData};
use crate::scip::{self, Descriptor, proto};

/// The SCIP role `Import`.
const ROLE_IMPORT: i32 = 0x2;

/// A use, ready to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRef {
    /// The index of the parsed file of the use.
    pub file: usize,
    pub target_id: i64,
    /// The smallest symbol that holds the use.
    pub enclosing_id: Option<i64>,
    /// From 1.
    pub line: usize,
    /// From 1, as SCIP gives it.
    pub col: usize,
    pub kind: &'static str,
}

/// The uses of a build, and the Rust files that have SCIP uses: (file
/// index, pending).
#[derive(Debug, Default)]
pub struct Resolved {
    pub refs: Vec<NewRef>,
    pub files: Vec<(usize, bool)>,
}

/// Where the uses come from.
pub enum Source<'a> {
    /// No SCIP data: no runner gave an index.
    Nothing,
    /// The SCIP indexes of the build, the languages that they cover, and the
    /// crate of each Rust file.
    Scip(&'a ScipData),
    /// The uses of base indexes, for the files that did not change. A new
    /// file of a language in `marked` waits for a SCIP run (Rust in a Cargo
    /// workspace, and each language that a base has SCIP for).
    Base {
        bases: &'a [BaseIndex],
        marked: &'a HashSet<&'a str>,
    },
}

/// The uses of a base index, by file path.
#[derive(Debug, Default)]
pub struct BaseRefs {
    files: HashMap<String, BaseFile>,
    /// The files that the SCIP run did not cover: no row in `refs_files`,
    /// in a language that has rows in this base (KAIROS-A-0083). A language
    /// whose SCIP run gave nothing has no such files.
    not_covered: HashSet<String>,
}

impl BaseRefs {
    /// Whether the SCIP run of this base did not cover `path`.
    pub fn not_covered(&self, path: &str) -> bool {
        self.not_covered.contains(path)
    }

    /// Whether this base has a `refs_files` row for `path`: SCIP uses, or a
    /// mark that they wait for a SCIP run.
    pub fn has_scip_row(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// A base with no SCIP uses, whose SCIP run did not cover `paths`.
    #[cfg(test)]
    pub(crate) fn not_covering(paths: &[&str]) -> Self {
        BaseRefs {
            files: HashMap::new(),
            not_covered: paths.iter().map(|p| p.to_string()).collect(),
        }
    }
}

/// Whether the SCIP run did not cover `path`: a base has the file as not
/// covered, and no base has a row for it. Such a file does not wait for
/// the next SCIP run (KAIROS-A-0083). Pure.
pub fn not_covered(bases: &[BaseIndex], path: &str) -> bool {
    bases.iter().any(|b| b.refs.not_covered(path))
        && !bases.iter().any(|b| b.refs.has_scip_row(path))
}

#[derive(Debug)]
struct BaseFile {
    content_hash: String,
    pending: bool,
    refs: Vec<BaseRef>,
}

#[derive(Debug)]
struct BaseRef {
    target: SymKey,
    enclosing: Option<SymKey>,
    line: usize,
    col: usize,
    kind: &'static str,
}

/// The kinds of a use, as the schema has them.
const KINDS: [&str; 5] = ["call", "type", "field", "import", "other"];

fn kind_of(text: &str) -> &'static str {
    KINDS
        .iter()
        .find(|k| **k == text)
        .copied()
        .unwrap_or("other")
}

/// Read the uses of the index at `conn`, with the key of each symbol.
pub fn read_base(conn: &Connection, keys: &HashMap<i64, SymKey>) -> Result<BaseRefs, IndexError> {
    let mut files: HashMap<i64, (String, BaseFile)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.path, f.content_hash, r.pending
             FROM refs_files r JOIN files f ON f.id = r.file_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        for row in rows {
            let (id, path, content_hash, pending) = row?;
            files.insert(
                id,
                (
                    path,
                    BaseFile {
                        content_hash,
                        pending,
                        refs: Vec::new(),
                    },
                ),
            );
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT file_id, target_id, enclosing_id, line, col, use_kind FROM refs ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (file, target, enclosing, line, col, kind) = row?;
            let (Some((_, f)), Some(target)) = (files.get_mut(&file), keys.get(&target)) else {
                continue;
            };
            f.refs.push(BaseRef {
                target: target.clone(),
                enclosing: enclosing.and_then(|id| keys.get(&id).cloned()),
                line: line as usize,
                col: col as usize,
                kind: kind_of(&kind),
            });
        }
    }
    let not_covered = {
        let mut stmt = conn.prepare(
            "SELECT path FROM files
             WHERE id NOT IN (SELECT file_id FROM refs_files)
               AND language IN (SELECT DISTINCT f.language FROM refs_files r
                                JOIN files f ON f.id = r.file_id)",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    Ok(BaseRefs {
        files: files.into_values().collect(),
        not_covered,
    })
}

/// The name of a descriptor that a narsil symbol can have.
fn descriptor_name(d: &Descriptor) -> Option<&str> {
    match d {
        Descriptor::Namespace(n)
        | Descriptor::Type(n)
        | Descriptor::Term(n)
        | Descriptor::Method(n)
        | Descriptor::Macro(n) => Some(n.split('<').next().unwrap_or(n)),
        _ => None,
    }
}

/// The smallest symbol of `file` named `name` whose span holds `byte`.
fn symbol_at(file: &ParsedFile<'_>, byte: usize, name: &str) -> Option<(i64, &'static str)> {
    file.symbols
        .iter()
        .filter(|s| s.name == name && s.start_byte <= byte && byte < s.end_byte)
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| (s.id, s.kind))
}

/// A narsil symbol and the kind of a use of it.
type Joined = (i64, &'static str);

/// The narsil symbol of a SCIP definition at `byte` of `file`, and the
/// kind of a use of it. `None` if no symbol matches.
fn join(file: &ParsedFile<'_>, byte: usize, descriptors: &[Descriptor]) -> Option<Joined> {
    let (last, rest) = descriptors.split_last()?;
    let kind = match last {
        Descriptor::Method(_) | Descriptor::Macro(_) => "call",
        Descriptor::Type(_) => "type",
        _ => "other",
    };
    if let Some((id, _)) = descriptor_name(last).and_then(|name| symbol_at(file, byte, name)) {
        return Some((id, kind));
    }
    // A struct field has no symbol: the use goes to its type. An enum
    // variant goes to its enum, as `other`. A method of a Go interface goes
    // to its interface, as `call`.
    match (last, rest.last()) {
        (Descriptor::Term(_), Some(Descriptor::Type(owner))) => {
            let (id, owner_kind) = symbol_at(file, byte, owner.split('<').next()?)?;
            Some((
                id,
                match owner_kind {
                    "enum" => "other",
                    // A term of a Go interface is a method (KAIROS-T-0475).
                    "interface" => "call",
                    _ => "field",
                },
            ))
        }
        _ => None,
    }
}

/// The smallest symbol of `file` that holds `byte`.
fn enclosing(file: &ParsedFile<'_>, byte: usize) -> Option<i64> {
    file.symbols
        .iter()
        .filter(|s| s.start_byte <= byte && byte < s.end_byte)
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| s.id)
}

/// Whether the text at `byte` is the word `Self`.
fn is_self(text: &str, byte: usize) -> bool {
    text[byte..].starts_with("Self") && !text[byte + 4..].starts_with(is_name_char)
}

/// The uses of all parsed files. `hashes` is the content hash of each file.
pub fn resolve(files: &[ParsedFile<'_>], hashes: &[&str], source: Source<'_>) -> Resolved {
    match source {
        Source::Nothing => Resolved::default(),
        Source::Scip(data) => from_scip(files, data),
        Source::Base { bases, marked } => from_base(files, hashes, bases, marked),
    }
}

fn from_scip(files: &[ParsedFile<'_>], data: &ScipData) -> Resolved {
    let (index, crates) = (&data.index, &data.crates);
    let by_path: HashMap<&str, usize> =
        files.iter().enumerate().map(|(i, f)| (f.path, i)).collect();
    let starts: Vec<Vec<usize>> = files.iter().map(|f| line_starts(f.text)).collect();
    let mut docs: Vec<(usize, &proto::Document)> = Vec::new();
    let mut definitions: HashMap<&str, Vec<(Option<usize>, usize)>> = HashMap::new();
    for doc in &index.documents {
        let file = by_path.get(doc.relative_path.as_str()).copied();
        if let Some(f) = file.filter(|&f| data.covers(files[f].language)) {
            docs.push((f, doc));
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
    docs.sort_by_key(|(f, _)| *f);

    let mut out = Resolved::default();
    // The narsil symbol of each SCIP symbol, once for each crate of a use.
    let mut joins: HashMap<(usize, usize, &str), Option<Joined>> = HashMap::new();
    for (i, doc) in docs {
        let file = &files[i];
        out.files.push((i, false));
        let mut seen: HashSet<(i64, i32, i32)> = HashSet::new();
        for occ in &doc.occurrences {
            if occ.range.len() < 3
                || occ.symbol_roles & proto::ROLE_DEFINITION != 0
                || occ.symbol.starts_with("local ")
            {
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
            // An operator is a reference to its trait method; `Self` is a
            // reference to the type of the impl. Neither is written as a use.
            if !file.text[byte..].starts_with(is_name_char) || is_self(file.text, byte) {
                continue;
            }
            let defs = definitions
                .get(occ.symbol.as_str())
                .map_or(&[][..], Vec::as_slice);
            let Target::Indexed(def_file, def_byte) = choose(defs, file.path, files, crates) else {
                continue;
            };
            let joined = *joins
                .entry((def_file, def_byte, occ.symbol.as_str()))
                .or_insert_with(|| {
                    scip::descriptors(&occ.symbol)
                        .and_then(|d| join(&files[def_file], def_byte, &d))
                });
            let Some((target_id, kind)) = joined else {
                continue;
            };
            if !seen.insert((target_id, occ.range[0], occ.range[1])) {
                continue;
            }
            let import = occ.symbol_roles & ROLE_IMPORT != 0
                || file.use_ranges.iter().any(|&(s, e)| s <= byte && byte < e);
            out.refs.push(NewRef {
                file: i,
                target_id,
                enclosing_id: enclosing(file, byte),
                line: occ.range[0] as usize + 1,
                col: occ.range[1] as usize + 1,
                kind: if import { "import" } else { kind },
            });
        }
    }
    out
}

fn from_base(
    files: &[ParsedFile<'_>],
    hashes: &[&str],
    bases: &[BaseIndex],
    marked: &HashSet<&str>,
) -> Resolved {
    let mut ids: HashMap<SymKey, i64> = HashMap::new();
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
    let mut out = Resolved::default();
    for (i, file) in files.iter().enumerate() {
        if !SCIP_LANGUAGES.contains(&file.language) {
            continue;
        }
        let known: Vec<&BaseFile> = bases
            .iter()
            .filter_map(|b| b.refs.files.get(file.path))
            .collect();
        if not_covered(bases, file.path) {
            // The SCIP run did not cover the file: it stays so.
            continue;
        }
        let Some(same) = known.iter().find(|b| b.content_hash == hashes[i]) else {
            // A changed file, or a new file of the workspace: no uses until
            // the next SCIP run.
            if !known.is_empty() || marked.contains(file.language) {
                out.files.push((i, true));
            }
            continue;
        };
        out.files.push((i, same.pending));
        for r in &same.refs {
            // The target is gone or renamed: the use is left out.
            let Some(&target_id) = ids.get(&r.target) else {
                continue;
            };
            out.refs.push(NewRef {
                file: i,
                target_id,
                enclosing_id: r.enclosing.as_ref().and_then(|k| ids.get(k).copied()),
                line: r.line,
                col: r.col,
                kind: r.kind,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::Sym;

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

    fn occ(line: i32, col: i32, len: i32, symbol: &str, roles: i32) -> proto::Occurrence {
        proto::Occurrence {
            range: vec![line, col, col + len],
            symbol: symbol.into(),
            symbol_roles: roles,
        }
    }

    const P: &str = "rust-analyzer cargo p 0.1.0 ";

    /// A type, a field, a method, an import and `Self`: each use has its
    /// kind and its enclosing symbol, and the definitions are not uses.
    #[test]
    fn scip_references_become_uses_with_their_kind() {
        let lib = "pub struct Queue {\n    items: u32,\n}\nimpl Queue {\n    pub fn new() -> Self { Self { items: 0 } }\n}\n";
        let main = "use p::Queue;\nfn run() -> Queue {\n    let q = Queue::new();\n    q.items;\n    q\n}\n";
        let new_at = lib.find("fn new").unwrap();
        let struct_end = lib.find("}\n").unwrap() + 1;
        let impl_at = lib.find("impl").unwrap();
        let lib_file = ParsedFile {
            path: "src/lib.rs",
            language: "rust",
            text: lib,
            symbols: vec![
                sym(1, "Queue", "struct", (0, struct_end)),
                sym(2, "Queue", "implementation", (impl_at, lib.len() - 1)),
                sym(3, "new", "function", (new_at - 4, lib.len() - 3)),
            ],
            calls: &[],
            use_ranges: &[],
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports: &[],
        };
        let use_ranges = [(0, 13)];
        let main_file = ParsedFile {
            path: "src/main.rs",
            language: "rust",
            text: main,
            symbols: vec![sym(10, "run", "function", (14, main.len() - 1))],
            calls: &[],
            use_ranges: &use_ranges,
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports: &[],
        };
        let queue = format!("{P}Queue#");
        let items = format!("{P}Queue#items.");
        let new = format!("{P}impl#[Queue]new().");
        let index = proto::Index {
            documents: vec![
                proto::Document {
                    relative_path: "src/lib.rs".into(),
                    occurrences: vec![
                        occ(0, 11, 5, &queue, proto::ROLE_DEFINITION),
                        occ(1, 4, 5, &items, proto::ROLE_DEFINITION),
                        occ(3, 5, 5, &queue, 0),
                        occ(4, 11, 3, &new, proto::ROLE_DEFINITION),
                        occ(4, 20, 4, &queue, 0),
                        occ(4, 27, 4, &queue, 0),
                        occ(4, 34, 5, &items, 0),
                    ],
                    position_encoding: proto::ENCODING_UTF8,
                },
                proto::Document {
                    relative_path: "src/main.rs".into(),
                    occurrences: vec![
                        occ(0, 7, 5, &queue, 0),
                        occ(1, 3, 3, &format!("{P}run()."), proto::ROLE_DEFINITION),
                        occ(1, 12, 5, &queue, 0),
                        occ(2, 12, 5, &queue, 0),
                        occ(2, 19, 3, &new, 0),
                        occ(2, 8, 1, "local 0", proto::ROLE_DEFINITION),
                        occ(3, 4, 1, "local 0", 0),
                        occ(3, 6, 5, &items, 0),
                        occ(3, 6, 5, &items, 0),
                        occ(0, 0, 3, &format!("{P}std/"), 0),
                    ],
                    position_encoding: proto::ENCODING_UTF8,
                },
            ],
        };
        let files = [lib_file, main_file];
        let data = ScipData {
            index,
            crates: scip::Crates::default(),
            languages: HashSet::from(["rust"]),
        };
        let out = resolve(&files, &["a", "b"], Source::Scip(&data));
        assert_eq!(out.files, [(0, false), (1, false)]);
        let got: Vec<(usize, i64, Option<i64>, usize, &str)> = out
            .refs
            .iter()
            .map(|r| (r.file, r.target_id, r.enclosing_id, r.line, r.kind))
            .collect();
        assert_eq!(
            got,
            [
                // `impl Queue`, in the impl block.
                (0, 1, Some(2), 4, "type"),
                // `Self { items: 0 }`: the field, in `new`. `Self` is not a use.
                (0, 1, Some(3), 5, "field"),
                (1, 1, None, 1, "import"),
                (1, 1, Some(10), 2, "type"),
                (1, 1, Some(10), 3, "type"),
                (1, 3, Some(10), 3, "call"),
                // The field once, though SCIP gives it 2 times.
                (1, 1, Some(10), 4, "field"),
            ]
        );
    }

    #[test]
    fn an_update_keeps_the_uses_of_an_unchanged_file_only() {
        let key = |file: &str, name: &str, kind: &str| SymKey {
            file: file.into(),
            container: None,
            name: name.into(),
            kind: kind.into(),
            ordinal: 0,
        };
        let base_file = |hash: &str, refs: Vec<BaseRef>| BaseFile {
            content_hash: hash.into(),
            pending: false,
            refs,
        };
        let mut base = BaseIndex::default();
        base.refs = BaseRefs {
            files: HashMap::from([
                (
                    "src/a.rs".to_string(),
                    base_file(
                        "same",
                        vec![
                            BaseRef {
                                target: key("src/b.rs", "B", "struct"),
                                enclosing: Some(key("src/a.rs", "f", "function")),
                                line: 3,
                                col: 9,
                                kind: "type",
                            },
                            BaseRef {
                                target: key("src/b.rs", "Gone", "struct"),
                                enclosing: None,
                                line: 1,
                                col: 5,
                                kind: "import",
                            },
                        ],
                    ),
                ),
                (
                    "src/b.rs".to_string(),
                    base_file(
                        "old",
                        vec![BaseRef {
                            target: key("src/b.rs", "B", "struct"),
                            enclosing: None,
                            line: 9,
                            col: 1,
                            kind: "type",
                        }],
                    ),
                ),
            ]),
            not_covered: HashSet::new(),
        };
        let file = |path, symbols| ParsedFile {
            path,
            language: "rust",
            text: "",
            symbols,
            calls: &[],
            use_ranges: &[],
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports: &[],
        };
        let files = [
            file("src/a.rs", vec![sym(5, "f", "function", (0, 10))]),
            file("src/b.rs", vec![sym(7, "B", "struct", (0, 10))]),
            file("src/c.rs", vec![]),
        ];
        let out = resolve(
            &files,
            &["same", "new", "x"],
            Source::Base {
                bases: std::slice::from_ref(&base),
                marked: &HashSet::new(),
            },
        );
        assert_eq!(out.files, [(0, false), (1, true)]);
        assert_eq!(
            out.refs,
            [NewRef {
                file: 0,
                target_id: 7,
                enclosing_id: Some(5),
                line: 3,
                col: 9,
                kind: "type",
            }]
        );
    }

    #[test]
    fn a_file_that_the_scip_run_did_not_cover_does_not_wait_for_it() {
        let mut base = BaseIndex::default();
        base.refs = BaseRefs {
            files: HashMap::new(),
            not_covered: HashSet::from(["src/gated.rs".to_string(), "src/edited.rs".to_string()]),
        };
        let file = |path| ParsedFile {
            path,
            language: "rust",
            text: "",
            symbols: vec![],
            calls: &[],
            use_ranges: &[],
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports: &[],
        };
        let files = [
            file("src/gated.rs"),
            file("src/edited.rs"),
            file("src/new.rs"),
        ];
        let marked = HashSet::from(["rust"]);
        let out = resolve(
            &files,
            &["same", "changed", "new"],
            Source::Base {
                bases: std::slice::from_ref(&base),
                marked: &marked,
            },
        );
        // The 2 files that SCIP did not cover have no row, changed or not.
        // A file that is new to the base waits for the next SCIP run.
        assert_eq!(out.files, [(2, true)]);
        assert!(out.refs.is_empty());
    }

    #[test]
    fn a_language_with_no_scip_rows_has_no_files_that_are_not_covered() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::prepare(&conn).unwrap();
        for (id, path, language) in [
            (1, "src/lib.rs", "rust"),
            (2, "src/gated.rs", "rust"),
            (3, "tools/run.py", "python"),
        ] {
            conn.execute(
                "INSERT INTO files (id, path, language, decision, rule, rule_origin, size,
                                    longest_line, content_hash)
                 VALUES (?1, ?2, ?3, 'source', 'r', 'default', 1, 1, 'h')",
                rusqlite::params![id, path, language],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO refs_files (file_id, pending) VALUES (1, 0)",
            [],
        )
        .unwrap();
        let base = read_base(&conn, &HashMap::new()).unwrap();
        // Rust has SCIP rows, so src/gated.rs is not covered. Python has none
        // (its SCIP run gave nothing): its files wait as before.
        assert!(base.not_covered("src/gated.rs"));
        assert!(!base.not_covered("src/lib.rs"));
        assert!(!base.not_covered("tools/run.py"));
    }
}
