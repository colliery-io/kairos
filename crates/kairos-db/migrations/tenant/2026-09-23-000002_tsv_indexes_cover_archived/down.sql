-- Reverse KAIROS-T-0157: put the `WHERE deleted_at IS NULL` predicate back
-- on the five full-text indexes.
--
-- Safe as DDL — index definitions carry no data of their own — but it is
-- not safe to run while the code that relies on it is deployed: a search
-- asking for archived work will still return the right rows, it will just
-- stop using an index to find them. Like its sibling KAIROS-T-0156, this
-- file exists mainly for the fleet-upgrade rehearsal in
-- `tests/tenant_provisioning.rs`.

DROP INDEX IF EXISTS idx_strategies_tsv;
DROP INDEX IF EXISTS idx_initiatives_tsv;
DROP INDEX IF EXISTS idx_tasks_tsv;
DROP INDEX IF EXISTS idx_documents_tsv;
DROP INDEX IF EXISTS idx_adrs_tsv;

CREATE INDEX idx_strategies_tsv ON strategies USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_initiatives_tsv ON initiatives USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_tsv ON tasks USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_documents_tsv ON documents USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_adrs_tsv ON adrs USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
