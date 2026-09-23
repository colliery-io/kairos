-- KAIROS-T-0156 (KAIROS-A-0020, initiative KAIROS-I-0015 D3): the two
-- directory views stop deciding what may be seen, and start reporting what
-- is so.
--
-- `searchable_items` and `entity_directory` each filtered
-- `deleted_at IS NULL` on every UNION branch, which made "archived" mean
-- "absent" for every consumer downstream of them — resolution, traverse,
-- full-text search and the whole relationship graph. ADR-20 says archived
-- means hidden BY DEFAULT, and a default is something a caller can opt out
-- of. A view that has already dropped the rows leaves nothing to opt into:
-- that is exactly why `--include-deleted` is a silent no-op next to a text
-- query today.
--
-- So the views now SELECT `deleted_at` instead of filtering on it, and
-- every call site names its liveness mode. This migration is deliberately
-- behaviour-neutral on its own: each consumer gained the `deleted_at IS
-- NULL` predicate the view used to apply, in the same commit.
--
-- No table changes, and no index changes either. The predicate moves from
-- the view body into the query, where the planner meets it before the
-- UNION branches are materialised, so the partial indexes
-- (`idx_*_tsv`, `idx_tasks_column` and siblings, all `WHERE deleted_at IS
-- NULL`) still match the default path.
--
-- `deleted_at` goes LAST in both column lists. Nothing selects `*` from
-- either view, but appending keeps positional readers honest and is the
-- shape `CREATE OR REPLACE VIEW` would have allowed had the down migration
-- not needed to remove a column again.

DROP VIEW IF EXISTS searchable_items;

CREATE VIEW searchable_items AS
    SELECT id, short_code, 'strategy' AS entity_type, title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')) AS tsv,
           deleted_at
    FROM strategies
    UNION ALL
    SELECT id, short_code, 'initiative', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM initiatives
    UNION ALL
    SELECT id, short_code, 'task', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM tasks
    UNION ALL
    SELECT id, short_code, 'document', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM documents
    UNION ALL
    SELECT id, short_code, 'adr', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM adrs;

DROP VIEW IF EXISTS entity_directory;

CREATE VIEW entity_directory AS
    SELECT id, short_code, 'strategy' AS entity_type, title, board_id, deleted_at
    FROM strategies
    UNION ALL
    SELECT id, short_code, 'initiative', title, board_id, deleted_at FROM initiatives
    UNION ALL
    SELECT id, short_code, 'task', title, board_id, deleted_at FROM tasks
    UNION ALL
    SELECT id, short_code, 'document', title, NULL, deleted_at FROM documents
    UNION ALL
    SELECT id, short_code, 'adr', title, board_id, deleted_at FROM adrs;
