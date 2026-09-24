-- KAIROS-T-0186 (KAIROS-I-0017, KAIROS-A-0021 rule 7): weight the title above
-- the content so that ranking means something.
--
-- `ts_rank_cd` ranks by term density and proximity. With the tsvector built as
-- `to_tsvector(title || ' ' || content)` every lexeme carries the default `D`
-- weight, so a long document that happens to mention a term four times
-- outranks a task whose *title* is that term. Shipping relevance on top of an
-- unweighted vector is shipping ranking in name only, and the first search
-- anyone runs shows it.
--
-- So the vector becomes two weighted halves concatenated: the title at `A`, the
-- content at `B`. Against PostgreSQL's default weights `{D,C,B,A}` =
-- `{0.1, 0.2, 0.4, 1.0}` a title hit is worth 2.5x a body hit.
--
-- Matching is unchanged, which is the part worth being sure about:
--
--   * the lexeme SET is identical — the same two strings are analysed, only
--     separately, and `' '` never contributed a lexeme;
--   * `tsvector || tsvector` shifts the right operand's positions past the
--     left's maximum, so positions stay monotonic and a phrase straddling the
--     title/content boundary still matches, exactly as it did when the two were
--     concatenated as text before analysis.
--
-- Therefore every `tsv @@ websearch_to_tsquery(...)` answer is the same set as
-- before; only `ts_rank_cd` sees a difference. No existing caller's results
-- change, because until this task nothing ranked.
--
-- The five `idx_*_tsv` GIN indexes are recreated on the new expression in the
-- same migration. KAIROS-T-0157's migration spells out why that is mandatory
-- rather than tidy: the index expression has to match `searchable_items`' own
-- expression character for character, or the planner stops recognising the
-- index as covering the view's `tsv` column and the default search path becomes
-- a sequential scan that re-derives every row's tsvector. They stay
-- non-partial, for the reasons T-0157 measured.

DROP VIEW IF EXISTS searchable_items;

CREATE VIEW searchable_items AS
    SELECT id, short_code, 'strategy' AS entity_type, title, content,
           setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B') AS tsv,
           deleted_at
    FROM strategies
    UNION ALL
    SELECT id, short_code, 'initiative', title, content,
           setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B'),
           deleted_at
    FROM initiatives
    UNION ALL
    SELECT id, short_code, 'task', title, content,
           setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B'),
           deleted_at
    FROM tasks
    UNION ALL
    SELECT id, short_code, 'document', title, content,
           setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B'),
           deleted_at
    FROM documents
    UNION ALL
    SELECT id, short_code, 'adr', title, content,
           setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B'),
           deleted_at
    FROM adrs;

DROP INDEX IF EXISTS idx_strategies_tsv;
DROP INDEX IF EXISTS idx_initiatives_tsv;
DROP INDEX IF EXISTS idx_tasks_tsv;
DROP INDEX IF EXISTS idx_documents_tsv;
DROP INDEX IF EXISTS idx_adrs_tsv;

CREATE INDEX idx_strategies_tsv ON strategies USING GIN ((setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B')));
CREATE INDEX idx_initiatives_tsv ON initiatives USING GIN ((setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B')));
CREATE INDEX idx_tasks_tsv ON tasks USING GIN ((setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B')));
CREATE INDEX idx_documents_tsv ON documents USING GIN ((setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B')));
CREATE INDEX idx_adrs_tsv ON adrs USING GIN ((setweight(to_tsvector('english', coalesce(title, '')), 'A') || setweight(to_tsvector('english', coalesce(content, '')), 'B')));
