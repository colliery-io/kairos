//! An index in 2 parts for a store that keeps many commits
//! (COLLIERY-T-1853): the structure of one commit, and the rows of a summary
//! pool that the commits of a repository share.
//!
//! - [`split`] reads an index file. It gives the structure as a gzip of the
//!   SQLite file with an empty `summaries` table, the rows of the pool, and
//!   the keys that the structure uses.
//! - [`assemble`] writes an index file from a structure and pool rows.
//!
//! `pool_meta` (the model of the vectors) stays in the structure. The store
//! must keep the vectors of one model for each repository.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{IndexError, SCHEMA_VERSION};

/// One row of a summary pool, as the store keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRow {
    pub key: String,
    pub level: String,
    pub summary: String,
    /// Little-endian f32, as in the index. `None` until it is embedded.
    pub vector: Option<Vec<u8>>,
}

/// An index file in 2 parts.
#[derive(Debug, Clone)]
pub struct Split {
    /// The gzip of the SQLite file, with an empty `summaries` table and with
    /// no parse cache.
    pub structure_gz: Vec<u8>,
    /// The size of the SQLite file in the gzip, in bytes.
    pub structure_bytes: u64,
    /// Each row of the pool of the file.
    pub pool: Vec<PoolRow>,
    /// The summary keys that the structure uses: symbols, files and
    /// modules. Sorted.
    pub keys: Vec<String>,
    /// The model of the vectors of the pool, as `provider/model/dimension`.
    pub vector_model: Option<String>,
}

/// Read the index file at `db` in 2 parts. The file is not changed. A file
/// that is not an index of [`SCHEMA_VERSION`] is refused.
pub fn split(db: &Path) -> Result<Split, IndexError> {
    let io = |source| IndexError::Io {
        path: db.to_path_buf(),
        source,
    };
    let work = tempfile::Builder::new()
        .prefix("kairos-index-split-")
        .tempdir()
        .map_err(io)?;
    let copy = work.path().join("index.db");
    std::fs::copy(db, &copy).map_err(io)?;

    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    check_version(&conn)?;
    let pool = pool_rows(&conn)?;
    let keys = used_keys(&conn)?;
    let vector_model = vector_model(&conn)?;
    conn.execute("DELETE FROM summaries", [])?;
    // The parse cache is local (KAIROS-T-0297).
    conn.execute("DROP TABLE IF EXISTS parse_cache", [])?;
    conn.execute_batch("VACUUM")?;
    drop(conn);

    let bytes = std::fs::read(&copy).map_err(io)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&bytes).map_err(io)?;
    let structure_gz = encoder.finish().map_err(io)?;
    Ok(Split {
        structure_gz,
        structure_bytes: bytes.len() as u64,
        pool,
        keys,
        vector_model,
    })
}

/// Write the index file `out` from the structure `structure_gz` (from
/// [`split`]) and the pool rows `pool`. `out` is replaced if it is there.
pub fn assemble(
    structure_gz: &[u8],
    pool: impl IntoIterator<Item = PoolRow>,
    out: &Path,
) -> Result<(), IndexError> {
    let io = |source| IndexError::Io {
        path: out.to_path_buf(),
        source,
    };
    let mut bytes = Vec::new();
    GzDecoder::new(structure_gz)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    std::fs::write(out, &bytes).map_err(io)?;
    let mut conn = Connection::open(out)?;
    check_version(&conn)?;
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT OR IGNORE INTO summaries (key, level, summary, vector)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for row in pool {
            insert.execute(params![row.key, row.level, row.summary, row.vector])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Copy the summary pool of the index file `from` into the index file `to`
/// (COLLIERY-T-1854): a checkout that downloads a base index keeps the
/// summaries of its old local index. A key that `to` has keeps its summary.
/// The 2 pools must have the vectors of one model.
pub fn copy_pool(from: &Path, to: &Path) -> Result<(), IndexError> {
    let source = Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    check_version(&source)?;
    let mut target = Connection::open(to)?;
    check_version(&target)?;
    crate::summary::copy_pool(&source, &mut target)
}

/// The summary keys that the index file at `db` uses. Sorted.
pub fn keys_of(db: &Path) -> Result<Vec<String>, IndexError> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    check_version(&conn)?;
    used_keys(&conn)
}

fn check_version(conn: &Connection) -> Result<(), IndexError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(IndexError::SchemaVersion { found: version });
    }
    Ok(())
}

fn pool_rows(conn: &Connection) -> Result<Vec<PoolRow>, IndexError> {
    let mut stmt =
        conn.prepare("SELECT key, level, summary, vector FROM summaries ORDER BY key")?;
    let rows = stmt.query_map([], |r| {
        Ok(PoolRow {
            key: r.get(0)?,
            level: r.get(1)?,
            summary: r.get(2)?,
            vector: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn used_keys(conn: &Connection) -> Result<Vec<String>, IndexError> {
    let mut keys = BTreeSet::new();
    for sql in [
        "SELECT summary_key FROM symbols WHERE summary_key IS NOT NULL",
        "SELECT summary_key FROM files WHERE summary_key IS NOT NULL",
        "SELECT summary_key FROM modules",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for key in rows {
            keys.insert(key?);
        }
    }
    Ok(keys.into_iter().collect())
}

fn vector_model(conn: &Connection) -> Result<Option<String>, IndexError> {
    Ok(conn
        .query_row(
            "SELECT value FROM pool_meta WHERE name = 'vector_model'",
            [],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_with_pool(path: &Path) {
        let conn = Connection::open(path).unwrap();
        crate::schema::prepare(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO files (id, path, language, decision, rule, rule_origin, size,
                                longest_line, content_hash, summary_key)
             VALUES (1, 'a.py', 'python', 'source', 'r', 'default', 10, 5, 'h', 'file-key');
             INSERT INTO symbols (id, file_id, name, kind, language, start_line, end_line,
                                  start_byte, end_byte, tree_hash, is_test, summary_key)
             VALUES (1, 1, 'f', 'function', 'python', 1, 2, 0, 9, 't', 0, 'sym-key');
             INSERT INTO modules (path, summary_key) VALUES ('.', 'mod-key');
             INSERT INTO summaries (key, level, summary, vector) VALUES
               ('sym-key', 'symbol', 'S.', x'0000803f'),
               ('file-key', 'file', 'F.', NULL),
               ('mod-key', 'module', 'M.', NULL),
               ('old-key', 'symbol', 'Old.', NULL);
             INSERT INTO pool_meta (name, value) VALUES ('vector_model', 'det/m/1');",
        )
        .unwrap();
    }

    #[test]
    fn a_split_has_no_parse_cache() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("in.db");
        index_with_pool(&db);
        Connection::open(&db)
            .unwrap()
            .execute(
                "INSERT INTO parse_cache (key, extract) VALUES ('k', x'7b7d')",
                [],
            )
            .unwrap();

        let split = split(&db).unwrap();
        let out = dir.path().join("out.db");
        assemble(&split.structure_gz, split.pool, &out).unwrap();
        let tables: i64 = Connection::open(&out)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'parse_cache'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        // The local index keeps its cache: the split does not change it.
        let entries: i64 = Connection::open(&db)
            .unwrap()
            .query_row("SELECT count(*) FROM parse_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(entries, 1);
    }

    #[test]
    fn a_split_then_an_assemble_gives_the_index_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("in.db");
        index_with_pool(&db);

        let split = split(&db).unwrap();
        assert_eq!(split.keys, ["file-key", "mod-key", "sym-key"]);
        assert_eq!(split.pool.len(), 4);
        assert_eq!(split.vector_model.as_deref(), Some("det/m/1"));

        let out = dir.path().join("out.db");
        let used: Vec<PoolRow> = split
            .pool
            .iter()
            .filter(|r| split.keys.contains(&r.key))
            .cloned()
            .collect();
        assemble(&split.structure_gz, used, &out).unwrap();
        let index = crate::Index::open(&out).unwrap();
        assert_eq!(index.symbols().unwrap().len(), 1);
        assert_eq!(
            index.summary("sym-key").unwrap().unwrap().vector,
            Some(vec![1.0])
        );
        assert!(index.summary("old-key").unwrap().is_none());
        assert_eq!(keys_of(&out).unwrap(), split.keys);
    }

    #[test]
    fn the_structure_has_no_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("in.db");
        index_with_pool(&db);
        let split = split(&db).unwrap();
        let out = dir.path().join("out.db");
        assemble(&split.structure_gz, [], &out).unwrap();
        let conn = Connection::open(&out).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM summaries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_file_that_is_not_an_index_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("empty.db");
        Connection::open(&db)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert!(matches!(
            split(&db),
            Err(IndexError::SchemaVersion { found: 0 })
        ));
        let text = dir.path().join("text.db");
        std::fs::write(&text, b"not a database at all, just text").unwrap();
        assert!(split(&text).is_err());
    }
}
