-- KAIROS-T-0358, down. A repository that follows the organization gets the
-- value that the organization gave it, so the builder does what it did.
UPDATE repositories
   SET code_index_summaries = COALESCE(
           (SELECT default_summaries FROM code_index_settings WHERE id = 1),
           'embedded')
 WHERE code_index_summaries = 'organization';
ALTER TABLE repositories
    ALTER COLUMN code_index_summaries SET DEFAULT 'embedded';
ALTER TABLE repositories
    DROP CONSTRAINT IF EXISTS repositories_code_index_summaries_check;
ALTER TABLE repositories
    ADD CONSTRAINT repositories_code_index_summaries_check
    CHECK (code_index_summaries IN ('embedded', 'hosted'));
ALTER TABLE code_index_settings DROP COLUMN IF EXISTS default_summaries;
