-- KAIROS-T-0341 (COLLIERY-I-0611): the run of the builder records the model
-- that wrote its summaries, as `provider/model`, so a person sees which
-- provider a run used. NULL for a run that ended before this version, or
-- that made no summary. Re-runnable.
ALTER TABLE code_index_builds ADD COLUMN IF NOT EXISTS model TEXT;
