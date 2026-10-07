-- KAIROS-T-0340 (COLLIERY-I-0611): a repository opts in to the hosted
-- summaries of its tenant.
--
--   repositories.code_index_summaries   'embedded' (the default) or 'hosted'.
--
-- 'embedded': the summaries of the repository come from the model in the
-- server, whatever the tenant set. 'hosted': they come from the provider of
-- the tenant (code_index_settings), and the code of each changed symbol
-- leaves the host. A person opts a repository in; the default keeps the
-- code on the host.
--
-- Each row that exists gets 'embedded', which is what the builder did until
-- now. Re-runnable.
ALTER TABLE repositories
    ADD COLUMN IF NOT EXISTS code_index_summaries TEXT NOT NULL DEFAULT 'embedded';

ALTER TABLE repositories
    DROP CONSTRAINT IF EXISTS repositories_code_index_summaries_check;
ALTER TABLE repositories
    ADD CONSTRAINT repositories_code_index_summaries_check
    CHECK (code_index_summaries IN ('embedded', 'hosted'));
