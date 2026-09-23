-- Tenant schema (KAIROS-S-0004): one copy per tenant, in `org_{slug}`.
-- Transcribed verbatim from the S-0004 specification — the hand-written DDL
-- is the source of truth (KAIROS-A-0009); do not redesign here.
--
-- All names are deliberately UNQUALIFIED: this tree is executed with
-- search_path pinned to the target tenant schema (kairos_db::tenant), so the
-- same migration provisions/upgrades every `org_{slug}` schema and diesel's
-- `__diesel_schema_migrations` bookkeeping table lands inside that schema.
--
-- Transcription note (recorded in KAIROS-T-0008): S-0004 lists the entity
-- tables before the template/metadata tables, but `documents.template_id`
-- references `templates(id)`, so the Template & Metadata section is created
-- first. The DDL text itself is unchanged.

-- ---------------------------------------------------------------------------
-- Organizational Tables
-- ---------------------------------------------------------------------------

-- Teams (delivery teams)
CREATE TABLE teams (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL UNIQUE,
    team_type       TEXT NOT NULL CHECK (team_type IN (
                        'stream_aligned', 'platform', 'enabling', 'complicated_subsystem'
                    )),
    deleted_at      TIMESTAMPTZ,  -- soft delete
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Team membership (which users belong to which teams)
CREATE TABLE team_members (
    team_id         UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL,  -- references public.users
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, user_id)
);

-- Delivery streams (cross-repo work flows)
CREATE TABLE delivery_streams (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL UNIQUE,
    description     TEXT,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Team-to-delivery-stream membership
CREATE TABLE team_delivery_streams (
    team_id             UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    delivery_stream_id  UUID NOT NULL REFERENCES delivery_streams(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, delivery_stream_id)
);

-- ---------------------------------------------------------------------------
-- Board Tables (KAIROS-A-0002)
-- ---------------------------------------------------------------------------

-- Boards
CREATE TABLE boards (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL,
    board_level     TEXT NOT NULL CHECK (board_level IN (
                        'strategy', 'initiative', 'delivery', 'adr'
                    )),
    team_id         UUID REFERENCES teams(id),  -- nullable; set for delivery boards
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Board columns (configurable per board)
CREATE TABLE board_columns (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    board_id        UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    position        INTEGER NOT NULL,  -- display ordering, 0-indexed
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (board_id, position),
    UNIQUE (board_id, name)
);

-- Allowed transitions between columns
CREATE TABLE board_transitions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    board_id        UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    from_column_id  UUID NOT NULL REFERENCES board_columns(id) ON DELETE CASCADE,
    to_column_id    UUID NOT NULL REFERENCES board_columns(id) ON DELETE CASCADE,
    CHECK (from_column_id != to_column_id),
    UNIQUE (board_id, from_column_id, to_column_id)
);

-- ---------------------------------------------------------------------------
-- Template & Metadata Tables (KAIROS-A-0003)
-- (created before the entity tables: documents.template_id references
-- templates(id) — see transcription note at the top of this file)
-- ---------------------------------------------------------------------------

-- Templates (tenant-level, seeded from system_templates)
CREATE TABLE templates (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT NOT NULL,
    slug              TEXT NOT NULL UNIQUE,
    content           TEXT NOT NULL,
    is_system_default BOOLEAN NOT NULL DEFAULT false,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Metadata field definitions (tenant-level, seeded from system defaults)
CREATE TABLE metadata_definitions (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT NOT NULL,
    slug              TEXT NOT NULL UNIQUE,
    field_type        TEXT NOT NULL CHECK (field_type IN ('string', 'enum', 'date')),
    is_system_default BOOLEAN NOT NULL DEFAULT false,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Enum options for metadata definitions
CREATE TABLE metadata_enum_options (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    metadata_definition_id UUID NOT NULL REFERENCES metadata_definitions(id) ON DELETE CASCADE,
    value                  TEXT NOT NULL,
    position               INTEGER NOT NULL DEFAULT 0,
    UNIQUE (metadata_definition_id, value)
);

-- Template-to-metadata associations (which fields a template carries)
CREATE TABLE template_metadata (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    template_id            UUID NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    metadata_definition_id UUID NOT NULL REFERENCES metadata_definitions(id) ON DELETE CASCADE,
    default_value          TEXT,
    required               BOOLEAN NOT NULL DEFAULT false,
    UNIQUE (template_id, metadata_definition_id)
);

-- Metadata values on entities
CREATE TABLE item_metadata (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id                UUID NOT NULL,
    metadata_definition_id UUID NOT NULL REFERENCES metadata_definitions(id) ON DELETE CASCADE,
    value                  TEXT NOT NULL,
    UNIQUE (item_id, metadata_definition_id)
);

CREATE INDEX idx_item_metadata_item ON item_metadata(item_id);
CREATE INDEX idx_item_metadata_definition ON item_metadata(metadata_definition_id);

-- ---------------------------------------------------------------------------
-- Entity Tables (KAIROS-A-0001, A-0004)
-- All entity tables share a UUID space. `version` supports optimistic
-- concurrency. `deleted_at` enables soft delete.
-- ---------------------------------------------------------------------------

-- Strategies (Flight Level 3)
CREATE TABLE strategies (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    board_id        UUID NOT NULL REFERENCES boards(id),
    column_id       UUID NOT NULL REFERENCES board_columns(id),
    hypothesis      TEXT,
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,  -- references public.users
    updated_by      UUID NOT NULL,  -- references public.users
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_strategies_board ON strategies(board_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_strategies_column ON strategies(board_id, column_id) WHERE deleted_at IS NULL;

-- Initiatives (Flight Level 2)
CREATE TABLE initiatives (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    board_id        UUID NOT NULL REFERENCES boards(id),
    column_id       UUID NOT NULL REFERENCES board_columns(id),
    complexity      TEXT CHECK (complexity IN ('xs', 's', 'm', 'l', 'xl')),
    is_bucket       BOOLEAN NOT NULL DEFAULT false,
    bucket_type     TEXT CHECK (bucket_type IN ('tech_debt', 'bug', 'ad_hoc')),
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((is_bucket = true AND bucket_type IS NOT NULL) OR (is_bucket = false AND bucket_type IS NULL))
);

CREATE INDEX idx_initiatives_board ON initiatives(board_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_initiatives_column ON initiatives(board_id, column_id) WHERE deleted_at IS NULL;

-- Tasks (Flight Level 1)
CREATE TABLE tasks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    board_id        UUID NOT NULL REFERENCES boards(id),
    column_id       UUID NOT NULL REFERENCES board_columns(id),
    task_type       TEXT NOT NULL CHECK (task_type IN ('task', 'bug', 'tech_debt')),
    team_id         UUID REFERENCES teams(id),
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tasks_board ON tasks(board_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_column ON tasks(board_id, column_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_team ON tasks(team_id) WHERE deleted_at IS NULL;

-- Supporting documents (children of any entity)
CREATE TABLE documents (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    template_id     UUID REFERENCES templates(id) ON DELETE SET NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Architecture Decision Records (children of any entity)
CREATE TABLE adrs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    board_id        UUID REFERENCES boards(id),
    column_id       UUID REFERENCES board_columns(id),
    decision_maker  TEXT,
    decision_date   DATE,
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((board_id IS NULL AND column_id IS NULL) OR (board_id IS NOT NULL AND column_id IS NOT NULL))
);

-- ---------------------------------------------------------------------------
-- Relationship Graph (KAIROS-A-0001)
-- ---------------------------------------------------------------------------

-- All entity relationships
CREATE TABLE item_relationships (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID NOT NULL,  -- the "from" entity
    target_id       UUID NOT NULL,  -- the "to" entity
    relationship    TEXT NOT NULL CHECK (relationship IN (
                        'parent', 'supports', 'informs', 'supersedes', 'blocks'
                    )),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (source_id != target_id),
    UNIQUE (source_id, target_id, relationship)
);

-- Relationship semantics:
--   parent:     source is parent of target (strategy -> initiative, initiative -> task)
--   supports:   target supports source (document supports initiative, ADR supports strategy)
--   informs:    source informs target (company vision informs strategy board)
--   supersedes: source supersedes target (new ADR supersedes old ADR)
--   blocks:     source blocks target (task A blocks task B)

CREATE INDEX idx_item_relationships_source ON item_relationships(source_id, relationship);
CREATE INDEX idx_item_relationships_target ON item_relationships(target_id, relationship);

-- ---------------------------------------------------------------------------
-- Content History (KAIROS-A-0004)
-- ---------------------------------------------------------------------------

-- Append-only content version history
CREATE TABLE item_history (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id         UUID NOT NULL,
    version         INTEGER NOT NULL,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    edited_by       UUID NOT NULL,  -- references public.users
    edited_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (item_id, version)
);

CREATE INDEX idx_item_history_item ON item_history(item_id);

-- ---------------------------------------------------------------------------
-- Activity Log (audit trail for non-content actions)
-- ---------------------------------------------------------------------------

-- Captures state transitions, relationship changes, capability grants/revocations, deletions
CREATE TABLE activity_log (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id        UUID NOT NULL,       -- references public.users
    action          TEXT NOT NULL,        -- 'transition', 'board_move', 'create', 'delete', 'relationship_add',
                                         -- 'relationship_remove', 'capability_grant', 'capability_revoke'
    entity_id       UUID,                -- the item acted on (nullable for relationship actions)
    entity_type     TEXT,                -- 'strategy', 'initiative', 'task', 'document', 'adr'
    details         TEXT NOT NULL,       -- structured context, e.g., "column:Draft->Active" or
                                         -- "relationship:parent:I-0001->T-0005"
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_activity_log_entity ON activity_log(entity_id);
CREATE INDEX idx_activity_log_actor ON activity_log(actor_id);
CREATE INDEX idx_activity_log_time ON activity_log(occurred_at);

-- ---------------------------------------------------------------------------
-- Authorization Tables (KAIROS-A-0006)
-- ---------------------------------------------------------------------------

-- Board-scoped capability grants (whitelist)
CREATE TABLE board_member_capabilities (
    board_id        UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL,  -- references public.users
    capability      TEXT NOT NULL,  -- specific capability or glob ('*', 'manage_*')
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    granted_by      UUID NOT NULL,  -- references public.users
    UNIQUE (board_id, user_id, capability)
);

CREATE INDEX idx_board_member_cap_board_user ON board_member_capabilities(board_id, user_id);

-- ---------------------------------------------------------------------------
-- Short Code Sequences
-- ---------------------------------------------------------------------------

-- PostgreSQL sequences for atomic short code generation per entity type
CREATE SEQUENCE seq_strategy_code;
CREATE SEQUENCE seq_initiative_code;
CREATE SEQUENCE seq_task_code;
CREATE SEQUENCE seq_document_code;
CREATE SEQUENCE seq_adr_code;

-- Short code format: {PREFIX}-{TYPE_LETTER}-{PADDED_NUMBER}
-- e.g., S-0001, I-0042, T-0123, D-0007, A-0003
-- PREFIX is not stored in the sequence; it is application config per tenant.

-- ---------------------------------------------------------------------------
-- Full-Text Search View
-- ---------------------------------------------------------------------------

-- Excludes soft-deleted items
CREATE VIEW searchable_items AS
    SELECT id, short_code, 'strategy' AS entity_type, title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, '')) AS tsv
    FROM strategies WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'initiative', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM initiatives WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'task', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM tasks WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'document', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM documents WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'adr', title, content,
           to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))
    FROM adrs WHERE deleted_at IS NULL;

-- GIN indexes on individual tables back the view's queries
CREATE INDEX idx_strategies_tsv ON strategies USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_initiatives_tsv ON initiatives USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_tsv ON tasks USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_documents_tsv ON documents USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;
CREATE INDEX idx_adrs_tsv ON adrs USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(content, ''))) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Entity Type Resolution View
-- Used by graph query hydration and anywhere a UUID needs to be resolved to
-- its entity type.
-- ---------------------------------------------------------------------------

CREATE VIEW entity_directory AS
    SELECT id, short_code, 'strategy' AS entity_type, title, board_id FROM strategies WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'initiative', title, board_id FROM initiatives WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'task', title, board_id FROM tasks WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'document', title, NULL FROM documents WHERE deleted_at IS NULL
    UNION ALL
    SELECT id, short_code, 'adr', title, board_id FROM adrs WHERE deleted_at IS NULL;
