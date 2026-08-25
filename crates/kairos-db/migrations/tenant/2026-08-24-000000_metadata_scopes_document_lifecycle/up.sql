-- KAIROS-T-0078 (tenant side): entity-type scoping for metadata
-- definitions + document lifecycle as a first-class column.
--
-- Two-vocabulary rule (see the KAIROS-T-0078 ADR): ticket status IS
-- board position; document lifecycle is a typed editorial label on the
-- documents table. The legacy 'status' metadata definition is migrated
-- into documents.lifecycle and deleted.
--
-- Runs with search_path pinned to the tenant schema (KAIROS-T-0008);
-- unqualified names are tenant tables. Guarded for re-runnability
-- (fleet-migration hygiene).

-- 1. Scopes: no rows = definition applies to every entity type.
CREATE TABLE IF NOT EXISTS metadata_definition_scopes (
    metadata_definition_id UUID NOT NULL
        REFERENCES metadata_definitions(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL
        CHECK (entity_type IN ('strategy', 'initiative', 'task', 'document', 'adr')),
    PRIMARY KEY (metadata_definition_id, entity_type)
);

-- Seed scopes for the SYSTEM definitions this tenant carries (guarded on
-- is_system_default so operator-created definitions are untouched).
-- document_type: documents only. complexity: everywhere except
-- initiatives (their native column is the source of truth there).
-- During provisioning this inserts nothing (definitions are copied after
-- migrations run) — provisioning copies scopes from the system tables.
INSERT INTO metadata_definition_scopes (metadata_definition_id, entity_type)
SELECT d.id, s.entity_type
FROM (VALUES
    ('document_type', 'document'),
    ('complexity', 'strategy'),
    ('complexity', 'task'),
    ('complexity', 'document'),
    ('complexity', 'adr')
) AS s(slug, entity_type)
JOIN metadata_definitions d ON d.slug = s.slug AND d.is_system_default
ON CONFLICT DO NOTHING;

-- 2. Document lifecycle: draft | review | published | archived.
ALTER TABLE documents
    ADD COLUMN IF NOT EXISTS lifecycle TEXT NOT NULL DEFAULT 'draft'
        CHECK (lifecycle IN ('draft', 'review', 'published', 'archived'));

-- 3. Backfill from the legacy 'status' metadata: draft->draft,
-- review->review, approved->published. Documents with NO legacy status
-- (charter/social_contract/vision templates never stamped one) backfill
-- to 'published' — recorded PO decision; new documents default 'draft'.
UPDATE documents d
SET lifecycle = CASE im.value WHEN 'approved' THEN 'published' ELSE im.value END
FROM item_metadata im
JOIN metadata_definitions md
    ON md.id = im.metadata_definition_id AND md.slug = 'status' AND md.is_system_default
WHERE im.item_id = d.id
  AND im.value IN ('draft', 'review', 'approved');

UPDATE documents d
SET lifecycle = 'published'
WHERE NOT EXISTS (
    SELECT 1 FROM item_metadata im
    JOIN metadata_definitions md
        ON md.id = im.metadata_definition_id AND md.slug = 'status' AND md.is_system_default
    WHERE im.item_id = d.id
)
AND d.lifecycle = 'draft';

-- 4. Retire the 'status' definition: values, template associations, enum
-- options, then the definition itself (guarded on is_system_default so a
-- coincidentally-named operator definition survives).
DELETE FROM item_metadata im
    USING metadata_definitions md
    WHERE md.id = im.metadata_definition_id AND md.slug = 'status' AND md.is_system_default;
DELETE FROM template_metadata tm
    USING metadata_definitions md
    WHERE md.id = tm.metadata_definition_id AND md.slug = 'status' AND md.is_system_default;
DELETE FROM metadata_enum_options meo
    USING metadata_definitions md
    WHERE md.id = meo.metadata_definition_id AND md.slug = 'status' AND md.is_system_default;
DELETE FROM metadata_definitions WHERE slug = 'status' AND is_system_default;

-- 5. Delete stray out-of-scope values (e.g. document_type already on a
-- task) and REPORT the count (recorded PO decision: delete + report).
-- RAISE NOTICE reaches the migration/provisioning log.
DO $$
DECLARE
    stray_count integer;
BEGIN
    WITH item_types AS (
        SELECT id, 'strategy' AS entity_type FROM strategies
        UNION ALL SELECT id, 'initiative' FROM initiatives
        UNION ALL SELECT id, 'task' FROM tasks
        UNION ALL SELECT id, 'document' FROM documents
        UNION ALL SELECT id, 'adr' FROM adrs
    ),
    deleted AS (
        DELETE FROM item_metadata im
        USING item_types it
        WHERE it.id = im.item_id
          AND EXISTS (
              SELECT 1 FROM metadata_definition_scopes s
              WHERE s.metadata_definition_id = im.metadata_definition_id
          )
          AND NOT EXISTS (
              SELECT 1 FROM metadata_definition_scopes s
              WHERE s.metadata_definition_id = im.metadata_definition_id
                AND s.entity_type = it.entity_type
          )
        RETURNING im.item_id
    )
    SELECT COUNT(*) INTO stray_count FROM deleted;
    RAISE NOTICE 'KAIROS-T-0078: deleted % stray out-of-scope item_metadata row(s)', stray_count;
END $$;
