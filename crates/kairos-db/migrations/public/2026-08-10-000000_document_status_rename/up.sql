-- KAIROS-T-0066: the system default metadata definition 'Status' is a
-- DOCUMENT-workflow field (draft/review/approved; declared by the document
-- templates, and documents are off-board) — not an item status, which is
-- the board column. Rename the display name so it cannot be read as board
-- position. Slug stays 'status': template associations reference it.
--
-- Guarded on the current name so an operator's customization is respected
-- (the provisioning seed never overwrites either, KAIROS-T-0008).
UPDATE public.system_metadata_definitions
SET name = 'Document status'
WHERE slug = 'status'
  AND name = 'Status';
