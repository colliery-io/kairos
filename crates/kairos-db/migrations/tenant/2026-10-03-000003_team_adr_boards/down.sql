-- Reverse of COLLIERY-T-3102: nothing to do. The up migration only sets the
-- team of an ADR board, and the code of before reads an ADR board with a
-- team. The down migration cannot know which team the up migration set and
-- which team an admin set, so each team stays.
SELECT 1;
