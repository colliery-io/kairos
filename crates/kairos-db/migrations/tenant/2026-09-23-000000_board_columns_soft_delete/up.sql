-- KAIROS-T-0161 (KAIROS-A-0020, initiative KAIROS-I-0015 D6): a board
-- column gets a soft delete of its own, so removing a column that holds
-- nothing but archived cards stops being impossible.
--
-- Why it has to be a soft delete. Every entity table declares
-- `column_id UUID NOT NULL REFERENCES board_columns(id)` with no
-- `ON DELETE` clause, so an archived card pins its column open at the
-- database level, not merely by policy. Keeping the row satisfies the FK,
-- which is what lets an archived card still report the real column name it
-- was put away in — the audit fact ADR-20 exists to protect.
ALTER TABLE board_columns
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

-- The two UNIQUE constraints have to stop seeing removed columns, or the
-- soft delete leaves landmines behind it: a column could never be re-added
-- under the name or at the position a removed one still occupies, and
-- `reorder_columns` (which parks columns on negative positions before
-- renumbering) would eventually map a removed row back onto a live one.
-- Partial unique indexes say what was always meant: unique among the
-- columns the board actually has.
ALTER TABLE board_columns DROP CONSTRAINT IF EXISTS board_columns_board_id_position_key;
ALTER TABLE board_columns DROP CONSTRAINT IF EXISTS board_columns_board_id_name_key;

CREATE UNIQUE INDEX IF NOT EXISTS board_columns_live_position_key
    ON board_columns (board_id, position)
    WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS board_columns_live_name_key
    ON board_columns (board_id, name)
    WHERE deleted_at IS NULL;

-- `board_columns_live_position_key` doubles as the index for the one query
-- every board surface runs — the live columns of a board in position order
-- — so no separate index is needed for it.
