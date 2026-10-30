-- KAIROS-T-0733 (COLLIERY-I-0664 decision 6): the system template
-- "Dictionary". A dictionary is a document made with this template. Its
-- entries are the rows of a table Word | Part of speech | Meaning.
-- KAIROS-A-0079 (KAIROS-T-0738): the organization has one dictionary, on
-- the board `strategy`. The text of the template says so; it changed in
-- this file because no release has this migration.
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
       E'# Dictionary\n\nThe terms of the organization. Write one row for each term. The organization has one dictionary, on the board strategy.\n\n| Word | Part of speech | Meaning |\n|---|---|---|\n',
       true
 WHERE NOT EXISTS (
        SELECT 1 FROM templates
         WHERE name = 'Dictionary' OR slug = 'dictionary'
       );
