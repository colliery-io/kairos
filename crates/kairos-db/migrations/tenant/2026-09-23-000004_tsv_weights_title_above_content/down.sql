-- Revert to the unweighted tsvector: one `to_tsvector` over title and content
-- concatenated as text, exactly as KAIROS-T-0157's migration left it. The view
-- and all five indexes go back together, because the index expression must
-- match the view's character for character.

DROP VIEW IF EXISTS searchable_items;

CREATE VIEW searchable_items AS
    SELECT id, short_code, 'strategy' AS entity_type, title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')) AS tsv,
           deleted_at
    FROM strategies
    UNION ALL
    SELECT id, short_code, 'initiative', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM initiatives
    UNION ALL
    SELECT id, short_code, 'task', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM tasks
    UNION ALL
    SELECT id, short_code, 'document', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM documents
    UNION ALL
    SELECT id, short_code, 'adr', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')),
           deleted_at
    FROM adrs;

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
