-- KAIROS-T-0732 (COLLIERY-I-0664): a typed section can be of the type
-- `ears`. Each requirement of an `ears` section must fit one frame of EARS
-- (Easy Approach to Requirements Syntax).
--
-- The CHECK on template_sections.section_type gets `ears`. The old CHECK
-- (from the inline column constraint of 2026-10-30-000100) is replaced.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- No row changes. template_sections is a small table (one row for each
-- typed section of a template), so the new CHECK is validated in place and
-- the lock is short (KAIROS-A-0062).
--
-- Re-runnable: the drop has a guard, and the add follows it.
ALTER TABLE template_sections
    DROP CONSTRAINT IF EXISTS template_sections_section_type_check;
ALTER TABLE template_sections
    ADD CONSTRAINT template_sections_section_type_check
    CHECK (section_type IN ('free', 'plain', 'ears'));
