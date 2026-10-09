-- KAIROS-T-0358: an organization admin sets the default summarizer of the
-- code index, and a repository follows it unless it sets its own.
--
--   code_index_settings.default_summaries   'embedded' (the default) or
--                                           'hosted': where the summaries
--                                           of a repository that follows
--                                           the organization are made.
--   repositories.code_index_summaries       'embedded', 'hosted', or now
--                                           'organization': follow the
--                                           default of the organization.
--                                           A new row gets 'organization'.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new column with the value 'embedded', so the builder does what it did.
-- No row of repositories is changed: a value that is there stays, because
-- a person can have set 'embedded' to keep the code on the host. Only the
-- default of a new row changes.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.
ALTER TABLE code_index_settings
    ADD COLUMN IF NOT EXISTS default_summaries TEXT NOT NULL DEFAULT 'embedded';
ALTER TABLE code_index_settings
    DROP CONSTRAINT IF EXISTS code_index_settings_default_summaries_check;
ALTER TABLE code_index_settings
    ADD CONSTRAINT code_index_settings_default_summaries_check
    CHECK (default_summaries IN ('embedded', 'hosted'));

ALTER TABLE repositories
    DROP CONSTRAINT IF EXISTS repositories_code_index_summaries_check;
ALTER TABLE repositories
    ADD CONSTRAINT repositories_code_index_summaries_check
    CHECK (code_index_summaries IN ('embedded', 'hosted', 'organization'));
ALTER TABLE repositories
    ALTER COLUMN code_index_summaries SET DEFAULT 'organization';
