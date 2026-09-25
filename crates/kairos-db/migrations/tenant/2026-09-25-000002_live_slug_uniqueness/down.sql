-- Restores the plain uniqueness, and with it the defect: a soft-deleted team,
-- stream or page holds its slug against re-creation for ever.
--
-- It can fail, legitimately. If a slug has been reused since the up migration
-- ran — which is the whole point of that migration — then live and soft-deleted
-- rows now share it, and the plain constraint cannot be rebuilt without deleting
-- data. That is a real conflict, not a bug in this file, and resolving it is a
-- decision for whoever is rolling back.
DROP INDEX IF EXISTS teams_live_slug_key;
ALTER TABLE teams ADD CONSTRAINT teams_slug_key UNIQUE (slug);

DROP INDEX IF EXISTS delivery_streams_live_slug_key;
ALTER TABLE delivery_streams ADD CONSTRAINT delivery_streams_slug_key UNIQUE (slug);

DROP INDEX IF EXISTS idx_team_pages_live_sibling_slug;
CREATE UNIQUE INDEX IF NOT EXISTS idx_team_pages_sibling_slug
    ON team_pages (
        team_id,
        COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid),
        slug
    );
