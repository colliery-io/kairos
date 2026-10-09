-- KAIROS-T-0362: a task can be cancelled, with a reason.
--
--   task_cancellations   The mark of a cancelled task: the person or the
--                        service account that cancelled it
--                        (`cancelled_by`), the reason, and the time. One
--                        mark for each task at most.
--
-- A cancel moves the task to the done column of its board and writes the
-- mark. A move of the task out of the done column removes the mark. The
-- `cancel` row of `activity_log` keeps the reason after that.
--
-- A table, not columns of `tasks`: the mark is a fact about the task in
-- the done column, as the claim is a fact about the task in Active
-- (2026-10-09-000003_task_claims). The views and the queries of `tasks`
-- do not change. The mark goes with the task when Kairos deletes the row
-- of the task (ON DELETE CASCADE).
--
-- No foreign key on cancelled_by: users are in the public schema.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table. No row is changed, and no task is cancelled.
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS task_cancellations (
    task_id      UUID PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    cancelled_by UUID NOT NULL,
    reason       TEXT NOT NULL CHECK (btrim(reason) <> ''),
    cancelled_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
