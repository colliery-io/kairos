-- KAIROS-T-0762 (COLLIERY-I-0674, decisions 1–4): a watch records who
-- added it.
--
--   item_watches.added_by   The person who added the watch. For a watch
--                           that the watcher started, the watcher.
--
-- Only the watcher or the person who added the watch can remove it.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new column on item_watches. Each row that is there gets
-- added_by = user_id: each watch before this migration was a self-watch.
-- item_watches is a small table (one row for each watch), so the UPDATE
-- and the NOT NULL run in one step.
--
-- Re-runnable: each statement has a guard.
ALTER TABLE item_watches ADD COLUMN IF NOT EXISTS added_by UUID;
UPDATE item_watches SET added_by = user_id WHERE added_by IS NULL;
ALTER TABLE item_watches ALTER COLUMN added_by SET NOT NULL;
