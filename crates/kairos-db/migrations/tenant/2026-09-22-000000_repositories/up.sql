-- KAIROS-T-0103 (KAIROS-I-0010, decision KAIROS-A-0019): repositories as a
-- first-class, team-owned entity. Boards and streams plan the work; the
-- repository is what a ticket is issued against and executed in.
--
-- `forge_connections` (KAIROS-T-0097) was the only repo-shaped table and
-- existed purely so webhooks could mirror PRs into `item_links`. It becomes
-- the webhook attribute OF a repository: it keeps `forge` (the webhook
-- dialect, also in the delivery URL) and gains `repository_id`; the repo
-- identity columns and the optional team attribution move to `repositories`,
-- where the owner is REQUIRED (exactly one owning team per repo).
--
-- Guarded for re-runnability, like every tenant migration.

CREATE TABLE IF NOT EXISTS repositories (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Tenant-unique, URL-safe handle: what agents and the CLI address.
    slug            TEXT NOT NULL,
    forge           TEXT NOT NULL CHECK (forge IN ('github', 'gitlab', 'other')),
    -- `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    repo_full_name  TEXT NOT NULL,
    repo_url        TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    -- Exactly one owning team (A-0019): repo -> team -> delivery board is
    -- how a ticket filed against a repo is routed.
    team_id         UUID NOT NULL REFERENCES teams(id),
    -- Short "how to work here" blurb agents read before starting.
    description     TEXT NOT NULL DEFAULT '',
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_slug
    ON repositories (slug)
    WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_forge_name
    ON repositories (forge, repo_full_name)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_repositories_team
    ON repositories (team_id)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- forge_connections: re-key on repository_id, backfill, drop moved columns.
-- The whole block is skipped on re-run (the columns are already gone).
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'forge_connections'
          AND column_name = 'repo_full_name'
    ) THEN
        -- A live connection with no team cannot become a repository (the
        -- owner is required). Fail loudly and name them: the operator
        -- attributes them via PATCH /api/forge-connections/{id} and re-runs.
        IF EXISTS (
            SELECT 1 FROM forge_connections
            WHERE deleted_at IS NULL AND team_id IS NULL
        ) THEN
            RAISE EXCEPTION
                'KAIROS-T-0103: forge connections without a team cannot be migrated to repositories; set a team on: %',
                (SELECT string_agg(id::text, ', ')
                   FROM forge_connections
                  WHERE deleted_at IS NULL AND team_id IS NULL);
        END IF;

        ALTER TABLE forge_connections
            ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES repositories(id);

        -- One live repository per live connection. The slug is derived from
        -- the full name (`acme/payments-api` -> `acme-payments-api`).
        INSERT INTO repositories
            (slug, forge, repo_full_name, repo_url, team_id, created_by, updated_by)
        SELECT lower(regexp_replace(repo_full_name, '[^A-Za-z0-9]+', '-', 'g')),
               forge, repo_full_name, repo_url, team_id, created_by, created_by
          FROM forge_connections
         WHERE deleted_at IS NULL;

        UPDATE forge_connections fc
           SET repository_id = r.id
          FROM repositories r
         WHERE fc.deleted_at IS NULL
           AND r.deleted_at IS NULL
           AND fc.forge = r.forge
           AND fc.repo_full_name = r.repo_full_name;

        -- Soft-deleted connections point at a live twin when one exists,
        -- else at a soft-deleted repository of their own (team may be
        -- absent on these; fall back to any live team so NOT NULL holds —
        -- the row is dead either way).
        UPDATE forge_connections fc
           SET repository_id = r.id
          FROM repositories r
         WHERE fc.deleted_at IS NOT NULL
           AND fc.repository_id IS NULL
           AND r.deleted_at IS NULL
           AND fc.forge = r.forge
           AND fc.repo_full_name = r.repo_full_name;

        INSERT INTO repositories
            (slug, forge, repo_full_name, repo_url, team_id, created_by, updated_by, deleted_at)
        SELECT DISTINCT ON (fc.forge, fc.repo_full_name)
               lower(regexp_replace(fc.repo_full_name, '[^A-Za-z0-9]+', '-', 'g')),
               fc.forge, fc.repo_full_name, fc.repo_url,
               COALESCE(fc.team_id,
                        (SELECT id FROM teams WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1)),
               fc.created_by, fc.created_by, fc.deleted_at
          FROM forge_connections fc
         WHERE fc.deleted_at IS NOT NULL
           AND fc.repository_id IS NULL
         ORDER BY fc.forge, fc.repo_full_name, fc.deleted_at DESC;

        UPDATE forge_connections fc
           SET repository_id = r.id
          FROM repositories r
         WHERE fc.repository_id IS NULL
           AND r.deleted_at IS NOT NULL
           AND fc.forge = r.forge
           AND fc.repo_full_name = r.repo_full_name;

        ALTER TABLE forge_connections ALTER COLUMN repository_id SET NOT NULL;

        DROP INDEX IF EXISTS idx_forge_connections_repo;
        DROP INDEX IF EXISTS idx_forge_connections_team;
        ALTER TABLE forge_connections
            DROP COLUMN repo_full_name,
            DROP COLUMN repo_url,
            DROP COLUMN team_id;
    END IF;
END $$;

-- One live webhook connection per repository.
CREATE UNIQUE INDEX IF NOT EXISTS idx_forge_connections_repository
    ON forge_connections (repository_id)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- tasks: at most one repository per task (multi-repo work decomposes).
-- ---------------------------------------------------------------------------
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES repositories(id);
CREATE INDEX IF NOT EXISTS idx_tasks_repository
    ON tasks (repository_id)
    WHERE deleted_at IS NULL;
