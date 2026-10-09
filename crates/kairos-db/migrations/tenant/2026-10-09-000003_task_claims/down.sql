-- KAIROS-T-0359, down. The claims of the tasks are lost, and no column
-- marks a claim. The tasks and their columns stay.
DROP TABLE IF EXISTS task_claims;
ALTER TABLE board_columns DROP COLUMN IF EXISTS claims;
