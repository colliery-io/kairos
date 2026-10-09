-- KAIROS-T-0362, down. The marks of the cancelled tasks are lost. The
-- tasks stay in their columns, and the `cancel` rows of `activity_log`
-- stay.
DROP TABLE IF EXISTS task_cancellations;
