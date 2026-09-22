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
        -- A soft-deleted, team-less connection in a tenant with no live team
        -- has no owner to borrow; unrecoverable here, so say so instead of
        -- tripping NOT NULL half-way through (KAIROS-T-0113).
        IF EXISTS (SELECT 1 FROM forge_connections WHERE deleted_at IS NOT NULL AND team_id IS NULL)
           AND NOT EXISTS (SELECT 1 FROM teams WHERE deleted_at IS NULL) THEN
            RAISE EXCEPTION
                'KAIROS-T-0113: soft-deleted forge connections without a team need at least one live team to migrate; hard-delete them or create a team: %',
                (SELECT string_agg(id::text, ', ')
                   FROM forge_connections
                  WHERE deleted_at IS NOT NULL AND team_id IS NULL);
        END IF;

        ALTER TABLE forge_connections
            ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES repositories(id);

        -- One repository per DISTINCT (forge, full name), live ones first.
        -- The slug derives from the full name exactly as
        -- kairos_core::repositories::slug_from_full_name does (lowercase,
        -- runs of non-alphanumerics -> one hyphen, edge hyphens trimmed,
        -- 63 bytes). The OLD uniqueness was (forge, full name), so two
        -- connections can collide on the derived slug (`acme/foo` on GitHub
        -- and GitLab; `Acme/Foo` and `acme/foo`): collisions get `-<forge>`
        -- then `-2`, `-3`, ... appended, cut so the result stays <= 63
        -- (KAIROS-T-0113). Temporary table so the loop is set-based.
        CREATE TEMP TABLE backfill_repos ON COMMIT DROP AS
        SELECT DISTINCT ON (fc.forge, fc.repo_full_name)
               fc.forge,
               fc.repo_full_name,
               fc.repo_url,
               COALESCE(fc.team_id,
                        (SELECT id FROM teams WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1)) AS team_id,
               fc.created_by,
               -- live if ANY connection for the pair is live
               bool_or(fc.deleted_at IS NULL) OVER (PARTITION BY fc.forge, fc.repo_full_name) AS live,
               rtrim(left(btrim(regexp_replace(lower(fc.repo_full_name), '[^a-z0-9]+', '-', 'g'), '-'), 63), '-') AS base_slug,
               NULL::text AS slug
          FROM forge_connections fc
         ORDER BY fc.forge, fc.repo_full_name, fc.deleted_at NULLS FIRST;

        -- First pass: unique base slugs keep them.
        UPDATE backfill_repos b SET slug = base_slug
         WHERE (SELECT count(*) FROM backfill_repos x WHERE x.base_slug = b.base_slug) = 1;
        -- Second pass: colliding base slugs get their forge appended.
        UPDATE backfill_repos b SET slug = left(base_slug, 63 - length(forge) - 1) || '-' || forge
         WHERE slug IS NULL
           AND (SELECT count(*) FROM backfill_repos x WHERE x.base_slug = b.base_slug AND x.forge = b.forge) = 1;
        -- Third pass: still colliding (same forge, case-only difference) get -2, -3, ...
        UPDATE backfill_repos b
           SET slug = left(base_slug, 63 - length(n.rn::text) - 1) || '-' || n.rn
          FROM (SELECT forge, repo_full_name,
                       row_number() OVER (PARTITION BY base_slug ORDER BY repo_full_name) + 1 AS rn
                  FROM backfill_repos WHERE slug IS NULL) n
         WHERE b.slug IS NULL AND b.forge = n.forge AND b.repo_full_name = n.repo_full_name;
        IF EXISTS (SELECT 1 FROM backfill_repos WHERE slug IS NULL OR slug = '' OR slug !~ '^[a-z0-9][a-z0-9-]{1,62}$') THEN
            RAISE EXCEPTION
                'KAIROS-T-0113: could not derive a valid repository slug for: %',
                (SELECT string_agg(forge || ':' || repo_full_name, ', ') FROM backfill_repos
                  WHERE slug IS NULL OR slug = '' OR slug !~ '^[a-z0-9][a-z0-9-]{1,62}$');
        END IF;

        INSERT INTO repositories
            (slug, forge, repo_full_name, repo_url, team_id, created_by, updated_by, deleted_at)
        SELECT slug, forge, repo_full_name, repo_url, team_id, created_by, created_by,
               CASE WHEN live THEN NULL ELSE now() END
          FROM backfill_repos;

        UPDATE forge_connections fc
           SET repository_id = r.id
          FROM repositories r
         WHERE fc.forge = r.forge
           AND fc.repo_full_name = r.repo_full_name;

        ALTER TABLE forge_connections ALTER COLUMN repository_id SET NOT NULL;
        DROP TABLE backfill_repos;

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
