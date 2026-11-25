-- KAIROS-T-0787 (COLLIERY-I-0683, KAIROS-A-0087): a template has an item
-- type, and a task template has a task type. One template for each fixed
-- type (strategy, initiative, adr, and each task type); a document type is
-- a template, so the documents can have many. `use_when` says when to use
-- the type.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 3 new columns on templates. Each template that exists is a document
-- template: item_type 'document', no task type, an empty use_when. No other
-- row is changed. templates is small: the CHECKs and the unique index are
-- checked at once.
--
-- Re-runnable.
ALTER TABLE templates ADD COLUMN IF NOT EXISTS item_type TEXT NOT NULL DEFAULT 'document';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS task_type TEXT;
ALTER TABLE templates ADD COLUMN IF NOT EXISTS use_when TEXT NOT NULL DEFAULT '';
ALTER TABLE templates DROP CONSTRAINT IF EXISTS templates_item_type_check;
ALTER TABLE templates ADD CONSTRAINT templates_item_type_check
    CHECK (item_type IN ('strategy', 'initiative', 'task', 'document', 'adr'));
ALTER TABLE templates DROP CONSTRAINT IF EXISTS templates_task_type_check;
ALTER TABLE templates ADD CONSTRAINT templates_task_type_check
    CHECK ((item_type = 'task') = (task_type IS NOT NULL)
           AND (task_type IS NULL OR task_type IN ('task', 'bug', 'tech_debt', 'support')));
CREATE UNIQUE INDEX IF NOT EXISTS templates_one_for_each_type
    ON templates (item_type, COALESCE(task_type, ''))
    WHERE item_type <> 'document';
