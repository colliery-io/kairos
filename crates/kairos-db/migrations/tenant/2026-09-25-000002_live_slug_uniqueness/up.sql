-- KAIROS-T-0184 item 2: a soft-deleted row must not burn its slug for ever.
--
-- The reported case was SCIM. An IdP that deletes and re-adds a group — a
-- routine reorganisation — got `409 uniqueness` permanently, because
-- `teams.slug` was UNIQUE with no `deleted_at` predicate while DELETE only
-- soft-deletes. The team name became unrecoverable.
--
-- This is the same landmine KAIROS-T-0161 hit on `board_columns`, and the fix is
-- the same: a partial unique index, which says what was always meant — unique
-- among the rows the tenant actually has.
--
-- The ticket asked for every UNIQUE on a soft-deletable table to be audited in
-- one pass instead of one ticket at a time. That audit found TWO MORE of these
-- beyond the reported one, both soft-deleted by handlers that already exist:
--
--   teams.slug              -- reported (SCIM group delete, and DELETE /api/teams)
--   delivery_streams.slug   -- found here (DELETE /api/streams soft-deletes)
--   team_pages sibling slug -- found here (team_pages::soft_delete_page)
--
-- Deliberately NOT changed: every `short_code` UNIQUE (strategies, initiatives,
-- tasks, documents, adrs) and every `*_pkey`. A short code must stay unique
-- across live AND archived items for ever — KAIROS-A-0020 exists so an archived
-- ACME-T-0001 remains readable and unambiguous, and re-issuing that code to a new
-- task would break exactly that. Those constraints are correct as they are, and
-- a blanket "add the predicate everywhere" pass would have broken them.

ALTER TABLE teams DROP CONSTRAINT IF EXISTS teams_slug_key;
CREATE UNIQUE INDEX IF NOT EXISTS teams_live_slug_key
    ON teams (slug)
    WHERE deleted_at IS NULL;

ALTER TABLE delivery_streams DROP CONSTRAINT IF EXISTS delivery_streams_slug_key;
CREATE UNIQUE INDEX IF NOT EXISTS delivery_streams_live_slug_key
    ON delivery_streams (slug)
    WHERE deleted_at IS NULL;

-- The sibling-slug index keeps its COALESCE expression (a root page has a NULL
-- parent_id, and NULLs do not collide in a plain unique index), and gains the
-- liveness predicate.
DROP INDEX IF EXISTS idx_team_pages_sibling_slug;
CREATE UNIQUE INDEX IF NOT EXISTS idx_team_pages_live_sibling_slug
    ON team_pages (
        team_id,
        COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid),
        slug
    )
    WHERE deleted_at IS NULL;
