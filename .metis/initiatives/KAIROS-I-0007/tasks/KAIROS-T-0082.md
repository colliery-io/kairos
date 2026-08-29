---
id: team-pages-schema-provisioning
level: task
title: "Team pages schema + provisioning: tables, models, seeded scaffold, backfill"
short_code: "KAIROS-T-0082"
created_at: 2026-08-29T02:58:52.246330+00:00
updated_at: 2026-08-29T11:39:33.301263+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Team pages schema + provisioning: tables, models, seeded scaffold, backfill

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. Design decisions (team_pages construct, NOT the Document entity; permission model; scaffold contents) are recorded there and in the archived KAIROS-T-0079.

## Objective

The data layer for team pages: three tenant tables, diesel models, the opinionated scaffold seeded transactionally at team creation, and a guarded backfill for existing teams.

## Implementation Notes

- **Tenant migration** (guarded/idempotent, fleet-migration hygiene like T-0077/T-0078/T-0080):
  - `team_pages`: id UUID PK default gen, team_id UUID NOT NULL REFERENCES teams(id), parent_id UUID NULL REFERENCES team_pages(id), kind TEXT NOT NULL CHECK (kind IN ('folder','page')), slug TEXT NOT NULL (same slug CHECK pattern as orgs/boards), title TEXT NOT NULL, content TEXT NOT NULL DEFAULT '', position INTEGER NOT NULL DEFAULT 0, is_protected BOOLEAN NOT NULL DEFAULT false, version INTEGER NOT NULL DEFAULT 1, created_by/updated_by UUID NOT NULL, deleted_at TIMESTAMPTZ, created_at/updated_at; UNIQUE(team_id, parent_id, slug) — NOTE Postgres treats NULL parent_id as distinct in plain UNIQUE constraints, so use a unique INDEX with COALESCE(parent_id, zero-uuid) or NULLS NOT DISTINCT.
  - `team_page_history`: mirror of item_history (page_id, version, title, content, edited_by, edited_at; UNIQUE(page_id, version)).
  - `team_announcements`: id, team_id FK, body TEXT NOT NULL, pinned BOOLEAN NOT NULL DEFAULT false, created_by, created_at. Append-only by convention (no updated_at).
- **Models** (kairos-db models/teams.rs or a new models/team_pages.rs): TeamPage/NewTeamPage/TeamPageChangeset, TeamPageHistory rows, TeamAnnouncement/NewTeamAnnouncement; TeamPageKind text_enum.
- **Scaffold** seeded by a `kairos-db` function called inside the SAME transaction as team creation (`create_team` path in org/teams — the delivery-board precedent): Team Charter page (is_protected, templated headings: Mission/Scope/Ways of Working/Success Measures — reuse the team_charter template skeleton text), "Support Processes" folder + "Overview" index page, "Documentation" folder with subfolders Planning, Tutorials, How-to Guides, Reference, Explanation, Design Docs. Creation writes v1 history baselines for pages (the T-0012 interpretation).
- **Backfill**: seeding is code-driven (needs UUIDs/audit actors) — for existing teams run the scaffold at migrate time? Precedent: migrations are pure SQL here. Decision: do the scaffold in SQL in the tenant migration for EXISTING teams (created_by = the team's earliest member or a fixed system UUID? teams carry no creator; use the zero UUID convention? created_by is NOT NULL UUID without FK — check users FK) — investigate at implementation and record; the clean fallback is a small idempotent `seed_team_scaffold` kairos-db function invoked per team by migrate-tenants tooling or first-write. Record the chosen mechanism here.
- schema.rs regen (`angreal db schema-sync`); remember the T-0077 lesson: touch kairos-db so the embedded migrations re-embed before schema-sync/migrate-tenants.
- tenant_provisioning test: EXPECTED_TABLES +3 (27); upgrade-path sim re-pins to THIS migration (the recurring maintenance point).

## Acceptance Criteria

## Acceptance Criteria

- [x] Tenant migration adds the three tables (guarded); documents table untouched; schema.rs regenerated (27 tenant tables).
- [x] create_team seeds the scaffold in the same transaction as the delivery board (and seed.rs's team helper does the same for reseeded tenants); scaffold pages carry v1 history rows (folders deliberately none).
- [x] Existing teams backfill via the migration's guarded DO-block (zero-UUID actor, decision recorded); re-runs converge (idempotency asserted in the db test).
- [x] Charter is_protected; sibling-slug uniqueness enforced incl. NULL-parent roots (COALESCE unique index; violation asserted).
- [x] NEW tests/team_pages.rs covers shape/protection/history/idempotency/uniqueness/second-team independence — green.
- [x] tenant_provisioning updated (27 tables, upgrade-path re-pinned) — full integration suite (31 targets) green first run.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved, "ralph 1-3").
- 2026-08-29 (impl): Migration `2026-08-29-000000_team_pages` (three tables; sibling-slug uniqueness via a COALESCE(zero-uuid) unique INDEX — plain UNIQUE treats NULL parents as distinct; SQL backfill DO-block scaffolds every existing live team, guarded per (team,slug), actor = zero UUID since audit cols carry no FK and migrations have no user — RECORDED DECISION). Applied to org_demo; schema.rs regenerated (27 tenant tables).
  - Models: models/team_pages.rs (TeamPage/New/Changeset, TeamPageHistory rows, TeamAnnouncement/New) + TeamPageKind text_enum (+test).
  - NEW kairos-db team_pages.rs: `seed_team_scaffold` (idempotent ensure_node per (team,parent,slug); pages get v1 history baselines; charter is_protected) — MUST stay in step with the migration backfill (cross-referenced in both).
  - Wired into BOTH team-creation paths: server create_team transaction (org/teams.rs, after create_board) and seed.rs's team helper (seed --force reprovisions from empty, so the migration backfill alone would miss reseeded teams).
  - Tests: NEW tests/team_pages.rs (scaffold shape incl. protection, history baselines pages-only, idempotency, NULL-parent sibling-slug violation, second team independent); tenant_provisioning EXPECTED_TABLES 24→27 + upgrade-path re-pinned to this migration.
  - Workspace + all targets compile; unit green. Integration running.