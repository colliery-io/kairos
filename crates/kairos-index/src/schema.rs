//! The SQLite schema of an index (COLLIERY-I-0264, "The two parts of an
//! index").
//!
//! - The structure: `files`, `symbols`, `edges`, `edge_candidates` and
//!   `scip_covered`. A build deletes and writes it again.
//! - The summary pool: `summaries` and `pool_meta`. A build of the structure
//!   keeps it. `symbols.summary_key`, `files.summary_key` and `modules` link
//!   the structure to the pool; the summarizer writes them (COLLIERY-T-1850).
//!
//! Version 3 (COLLIERY-T-1851): the key of a symbol summary is no longer the
//! tree hash (`symbols.summary_key`), the key of a file summary has the path,
//! an edge can come from the text of a macro (`macro-text`) and can wait for
//! a SCIP run (`scip_pending`), and `scip_covered` keeps what SCIP resolved.
//! An index of an earlier version is refused.
//!
//! `PRAGMA user_version` holds the schema version.

use rusqlite::Connection;

use crate::IndexError;

/// The schema version that this code writes and reads.
pub const SCHEMA_VERSION: i64 = 3;

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
    tree_hash   TEXT    NOT NULL,           -- sha256 of the normalized tree
    is_test     INTEGER NOT NULL CHECK (is_test IN (0, 1)),
    summary_key TEXT                        -- the key of its summary in the pool
) STRICT;
CREATE INDEX symbols_file ON symbols (file_id);
CREATE INDEX symbols_name ON symbols (name);
CREATE INDEX symbols_tree_hash ON symbols (tree_hash);

-- Call and import edges (COLLIERY-T-1849). A certain edge names its callee;
-- a possible edge lists its candidates in edge_candidates; an external edge
-- has only the name. 'macro-text': a name class for a call that the text of
-- a Rust macro invocation shows and SCIP did not resolve (COLLIERY-T-1851).
-- scip_pending: a name class in a Rust file that changed after the last SCIP
-- run. A run with the Rust edges replaces it.
CREATE TABLE edges (
    id          INTEGER PRIMARY KEY,
    caller_id   INTEGER NOT NULL REFERENCES symbols (id) ON DELETE CASCADE,
    callee_id   INTEGER REFERENCES symbols (id) ON DELETE CASCADE,
    callee_name TEXT    NOT NULL,
    edge_kind   TEXT    NOT NULL CHECK (edge_kind IN ('call', 'import')),
    class       TEXT    NOT NULL CHECK (class IN ('certain', 'possible', 'external')),
    origin      TEXT    NOT NULL CHECK (origin IN ('scip', 'name', 'macro-text')),
    line        INTEGER NOT NULL,
    col         INTEGER NOT NULL,
    scip_pending INTEGER NOT NULL DEFAULT 0 CHECK (scip_pending IN (0, 1))
) STRICT;
CREATE INDEX edges_caller ON edges (caller_id);
CREATE INDEX edges_callee ON edges (callee_id);

CREATE TABLE edge_candidates (
    edge_id   INTEGER NOT NULL REFERENCES edges (id) ON DELETE CASCADE,
    symbol_id INTEGER NOT NULL REFERENCES symbols (id) ON DELETE CASCADE,
    PRIMARY KEY (edge_id, symbol_id)
) STRICT;

-- The call sites of a Rust file that the last SCIP run resolved: with a SCIP
-- edge, or with no edge (a tuple struct, an enum variant). An update that
-- keeps the SCIP edges of an unchanged function gives these no name class.
CREATE TABLE scip_covered (
    file_id    INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    start_byte INTEGER NOT NULL,
    PRIMARY KEY (file_id, start_byte)
) STRICT, WITHOUT ROWID;

-- The summary pool (COLLIERY-T-1850). The key is a hash of what made the
-- summary: the tree hash and the callee signatures of a symbol, the path and
-- the symbol summaries of a file, or the file summaries of a module. The same code gives the same key, so 2 pools merge with
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
