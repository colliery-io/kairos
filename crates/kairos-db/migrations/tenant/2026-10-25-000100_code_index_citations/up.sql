-- KAIROS-T-0703: the ADRs that the code of an indexed commit cites. The
-- code index records each ADR short code in a comment, a doc string or the
-- name of a test (index schema version 8, table `adr_citations`). The
-- structure of a commit is a gzip of the SQLite file, so the server keeps
-- the cited codes here too: `get_repository` lists them with no read of
-- the structure.
--
--   repository_id, commit_sha   The indexed commit (`code_indexes`).
--   code                        The ADR short code, as KAIROS-A-0065.
--   places                      The count of the places that cite it.
--   files                       The count of the files that cite it.
--
-- A write of the index of a commit replaces its rows. A delete of the
-- index deletes them (ON DELETE CASCADE).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new empty table. No row of another table is read or changed. The
-- foreign key takes a short lock on `code_indexes` and reads no row of it,
-- because the new table is empty (KAIROS-A-0062). An index stored before
-- this migration has no rows: the next build of its repository writes
-- them.
CREATE TABLE IF NOT EXISTS code_index_citations (
    repository_id UUID    NOT NULL,
    commit_sha    TEXT    NOT NULL,
    code          TEXT    NOT NULL CHECK (code ~ '^[A-Z][A-Z0-9]*-A-[0-9]{4,}$'),
    places        INTEGER NOT NULL CHECK (places > 0),
    files         INTEGER NOT NULL CHECK (files > 0),
    PRIMARY KEY (repository_id, commit_sha, code),
    FOREIGN KEY (repository_id, commit_sha)
        REFERENCES code_indexes (repository_id, commit_sha) ON DELETE CASCADE
);
