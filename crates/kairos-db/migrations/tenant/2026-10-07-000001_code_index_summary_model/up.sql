-- KAIROS-T-0338 (COLLIERY-I-0611): the pool records the model that wrote
-- each summary, as `provider/model`. The model is part of each summary key
-- from this version on, so a pool holds the summaries of 2 models as 2 sets
-- of rows; the column says which is which.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- Each row that exists gets 'embedded/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M':
-- the one model that wrote summaries until now. No row is deleted. The keys
-- of those rows have no model in them, so a run of this version makes new
-- rows and does not read them; they are left as they are (a later task can
-- remove rows that no index uses).
--
-- Re-runnable: the statement has a guard, and the second run changes
-- nothing.
ALTER TABLE code_index_summaries
    ADD COLUMN IF NOT EXISTS model TEXT NOT NULL
    DEFAULT 'embedded/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M';
