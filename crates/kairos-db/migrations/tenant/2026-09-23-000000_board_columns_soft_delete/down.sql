-- Reverse KAIROS-T-0161. Any column already soft-deleted has to go for
-- real, and with it the transition edges that only the FK cascade was
-- holding — there is nowhere else to put them once the flag is gone.
-- Cards still pointing at such a column would block the delete, which is
-- the pre-T-0161 behaviour reasserting itself: this down migration is only
-- safe on a tenant where no removed column still holds archived cards.
DELETE FROM board_columns WHERE deleted_at IS NOT NULL;

DROP INDEX IF EXISTS board_columns_live_position_key;
DROP INDEX IF EXISTS board_columns_live_name_key;

ALTER TABLE board_columns
    ADD CONSTRAINT board_columns_board_id_position_key UNIQUE (board_id, position);
ALTER TABLE board_columns
    ADD CONSTRAINT board_columns_board_id_name_key UNIQUE (board_id, name);

ALTER TABLE board_columns DROP COLUMN IF EXISTS deleted_at;
