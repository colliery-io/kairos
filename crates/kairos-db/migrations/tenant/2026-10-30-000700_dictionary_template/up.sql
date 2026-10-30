-- KAIROS-T-0733 (COLLIERY-I-0664 decision 6): the system template
-- "Dictionary". A dictionary is a document made with this template. Its
-- entries are the rows of a table Word | Part of speech | Meaning.
--
-- A new tenant gets the template from public.system_templates at
-- provisioning (kairos-db tenant.rs). That copy runs after this migration
-- and skips a slug that is there.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- One row in `templates`, when the tenant has no template with the name
-- "Dictionary" and no template with the slug `dictionary`. A template that
-- the tenant made is not read and not changed. No other row is changed.
--
-- Re-runnable: the insert has a guard. `templates` is a small table
-- (KAIROS-A-0062).

-- The text is the text of the system default in `kairos_db::tenant`. A test
-- compares the two.
INSERT INTO templates (name, slug, content, is_system_default)
SELECT 'Dictionary', 'dictionary',
       E'# Dictionary\n\nThe terms of this board. Write one row for each term. A term of a team replaces a term of the organization that has the same word.\n\n| Word | Part of speech | Meaning |\n|---|---|---|\n',
       true
 WHERE NOT EXISTS (
        SELECT 1 FROM templates
         WHERE name = 'Dictionary' OR slug = 'dictionary'
       );
