-- KAIROS-T-0331 (COLLIERY-I-0610): each run of the code index builder
-- leaves a record, so a person sees when the builder last worked on a
-- repository and what came of it.
--
--   code_index_builds   One row for each run: a build after a push, the
--                       first build of a repository, a build on request,
--                       or an upload of an index file.
--
-- `commit_sha` is NULL until the fetch gives the head of the default
-- branch. `trigger`: 'push' (the update lane), 'first' (the first-build
-- lane), 'request' (a person asked, KAIROS-T-0332), 'upload' (an index
-- file was sent). `outcome`: 'running' until the run ends, then 'ok' or
-- 'failed'; `error` holds the text of a failure. The counts are those of
-- the index that the run wrote. `requested_by` is the user of a request or
-- an upload; NULL for the builder.
--
-- `repository_id` has no foreign key, as `code_indexes.repository_id` has
-- none: a repository is never removed from its table.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty. No row of a table that the tenant has is changed or
-- deleted.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.
CREATE TABLE IF NOT EXISTS code_index_builds (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repository_id   UUID NOT NULL,
    commit_sha      TEXT CHECK (commit_sha ~ '^[0-9a-f]{40}([0-9a-f]{24})?$'),
    ref_name        TEXT,
    trigger         TEXT NOT NULL CHECK (trigger IN ('push', 'first', 'request', 'upload')),
    outcome         TEXT NOT NULL CHECK (outcome IN ('running', 'ok', 'failed')),
    error           TEXT,
    files           INTEGER,
    symbols         INTEGER,
    edges           INTEGER,
    summaries_made  INTEGER,
    requested_by    UUID,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    CHECK ((outcome = 'running') = (finished_at IS NULL)),
    CHECK (outcome <> 'failed' OR error IS NOT NULL)
);

-- The list of a repository reads the newest runs first.
CREATE INDEX IF NOT EXISTS code_index_builds_repository_started
    ON code_index_builds (repository_id, started_at DESC);
