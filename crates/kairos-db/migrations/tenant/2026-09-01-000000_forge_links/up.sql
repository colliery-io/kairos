-- KAIROS-T-0097 (KAIROS-I-0009): git forge connections and the branch /
-- pull-request links they ingest.
--
-- `item_links` is DERIVED data mirrored from GitHub/GitLab — Kairos is not
-- the author. So, deliberately: no `version` column, no history table, no
-- A-0004 optimistic concurrency. The forge's own `forge_updated_at` is the
-- ordering authority (webhooks retry and arrive out of order; see the
-- guarded upsert in KAIROS-T-0099).
--
-- No webhook secret is stored: it is derived per connection from the
-- deployment signing key (HMAC over the connection id), so rotation is a
-- new connection id and nothing sensitive lives at rest. Guarded for
-- re-runnability.

CREATE TABLE IF NOT EXISTS forge_connections (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    forge           TEXT NOT NULL CHECK (forge IN ('github', 'gitlab')),
    -- `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    repo_full_name  TEXT NOT NULL,
    repo_url        TEXT NOT NULL,
    -- Optional attribution: repo-level activity can roll up to a team even
    -- when an individual work item carries no team (KAIROS-T-0101).
    team_id         UUID REFERENCES teams(id),
    created_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One live connection per repo per forge (soft-deleted rows may repeat, so
-- the uniqueness is partial).
CREATE UNIQUE INDEX IF NOT EXISTS idx_forge_connections_repo
    ON forge_connections (forge, repo_full_name)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_forge_connections_team
    ON forge_connections (team_id)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS item_links (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The linked work item. NO FK: item ids span the five entity tables in
    -- one shared UUID space (the `item_metadata` convention).
    item_id           UUID NOT NULL,
    connection_id     UUID NOT NULL REFERENCES forge_connections(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL CHECK (kind IN ('branch', 'pull_request')),
    -- PR/MR number, or the branch ref. Unique per (connection, kind).
    external_id       TEXT NOT NULL,
    title             TEXT NOT NULL,
    url               TEXT NOT NULL,
    state             TEXT NOT NULL CHECK (state IN ('open', 'merged', 'closed', 'draft')),
    author            TEXT NOT NULL DEFAULT '',
    -- The forge's own last-update timestamp: the ordering guard that keeps
    -- a redelivered "opened" from regressing a merged PR.
    forge_updated_at  TIMESTAMPTZ NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The upsert key: one row per (repo, kind, PR-or-branch) per item.
CREATE UNIQUE INDEX IF NOT EXISTS idx_item_links_identity
    ON item_links (connection_id, kind, external_id, item_id);
-- The item Development panel's lookup (KAIROS-T-0100).
CREATE INDEX IF NOT EXISTS idx_item_links_item
    ON item_links (item_id);
-- The team in-flight rollup filters by state (KAIROS-T-0101).
CREATE INDEX IF NOT EXISTS idx_item_links_state
    ON item_links (state);
