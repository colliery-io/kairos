-- KAIROS-T-0718 (COLLIERY-I-0662): a task has one review state, a reserved
-- field like the claim, the cancel mark and the live state.
--
--   task_review   Whether the task needs a review before it is done:
--                 `not_needed`, `needed` or `done`, with who set it when.
--                 One row for each task at most, replaced on each write.
--                 No row: nobody decided yet.
--
-- The row stays in each column, Completed too. No history: the activity
-- log holds the record (one `update` row with `review:<value>` per write).
-- No foreign key on updated_by: users are in the public schema (the rule
-- of 2026-10-09-000003_task_claims). The review goes with the task when
-- the retention sweep deletes the row of the task (ON DELETE CASCADE).
--
-- A dispatch rule can fire when the review state changes (the trigger
-- `review_changed`). Its column is optional, as for `unblocked`: with a
-- column, the task must be in it.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty: each task has no review state. The CHECK of
-- dispatch_rules.trigger allows one more value; no row is changed.
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS task_review (
    task_id    UUID PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    review     TEXT NOT NULL CHECK (review IN ('not_needed', 'needed', 'done')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID NOT NULL
);

ALTER TABLE dispatch_rules DROP CONSTRAINT IF EXISTS dispatch_rules_trigger_check;
ALTER TABLE dispatch_rules ADD CONSTRAINT dispatch_rules_trigger_check
    CHECK (trigger IN ('entered_column', 'unblocked', 'review_changed')
           AND (trigger <> 'entered_column' OR column_id IS NOT NULL));
