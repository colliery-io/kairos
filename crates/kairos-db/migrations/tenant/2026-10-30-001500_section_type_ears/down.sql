-- KAIROS-T-0732, down. An `ears` section becomes a `free` section: it must
-- be there and have text, with no EARS check.
UPDATE template_sections SET section_type = 'free' WHERE section_type = 'ears';
ALTER TABLE template_sections
    DROP CONSTRAINT IF EXISTS template_sections_section_type_check;
ALTER TABLE template_sections
    ADD CONSTRAINT template_sections_section_type_check
    CHECK (section_type IN ('free', 'plain'));
