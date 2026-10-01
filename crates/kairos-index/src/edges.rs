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

use std::collections::{HashMap, HashSet};

use crate::EdgeStats;
use crate::calls::CallSite;
use crate::scip::{self, proto};

/// A symbol of a parsed file, with its row id.
pub struct Sym {
    pub id: i64,
    pub name: String,
    pub kind: &'static str,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A parsed file, as the edges need it.
pub struct ParsedFile<'a> {
    pub path: &'a str,
    pub language: &'static str,
    pub text: &'a str,
    pub symbols: Vec<Sym>,
    pub calls: &'a [CallSite],
    pub use_ranges: &'a [(usize, usize)],
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

/// The edges of all parsed files. `scip` is the index of the Cargo
/// workspace at the root and the crate of each file, if a run was made.
pub fn resolve(
    files: &[ParsedFile<'_>],
    scip: Option<(&proto::Index, &scip::Crates)>,
) -> (Vec<NewEdge>, EdgeStats) {
    let no_crates = scip::Crates::default();
    let crates = scip.map_or(&no_crates, |(_, c)| c);
    let names = Names::new(files);
    let by_path: HashMap<&str, usize> =
        files.iter().enumerate().map(|(i, f)| (f.path, i)).collect();
    let starts: Vec<Vec<usize>> = files.iter().map(|f| line_starts(f.text)).collect();
    let mut stats = EdgeStats::default();
    let mut edges = Vec::new();

    // The Rust documents of the SCIP index, by file, and the definition of
    // each symbol.
    let mut scip_docs: HashMap<usize, &proto::Document> = HashMap::new();
    let mut definitions: HashMap<&str, Vec<(Option<usize>, usize)>> = HashMap::new();
    if let Some((index, _)) = scip {
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
    let mut joins: HashMap<(usize, usize), Option<i64>> = HashMap::new();

    for (i, file) in files.iter().enumerate() {
        let mut resolved_at: HashSet<usize> = HashSet::new();
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
                                });
                            }
                            None => {
                                stats.scip_join_misses += 1;
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
                        });
                    }
                }
            }
        }

        for call in file.calls {
            if resolved_at.contains(&call.start_byte) {
                continue;
            }
            let Some(caller_id) = caller_at(file, call.start_byte) else {
                continue;
            };
            if file.language == "rust" {
                stats.rust_unresolved += 1;
            }
            edges.push(names.edge(
                file.language,
                i,
                caller_id,
                &call.name,
                &call.written,
                call.line,
                call.col,
                &mut stats,
            ));
        }
    }
    (edges, stats)
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
        assert_eq!(byte_offset(&starts, text, 0, 5, 1), Some(5));
        assert_eq!(byte_offset(&starts, text, 1, 4, 1), Some(15));
        assert_eq!(byte_offset(&starts, text, 9, 0, 1), None);
    }

    fn sym(id: i64, name: &str, kind: &'static str, span: (usize, usize)) -> Sym {
        Sym {
            id,
            name: name.into(),
            kind,
            start_byte: span.0,
            end_byte: span.1,
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
            },
            ParsedFile {
                path: "c.go",
                language: "go",
                text: "",
                symbols: vec![sym(5, "load", "function", (0, 9))],
                calls: &[],
                use_ranges: &[],
            },
        ];
        let (edges, stats) = resolve(&files, None);
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
}
