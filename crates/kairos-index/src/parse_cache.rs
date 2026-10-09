//! The parse cache of an index (KAIROS-T-0297): what the parser found in
//! each file, so that an update parses only the files that changed.
//!
//! The parse is most of the time of an update: 2.1 s of 2.8 s on Kairos,
//! also when no file changed. An entry holds the symbols and the call sites
//! of one file. Its key is a hash of the path, the language, the test mark
//! and the content hash of the file, and of the version of this code. So an
//! entry is used only for the same input to the same parser, and the result
//! of a build is the same with or without the cache. The edges are resolved
//! again from all the files at each build, because an edge from an unchanged
//! file can go to a symbol of a changed file.
//!
//! The cache is local. It is in the index file, in the table `parse_cache`,
//! which is not part of the schema version: a reader that does not know the
//! table does not read it. [`crate::store::split`] removes it before an
//! upload, so a base index from Kairos has no cache, and the first update
//! after its download parses all the files.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::IndexError;
use crate::calls::CallSite;
use crate::extract::{self, Extracted, FileExtract};
use crate::schema::SCHEMA_VERSION;

/// Change this number when the parse of a file gives a different result
/// with no change to the crate version, for example in a branch that changes
/// the extraction.
const VERSION: u32 = 2;

/// The table of the cache. `CREATE ... IF NOT EXISTS`, because an index of
/// the current schema version can be from before the cache.
pub(crate) const TABLE: &str = "
CREATE TABLE IF NOT EXISTS parse_cache (
    key     TEXT PRIMARY KEY,   -- see parse_cache::key
    extract BLOB NOT NULL       -- JSON: the symbols and the call sites of a file
) STRICT, WITHOUT ROWID;
";

/// The key of the parse of one file.
pub(crate) fn key(path: &str, language: &str, test_file: bool, content_hash: &str) -> String {
    let mut hasher = Sha256::new();
    let version = VERSION.to_string();
    for part in [
        version.as_str(),
        env!("CARGO_PKG_VERSION"),
        path,
        language,
        if test_file { "test" } else { "source" },
        content_hash,
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One entry, as it is stored. The token vectors are not in it: the index
/// has them by tree hash.
#[derive(Serialize, Deserialize)]
struct Entry {
    symbols: Vec<Symbol>,
    calls: Vec<CallSite>,
    use_ranges: Vec<(usize, usize)>,
    macro_calls: Vec<CallSite>,
    macro_ranges: Vec<(usize, usize)>,
}

#[derive(Serialize, Deserialize)]
struct Symbol {
    name: String,
    container: Option<String>,
    kind: String,
    start_line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
    signature: Option<String>,
    tree_hash: String,
    is_test: bool,
    /// Whether the symbol has a token vector.
    vector: bool,
}

/// The entry of a parse.
pub(crate) fn encode(found: &FileExtract) -> Result<Vec<u8>, IndexError> {
    let entry = Entry {
        symbols: found
            .symbols
            .iter()
            .map(|s| Symbol {
                name: s.name.clone(),
                container: s.container.clone(),
                kind: s.kind.to_string(),
                start_line: s.start_line,
                end_line: s.end_line,
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                signature: s.signature.clone(),
                tree_hash: s.tree_hash.clone(),
                is_test: s.is_test,
                vector: s.token_vector.is_some(),
            })
            .collect(),
        calls: found.calls.clone(),
        use_ranges: found.use_ranges.clone(),
        macro_calls: found.macro_calls.clone(),
        macro_ranges: found.macro_ranges.clone(),
    };
    serde_json::to_vec(&entry).map_err(|e| IndexError::ParseCache(e.to_string()))
}

/// The parse from an entry, with the token vectors from `known`. None if the
/// entry cannot be read, or if `known` has no vector for a symbol that has
/// one: the file is then parsed again.
pub(crate) fn decode(
    bytes: &[u8],
    known: &HashMap<String, (Vec<u32>, u32)>,
) -> Option<FileExtract> {
    let entry: Entry = serde_json::from_slice(bytes).ok()?;
    let mut symbols = Vec::with_capacity(entry.symbols.len());
    for s in entry.symbols {
        let (token_vector, token_count) = if s.vector {
            let (vector, count) = known.get(&s.tree_hash)?;
            (Some(vector.clone()), Some(*count))
        } else {
            (None, None)
        };
        symbols.push(Extracted {
            name: s.name,
            container: s.container,
            kind: extract::kind_from_name(&s.kind)?,
            start_line: s.start_line,
            end_line: s.end_line,
            start_byte: s.start_byte,
            end_byte: s.end_byte,
            signature: s.signature,
            tree_hash: s.tree_hash,
            is_test: s.is_test,
            token_vector,
            token_count,
            token_vector_made: false,
        });
    }
    Some(FileExtract {
        symbols,
        calls: entry.calls,
        use_ranges: entry.use_ranges,
        macro_calls: entry.macro_calls,
        macro_ranges: entry.macro_ranges,
    })
}

/// The cache of the index at `db`, open to read.
pub(crate) struct Reader {
    conn: Connection,
}

impl Reader {
    /// The cache of `db`. None if `db` is not there, if it has another
    /// schema version, or if it has no cache.
    pub(crate) fn open(db: &Path) -> Result<Option<Self>, IndexError> {
        if !db.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(crate::BUSY_TIMEOUT)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let table: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'parse_cache'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if version != SCHEMA_VERSION || table.is_none() {
            return Ok(None);
        }
        Ok(Some(Self { conn }))
    }

    /// The entry of `key`.
    pub(crate) fn get(&self, key: &str) -> Result<Option<Vec<u8>>, IndexError> {
        Ok(self
            .conn
            .prepare_cached("SELECT extract FROM parse_cache WHERE key = ?1")?
            .query_row([key], |r| r.get(0))
            .optional()?)
    }
}

/// Write the entries of `fresh`, and remove each entry whose key is not in
/// `used`: the cache holds the parses of the current tree only.
pub(crate) fn write(
    tx: &Transaction<'_>,
    used: &HashSet<String>,
    fresh: &[(String, Vec<u8>)],
) -> Result<(), IndexError> {
    let stale: Vec<String> = {
        let mut stmt = tx.prepare("SELECT key FROM parse_cache")?;
        let keys = stmt.query_map([], |r| r.get::<_, String>(0))?;
        keys.filter(|k| k.as_ref().map_or(true, |k| !used.contains(k)))
            .collect::<Result<_, _>>()?
    };
    let mut delete = tx.prepare("DELETE FROM parse_cache WHERE key = ?1")?;
    for key in &stale {
        delete.execute([key])?;
    }
    let mut insert =
        tx.prepare("INSERT OR REPLACE INTO parse_cache (key, extract) VALUES (?1, ?2)")?;
    for (key, bytes) in fresh {
        insert.execute(params![key, bytes])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use kairos_narsil::parser::LanguageParser;

    use super::*;

    const RUST: &str = "pub struct S;\n\nimpl S {\n    pub fn run(&self) -> u32 {\n        helper(1) + format!(\"{}\", other()).len() as u32\n    }\n}\n\nfn helper(x: u32) -> u32 {\n    x + 1\n}\n\nfn other() -> u32 {\n    2\n}\n";

    fn parse(known: &HashMap<String, (Vec<u32>, u32)>) -> FileExtract {
        let parser = LanguageParser::new().unwrap();
        extract::extract(&parser, "src/lib.rs", "rust", RUST, false, known).unwrap()
    }

    fn vectors(found: &FileExtract) -> HashMap<String, (Vec<u32>, u32)> {
        found
            .symbols
            .iter()
            .filter_map(|s| {
                Some((
                    s.tree_hash.clone(),
                    (s.token_vector.clone()?, s.token_count?),
                ))
            })
            .collect()
    }

    #[test]
    fn an_entry_gives_the_same_parse() {
        let first = parse(&HashMap::new());
        let known = vectors(&first);
        assert!(!known.is_empty(), "the functions have token vectors");
        // A parse with the vectors of the index reuses them, as an update does.
        let parsed = parse(&known);
        let cached = decode(&encode(&first).unwrap(), &known).expect("the entry is read");
        assert_eq!(format!("{cached:?}"), format!("{parsed:?}"));
    }

    #[test]
    fn a_missing_vector_parses_the_file_again() {
        let first = parse(&HashMap::new());
        assert!(decode(&encode(&first).unwrap(), &HashMap::new()).is_none());
    }

    #[test]
    fn a_bad_entry_parses_the_file_again() {
        assert!(decode(b"not json", &HashMap::new()).is_none());
    }

    #[test]
    fn the_key_changes_with_each_part() {
        let base = key("src/a.rs", "rust", false, "abc");
        for other in [
            key("src/b.rs", "rust", false, "abc"),
            key("src/a.rs", "python", false, "abc"),
            key("src/a.rs", "rust", true, "abc"),
            key("src/a.rs", "rust", false, "abd"),
        ] {
            assert_ne!(base, other);
        }
        assert_eq!(base, key("src/a.rs", "rust", false, "abc"));
    }
}
