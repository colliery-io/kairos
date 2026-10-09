-- KAIROS-T-0359 (KAIROS-A-0024, decisions 4 and 5): a task in Active has a
-- claim.
--
--   board_columns.claims   A task that a person moves into a column with
--                          this flag gets a claim for the person. The task
--                          loses the claim when it leaves the column.
--   task_claims            The claim of a task: the person
--                          (`user_id`), the agent key of the request
--                          that made the claim (`agent_key_id`, or NULL),
--                          and the time. One claim for each task at most.
--
-- No foreign key on user_id and agent_key_id: users are in the public
-- schema, and the mark of the agent key stays when the key is revoked
-- (the rule of 2026-10-09-000002_agent_key_marks). The claim of a task
-- goes with the task when the retention sweep deletes the row of the
-- task (ON DELETE CASCADE).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new column and 1 new table. Each live column named 'Active' on a
-- delivery board gets claims = true. Each other column gets false. A
-- removed column (deleted_at set) and a column of a deleted board are
-- not changed. No task gets a claim: the tasks in Active now have no
-- claim, and the next person who moves a task into Active gets it. No
-- other row is changed.
--
-- Re-runnable: each statement has a guard, and a second run right after
-- the first changes nothing.
ALTER TABLE board_columns
    ADD COLUMN IF NOT EXISTS claims BOOLEAN NOT NULL DEFAULT false;

UPDATE board_columns AS c
   SET claims = true
  FROM boards AS b
 WHERE b.id = c.board_id
   AND b.board_level = 'delivery'
   AND b.deleted_at IS NULL
   AND c.deleted_at IS NULL
   AND c.name = 'Active'
   AND NOT c.claims;

CREATE TABLE IF NOT EXISTS task_claims (
    task_id      UUID PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL,
    agent_key_id UUID NULL,
    claimed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS task_claims_user_id_idx ON task_claims (user_id);
