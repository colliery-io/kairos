-- Reverse of COLLIERY-T-0269: removes the `impacts` links and the owner
-- board of each document. A document that named a board and supports no
-- item has no owner after this: only its creator and an organization admin
-- can edit it.
--
-- The template "Product Vision" stays. A document can have it as its
-- template, and a tenant can have changed it.
DROP INDEX IF EXISTS idx_item_impacts_target;
DROP TABLE IF EXISTS item_impacts;

DROP INDEX IF EXISTS idx_documents_board;
ALTER TABLE documents DROP COLUMN IF EXISTS board_id;
