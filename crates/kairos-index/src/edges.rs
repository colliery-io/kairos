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
//! - **A qualified Rust call** (`boards::f`, `Type::f`, `crate::m::f`)
//!   gets a `certain` name class only from a function whose container or
//!   module matches the path. If none matches, the edge is `possible` with
//!   the other functions of that name, or `external` (COLLIERY-T-2531).
//! - **A Go call of an imported package** (`pkg.F`): the import path,
//!   through the `go.mod` files of the repository, gives the folder of the
//!   package; the edge goes to the function `F` there. A package outside
//!   the modules of the repository is `external`. A plain Go call (`F()`)
//!   goes to its own folder first (KAIROS-T-0469).
//! - **A Terraform reference** (`var.x`, `local.x`, `module.z`, `data.t.n`,
//!   `t.n`) goes to the block of that address in the same folder: the scope
//!   of Terraform is the folder. `module.z.o` goes to the output `o` in the
//!   folder of the local `source` of the module call `z`; an argument `x` of
//!   the module call goes to its variable `x`. A remote source is `external`
//!   (KAIROS-T-0471).
//! - **A method call** (`x.f()`) never goes to the caller itself by name:
//!   the receiver is not known, so a name gives no evidence of recursion.
//! - **A SCIP definition with no symbol** (a function that a macro makes):
//!   the edge is `certain` with no callee, and keeps the place of the
//!   definition as its target, marked if a macro invocation holds it. It
//!   never goes to a function of the same name (COLLIERY-T-2531).
//! - **A local update** runs no SCIP. Each Rust function whose code did not
//!   change keeps the SCIP edges of the base index, also in a changed file
//!   and when its lines moved. A changed function gets name classes, marked
//!   `scip_pending`, until a run with the Rust edges (COLLIERY-T-1851).

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::calls::{CallSite, Import};
use crate::runners::ScipData;
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
    /// The byte ranges of the Rust macro invocations (COLLIERY-T-2531).
    pub macro_ranges: &'a [(usize, usize)],
    /// The package of a Go file (KAIROS-T-0469).
    pub package: Option<&'a str>,
    /// The imports of a Go file (KAIROS-T-0469), or the module calls of a
    /// Terraform file with their sources (KAIROS-T-0471).
    pub imports: &'a [Import],
}

/// A Go module of the repository, from a `go.mod` file (KAIROS-T-0469): the
/// folder of the file (`""` at the root) and the module path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoModule {
    pub folder: String,
    pub path: String,
}

impl GoModule {
    /// The module of the text of a `go.mod` file in `folder`.
    pub fn read(folder: &str, text: &str) -> Option<GoModule> {
        let path = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("module "))?
            .split("//")
            .next()?
            .trim()
            .trim_matches(['"', '`']);
        (!path.is_empty()).then(|| GoModule {
            folder: folder.to_string(),
            path: path.to_string(),
        })
    }
}

/// What the name of an import of a Go file gives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GoPackage {
    /// A package of a module of the repository: its folder.
    Folder(String),
    /// The standard library or a package of another module.
    External,
}

/// The folder of a path: `""` for a file at the root.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The folder of a local Terraform module source, from the folder of the
/// module call: `./modules/bucket` from `infra` is `infra/modules/bucket`.
/// `None` for a remote source (a registry, git or a URL).
pub(crate) fn terraform_source_folder(here: &str, source: &str) -> Option<String> {
    if !(source.starts_with("./") || source.starts_with("../")) {
        return None;
    }
    let mut parts: Vec<&str> = here.split('/').filter(|p| !p.is_empty()).collect();
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The name that an import with no name gives, for a package outside the
/// repository: the last part of the path, with no major version
/// (`gopkg.in/yaml.v3` and `github.com/x/yaml/v3` give `yaml`).
fn external_package_name(path: &str) -> &str {
    let mut parts = path.rsplit('/');
    let mut last = parts.next().unwrap_or(path);
    if last.len() > 1 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit()) {
        last = parts.next().unwrap_or(last);
    }
    let last = match last.rsplit_once(".v") {
        Some((name, v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => name,
        _ => last,
    };
    last.strip_prefix("go-")
        .or_else(|| last.strip_suffix("-go"))
        .unwrap_or(last)
}

/// The place of a definition with no symbol: a function that a macro makes
/// (COLLIERY-T-2531).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTarget {
    /// The index of the parsed file.
    pub file: usize,
    /// From 1.
    pub line: usize,
    /// A macro invocation holds the place.
    pub macro_made: bool,
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
    /// The place of the definition, for a certain edge with no callee.
    pub target: Option<NewTarget>,
}

/// Where the edges of the languages with a SCIP runner come from
/// (KAIROS-T-0473).
pub enum ScipEdges<'a> {
    /// Name classes only: no runner gave an index.
    Names,
    /// The SCIP indexes of the build, the languages that they cover, and the
    /// crate of each Rust file.
    Scip(&'a ScipData),
    /// No SCIP run. A function of a language in `marked`, with the code of
    /// a function of a base index whose edges wait for no SCIP run, keeps its
    /// SCIP edges. The other functions of those languages get name classes,
    /// marked `scip_pending`.
    Base {
        bases: &'a [BaseIndex],
        marked: &'a HashSet<&'a str>,
    },
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
pub(crate) fn sym_keys<'a>(
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
    /// The languages of the files with SCIP edges or SCIP uses.
    languages: HashSet<String>,
    /// The SCIP references of its Rust files (KAIROS-T-0419).
    pub(crate) refs: crate::refs::BaseRefs,
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
    /// The place of a definition with no symbol: file, line, macro mark.
    target: Option<(String, usize, bool)>,
}

impl BaseIndex {
    /// The languages that the base has SCIP data for.
    pub fn scip_languages(&self) -> impl Iterator<Item = &str> {
        self.languages.iter().map(String::as_str)
    }

    /// Read the files of the index at `conn` whose language had SCIP: their
    /// symbols, their SCIP edges and the call sites that SCIP resolved. Rust
    /// files are always read.
    pub fn read(conn: &Connection) -> Result<Self, IndexError> {
        let mut files: HashMap<String, BaseFile> = HashMap::new();
        let mut paths: HashMap<i64, String> = HashMap::new();
        let languages: HashSet<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT f.language FROM files f
                 WHERE f.language IS NOT NULL
                   AND (f.id IN (SELECT file_id FROM refs_files)
                        OR f.id IN (SELECT s.file_id FROM edges e JOIN symbols s ON s.id = e.caller_id
                                    WHERE e.origin = 'scip'))",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        {
            let mut stmt = conn.prepare(
                "SELECT id, path FROM files
                 WHERE language = 'rust'
                    OR language IN (SELECT DISTINCT f.language FROM refs_files r
                                    JOIN files f ON f.id = r.file_id)
                    OR id IN (SELECT s.file_id FROM edges e JOIN symbols s ON s.id = e.caller_id
                              WHERE e.origin = 'scip')",
            )?;
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
                        e.scip_pending, t.path, e.target_line, e.target_macro
                 FROM edges e JOIN symbols c ON c.id = e.caller_id
                 LEFT JOIN files t ON t.id = e.target_file_id
                 WHERE e.origin = 'scip' OR e.scip_pending = 1",
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
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                    r.get::<_, bool>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    caller,
                    callee,
                    callee_name,
                    class,
                    origin,
                    line,
                    col,
                    pending,
                    target_file,
                    target_line,
                    target_macro,
                ) = row?;
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
                        target: target_file
                            .zip(target_line)
                            .map(|(f, l)| (f, l as usize, target_macro)),
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
        let refs = crate::refs::read_base(conn, &keys)?;
        Ok(BaseIndex {
            files,
            languages,
            refs,
        })
    }
}

/// The word of a called name that its definition line has: the last part
/// after `::` (Rust) and `.` (`api.bearer` of TypeScript, Python and Go,
/// KAIROS-T-0760). A SCIP tag in `<>` is cut: `Persona.<get>credentials`
/// gives `credentials`, and `CliError.<constructor>` gives `constructor`,
/// the word of the line that defines it. Pure.
fn last_name(name: &str) -> &str {
    let last = name
        .rsplit("::")
        .next()
        .and_then(|n| n.rsplit('.').next())
        .unwrap_or_default();
    match last.strip_prefix('<').and_then(|rest| rest.split_once('>')) {
        Some((tag, "")) => tag,
        Some((_, rest)) => rest,
        None => last,
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
                && file.text[at..].starts_with(is_name_char)
                && !file.text[..at].ends_with(is_name_char)
        })
    })
}

/// A character of a Rust name.
pub(crate) fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The kinds that a call can start in.
fn is_function(kind: &str) -> bool {
    matches!(kind, "function" | "method" | "constructor")
}

/// The kinds that a name can call: in Python, TypeScript and JavaScript, a
/// call of a class makes an object.
fn is_callable(language: &str, kind: &str) -> bool {
    is_function(kind)
        || (kind == "class" && matches!(language, "python" | "typescript" | "tsx" | "javascript"))
}

/// The languages whose names can call each other: TypeScript, TSX and
/// JavaScript are one family (KAIROS-T-0352).
fn family(language: &str) -> &str {
    match language {
        "tsx" | "javascript" => "typescript",
        other => other,
    }
}

/// The smallest function of `file` whose span holds `byte`, or the
/// `macro_rules!` definition that holds it: a call in the body of a macro
/// has the macro as its caller (KAIROS-T-0353).
fn caller_at(file: &ParsedFile<'_>, byte: usize) -> Option<i64> {
    let terraform = file.language == "terraform";
    file.symbols
        .iter()
        .filter(|s| {
            (is_function(s.kind) || s.kind == "macro" || terraform)
                && s.start_byte <= byte
                && byte < s.end_byte
        })
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| s.id)
}

/// The module of a Rust file, from its path: the folder of the crate root
/// (`crates/kairos-db/src`), the crate name (`kairos_db`) and the modules
/// under the root (`["boards"]`). `lib.rs`, `main.rs` and `mod.rs` add no
/// module.
struct RustModule {
    root: String,
    crate_name: Option<String>,
    modules: Vec<String>,
}

fn rust_module(path: &str) -> RustModule {
    let parts: Vec<&str> = path.split('/').collect();
    let at = parts[..parts.len().saturating_sub(1)]
        .iter()
        .rposition(|p| matches!(*p, "src" | "tests" | "examples" | "benches"));
    let (root, crate_name, rest) = match at {
        Some(i) => (
            parts[..=i].join("/"),
            i.checked_sub(1).map(|j| parts[j].replace('-', "_")),
            &parts[i + 1..],
        ),
        None => (String::new(), None, &parts[..]),
    };
    let mut modules: Vec<String> = rest
        .iter()
        .map(|p| p.strip_suffix(".rs").unwrap_or(p).to_string())
        .collect();
    if modules
        .last()
        .is_some_and(|m| matches!(m.as_str(), "lib" | "main" | "mod"))
    {
        modules.pop();
    }
    RustModule {
        root,
        crate_name,
        modules,
    }
}

/// A container without its generic part: `Stack<T>` gives `Stack`.
fn plain(container: Option<&str>) -> Option<&str> {
    container.map(|c| c.split('<').next().unwrap_or(c).trim())
}

/// The path before the name of a qualified Rust call, by part:
/// `crate::boards::f` gives `["crate", "boards"]`. `None` for a call with no
/// `::` path.
fn rust_qualifier<'w>(written: &'w str, name: &str) -> Option<Vec<&'w str>> {
    let path = written.strip_suffix(name)?.strip_suffix("::")?;
    let parts: Vec<&str> = path.split("::").collect();
    (!parts.is_empty() && parts.iter().all(|p| !p.is_empty() && !p.contains('.'))).then_some(parts)
}

/// A function that a name can call: (file index, symbol id).
type Hit = (usize, i64);

struct Names<'a> {
    files: &'a [ParsedFile<'a>],
    /// (language family, name) to (file index, symbol id).
    by_name: HashMap<(&'a str, &'a str), Vec<(usize, i64)>>,
    /// The module of each Rust file, by file index.
    modules: HashMap<usize, RustModule>,
    /// The packages that the imports of each Go file name, by file index
    /// and by the name in the code (KAIROS-T-0469).
    go_imports: HashMap<usize, HashMap<String, GoPackage>>,
    /// The Terraform blocks by folder, kind, container and name
    /// (KAIROS-T-0471).
    terraform: HashMap<TerraformKey<'a>, Vec<i64>>,
    /// The source of each Terraform module call, by folder and name.
    terraform_modules: HashMap<(&'a str, &'a str), &'a str>,
}

/// A Terraform block: its folder, its kind, its container and its name.
type TerraformKey<'a> = (&'a str, &'a str, Option<&'a str>, &'a str);

impl<'a> Names<'a> {
    fn new(files: &'a [ParsedFile<'a>], go_modules: &[GoModule]) -> Self {
        let mut by_name: HashMap<(&str, &str), Vec<(usize, i64)>> = HashMap::new();
        let mut modules = HashMap::new();
        // The package name of each Go folder, from its files (not the
        // `_test` package of a test file).
        let mut packages: HashMap<&str, &str> = HashMap::new();
        for file in files.iter().filter(|f| f.language == "go") {
            if let Some(package) = file.package.filter(|p| !p.ends_with("_test")) {
                packages.entry(folder_of(file.path)).or_insert(package);
            }
        }
        let mut go_imports: HashMap<usize, HashMap<String, GoPackage>> = HashMap::new();
        for (i, file) in files.iter().enumerate() {
            if file.language != "go" {
                continue;
            }
            let mut names = HashMap::new();
            for import in file.imports {
                if matches!(import.name.as_deref(), Some("_" | ".")) {
                    continue;
                }
                let module = go_modules
                    .iter()
                    .filter(|m| {
                        import.path == m.path
                            || import
                                .path
                                .strip_prefix(m.path.as_str())
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
                    .max_by_key(|m| m.path.len());
                let (package, default_name) = match module {
                    Some(m) => {
                        let rest = import.path[m.path.len()..].trim_start_matches('/');
                        let folder = match (m.folder.as_str(), rest) {
                            ("", rest) => rest.to_string(),
                            (folder, "") => folder.to_string(),
                            (folder, rest) => format!("{folder}/{rest}"),
                        };
                        let name = packages
                            .get(folder.as_str())
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| {
                                import
                                    .path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or_default()
                                    .to_string()
                            });
                        (GoPackage::Folder(folder), name)
                    }
                    None => (
                        GoPackage::External,
                        external_package_name(&import.path).to_string(),
                    ),
                };
                names.insert(import.name.clone().unwrap_or(default_name), package);
            }
            go_imports.insert(i, names);
        }
        for (i, file) in files.iter().enumerate() {
            if file.language == "rust" {
                modules.insert(i, rust_module(file.path));
            }
            for s in &file.symbols {
                if is_callable(file.language, s.kind) {
                    by_name
                        .entry((family(file.language), s.name.as_str()))
                        .or_default()
                        .push((i, s.id));
                }
            }
        }
        let mut terraform: HashMap<TerraformKey<'a>, Vec<i64>> = HashMap::new();
        let mut terraform_modules = HashMap::new();
        for file in files.iter().filter(|f| f.language == "terraform") {
            let folder = folder_of(file.path);
            for s in &file.symbols {
                terraform
                    .entry((folder, s.kind, s.container.as_deref(), s.name.as_str()))
                    .or_default()
                    .push(s.id);
            }
            for import in file.imports {
                if let Some(name) = &import.name {
                    terraform_modules.insert((folder, name.as_str()), import.path.as_str());
                }
            }
        }
        Names {
            files,
            by_name,
            modules,
            go_imports,
            terraform,
            terraform_modules,
        }
    }

    fn sym(&self, file: usize, id: i64) -> Option<&Sym> {
        self.files[file].symbols.iter().find(|s| s.id == id)
    }

    /// Whether the function `id` of file `file` matches the path `qualifier`
    /// of a call from `caller` in file `from`: its container or its module
    /// is the last part of the path (COLLIERY-T-2531).
    fn matches_path(
        &self,
        qualifier: &[&str],
        from: usize,
        caller: Option<&Sym>,
        file: usize,
        id: i64,
    ) -> bool {
        let (Some(here), Some(there), Some(s)) = (
            self.modules.get(&from),
            self.modules.get(&file),
            self.sym(file, id),
        ) else {
            return false;
        };
        let same_crate = here.root == there.root;
        match *qualifier.last().expect("a qualifier has a part") {
            "self" => file == from,
            "Self" => {
                let own = plain(caller.and_then(|c| c.container.as_deref()));
                own.is_some() && plain(s.container.as_deref()) == own
            }
            "crate" => same_crate && there.modules.is_empty(),
            "super" => {
                same_crate
                    && (file == from
                        || here.modules.split_last().map(|(_, parent)| parent)
                            == Some(&there.modules[..]))
            }
            part => {
                plain(s.container.as_deref()) == Some(part)
                    || there.modules.last().is_some_and(|m| m == part)
                    || (there.modules.is_empty() && there.crate_name.as_deref() == Some(part))
            }
        }
    }

    /// The edge of a Terraform reference from file `file` (KAIROS-T-0471).
    fn terraform_edge(
        &self,
        file: usize,
        caller_id: i64,
        call: &CallSite,
        stats: &mut EdgeStats,
    ) -> NewEdge {
        let here = folder_of(self.files[file].path);
        // The folder of a module call of this folder: `None` when the call
        // is not known or its source is remote.
        let module_folder = |z: &str| {
            self.terraform_modules
                .get(&(here, z))
                .and_then(|source| terraform_source_folder(here, source))
        };
        let find = |folder: &str, kind: &str, container: Option<&str>, name: &str| -> Vec<i64> {
            self.terraform
                .get(&(folder, kind, container, name))
                .cloned()
                .unwrap_or_default()
        };
        let parts: Vec<&str> = call.written.split('.').collect();
        let found = match (call.module.as_deref(), parts.as_slice()) {
            (Some(z), _) => module_folder(z)
                .map(|f| find(&f, "variable", Some("var"), &call.name))
                .unwrap_or_default(),
            (None, ["var", x]) => find(here, "variable", Some("var"), x),
            (None, ["local", x]) => find(here, "local", Some("local"), x),
            (None, ["module", z]) => find(here, "module", Some("module"), z),
            (None, ["module", z, o]) => module_folder(z)
                .map(|f| find(&f, "output", None, o))
                .unwrap_or_default(),
            (None, ["data", t, n]) => find(here, "data", Some(&format!("data.{t}")), n),
            (None, [t, n]) => find(here, "resource", Some(t), n),
            _ => Vec::new(),
        };
        let (class, callee_id, candidates) = match found.as_slice() {
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
            callee_name: call.written.clone(),
            class,
            origin: "name",
            line: call.line,
            col: call.col,
            candidates,
            scip_pending: false,
            target: None,
        }
    }

    /// The name class of a call of `name` from file `file`. A qualified Rust
    /// call (`a::f`) goes only to a function whose container or module
    /// matches its path. A method call (`x.f`) does not go to the caller
    /// itself (COLLIERY-T-2531).
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
        let prefer_same_file = |pool: Vec<(usize, i64)>| -> Vec<i64> {
            let same: Vec<i64> = pool
                .iter()
                .filter(|(f, _)| *f == file)
                .map(|(_, id)| *id)
                .collect();
            if same.is_empty() {
                pool.iter().map(|(_, id)| *id).collect()
            } else {
                same
            }
        };
        let qualifier = if language == "rust" {
            rust_qualifier(written, name)
        } else {
            None
        };
        // A Go call `pkg.F` of an imported package (KAIROS-T-0469).
        let go_package = (language == "go")
            .then(|| written.strip_suffix(name)?.strip_suffix('.'))
            .flatten()
            .filter(|q| !q.contains('.'))
            .and_then(|q| self.go_imports.get(&file)?.get(q));
        let (chosen, unmatched) = match (&qualifier, go_package) {
            (_, Some(GoPackage::External)) => (Vec::new(), Vec::new()),
            (_, Some(GoPackage::Folder(folder))) => {
                let chosen = all
                    .iter()
                    .filter(|(f, id)| {
                        let there = &self.files[*f];
                        there.language == "go"
                            && folder_of(there.path) == folder
                            && self.sym(*f, *id).is_some_and(|s| s.kind == "function")
                    })
                    .map(|(_, id)| *id)
                    .collect();
                (chosen, Vec::new())
            }
            // A plain Go call goes to its own package first: the folder.
            (_, None) if language == "go" && !written.contains('.') => {
                let here = folder_of(self.files[file].path);
                let in_folder: Vec<(usize, i64)> = all
                    .iter()
                    .copied()
                    .filter(|(f, _)| folder_of(self.files[*f].path) == here)
                    .collect();
                if in_folder.is_empty() {
                    (prefer_same_file(all.to_vec()), Vec::new())
                } else {
                    (prefer_same_file(in_folder), Vec::new())
                }
            }
            (qualifier, None) => match qualifier {
                Some(qualifier) => {
                    let caller = self.sym(file, caller_id);
                    let (matching, other): (Vec<Hit>, Vec<Hit>) = all
                        .iter()
                        .partition(|(f, id)| self.matches_path(qualifier, file, caller, *f, *id));
                    let other: Vec<i64> = other
                        .into_iter()
                        .map(|(_, id)| id)
                        .filter(|id| *id != caller_id)
                        .collect();
                    (prefer_same_file(matching), other)
                }
                None => {
                    let method_call = written.contains('.');
                    let pool: Vec<(usize, i64)> = all
                        .iter()
                        .copied()
                        .filter(|(_, id)| !(method_call && *id == caller_id))
                        .collect();
                    (prefer_same_file(pool), Vec::new())
                }
            },
        };
        let (class, callee_id, candidates) = match (chosen.as_slice(), unmatched.as_slice()) {
            ([], []) => {
                stats.external_name += 1;
                ("external", None, Vec::new())
            }
            ([one], _) => {
                stats.certain_name += 1;
                ("certain", Some(*one), Vec::new())
            }
            ([], many) | (many, _) => {
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
            target: None,
        }
    }
}

/// The byte offset of a SCIP position (line and column from 0).
pub(crate) fn byte_offset(
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

pub(crate) fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// Where a SCIP reference goes.
pub(crate) enum Target {
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
pub(crate) fn choose(
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
pub fn resolve(files: &[ParsedFile<'_>], scip: ScipEdges<'_>, go_modules: &[GoModule]) -> Resolved {
    let no_crates = scip::Crates::default();
    let crates = match &scip {
        ScipEdges::Scip(data) => &data.crates,
        _ => &no_crates,
    };
    let names = Names::new(files, go_modules);
    let by_path: HashMap<&str, usize> =
        files.iter().enumerate().map(|(i, f)| (f.path, i)).collect();
    let starts: Vec<Vec<usize>> = files.iter().map(|f| line_starts(f.text)).collect();
    let mut stats = EdgeStats::default();
    let mut edges = Vec::new();
    let mut covered = Vec::new();

    // The documents of the SCIP indexes, by file, and the definition of
    // each symbol.
    let mut scip_docs: HashMap<usize, &proto::Document> = HashMap::new();
    let mut definitions: HashMap<&str, Vec<(Option<usize>, usize)>> = HashMap::new();
    if let ScipEdges::Scip(data) = &scip {
        for doc in &data.index.documents {
            let file = by_path.get(doc.relative_path.as_str()).copied();
            if let Some(f) = file.filter(|&f| data.covers(files[f].language)) {
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
    if let ScipEdges::Base { .. } = &scip {
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
    // The place of each call site, for the calls of a class.
    let call_starts: HashSet<(usize, usize)> = files
        .iter()
        .enumerate()
        .flat_map(|(i, f)| f.calls.iter().map(move |c| (i, c.start_byte)))
        .collect();

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
            let rust = file.language == "rust";
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
                // Each reference of rust-analyzer resolves its place: a tuple
                // struct or an enum variant gets no name class. In the other
                // languages, only a place with a SCIP edge is resolved: a
                // call of a value or of a type that SCIP does not join keeps
                // its name class (KAIROS-T-0473).
                if rust {
                    resolved_at.insert(byte);
                }
                // An operator (`*`, `+`, a deref) is a reference to its trait
                // method in newer rust-analyzer releases. It is not a call
                // that the code writes, so it is not an edge.
                if occ.symbol_roles & proto::ROLE_DEFINITION != 0
                    || file.use_ranges.iter().any(|&(s, e)| s <= byte && byte < e)
                    || !file.text[byte..].starts_with(is_name_char)
                {
                    continue;
                }
                let Some(descriptors) = scip::descriptors(&occ.symbol) else {
                    continue;
                };
                let Some(caller_id) = caller_at(file, byte) else {
                    continue;
                };
                let line = occ.range[0] as usize + 1;
                // The column in bytes, whatever the encoding of the index.
                let col = byte - starts[i][occ.range[0] as usize] + 1;
                let defs = definitions
                    .get(occ.symbol.as_str())
                    .map_or(&[][..], Vec::as_slice);
                // A term at a call site is called too, outside Rust: a method
                // of a Go interface, a function in a variable.
                let at_call = !rust && call_starts.contains(&(i, byte));
                let Some((name, short)) = scip::function_name(&descriptors, family(file.language))
                    .or_else(|| {
                        at_call
                            .then(|| scip::term_name(&descriptors, family(file.language)))
                            .flatten()
                    })
                else {
                    // A call of a class makes an object (Python, TypeScript,
                    // JavaScript): the reference is to the type.
                    if rust || !call_starts.contains(&(i, byte)) {
                        continue;
                    }
                    let Some((type_name, short)) = scip::type_name(&descriptors) else {
                        continue;
                    };
                    let Target::Indexed(def_file, def_byte) =
                        choose(defs, file.path, files, crates)
                    else {
                        continue;
                    };
                    let callee = files[def_file]
                        .symbols
                        .iter()
                        .filter(|s| {
                            is_callable(files[def_file].language, s.kind)
                                && s.name == type_name
                                && s.start_byte <= def_byte
                                && def_byte < s.end_byte
                        })
                        .min_by_key(|s| s.end_byte - s.start_byte)
                        .map(|s| s.id);
                    if let Some(callee_id) = callee {
                        resolved_at.insert(byte);
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
                            target: None,
                        });
                    }
                    continue;
                };
                if !rust {
                    resolved_at.insert(byte);
                }
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
                                    target: None,
                                });
                            }
                            // A definition with no symbol: a function that a
                            // macro makes. The edge keeps its place and goes
                            // to no function by name (COLLIERY-T-2531).
                            None => {
                                stats.scip_join_misses += 1;
                                stats.certain_scip += 1;
                                let def = &files[def_file];
                                edges.push(NewEdge {
                                    caller_id,
                                    callee_id: None,
                                    callee_name: short,
                                    class: "certain",
                                    origin: "scip",
                                    line,
                                    col,
                                    candidates: Vec::new(),
                                    scip_pending: false,
                                    target: Some(NewTarget {
                                        file: def_file,
                                        line: starts[def_file].partition_point(|&s| s <= def_byte),
                                        macro_made: def
                                            .macro_ranges
                                            .iter()
                                            .any(|&(s, e)| s <= def_byte && def_byte < e),
                                    }),
                                });
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
                            target: None,
                        });
                    }
                }
            }
        } else if let ScipEdges::Base { bases, marked } = &scip
            && marked.contains(file.language)
        {
            // A file that the SCIP run did not cover (KAIROS-A-0083): an edit
            // does not make the next run cover it, so its edges by name do
            // not wait for that run.
            pending = !crate::refs::not_covered(bases, file.path);
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
                    // A definition with no symbol keeps its place while the
                    // line there still has the called name; else a name
                    // class until the next SCIP run (COLLIERY-T-2531).
                    let mut target = None;
                    if let (None, Some((path, at, macro_made))) = (&e.callee, &e.target) {
                        let last = last_name(&e.callee_name);
                        let place = by_path.get(path.as_str()).copied().filter(|&f| {
                            files[f]
                                .text
                                .lines()
                                .nth(at.saturating_sub(1))
                                .is_some_and(|l| !last.is_empty() && l.contains(last))
                        });
                        match place {
                            Some(f) => {
                                target = Some(NewTarget {
                                    file: f,
                                    line: *at,
                                    macro_made: *macro_made,
                                });
                            }
                            None => {
                                let mut edge = names.edge(
                                    file.language,
                                    i,
                                    caller_id,
                                    last,
                                    &e.callee_name,
                                    line,
                                    e.col,
                                    &mut stats,
                                );
                                edge.scip_pending = true;
                                stats.pending += 1;
                                edges.push(edge);
                                continue;
                            }
                        }
                    }
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
                                edge.scip_pending = true;
                                stats.pending += 1;
                                edges.push(edge);
                                continue;
                            }
                        },
                    };
                    stats.kept_scip += 1;
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
                        target,
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
            if file.language == "terraform" {
                edges.push(names.terraform_edge(i, caller_id, call, &mut stats));
                continue;
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
            // SCIP gives no reference in the body of a macro_rules!
            // definition, so a SCIP run does not replace such an edge
            // (KAIROS-T-0353).
            let in_macro_body = file
                .symbols
                .iter()
                .any(|s| s.id == caller_id && s.kind == "macro");
            edge.scip_pending = pending && !kept.contains(&caller_id) && !in_macro_body;
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

    /// KAIROS-T-0469: a Go call `pkg.F` goes to `F` in the folder of the
    /// imported package; a package outside the module is external.
    #[test]
    fn a_go_call_goes_to_the_imported_package() {
        let imports = [
            Import {
                name: None,
                path: "example.com/m/a".into(),
            },
            Import {
                name: Some("bee".into()),
                path: "example.com/m/b".into(),
            },
            Import {
                name: None,
                path: "fmt".into(),
            },
            Import {
                name: None,
                path: "gopkg.in/yaml.v3".into(),
            },
        ];
        let calls = [
            call("F", "a.F", 5),
            call("F", "bee.F", 6),
            call("Println", "fmt.Println", 7),
            call("Marshal", "yaml.Marshal", 8),
            call("F", "x.F", 9),
            call("G", "G", 10),
        ];
        let go = |path, symbols, calls, imports| ParsedFile {
            path,
            language: "go",
            text: "",
            symbols,
            calls,
            use_ranges: &[],
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports,
        };
        let files = [
            go(
                "cmd/main.go",
                vec![sym(1, "main", "function", (0, 50))],
                &calls[..],
                &imports[..],
            ),
            go("a/a.go", vec![sym(2, "F", "function", (0, 9))], &[], &[]),
            go(
                "b/b.go",
                vec![
                    sym(3, "F", "function", (0, 9)),
                    sym(4, "Marshal", "function", (10, 20)),
                ],
                &[],
                &[],
            ),
            go("cmd/g.go", vec![sym(5, "G", "function", (0, 9))], &[], &[]),
            go(
                "other/g.go",
                vec![sym(6, "G", "function", (0, 9))],
                &[],
                &[],
            ),
        ];
        let modules =
            [GoModule::read("", "// x\nmodule example.com/m // the root\n\ngo 1.22\n").unwrap()];
        assert_eq!(modules[0].path, "example.com/m");
        let Resolved { edges, .. } = resolve(&files, ScipEdges::Names, &modules);
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
                ("a.F", "certain", Some(2), vec![]),
                ("bee.F", "certain", Some(3), vec![]),
                ("fmt.Println", "external", None, vec![]),
                ("yaml.Marshal", "external", None, vec![]),
                // A call on a value is name-matched, as before.
                ("x.F", "possible", None, vec![2, 3]),
                // A plain call goes to its own folder.
                ("G", "certain", Some(5), vec![]),
            ]
        );
    }

    /// KAIROS-T-0471: a Terraform reference stays in its folder.
    #[test]
    fn a_terraform_reference_stays_in_its_folder() {
        let mut topic_a = sym(1, "events", "resource", (0, 40));
        topic_a.container = Some("aws_sns_topic".into());
        let mut out_a = sym(2, "topic", "output", (50, 90));
        out_a.container = None;
        let mut topic_b = sym(3, "events", "resource", (0, 40));
        topic_b.container = Some("aws_sns_topic".into());
        let mut use_b = sym(4, "copy", "resource", (50, 90));
        use_b.container = Some("aws_sns_topic".into());
        let reference = |at| CallSite {
            name: "events".into(),
            written: "aws_sns_topic.events".into(),
            start_byte: at,
            end_byte: at + 6,
            line: 1,
            col: 1,
            module: None,
        };
        let a_calls = [reference(60)];
        let b_calls = [reference(60)];
        let tf = |path, symbols, calls| ParsedFile {
            path,
            language: "terraform",
            text: "",
            symbols,
            calls,
            use_ranges: &[],
            macro_calls: &[],
            macro_ranges: &[],
            package: None,
            imports: &[],
        };
        let files = [
            tf("a/main.tf", vec![topic_a, out_a], &a_calls[..]),
            tf("b/main.tf", vec![topic_b, use_b], &b_calls[..]),
        ];
        let Resolved { edges, .. } = resolve(&files, ScipEdges::Names, &[]);
        let got: Vec<_> = edges
            .iter()
            .map(|e| (e.caller_id, e.class, e.callee_id))
            .collect();
        assert_eq!(got, [(2, "certain", Some(1)), (4, "certain", Some(3))]);
    }

    #[test]
    fn the_folder_of_a_terraform_module_source() {
        assert_eq!(
            terraform_source_folder("infra", "./modules/bucket").as_deref(),
            Some("infra/modules/bucket")
        );
        assert_eq!(
            terraform_source_folder("infra/live", "../modules/net/").as_deref(),
            Some("infra/modules/net")
        );
        assert_eq!(terraform_source_folder("", "./m").as_deref(), Some("m"));
        assert_eq!(terraform_source_folder("", "../m"), None);
        assert_eq!(
            terraform_source_folder("infra", "terraform-aws-modules/vpc/aws"),
            None
        );
        assert_eq!(
            terraform_source_folder("infra", "git::https://example.com/m.git"),
            None
        );
    }

    #[test]
    fn the_name_of_an_external_go_package() {
        for (path, name) in [
            ("fmt", "fmt"),
            ("net/http", "http"),
            ("gopkg.in/yaml.v3", "yaml"),
            ("github.com/x/yaml/v3", "yaml"),
            ("github.com/mattn/go-sqlite3", "sqlite3"),
        ] {
            assert_eq!(external_package_name(path), name, "{path}");
        }
    }

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

    fn rust_marked() -> HashSet<&'static str> {
        HashSet::from(["rust"])
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
            module: None,
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
                macro_ranges: &[],
                package: None,
                imports: &[],
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
                macro_ranges: &[],
                package: None,
                imports: &[],
            },
            ParsedFile {
                path: "c.go",
                language: "go",
                text: "",
                symbols: vec![sym(5, "load", "function", (0, 9))],
                calls: &[],
                use_ranges: &[],
                macro_calls: &[],
                macro_ranges: &[],
                package: None,
                imports: &[],
            },
        ];
        let Resolved { edges, stats, .. } = resolve(&files, ScipEdges::Names, &[]);
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
            target: None,
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
            ..Default::default()
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
            macro_ranges: &[],
            package: None,
            imports: &[],
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
            ScipEdges::Base {
                bases: &bases,
                marked: &rust_marked(),
            },
            &[],
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
    fn a_file_that_the_scip_run_did_not_cover_has_no_pending_edges() {
        // `a` changed, in a file that the SCIP run of the base did not cover
        // (KAIROS-T-0760): its edge by name does not wait for a SCIP run.
        let text = "fn a() { b(); }\nfn b() {}\n";
        let mut a = sym(1, "a", "function", (0, 15));
        a.tree_hash = "new".into();
        let b = sym(2, "b", "function", (16, 25));
        let calls = [call("b", "b", 9)];
        let files = [rust_file(text, vec![a, b], &calls)];
        let mut not_covered = base();
        not_covered.refs = crate::refs::BaseRefs::not_covering(&["src/lib.rs"]);
        for (bases, pending) in [([base()], true), ([not_covered], false)] {
            let Resolved { edges, .. } = resolve(
                &files,
                ScipEdges::Base {
                    bases: &bases,
                    marked: &rust_marked(),
                },
                &[],
            );
            let got: Vec<_> = edges
                .iter()
                .map(|e| (e.caller_id, e.scip_pending))
                .collect();
            assert_eq!(got, [(1, pending)]);
        }
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
            ScipEdges::Base {
                bases: &bases,
                marked: &rust_marked(),
            },
            &[],
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

    /// A function of `path` with a container.
    fn file<'a>(
        path: &'a str,
        text: &'a str,
        symbols: Vec<Sym>,
        calls: &'a [CallSite],
    ) -> ParsedFile<'a> {
        ParsedFile {
            path,
            ..rust_file(text, symbols, calls)
        }
    }

    #[test]
    fn a_qualified_call_needs_its_module_or_its_container() {
        let mut new = sym(3, "new", "function", (0, 9));
        new.container = Some("Stack".into());
        let mut get = sym(4, "get", "function", (0, 40));
        get.container = Some("Wrapper".into());
        let run_calls = [
            call("start", "b::start", 1),
            call("start", "c::start", 2),
            call("new", "Stack::new", 3),
            call("start", "crate::c::start", 4),
            call("start", "super::start", 5),
        ];
        let get_calls = [call("get", "self.inner.get", 10)];
        let files = [
            file(
                "crates/x-y/src/a.rs",
                "",
                vec![sym(1, "run", "function", (0, 9))],
                &run_calls,
            ),
            file(
                "crates/x-y/src/c.rs",
                "",
                vec![sym(2, "start", "function", (0, 9))],
                &[],
            ),
            file("crates/x-y/src/stack.rs", "", vec![new], &[]),
            file("crates/x-y/src/wrapper.rs", "", vec![get], &get_calls),
        ];
        let Resolved { edges, .. } = resolve(&files, ScipEdges::Names, &[]);
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
                // Module b has no start: not the start of module c.
                ("b::start", "possible", None, vec![2]),
                ("c::start", "certain", Some(2), vec![]),
                ("Stack::new", "certain", Some(3), vec![]),
                ("crate::c::start", "certain", Some(2), vec![]),
                // The parent of module a is the crate root, which has no
                // start.
                ("super::start", "possible", None, vec![2]),
                // A method call does not go to the caller by name.
                ("self.inner.get", "external", None, vec![]),
            ]
        );
    }

    #[test]
    fn the_module_of_a_rust_file_comes_from_its_path() {
        let m = rust_module("crates/kairos-db/src/boards.rs");
        assert_eq!(
            (m.root.as_str(), m.crate_name.as_deref(), m.modules),
            (
                "crates/kairos-db/src",
                Some("kairos_db"),
                vec!["boards".to_string()]
            )
        );
        let m = rust_module("crates/kairos-server/src/api/mod.rs");
        assert_eq!(m.modules, ["api"]);
        let m = rust_module("src/lib.rs");
        assert_eq!(
            (m.root.as_str(), m.crate_name, m.modules.len()),
            ("src", None, 0)
        );
        let m = rust_module("tests/common/mod.rs");
        assert_eq!(m.modules, ["common"]);
    }

    /// A base where `a` (line 1) calls `m::f`, which `make!` (line 2) makes.
    fn macro_base() -> BaseIndex {
        let text = "fn a() { m::f(); }\nmake!(f);\n";
        let a_end = text.find('\n').unwrap();
        let file = BaseFile {
            covered: vec![12],
            symbols: HashMap::from([(
                key("a"),
                BaseSym {
                    tree_hash: "hash a".into(),
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: a_end,
                    pending: false,
                    edges: vec![BaseEdge {
                        callee: None,
                        callee_name: "m::f".into(),
                        class: "certain".into(),
                        line: 1,
                        col: 13,
                        target: Some(("src/lib.rs".into(), 2, true)),
                    }],
                },
            )]),
        };
        BaseIndex {
            files: HashMap::from([("src/lib.rs".to_string(), file)]),
            ..Default::default()
        }
    }

    #[test]
    fn a_kept_edge_keeps_the_place_of_a_made_function() {
        let mut a = sym(1, "a", "function", (0, 18));
        a.tree_hash = "hash a".into();
        let calls = [call("f", "m::f", 12)];
        let bases = [macro_base()];
        let resolve_text = |text: &str| {
            let files = [rust_file(text, vec![sym_like(&a)], &calls)];
            resolve(
                &files,
                ScipEdges::Base {
                    bases: &bases,
                    marked: &rust_marked(),
                },
                &[],
            )
            .edges
        };
        let kept = resolve_text("fn a() { m::f(); }\nmake!(f);\n");
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            (kept[0].class, kept[0].origin, kept[0].callee_id),
            ("certain", "scip", None)
        );
        assert_eq!(
            kept[0].target,
            Some(NewTarget {
                file: 0,
                line: 2,
                macro_made: true
            })
        );
        // The made function is gone: a name class until the next SCIP run.
        let gone = resolve_text("fn a() { m::f(); }\nmake!(g);\n");
        assert_eq!(gone.len(), 1, "{gone:?}");
        assert_eq!(
            (gone[0].origin, gone[0].scip_pending, gone[0].target.clone()),
            ("name", true, None)
        );
    }

    #[test]
    fn the_last_part_of_a_called_name_is_after_the_last_separator() {
        assert_eq!(last_name("m::f"), "f");
        assert_eq!(last_name("api.bearer"), "bearer");
        assert_eq!(last_name("crate::m::T::f"), "f");
        assert_eq!(last_name("f"), "f");
        assert_eq!(last_name("CliError.<constructor>"), "constructor");
        assert_eq!(last_name("Persona.<get>credentials"), "credentials");
    }

    /// KAIROS-T-0760: a kept edge with a dotted name (TypeScript, Python,
    /// Go) keeps its place, and does not wait for a SCIP run.
    #[test]
    fn a_kept_edge_with_a_dotted_name_keeps_its_place() {
        let mut a = sym(1, "a", "function", (0, 18));
        a.tree_hash = "hash a".into();
        let calls = [call("f", "m::f", 12)];
        let mut base = macro_base();
        for f in base.files.values_mut() {
            for s in f.symbols.values_mut() {
                for e in &mut s.edges {
                    e.callee_name = "api.f".into();
                }
            }
        }
        let bases = [base];
        let files = [rust_file(
            "fn a() { m::f(); }\nmake!(f);\n",
            vec![sym_like(&a)],
            &calls,
        )];
        let edges = resolve(
            &files,
            ScipEdges::Base {
                bases: &bases,
                marked: &rust_marked(),
            },
            &[],
        )
        .edges;
        assert_eq!(edges.len(), 1, "{edges:?}");
        assert_eq!((edges[0].origin, edges[0].scip_pending), ("scip", false));
    }

    fn sym_like(s: &Sym) -> Sym {
        Sym {
            id: s.id,
            name: s.name.clone(),
            container: s.container.clone(),
            kind: s.kind,
            start_line: s.start_line,
            end_line: s.end_line,
            start_byte: s.start_byte,
            end_byte: s.end_byte,
            tree_hash: s.tree_hash.clone(),
        }
    }
}
