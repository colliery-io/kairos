-- KAIROS-T-0066 (tenant side): rename the per-tenant copy of the system
-- default 'Status' metadata definition to 'Document status' — it is the
-- document-workflow field (declared by the document templates; documents
-- are off-board), not an item status (that is the board column).
--
-- Runs with search_path pinned to the tenant schema (KAIROS-T-0008), so
-- the table name is unqualified. Guarded on is_system_default AND the
-- current name so tenant-customized definitions are untouched.
UPDATE metadata_definitions
SET name = 'Document status'
WHERE slug = 'status'
  AND is_system_default
  AND name = 'Status';
