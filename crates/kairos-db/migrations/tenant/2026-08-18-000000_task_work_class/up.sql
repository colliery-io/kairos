-- KAIROS-T-0077: the Planned/Support lane axis on delivery boards.
--
-- `work_class` records whether the work was planned ('planned') or
-- arrived as unplanned intake ('support') — orthogonal to `task_type`,
-- which says what KIND of work it is and gains 'support' for support
-- requests. Recorded product decision: there is NO bug-remediation lane;
-- a bug's lane is its planned-ness, its type stays 'bug'.
--
-- Runs with search_path pinned to the tenant schema (KAIROS-T-0008), so
-- table names are unqualified. Existing rows backfill to 'planned' via
-- the column default.
-- Guards keep the migration re-runnable (fleet-migration hygiene: a
-- crash between apply and bookkeeping must not wedge the tenant).
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS work_class TEXT NOT NULL DEFAULT 'planned'
        CHECK (work_class IN ('planned', 'support'));

ALTER TABLE tasks DROP CONSTRAINT IF EXISTS tasks_task_type_check;
ALTER TABLE tasks
    ADD CONSTRAINT tasks_task_type_check
        CHECK (task_type IN ('task', 'bug', 'tech_debt', 'support'));
