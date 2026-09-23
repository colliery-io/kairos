-- Reverse KAIROS-T-0159: put the `WHERE deleted_at IS NULL` predicate back
-- on the three board indexes.
--
-- Safe as DDL — index definitions carry no data of their own — but it is
-- not safe to run while the code that relies on it is deployed: a board
-- listing asking for archived cards will still return the right rows, it
-- will just go back to scanning the family table to find them. Like its
-- siblings KAIROS-T-0156 and KAIROS-T-0157, this file exists mainly for
-- the fleet-upgrade rehearsal in `tests/tenant_provisioning.rs`.

DROP INDEX IF EXISTS idx_strategies_board;
DROP INDEX IF EXISTS idx_initiatives_board;
DROP INDEX IF EXISTS idx_tasks_board;

CREATE INDEX idx_strategies_board ON strategies(board_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_initiatives_board ON initiatives(board_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_board ON tasks(board_id) WHERE deleted_at IS NULL;
