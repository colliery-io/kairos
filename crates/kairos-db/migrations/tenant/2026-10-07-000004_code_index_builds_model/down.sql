-- Reverse of KAIROS-T-0341: removes the model of each run.
ALTER TABLE code_index_builds DROP COLUMN IF EXISTS model;
