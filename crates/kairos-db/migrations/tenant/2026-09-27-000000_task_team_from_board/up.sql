-- COLLIERY-T-0216 (COLLIERY-A-0023 rule 2): the board of a task decides its
-- team.
--
-- Until now `tasks.team_id` was a value a caller sent. Three writers
-- disagreed about it: POST /api/tasks stored whatever the request named, the
-- MCP tool sent none and so stored NULL, and binding a repository overwrote
-- it with the repository's owner. Four reads trust the column - a team's
-- work documents, its link rollup, the search team filter, and the text an
-- embedding is made from - so a task could be counted as the work of a team
-- whose board it was never on.
--
-- The writers are fixed in code. This brings the rows they already wrote into
-- line: every task takes the team of the board it sits on. A board with no
-- team gives a task with no team.
--
-- Archived tasks are included on purpose. An archived task still answers by
-- short code and still appears in an include-archived search, and restoring
-- it must not bring back a team it should never have had.
--
-- Re-runnable: IS DISTINCT FROM makes the second run match nothing.

UPDATE tasks t
   SET team_id = b.team_id
  FROM boards b
 WHERE b.id = t.board_id
   AND t.team_id IS DISTINCT FROM b.team_id;
