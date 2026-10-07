-- Reverse of KAIROS-T-0340: removes the opt-in. Each repository is then
-- summarized by the embedded model.
ALTER TABLE repositories DROP COLUMN IF EXISTS code_index_summaries;
