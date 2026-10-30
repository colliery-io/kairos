-- KAIROS-T-0729 (COLLIERY-I-0664): a template names its typed sections.
--
--   template_sections   One row for each typed section of a template: the
--                       text of its heading (no `#`; it matches a heading
--                       of each level in the content) and its type. The
--                       server checks the text under each typed section
--                       (KAIROS-T-0730).
--
-- The types are `free` and `plain`. `ace` comes later (KAIROS-T-0732): it
-- changes the CHECK on this small table. `position` keeps the order that
-- the admin sent. The rows go with the template (ON DELETE CASCADE).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty: each template has no typed section. No other table
-- changes, so no lock on a large table (KAIROS-A-0062).
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS template_sections (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    template_id  UUID NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    heading      TEXT NOT NULL CHECK (btrim(heading) <> ''),
    section_type TEXT NOT NULL CHECK (section_type IN ('free', 'plain')),
    position     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (template_id, heading)
);
