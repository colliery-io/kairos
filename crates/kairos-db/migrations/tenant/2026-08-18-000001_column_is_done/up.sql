-- KAIROS-T-0080: machine-readable "done". `is_done` marks the columns
-- whose occupants count as completed for the children-progress rollup —
-- replacing the dead-end heuristic (kairos-core board.rs), which stays
-- as an admin-UI suggestion only and never sets this flag itself.
--
-- Existing tenants: every column starts false (strictly manual opt-in
-- per board; recorded decision on KAIROS-T-0080). Freshly provisioned
-- boards get their terminal columns flagged in the board-creation path.
--
-- Guarded for re-runnability (fleet-migration hygiene).
ALTER TABLE board_columns
    ADD COLUMN IF NOT EXISTS is_done BOOLEAN NOT NULL DEFAULT false;
