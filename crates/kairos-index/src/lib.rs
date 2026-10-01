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
//! ([`Index`]). The edges (COLLIERY-T-1849) and the summaries
//! (COLLIERY-T-1850) come later; their tables are in the schema already.

mod extract;
pub mod rules;
mod schema;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use kairos_narsil::parser::LanguageParser;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

pub use rules::{Decision, Origin, RuleSet};
pub use schema::SCHEMA_VERSION;

/// A file larger than this is read only in part: enough for the header and
/// the rules. The default rules exclude it (`too-large`), so it is never
/// parsed.
const READ_LIMIT: u64 = 16 * 1024 * 1024;

/// What a build of the structure did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub files: usize,
    pub parsed_files: usize,
    pub symbols: usize,
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
}

/// An error of the index. The texts follow ASD-STE100: a user can see them.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("The repository root {0} is not a folder.")]
    NotAFolder(PathBuf),
    #[error("Cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read the tree: {0}")]
    Walk(#[from] ignore::Error),
    #[error("The rules file {path} is not valid: {message}")]
    Rules { path: PathBuf, message: String },
    #[error("The index has schema version {found}. This version of Kairos reads version 1.")]
    SchemaVersion { found: i64 },
    #[error("The index database has an error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("The parser did not start: {0}")]
    Parser(String),
}

/// Build the structure of the tree at `root` into the index at `db`.
///
/// The index file is made if it is not there. Its structure is deleted and
/// written again; its summary pool is kept. The repository's rules are read
/// from `.kairos/index-rules.toml` under `root`.
pub fn build_structure(root: &Path, db: &Path) -> Result<BuildReport, IndexError> {
    if !root.is_dir() {
        return Err(IndexError::NotAFolder(root.to_path_buf()));
    }
    let rules = RuleSet::for_repository(root)?;
    let parser = LanguageParser::new().map_err(|e| IndexError::Parser(e.to_string()))?;
    let paths = walk(root)?;

    let mut conn = Connection::open(db)?;
    schema::prepare(&conn)?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "DELETE FROM edge_candidates; DELETE FROM edges; DELETE FROM symbols; DELETE FROM files;",
    )?;

    let mut report = BuildReport {
        files: 0,
        parsed_files: 0,
        symbols: 0,
    };
    {
        let mut insert_file = tx.prepare(
            "INSERT INTO files (path, language, decision, rule, rule_origin, size, longest_line, content_hash, parse_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        let mut insert_symbol = tx.prepare(
            "INSERT INTO symbols (file_id, name, container, kind, language, start_line, end_line,
                                  start_byte, end_byte, signature, tree_hash, is_test)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;

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

            let mut symbols = Vec::new();
            let mut parse_error = None;
            if let (true, Some(language)) = (verdict.decision.is_parsed(), language) {
                if !file.complete || !file.utf8 {
                    parse_error = Some("the file is not complete UTF-8 text".to_string());
                } else {
                    match extract::extract(
                        &parser,
                        rel,
                        language,
                        &file.text,
                        verdict.decision == Decision::Test,
                    ) {
                        Ok(found) => symbols = found,
                        Err(e) => parse_error = Some(e),
                    }
                    report.parsed_files += 1;
                }
            }

            insert_file.execute(params![
                rel,
                language,
                verdict.decision.as_str(),
                verdict.rule,
                verdict.origin.as_str(),
                file.size as i64,
                file.longest_line as i64,
                file.content_hash,
                parse_error,
            ])?;
            let file_id = tx.last_insert_rowid();
            report.files += 1;

            for s in &symbols {
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
                ])?;
            }
            report.symbols += symbols.len();
        }
    }
    tx.commit()?;
    Ok(report)
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
    let mut files = Vec::new();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let abs = entry.into_path();
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
                    s.start_byte, s.end_byte, s.signature, s.tree_hash, s.is_test
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
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
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
}
