-- COLLIERY-T-3102 (COLLIERY-I-0407): team ADR boards.
--
-- An ADR board can have a team now: the board of the delivery ADRs of the
-- team. A team has at most one ADR board, and it has the prefix of the
-- team (the prefix of the delivery board of the team). kairos_db::boards::
-- create_board holds these rules for a new board.
--
-- This migration gives a team to an existing ADR board when the prefix of
-- the board names one team: a live ADR board with no team takes the team
-- of the live delivery board with the same prefix, if that team is live
-- and has no ADR board. On the tenant `colliery`, the board `adrs`
-- (COLLIERY) becomes the ADR board of the team `colliery-io` (its delivery
-- board `colliery-io-delivery` has COLLIERY). A prefix is unique for each
-- level among the live boards (COLLIERY-T-3099), so at most one delivery
-- board has the prefix of an ADR board.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- It sets `boards.team_id` on 0 or more ADR boards that have no team. No
-- other column and no other row changes. No short code changes: the prefix
-- of the board stays the same. The ADR boards of new teams (skadi, crt,
-- graphqlite) are not made here. An admin makes them with POST /api/boards.
--
-- Re-runnable: it only fills a team that is NULL, and a team that has an
-- ADR board gets no second one. The second run changes nothing.
UPDATE boards AS adr
SET team_id = delivery.team_id,
    updated_at = now()
FROM boards AS delivery
JOIN teams AS team ON team.id = delivery.team_id AND team.deleted_at IS NULL
WHERE adr.board_level = 'adr'
  AND adr.deleted_at IS NULL
  AND adr.team_id IS NULL
  AND delivery.board_level = 'delivery'
  AND delivery.deleted_at IS NULL
  AND delivery.code_prefix = adr.code_prefix
  AND NOT EXISTS (
      SELECT 1
      FROM boards AS other
      WHERE other.team_id = delivery.team_id
        AND other.board_level = 'adr'
        AND other.deleted_at IS NULL
  );
