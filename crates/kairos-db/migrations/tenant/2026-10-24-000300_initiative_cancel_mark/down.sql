-- KAIROS-T-0678, down. The marks of initiatives are deleted, because the
-- foreign key to `tasks` comes back. The initiatives stay in their done
-- column. The `cancel` rows of `activity_log` keep the reasons.
DELETE FROM task_cancellations k WHERE NOT EXISTS (SELECT 1 FROM tasks t WHERE t.id = k.task_id);
ALTER TABLE task_cancellations DROP CONSTRAINT IF EXISTS task_cancellations_task_id_fkey;
ALTER TABLE task_cancellations
    ADD CONSTRAINT task_cancellations_task_id_fkey
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE;
