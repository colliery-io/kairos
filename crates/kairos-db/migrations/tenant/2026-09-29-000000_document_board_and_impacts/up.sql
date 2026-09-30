-- COLLIERY-T-0269 (COLLIERY-I-0019): a document names its board, and
-- impacts a repository.
--
-- The owner decided the model on 2026-09-29. A document has two links that
-- say two different things:
--
--   document -> board                          THE OWNER. The board gives the
--                                              right to edit.
--   document -> repository, by `impacts`       WHAT THE DOCUMENT IS ABOUT.
--                                              The link gives no right.
--
-- Until now a document had no board. It took its authority from the board
-- of the earliest item that it supports (KAIROS-A-0006), so a document that
-- belongs to a repository as a whole (its vision, its architecture) had to
-- support some work item to have an owner.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1. `documents.board_id` is a new column, and it is NULL in each row that
--    the tenant has. NULL means "the document names no board": its
--    authorization board is the board of its earliest `supports` parent,
--    as before. So no document gets a different owner, and no person gets
--    or loses the right to edit a document.
-- 2. `item_impacts` is a new table, and it is empty.
-- 3. The tenant gets the template "Product Vision" when it has no template
--    with that name and no template with the slug `product_vision`. A
--    template that the tenant made or changed is not read and not changed.
--    The template gets the metadata default `document_type = vision` when
--    the tenant has that definition and that value.
--
-- No row is deleted, and no row of a table that the tenant has is changed.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.

-- ---------------------------------------------------------------------------
-- 1. The owner board of a document
-- ---------------------------------------------------------------------------

-- A board is never removed from the table (a delete sets `deleted_at`), so
-- the reference has no ON DELETE rule. A document that names a deleted
-- board keeps its owner, as a task on a deleted board keeps its board.
ALTER TABLE documents ADD COLUMN IF NOT EXISTS board_id UUID REFERENCES boards(id);

-- Not partial, as each `idx_*_board` index (KAIROS-T-0159): a read that
-- asks for archived rows too must be able to use it.
CREATE INDEX IF NOT EXISTS idx_documents_board
    ON documents (board_id);

-- ---------------------------------------------------------------------------
-- 2. The `impacts` links
-- ---------------------------------------------------------------------------

-- A table of its own, and not rows of `item_relationships`. Each end of an
-- `item_relationships` row is an item, and the graph view, the traversal of
-- the search, `related_work` and the cascade of an archive read that table
-- with that rule. A repository is not an item.
--
-- `item_id` has no foreign key, for the reason that `item_relationships`
-- has none: an item is a row in one of five tables. `item_type` says which.
--
-- `target_kind` is `repository` only. A later target kind (a team, the
-- organization) is a new value of the CHECK, and `target_id` is then the id
-- of a row of that kind. For that reason `target_id` has no foreign key. A
-- repository is never removed from its table (a delete sets `deleted_at`),
-- so the id always names a row.
CREATE TABLE IF NOT EXISTS item_impacts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id         UUID NOT NULL,
    item_type       TEXT NOT NULL CHECK (item_type IN ('document', 'adr')),
    target_kind     TEXT NOT NULL DEFAULT 'repository'
                    CHECK (target_kind IN ('repository')),
    target_id       UUID NOT NULL,
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (item_id, target_kind, target_id)
);

CREATE INDEX IF NOT EXISTS idx_item_impacts_target
    ON item_impacts (target_kind, target_id);

-- ---------------------------------------------------------------------------
-- 3. The template "Product Vision"
-- ---------------------------------------------------------------------------

-- The text is the text of the system default in `kairos_db::tenant`. A test
-- compares the two.
INSERT INTO templates (name, slug, content, is_system_default)
SELECT 'Product Vision', 'product_vision',
       E'# Product Vision\n\n## Purpose\n\n## Who It Is For\n\n## Current State\n\n## Future State\n\n## Principles\n\n## What It Is Not\n',
       true
 WHERE NOT EXISTS (
        SELECT 1 FROM templates
         WHERE name = 'Product Vision' OR slug = 'product_vision'
       );

-- Only for the template that this migration made: `is_system_default` and
-- the slug. A template "Product Vision" of the tenant keeps its metadata.
INSERT INTO template_metadata (template_id, metadata_definition_id, default_value, required)
SELECT t.id, d.id, 'vision', false
  FROM templates t
  JOIN metadata_definitions d ON d.slug = 'document_type'
 WHERE t.slug = 'product_vision'
   AND t.name = 'Product Vision'
   AND t.is_system_default
   AND EXISTS (
        SELECT 1 FROM metadata_enum_options o
         WHERE o.metadata_definition_id = d.id AND o.value = 'vision'
       )
ON CONFLICT (template_id, metadata_definition_id) DO NOTHING;
