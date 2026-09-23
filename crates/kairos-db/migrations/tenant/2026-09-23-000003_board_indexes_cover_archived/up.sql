-- KAIROS-T-0159 (KAIROS-A-0020, initiative KAIROS-I-0015 D3): the three
-- `idx_*_board` indexes stop being partial, so `GET /api/boards/{id}/items
-- ?include_deleted=true` can find a board's archived cards instead of
-- reading the whole table to rule the others out.
--
-- The indexes were created `WHERE deleted_at IS NULL` (the original tenant
-- DDL, up.sql:180/203/224), which was free while every board query carried
-- exactly that predicate. The audit view of a board declines the predicate,
-- and a partial index cannot serve a query that declines its predicate — so
-- the wide mode fell back to a sequential scan of the entire family table.
-- That is O(tenant) where the board listing should be O(board): the cost of
-- reading one team's archived cards grew with every other team's live ones.
--
-- Measured on 200k tasks over 20 boards (10k each), 15% archived scattered
-- uniformly, PostgreSQL 16, warm cache, `EXPLAIN (ANALYZE, BUFFERS)` on
-- `WHERE board_id = $1 [AND deleted_at IS NULL] ORDER BY short_code`:
--
--                     partial (before)          non-partial (after)
--   default listing   3.97 ms, 2181 buffers     3.97 ms, 2190 buffers
--   include archived  29.7 ms, 10440 buffers    21.3 ms, 2848 buffers
--
-- The DEFAULT path is the one that had to be protected, and it does not
-- move: the plan is a Bitmap Heap Scan either way, so it was already
-- fetching the heap tuple, and `deleted_at IS NULL` costs the same as a
-- heap filter as it did folded into the index predicate. What changes is
-- that the wide mode stops reading 3.7x the buffers, and stops scaling
-- with the tenant instead of the board.
--
-- Two shapes were measured, not one, because KAIROS-T-0157's conclusion
-- for the full-text indexes does not transfer: those sit on a cold path,
-- these sit on the hot one that every board render uses.
--
--   (a) replace the partial index with a full one   — measured above
--   (b) keep the partial and ADD a second, full one — 3.99 ms / 20.1 ms
--
-- The two perform identically, within noise, on both paths. (b) costs a
-- whole extra index to maintain on every task write plus 1400 kB; (a)
-- costs 264 kB (1136 kB → 1400 kB, +23%) and nothing else. Same benefit,
-- strictly lower price, so (a).
--
-- An archived-only index (`WHERE deleted_at IS NOT NULL`) was considered
-- and rejected on mechanism rather than on cost: the widened query says
-- nothing at all about `deleted_at`, so neither half of a complementary
-- pair of partial indexes is usable by it. Postgres does not reason that
-- two partial indexes together cover a table.
--
-- `idx_*_column` stays partial deliberately. Nothing queries by column —
-- the listing buckets rows into columns in Rust — and leaving it narrow
-- keeps a small index available for the planner on the default path while
-- the full `idx_*_board` serves the audit path. ADRs have no board index
-- to widen: an ADR may be off-board entirely, and that listing already
-- scanned in both modes.

DROP INDEX IF EXISTS idx_strategies_board;
DROP INDEX IF EXISTS idx_initiatives_board;
DROP INDEX IF EXISTS idx_tasks_board;

CREATE INDEX idx_strategies_board ON strategies(board_id);
CREATE INDEX idx_initiatives_board ON initiatives(board_id);
CREATE INDEX idx_tasks_board ON tasks(board_id);
