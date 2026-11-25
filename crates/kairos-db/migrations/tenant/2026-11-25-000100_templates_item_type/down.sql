-- KAIROS-T-0787: the item type, the task type and use_when of templates go.
DROP INDEX IF EXISTS templates_one_for_each_type;
ALTER TABLE templates DROP CONSTRAINT IF EXISTS templates_task_type_check;
ALTER TABLE templates DROP CONSTRAINT IF EXISTS templates_item_type_check;
ALTER TABLE templates DROP COLUMN IF EXISTS use_when;
ALTER TABLE templates DROP COLUMN IF EXISTS task_type;
ALTER TABLE templates DROP COLUMN IF EXISTS item_type;
