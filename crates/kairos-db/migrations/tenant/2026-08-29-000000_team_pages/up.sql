-- KAIROS-T-0082 (KAIROS-I-0007): team pages, page history, and one-way
-- announcements — the team landing-page content store. A DEDICATED
-- construct, deliberately NOT the documents table: team pages authorize
-- by team membership (tenant-wide reads), documents by their parent's
-- board (T-0018/A-0006). Guarded for re-runnability.

CREATE TABLE IF NOT EXISTS team_pages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id         UUID NOT NULL REFERENCES teams(id),
    parent_id       UUID REFERENCES team_pages(id),
    kind            TEXT NOT NULL CHECK (kind IN ('folder', 'page')),
    slug            TEXT NOT NULL,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    position        INTEGER NOT NULL DEFAULT 0,
    -- The Charter: content-editable, never renamed/moved/deleted.
    is_protected    BOOLEAN NOT NULL DEFAULT false,
    version         INTEGER NOT NULL DEFAULT 1,
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Slug uniqueness within (team, parent) INCLUDING the NULL-parent roots
-- (plain UNIQUE treats NULLs as distinct).
CREATE UNIQUE INDEX IF NOT EXISTS idx_team_pages_sibling_slug
    ON team_pages (team_id, COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), slug);
CREATE INDEX IF NOT EXISTS idx_team_pages_team
    ON team_pages (team_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_team_pages_parent
    ON team_pages (parent_id) WHERE deleted_at IS NULL;

-- Version history, mirroring item_history (A-0004 posture: complete from
-- birth — v1 baseline written at creation).
CREATE TABLE IF NOT EXISTS team_page_history (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id         UUID NOT NULL REFERENCES team_pages(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    edited_by       UUID NOT NULL,
    edited_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (page_id, version)
);

-- One-way announcements: append-only by convention (no updated_at, no
-- edit route; KAIROS-I-0007 "no comments" decision).
CREATE TABLE IF NOT EXISTS team_announcements (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id         UUID NOT NULL REFERENCES teams(id),
    body            TEXT NOT NULL,
    pinned          BOOLEAN NOT NULL DEFAULT false,
    created_by      UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_team_announcements_team
    ON team_announcements (team_id);

-- Backfill: every EXISTING live team gets the scaffold (new teams get it
-- transactionally at creation via kairos_db::team_pages::seed_team_scaffold —
-- the two MUST stay in step). created_by/updated_by use the zero UUID:
-- audit columns carry no FK (the item-tables precedent) and migrations
-- have no acting user. Guarded per (team, slug): re-runs and partially
-- scaffolded teams converge.
DO $$
DECLARE
    team RECORD;
    charter_id UUID;
    support_id UUID;
    docs_id UUID;
    sub RECORD;
    actor CONSTANT UUID := '00000000-0000-0000-0000-000000000000';
BEGIN
    FOR team IN SELECT id FROM teams WHERE deleted_at IS NULL LOOP
        -- Charter (protected page, root position 0) + v1 history baseline.
        IF NOT EXISTS (SELECT 1 FROM team_pages p WHERE p.team_id = team.id AND p.parent_id IS NULL AND p.slug = 'charter') THEN
            INSERT INTO team_pages (team_id, parent_id, kind, slug, title, content, position, is_protected, created_by, updated_by)
            VALUES (team.id, NULL, 'page', 'charter', 'Team Charter',
                    E'# Team Charter\n\n## Mission\n\n## Scope\n\n## Ways of Working\n\n## Success Measures\n',
                    0, true, actor, actor)
            RETURNING id INTO charter_id;
            INSERT INTO team_page_history (page_id, version, title, content, edited_by)
            SELECT charter_id, 1, p.title, p.content, actor FROM team_pages p WHERE p.id = charter_id;
        END IF;

        -- Support Processes folder + Overview page.
        SELECT p.id INTO support_id FROM team_pages p WHERE p.team_id = team.id AND p.parent_id IS NULL AND p.slug = 'support-processes';
        IF support_id IS NULL THEN
            INSERT INTO team_pages (team_id, parent_id, kind, slug, title, position, created_by, updated_by)
            VALUES (team.id, NULL, 'folder', 'support-processes', 'Support Processes', 1, actor, actor)
            RETURNING id INTO support_id;
        END IF;
        IF NOT EXISTS (SELECT 1 FROM team_pages p WHERE p.team_id = team.id AND p.parent_id = support_id AND p.slug = 'overview') THEN
            INSERT INTO team_pages (team_id, parent_id, kind, slug, title, content, position, created_by, updated_by)
            VALUES (team.id, support_id, 'page', 'overview', 'Overview',
                    E'# Support Processes\n\nHow to reach this team and how support work is handled.\n',
                    0, actor, actor);
            INSERT INTO team_page_history (page_id, version, title, content, edited_by)
            SELECT p.id, 1, p.title, p.content, actor FROM team_pages p
            WHERE p.team_id = team.id AND p.parent_id = support_id AND p.slug = 'overview';
        END IF;

        -- Documentation folder + the section subfolders.
        SELECT p.id INTO docs_id FROM team_pages p WHERE p.team_id = team.id AND p.parent_id IS NULL AND p.slug = 'documentation';
        IF docs_id IS NULL THEN
            INSERT INTO team_pages (team_id, parent_id, kind, slug, title, position, created_by, updated_by)
            VALUES (team.id, NULL, 'folder', 'documentation', 'Documentation', 2, actor, actor)
            RETURNING id INTO docs_id;
        END IF;
        FOR sub IN SELECT * FROM (VALUES
            ('planning', 'Planning', 0),
            ('tutorials', 'Tutorials', 1),
            ('how-to-guides', 'How-to Guides', 2),
            ('reference', 'Reference', 3),
            ('explanation', 'Explanation', 4),
            ('design-docs', 'Design Docs', 5)
        ) AS s(slug, title, position) LOOP
            IF NOT EXISTS (SELECT 1 FROM team_pages p WHERE p.team_id = team.id AND p.parent_id = docs_id AND p.slug = sub.slug) THEN
                INSERT INTO team_pages (team_id, parent_id, kind, slug, title, position, created_by, updated_by)
                VALUES (team.id, docs_id, 'folder', sub.slug, sub.title, sub.position, actor, actor);
            END IF;
        END LOOP;
    END LOOP;
END $$;
