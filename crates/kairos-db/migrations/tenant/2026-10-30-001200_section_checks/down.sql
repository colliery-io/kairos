-- KAIROS-T-0731, down. The stored checks are lost; the next writes and
-- the periodic pass do not write them again.
DROP TABLE IF EXISTS section_checks;
