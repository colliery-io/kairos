-- COLLIERY-T-1853 (COLLIERY-I-0264, "The flow"): Kairos keeps the base
-- code index of each repository.
--
-- An index has 2 parts (COLLIERY-I-0264, "The two parts of an index"):
--
--   code_index_summaries   THE SUMMARY POOL of a repository. One row for
--                          each summary key. All commits of the repository
--                          share it, so a summary that 2 commits use is
--                          stored one time.
--   code_indexes           THE STRUCTURE of one commit: the gzip of the
--                          SQLite index file with an empty `summaries`
--                          table. About 2 MB for Kairos (7 MB before gzip).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 2 new tables, both empty. No row of a table that the tenant has is
-- changed or deleted.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.

-- The key is a sha256 in hex. The same key always means the same input
-- (COLLIERY-T-1851), so a second upload of a key keeps the first row.
-- `vector`: little-endian f32, as in the index file. NULL until embedded.
-- `repository_id` has no foreign key, as `item_impacts.target_id` has none:
-- a repository is never removed from its table (a delete sets `deleted_at`),
-- so the id always names a row, and the down migration of `repositories`
-- can still drop that table.
CREATE TABLE IF NOT EXISTS code_index_summaries (
    repository_id   UUID NOT NULL,
    key             TEXT NOT NULL,
    level           TEXT NOT NULL CHECK (level IN ('symbol', 'file', 'module')),
    summary         TEXT NOT NULL,
    vector          BYTEA,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (repository_id, key)
);

-- One row for each indexed commit. `ref_name` is the branch, the pull
-- request or the tag that the index is for (`main`, `pull/12`, `v1.0.0`),
-- when the sender says it. `source`: an upload, or the builder of the
-- server after a push (`created_by` is then NULL). `vector_model` is the
-- model of the vectors of the summaries (`provider/model/dimension`); one
-- repository has one.
CREATE TABLE IF NOT EXISTS code_indexes (
    repository_id   UUID NOT NULL,
    commit_sha      TEXT NOT NULL CHECK (commit_sha ~ '^[0-9a-f]{40}([0-9a-f]{24})?$'),
    ref_name        TEXT,
    source          TEXT NOT NULL CHECK (source IN ('upload', 'build')),
    structure       BYTEA NOT NULL,
    structure_bytes BIGINT NOT NULL,
    summary_keys    INTEGER NOT NULL,
    vector_model    TEXT,
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (repository_id, commit_sha)
);

-- The structure is a gzip already. EXTERNAL: out of line, with no second
-- compression by Postgres.
ALTER TABLE code_indexes ALTER COLUMN structure SET STORAGE EXTERNAL;
