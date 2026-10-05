-- KAIROS-T-0318 (COLLIERY-I-0023): a repository can opt out of the code
-- index builder.
--
--   repositories.code_index_build   'on' (the default) or 'off'.
--
-- 'on': the builder makes the first index of the repository when it has
-- none, and updates the index after each push to the default branch.
-- 'off': the builder does nothing for the repository (for example a
-- template or a static site). An upload of an index still works.
--
-- Each row that exists gets 'on', which is what the builder did until now
-- for a repository with an index. Re-runnable.
ALTER TABLE repositories
    ADD COLUMN IF NOT EXISTS code_index_build TEXT NOT NULL DEFAULT 'on';

ALTER TABLE repositories
    DROP CONSTRAINT IF EXISTS repositories_code_index_build_check;
ALTER TABLE repositories
    ADD CONSTRAINT repositories_code_index_build_check
    CHECK (code_index_build IN ('on', 'off'));
