-- KAIROS-T-0187 (KAIROS-I-0017, KAIROS-A-0021 rules 2, 3 and 4): somewhere to
-- put vectors. Two tables, no index, nothing embedded yet — this migration
-- builds the box.
--
-- TWO tables rather than one column on each entity table, for three reasons:
--
--   1. They answer different questions. `item_embeddings` answers "what is this
--      item about"; `item_chunks` answers "which PART of it matched", which is
--      what a citation needs to point at something a reader can find.
--   2. They invalidate at different rates. Metis-style agents append to a
--      Status Updates section every few tool calls, and that must re-embed ONE
--      chunk, not a 12 KB document. Measured on 4,927 real documents
--      (KAIROS-T-0190): median 9 sections, so the saving is most of the work.
--   3. A column per entity table would be five columns and five backfills, and
--      items live in five tables precisely because they are different shapes.
--
-- `item_id` carries no foreign key, for the same reason `item_relationships`
-- (tenant DDL, 2026-07-09) carries none: an item is a row in one of five
-- tables, so there is nothing single to reference. `entity_type` records which,
-- so a reader knows where to join, and the CHECK keeps the vocabulary closed.
--
-- DIMENSION IS NOT PINNED IN THE COLUMN TYPE. `vector` without a modifier
-- accepts any width, and that is deliberate: KAIROS-T-0189 has not settled on a
-- model yet, and writing `vector(384)` here would encode a guess from a spike as
-- a schema constraint. The width is recorded per row in `dimension` instead, so
-- a provider or model change is DETECTABLE rather than silently comparing
-- vectors from two different spaces. KAIROS-T-0190 pins the type when it adds
-- the index, which is the point where pgvector actually requires a fixed width
-- and the point where the model is known.
--
-- NO VECTOR INDEX HERE, also deliberate. HNSW and IVFFlat both need tuning
-- against real vector counts, and an IVFFlat index built on an empty table is
-- worse than none — its lists are chosen from the data present when it is
-- built. Exact search is correct at any size and fast at the sizes measured.
-- The index arrives with T-0190's backfill, on populated tables, with its
-- parameters and build time recorded there.
--
-- `content_hash` is what makes re-embedding skippable and staleness visible: it
-- is the hash of the exact text that was embedded, so a writer can tell whether
-- the text changed without keeping a copy of it, and an operator can count how
-- far behind the vectors are.
--
-- THE TYPE IS SCHEMA-QUALIFIED as `public.vector`, and has to be. Tenant
-- migrations run with `search_path` pinned to the tenant schema ALONE
-- (`kairos_db::tenant`, `SET LOCAL search_path TO "org_<slug>"`), which is what
-- keeps unqualified names from ever resolving to a public table by accident.
-- `gen_random_uuid()` works unqualified only because it lives in `pg_catalog`,
-- which is always searched; `vector` is an extension type in `public` and is
-- not. Writing it bare fails with `type "vector" does not exist`, which is
-- exactly how this was found.
--
-- Guarded for re-runnability, like every tenant migration.

CREATE TABLE IF NOT EXISTS item_embeddings (
    -- One row per item: the composed "what is this about" vector (rule 4).
    item_id         UUID PRIMARY KEY,
    entity_type     TEXT NOT NULL CHECK (entity_type IN (
                        'strategy', 'initiative', 'task', 'document', 'adr'
                    )),
    -- The composed primary vector. Width is per-row, see `dimension`.
    embedding       public.vector NOT NULL,
    -- Which model produced it, so a configuration change is detectable rather
    -- than a silent comparison across two vector spaces.
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    dimension       INTEGER NOT NULL CHECK (dimension > 0),
    -- SHA-256 of the exact composed text that was embedded.
    content_hash    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_item_embeddings_type
    ON item_embeddings (entity_type);
-- "Which items are stale or embedded by a model we no longer use?" is the
-- question an operator asks when retrieval looks thin, and the question the
-- backfill asks on every run.
CREATE INDEX IF NOT EXISTS idx_item_embeddings_model
    ON item_embeddings (provider, model);

CREATE TABLE IF NOT EXISTS item_chunks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id         UUID NOT NULL,
    entity_type     TEXT NOT NULL CHECK (entity_type IN (
                        'strategy', 'initiative', 'task', 'document', 'adr'
                    )),
    -- Position within the item, 0-based, contiguous.
    ordinal         INTEGER NOT NULL CHECK (ordinal >= 0),
    -- The LITERAL heading text this chunk sat under, or NULL for a
    -- sliding-window chunk from text with no headings (rule 3). Nothing may
    -- key on its value: measured across 4,927 documents, 85% of heading
    -- strings occur exactly once, and the tenth most common heading is a
    -- template marker nobody deleted. It is here so a citation can echo where
    -- the match was, not so code can recognise a section.
    heading         TEXT,
    -- Character range within the item's content, so a citation points at a
    -- location the reader still has.
    char_start      INTEGER NOT NULL CHECK (char_start >= 0),
    char_end        INTEGER NOT NULL,
    chunk_text      TEXT NOT NULL,
    embedding       public.vector NOT NULL,
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    dimension       INTEGER NOT NULL CHECK (dimension > 0),
    -- SHA-256 of `chunk_text`: the per-chunk skip check that makes an append
    -- re-embed one section rather than a whole document.
    content_hash    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (char_end >= char_start),
    UNIQUE (item_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_item_chunks_item
    ON item_chunks (item_id);
CREATE INDEX IF NOT EXISTS idx_item_chunks_model
    ON item_chunks (provider, model);
