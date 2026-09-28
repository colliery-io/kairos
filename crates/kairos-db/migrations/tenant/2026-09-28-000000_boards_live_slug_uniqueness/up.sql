-- COLLIERY-T-0255: two live boards cannot have the same slug.
--
-- A board is addressed by its slug: `/boards/{slug}` in the GUI, `board` in
-- MCP, `--board` in the CLI. The table `boards` had no unique index on
-- `slug`, and no code did the check, so a tenant could have two live boards
-- with one slug. A reference by slug then gave one of the two.
--
-- The rule is that of `teams.slug` (KAIROS-T-0184): unique among the rows
-- that the tenant has, which are the LIVE rows. A deleted board does not
-- keep its slug.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- A tenant with no two live boards of one slug: nothing. The migration adds
-- the index only.
--
-- A tenant with two or more live boards of one slug: the index cannot be
-- made, and the migration, the start of the server and `migrate-tenants`
-- fail. So this migration first gives each such board a slug of its own:
--
--   - The OLDEST board keeps the slug (`created_at`, then `id`). The links
--     that people have go to the board that had the slug first.
--   - Each later board gets `<slug>-2`, `<slug>-3`, ... in the order of its
--     age. A number that a live board has is not used: with the live boards
--     `a`, `a`, `a-2`, the second `a` gets `a-3`.
--   - `updated_at` of a board with a new slug is the time of the migration.
--   - A NOTICE in the log of the migration names each board with a new slug.
--
-- Nothing else changes: the id, the name, the columns, the items and the
-- team of the board stay. A deleted board is not read and not changed.
--
-- Re-runnable: the second run finds no two live boards of one slug, and the
-- index is there.

DO $$
DECLARE
    later     RECORD;
    number    INTEGER;
    candidate TEXT;
BEGIN
    FOR later IN
        SELECT ranked.id, ranked.slug
          FROM (
                SELECT b.id,
                       b.slug,
                       row_number() OVER (
                           PARTITION BY b.slug
                           ORDER BY b.created_at, b.id
                       ) AS age_rank
                  FROM boards b
                 WHERE b.deleted_at IS NULL
               ) ranked
         WHERE ranked.age_rank > 1
         ORDER BY ranked.slug, ranked.age_rank
    LOOP
        number := 2;
        LOOP
            candidate := later.slug || '-' || number;
            EXIT WHEN NOT EXISTS (
                SELECT 1
                  FROM boards b
                 WHERE b.slug = candidate
                   AND b.deleted_at IS NULL
            );
            number := number + 1;
        END LOOP;
        UPDATE boards
           SET slug = candidate,
               updated_at = now()
         WHERE id = later.id;
        RAISE NOTICE 'COLLIERY-T-0255: the board % had the slug "%" of an older board. Its slug is now "%".',
            later.id, later.slug, candidate;
    END LOOP;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS boards_live_slug_key
    ON boards (slug)
    WHERE deleted_at IS NULL;
