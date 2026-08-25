-- Reverse KAIROS-T-0078 (tenant side): restore the 'Document status'
-- definition from documents.lifecycle, then drop the column and the
-- scopes table. Deleted stray out-of-scope rows are NOT restorable.
INSERT INTO metadata_definitions (name, slug, field_type, is_system_default)
VALUES ('Document status', 'status', 'enum', true)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO metadata_enum_options (metadata_definition_id, value, position)
SELECT d.id, o.value, o.position
FROM (VALUES
    ('status', 'draft', 0),
    ('status', 'review', 1),
    ('status', 'approved', 2)
) AS o(slug, value, position)
JOIN metadata_definitions d ON d.slug = o.slug
ON CONFLICT (metadata_definition_id, value) DO NOTHING;

-- lifecycle -> legacy status values (published/archived -> approved).
INSERT INTO item_metadata (item_id, metadata_definition_id, value)
SELECT d.id, md.id,
       CASE d.lifecycle WHEN 'draft' THEN 'draft'
                        WHEN 'review' THEN 'review'
                        ELSE 'approved' END
FROM documents d
JOIN metadata_definitions md ON md.slug = 'status' AND md.is_system_default
ON CONFLICT (item_id, metadata_definition_id) DO NOTHING;

INSERT INTO template_metadata (template_id, metadata_definition_id, default_value, required)
SELECT t.id, d.id, 'draft', false
FROM templates t
JOIN metadata_definitions d ON d.slug = 'status' AND d.is_system_default
WHERE t.slug IN ('prd', 'system_context', 'architecture_framing')
ON CONFLICT (template_id, metadata_definition_id) DO NOTHING;

ALTER TABLE documents DROP COLUMN IF EXISTS lifecycle;
DROP TABLE IF EXISTS metadata_definition_scopes;
