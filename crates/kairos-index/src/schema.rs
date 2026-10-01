//! The SQLite schema of an index (COLLIERY-I-0264, "The two parts of an
//! index").
//!
//! - The structure: `files`, `symbols`, `edges` and `edge_candidates`. A build
//!   deletes and writes it again.
//! - The summary pool: `summaries` and `pool_meta`. A build of the structure
//!   keeps it. `files.summary_key` and `modules` link the structure to the
//!   pool; the summarizer writes them (COLLIERY-T-1850).
//!
//! `PRAGMA user_version` holds the schema version.

use rusqlite::Connection;

use crate::IndexError;

/// The schema version that this code writes and reads.
pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = r#"
-- One row for each file of the tree, with the decision of the file rules.
CREATE TABLE files (
    id           INTEGER PRIMARY KEY,
    path         TEXT    NOT NULL UNIQUE,   -- from the root, '/' between parts
    language     TEXT,                      -- NULL: no parser for the file
    decision     TEXT    NOT NULL CHECK (decision IN
                     ('source', 'test', 'vendored', 'generated', 'fixture', 'docs', 'excluded')),
    rule         TEXT    NOT NULL,          -- the id of the rule that decided
    rule_origin  TEXT    NOT NULL CHECK (rule_origin IN ('default', 'repository')),
    size         INTEGER NOT NULL,          -- bytes
    longest_line INTEGER NOT NULL,          -- characters
    content_hash TEXT    NOT NULL,          -- sha256 of the bytes, for updates
    parse_error  TEXT,                      -- set when a parsed file failed
    summary_key  TEXT                       -- the key of its summary in the pool
) STRICT;

-- One row for each symbol that narsil extracts from a source or test file.
CREATE TABLE symbols (
    id          INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    container   TEXT,                       -- the smallest symbol that holds it
    kind        TEXT    NOT NULL,           -- narsil's kind, snake case
    language    TEXT    NOT NULL,
    start_line  INTEGER NOT NULL,           -- from 1, included
    end_line    INTEGER NOT NULL,           -- from 1, included
    start_byte  INTEGER NOT NULL,
    end_byte    INTEGER NOT NULL,           -- not included
    signature   TEXT,
    tree_hash   TEXT    NOT NULL,           -- sha256 of the normalized tree; the summary key
    is_test     INTEGER NOT NULL CHECK (is_test IN (0, 1))
) STRICT;
CREATE INDEX symbols_file ON symbols (file_id);
CREATE INDEX symbols_name ON symbols (name);
CREATE INDEX symbols_tree_hash ON symbols (tree_hash);

-- Call and import edges (COLLIERY-T-1849). A certain edge names its callee;
-- a possible edge lists its candidates in edge_candidates; an external edge
-- has only the name.
CREATE TABLE edges (
    id          INTEGER PRIMARY KEY,
    caller_id   INTEGER NOT NULL REFERENCES symbols (id) ON DELETE CASCADE,
    callee_id   INTEGER REFERENCES symbols (id) ON DELETE CASCADE,
    callee_name TEXT    NOT NULL,
    edge_kind   TEXT    NOT NULL CHECK (edge_kind IN ('call', 'import')),
    class       TEXT    NOT NULL CHECK (class IN ('certain', 'possible', 'external')),
    origin      TEXT    NOT NULL CHECK (origin IN ('scip', 'name')),
    line        INTEGER NOT NULL,
    col         INTEGER NOT NULL
) STRICT;
CREATE INDEX edges_caller ON edges (caller_id);
CREATE INDEX edges_callee ON edges (callee_id);

CREATE TABLE edge_candidates (
    edge_id   INTEGER NOT NULL REFERENCES edges (id) ON DELETE CASCADE,
    symbol_id INTEGER NOT NULL REFERENCES symbols (id) ON DELETE CASCADE,
    PRIMARY KEY (edge_id, symbol_id)
) STRICT;

-- The summary pool (COLLIERY-T-1850). The key is a hash of what made the
-- summary: the tree hash of a symbol, or the hash of the summaries below a
-- file or a module. The same code gives the same key, so 2 pools merge with
-- no conflict.
CREATE TABLE summaries (
    key     TEXT PRIMARY KEY,
    level   TEXT NOT NULL CHECK (level IN ('symbol', 'file', 'module')),
    summary TEXT NOT NULL,
    vector  BLOB                            -- little-endian f32, NULL until embedded
) STRICT;

-- Facts about the pool. 'vector_model': the model of the vectors, as
-- provider/model/dimension. One pool holds the vectors of one model only.
CREATE TABLE pool_meta (
    name  TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- A module is a folder that holds summarized files. It is part of the
-- structure: a build deletes it, and the summarizer writes it again.
CREATE TABLE modules (
    path        TEXT PRIMARY KEY,           -- from the root; '.' for the root
    summary_key TEXT NOT NULL
) STRICT;
"#;

/// Open (or create) the index at `conn` and check its schema version.
pub fn prepare(conn: &Connection) -> Result<(), IndexError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        SCHEMA_VERSION => Ok(()),
        found => Err(IndexError::SchemaVersion { found }),
    }
}
