-- Reverse KAIROS-T-0080: drop the done flag.
ALTER TABLE board_columns DROP COLUMN IF EXISTS is_done;
