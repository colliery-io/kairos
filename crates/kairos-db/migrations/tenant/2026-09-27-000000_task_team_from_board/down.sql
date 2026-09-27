-- COLLIERY-T-0216: nothing to undo. The up migration overwrote `team_id`
-- with the board's team and did not keep what was there, because what was
-- there was the defect. Restoring from a backup is the only way back.
SELECT 1;
