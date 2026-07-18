-- Public schema (KAIROS-S-0004): shared across all tenants.
-- Transcribed verbatim from the S-0004 specification — the hand-written DDL
-- is the source of truth (KAIROS-A-0009); do not redesign here.

-- Organizations (tenants)
CREATE TABLE organizations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z][a-z0-9_-]{1,62}$'),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Users (from OIDC/Keycloak)
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id     TEXT NOT NULL UNIQUE,  -- OIDC subject identifier
    email           TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Organization membership
CREATE TABLE organization_members (
    organization_id UUID NOT NULL REFERENCES organizations(id),
    user_id         UUID NOT NULL REFERENCES users(id),
    role            TEXT NOT NULL DEFAULT 'member',  -- 'admin' | 'member'
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id)
);

-- System-level default templates (copied into tenant schemas on provisioning)
CREATE TABLE system_templates (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL UNIQUE,
    content         TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- System-level default metadata definitions (copied into tenant schemas on provisioning)
CREATE TABLE system_metadata_definitions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            TEXT NOT NULL UNIQUE,
    field_type      TEXT NOT NULL CHECK (field_type IN ('string', 'enum', 'date')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Enum options for system metadata definitions
CREATE TABLE system_metadata_enum_options (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    metadata_definition_id UUID NOT NULL REFERENCES system_metadata_definitions(id) ON DELETE CASCADE,
    value                  TEXT NOT NULL,
    position               INTEGER NOT NULL DEFAULT 0,
    UNIQUE (metadata_definition_id, value)
);

-- System-level template-to-metadata associations
CREATE TABLE system_template_metadata (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    template_id            UUID NOT NULL REFERENCES system_templates(id) ON DELETE CASCADE,
    metadata_definition_id UUID NOT NULL REFERENCES system_metadata_definitions(id) ON DELETE CASCADE,
    default_value          TEXT,
    required               BOOLEAN NOT NULL DEFAULT false,
    UNIQUE (template_id, metadata_definition_id)
);

-- Default board configurations (used when provisioning new boards)
CREATE TABLE system_board_defaults (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    board_level     TEXT NOT NULL CHECK (board_level IN ('strategy', 'initiative', 'delivery', 'adr')),
    columns         TEXT NOT NULL,  -- ordered column names, newline-separated
    transitions     TEXT NOT NULL,  -- "from -> to" pairs, newline-separated
    UNIQUE (board_level)
);
