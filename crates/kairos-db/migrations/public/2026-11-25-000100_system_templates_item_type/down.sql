-- KAIROS-T-0787: the item type, the task type and use_when of the system templates go.
DROP INDEX IF EXISTS public.system_templates_one_for_each_type;
ALTER TABLE public.system_templates DROP CONSTRAINT IF EXISTS system_templates_task_type_check;
ALTER TABLE public.system_templates DROP CONSTRAINT IF EXISTS system_templates_item_type_check;
ALTER TABLE public.system_templates DROP COLUMN IF EXISTS use_when;
ALTER TABLE public.system_templates DROP COLUMN IF EXISTS task_type;
ALTER TABLE public.system_templates DROP COLUMN IF EXISTS item_type;
