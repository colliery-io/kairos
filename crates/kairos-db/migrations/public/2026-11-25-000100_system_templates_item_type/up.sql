-- KAIROS-T-0787 (COLLIERY-I-0683, KAIROS-A-0087): a template has an item
-- type, and a task template has a task type. One template for each fixed
-- type (strategy, initiative, adr, and each task type); a document type is
-- a template, so the documents can have many. `use_when` says when to use
-- the type.
--
-- WHAT THIS DOES TO THE DATA
--
-- 3 new columns on public.system_templates, the source of the templates of
-- a new tenant. Each system template that exists is a document
-- template: item_type 'document', no task type, an empty use_when. No other
-- row is changed. The table is small: the CHECKs and the unique index are
-- checked at once.
--
-- Re-runnable.
ALTER TABLE public.system_templates ADD COLUMN IF NOT EXISTS item_type TEXT NOT NULL DEFAULT 'document';
ALTER TABLE public.system_templates ADD COLUMN IF NOT EXISTS task_type TEXT;
ALTER TABLE public.system_templates ADD COLUMN IF NOT EXISTS use_when TEXT NOT NULL DEFAULT '';
ALTER TABLE public.system_templates DROP CONSTRAINT IF EXISTS system_templates_item_type_check;
ALTER TABLE public.system_templates ADD CONSTRAINT system_templates_item_type_check
    CHECK (item_type IN ('strategy', 'initiative', 'task', 'document', 'adr'));
ALTER TABLE public.system_templates DROP CONSTRAINT IF EXISTS system_templates_task_type_check;
ALTER TABLE public.system_templates ADD CONSTRAINT system_templates_task_type_check
    CHECK ((item_type = 'task') = (task_type IS NOT NULL)
           AND (task_type IS NULL OR task_type IN ('task', 'bug', 'tech_debt', 'support')));
CREATE UNIQUE INDEX IF NOT EXISTS system_templates_one_for_each_type
    ON public.system_templates (item_type, COALESCE(task_type, ''))
    WHERE item_type <> 'document';
