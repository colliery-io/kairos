-- Reverse KAIROS-T-0077: retype support tasks to plain tasks so the
-- narrower CHECK can be restored, then drop the lane axis.
UPDATE tasks SET task_type = 'task' WHERE task_type = 'support';

ALTER TABLE tasks DROP CONSTRAINT tasks_task_type_check;
ALTER TABLE tasks
    ADD CONSTRAINT tasks_task_type_check
        CHECK (task_type IN ('task', 'bug', 'tech_debt'));

ALTER TABLE tasks DROP COLUMN work_class;
