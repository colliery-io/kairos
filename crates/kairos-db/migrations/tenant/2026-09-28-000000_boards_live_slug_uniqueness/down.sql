-- Removes the index, and with it the rule: two live boards can have one
-- slug again.
--
-- It does not give a board the slug that it had before the up migration.
-- The up migration keeps no record of the old slug apart from the NOTICE in
-- its log, and the old state was the defect.
DROP INDEX IF EXISTS boards_live_slug_key;
