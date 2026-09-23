-- Reverse KAIROS-T-0156: put the liveness filter back inside the views.
--
-- Safe to run at any time — the views hold no data of their own. What it
-- is NOT safe to do is revert this migration while the code that relies on
-- it is still deployed: every consumer now spells its own
-- `deleted_at IS NULL`, and against these filtering views those predicates
-- become redundant rather than wrong. The reverse direction is the
-- dangerous one, which is why this file exists mainly for the fleet-upgrade
-- rehearsal in `tests/tenant_provisioning.rs`.
--
-- `deleted_at` leaves the column list, so this has to DROP and recreate;
-- `CREATE OR REPLACE VIEW` may only append columns.

DROP VIEW IF EXISTS searchable_items;

CREATE VIEW searchable_items AS
    SELECT id, short_code, 'strategy' AS entity_type, title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')) AS tsv
    FROM strategies WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'initiative', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM initiatives WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'task', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM tasks WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'document', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM documents WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'adr', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM adrs WHERE deleted_at IS NULL;

DROP VIEW IF EXISTS entity_directory;

CREATE VIEW entity_directory AS
    SELECT id, short_code, 'strategy' AS entity_type, title, board_id FROM strategies WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'initiative', title, board_id FROM initiatives WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'task', title, board_id FROM tasks WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'document', title, NULL FROM documents WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'adr', title, board_id FROM adrs WHERE deleted_at IS NULL;
