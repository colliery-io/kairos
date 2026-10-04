-- Reverse of COLLIERY-T-3109: a document can name no board again. No row
-- changes: each document keeps its owner board.
ALTER TABLE documents ALTER COLUMN board_id DROP NOT NULL;
