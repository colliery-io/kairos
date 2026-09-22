-- Reverse of KAIROS-T-0103: restore the repo identity columns on
-- forge_connections from the repository join, then drop the new surface.

DROP INDEX IF EXISTS idx_tasks_repository;
ALTER TABLE tasks DROP COLUMN IF EXISTS repository_id;

DROP INDEX IF EXISTS idx_forge_connections_repository;

ALTER TABLE forge_connections
    ADD COLUMN IF NOT EXISTS repo_full_name TEXT,
    ADD COLUMN IF NOT EXISTS repo_url TEXT,
    ADD COLUMN IF NOT EXISTS team_id UUID REFERENCES teams(id);

UPDATE forge_connections fc
   SET repo_full_name = r.repo_full_name,
       repo_url = r.repo_url,
       team_id = r.team_id
  FROM repositories r
 WHERE r.id = fc.repository_id;

ALTER TABLE forge_connections
    ALTER COLUMN repo_full_name SET NOT NULL,
    ALTER COLUMN repo_url SET NOT NULL;

ALTER TABLE forge_connections DROP COLUMN IF EXISTS repository_id;

CREATE UNIQUE INDEX IF NOT EXISTS idx_forge_connections_repo
    ON forge_connections (forge, repo_full_name)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_forge_connections_team
    ON forge_connections (team_id)
    WHERE deleted_at IS NULL;

DROP TABLE IF EXISTS repositories;
