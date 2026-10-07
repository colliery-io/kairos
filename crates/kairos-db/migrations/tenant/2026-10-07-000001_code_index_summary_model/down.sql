-- Reverse of KAIROS-T-0338: removes the model of each summary.
ALTER TABLE code_index_summaries DROP COLUMN IF EXISTS model;
