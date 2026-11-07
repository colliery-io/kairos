-- KAIROS-T-0750, KAIROS-A-0082: the refs that point at a stored code index. The builder
-- indexes the head of the default branch, of each open pull request and
-- of each tag (COLLIERY-I-0671). One sha can carry many refs: a tag on the
-- head of main, a pull request whose head is a tag. `code_indexes.ref_name`
-- stays the ref that a build or an upload was for; this table has each
-- ref that points at an index now.
--
--   repository_id   The repository (`repositories.id`, no foreign key, as
--                   `code_indexes` has none).
--   ref_name        The ref with no `refs/` prefix: `main`, `pull/12`,
--                   `merge-requests/12`, `v1.0.0`.
--   commit_sha      The commit that the ref points at.
--   kind            branch | pull | tag.
--   updated_at      When the builder last saw the ref at this commit.
--
-- A ref that moves gets the new commit. A ref that is gone from the
-- remote, or outside the window of the builder, is deleted by the sweep.
-- A delete of the index deletes its rows (ON DELETE CASCADE).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new empty table. No row of another table is read or changed. The
-- foreign key takes a short lock on `code_indexes` and reads no row of it,
-- because the new table is empty (KAIROS-A-0062). An index stored before
-- this migration has no row here: the next sweep writes the rows of the
-- refs that still point at it.
CREATE TABLE IF NOT EXISTS code_index_refs (
    repository_id UUID        NOT NULL,
    ref_name      TEXT        NOT NULL CHECK (ref_name <> '' AND ref_name NOT LIKE 'refs/%'),
    commit_sha    TEXT        NOT NULL,
    kind          TEXT        NOT NULL CHECK (kind IN ('branch', 'pull', 'tag')),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (repository_id, ref_name),
    FOREIGN KEY (repository_id, commit_sha)
        REFERENCES code_indexes (repository_id, commit_sha) ON DELETE CASCADE
);
