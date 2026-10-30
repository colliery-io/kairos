-- KAIROS-T-0731: validate the foreign key of section_checks that the
-- migration before added NOT VALID (KAIROS-A-0062). VALIDATE reads each row
-- and does not block a write.
ALTER TABLE section_checks VALIDATE CONSTRAINT section_checks_document_id_fkey;
