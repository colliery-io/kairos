-- KAIROS-T-0157 (KAIROS-A-0020, initiative KAIROS-I-0015 D4): the
-- full-text indexes stop being partial, so archived work can be searched
-- for rather than merely scanned for.
--
-- The five `idx_*_tsv` GIN indexes were created `WHERE deleted_at IS NULL`
-- (the original tenant DDL, up.sql:386-390). That was free while search was
-- live-only, because every full-text query carried exactly that predicate.
-- It stops being free the moment `include_deleted` reaches the `q` branch:
-- a partial index cannot serve a query that declines its predicate, so the
-- wide mode falls back to a parallel sequential scan that re-derives every
-- row's tsvector — and it degrades linearly with the table, which is the
-- opposite of what a search index is for.
--
-- Measured on 20k strategies + 20k tasks, 10% archived, a term matching
-- 0.5% of rows (org_perfbench, PostgreSQL 16):
--
--                         partial (before)        non-partial (after)
--   default mode          cost 594, 0.47 ms       cost 644, 0.51 ms
--   include archived      cost 8906, 133.3 ms     cost 644, 0.09 ms
--
-- One index per table now serves both modes. The default path keeps its
-- `Bitmap Index Scan on idx_*_tsv` and reads the same 206 buffers; what it
-- gives up is having `deleted_at IS NULL` absorbed INTO the index
-- predicate, so the planner applies it as a heap filter instead — worth
-- about 8% of a cost estimate that was already sub-millisecond. The wide
-- mode stops being a table scan.
--
-- Why non-partial rather than a second archived-only index set: archived
-- rows are read-only under ADR-20 / KAIROS-I-0015 D5 — every mutating path
-- loads `LiveOnly` — so they are written exactly once, at archive time.
-- The usual argument for a partial index (keep the hot path's writes off
-- the cold rows) barely applies when the cold rows never take another
-- write. The cost is index size, and it is proportional and small: 1528 kB
-- over 18k live rows became 1648 kB over 20k rows, +7.8%.
--
-- The `tsvector` expression is repeated verbatim from the original DDL. It
-- has to match `searchable_items`' own expression character for character
-- or the planner will not recognise the index as covering the view's `tsv`
-- column.

DROP INDEX IF EXISTS idx_strategies_tsv;
DROP INDEX IF EXISTS idx_initiatives_tsv;
DROP INDEX IF EXISTS idx_tasks_tsv;
DROP INDEX IF EXISTS idx_documents_tsv;
DROP INDEX IF EXISTS idx_adrs_tsv;

CREATE INDEX idx_strategies_tsv ON strategies USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')));
CREATE INDEX idx_initiatives_tsv ON initiatives USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')));
CREATE INDEX idx_tasks_tsv ON tasks USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')));
CREATE INDEX idx_documents_tsv ON documents USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')));
CREATE INDEX idx_adrs_tsv ON adrs USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')));
