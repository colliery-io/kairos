---
id: team-landing-pages-seeded
level: initiative
title: "Team Landing Pages - Seeded Documentation Tree, Charter, and One-Way Announcements"
short_code: "KAIROS-I-0007"
created_at: 2026-08-29T02:48:32.769579+00:00
updated_at: 2026-08-29T15:48:22.832140+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: team-landing-pages-seeded
---

# Team Landing Pages - Seeded Documentation Tree, Charter, and One-Way Announcements Initiative

> Promoted from backlog ticket KAIROS-T-0079 (UAT design wave, session ffc0d1f9); design + external survey live there and are summarized here. During UAT review of T-0078 the PO independently re-hit the core gap: "why can't I see a list of my documents for a team?"

## Context

UAT verdict on `/teams/:slug` was "this is bare": the page renders only a header, member roster, a delivery-board link, and a streams list (pages/teams.rs:166–255, KAIROS-T-0067). Teams have nowhere in Kairos to say who they are, how to engage them, or where their documentation lives — and no way to see their documents at all. Structurally, nothing connects teams to content: the `Team` model has no content fields (models/teams.rs:23–31); documents attach only to work items via `supports` edges with board-inherited authorization (T-0018/A-0006); teams are not relationship-graph nodes, so a team cannot own a document today.

PO direction (hard requirements, from the T-0079 review):
- A **folder-style system** of markdown pages rendered as the display, with a simple WYSIWYG-style editorial mode.
- The system stays flexible, but Kairos ships **opinionated layouts and defaults**.
- Comms are **one-way** in v1 — announcements/broadcast, no comments anywhere.
- Every team gets **Team Documentation** sections: **Team Charter** (always present), **Support Processes**, and **Documentation** subdivided into planning docs, diataxis-style docs (tutorials/how-to/reference/explanation), and design docs.

External survey (full citations in T-0079): Atlas-style fixed team profile (comparable, zero-maintenance) + Confluence-style template-seeded space content + GitLab's folder-tree-of-markdown storage model + Atlas-style one-way updates feed; Slab/Slite prove WYSIWYG-over-markdown live formatting is the hard bit (deterministic round-tripping) — deferred to v2.

## Goals & Non-Goals

**Goals (v1):**
- A team landing page teams can make their own: rendered Charter, announcements, members, board link, streams, and a navigable documentation tree.
- A seeded scaffold on team creation (and backfill for existing teams): Charter (protected), Support Processes, Documentation with Planning / Tutorials / How-to Guides / Reference / Explanation / Design Docs.
- Page editing with the proven editor pattern (Edit/Preview + markdown toolbar + A-0004-style version-checked saves and merge dialog); markdown is the source of truth.
- One-way announcements (append-only, pinned-first, team members + org admins post; no comments/reactions/threading).
- The team's WORK documents visible from the landing page: a derived panel listing documents whose parent work items belong to the team (the PO's direct ask).
- Permissions: team members + org admins write; all tenant users read. Server-side content size cap (none exists anywhere today).

**Non-Goals (v1 — recorded for the v2 wave):**
- True WYSIWYG-over-markdown live formatting (deterministic round-tripping is the hard bit).
- Configurable landing-page blocks (v1 is a fixed opinionated layout).
- Announcement push/digest distribution; verification/freshness badges; page move/rename redirects; export.
- Cross-linking team pages into the work-item relationship graph (team pages are not graph nodes in v1).
- Comments of any kind.

## Detailed Design

**Option decision (from the T-0079 review): a new `team_pages` construct — NOT extending the Document entity.** Documents' creation contract requires a workflow parent with board-inherited authorization; team pages need the opposite (team-membership writes, tenant reads), and teams aren't graph nodes. A dedicated construct keeps both contracts clean. The Document entity and its T-0018/A-0006 invariants are untouched.

- **Schema (tenant migration)**: `team_pages` (id, team_id FK, parent_id nullable self-FK, kind folder|page, slug, title, content, position, version, audit cols, deleted_at; UNIQUE(team_id, parent_id, slug); `is_protected` for the Charter), `team_page_history` (item_history mirror), `team_announcements` (id, team_id, body, created_by, created_at, pinned). Scaffold seeded in the SAME transaction as team creation (the delivery-board precedent, org/teams.rs:5–10); guarded backfill migration seeds existing teams.
- **API**: `GET /api/teams/by-slug/{slug}` (kills the client-side scan at teams.rs:125–133); `GET/POST/PATCH/DELETE /api/teams/{id}/pages` + tree endpoint; `GET/POST /api/teams/{id}/announcements` (append-only; delete own / org-admin; no edit); derived `GET /api/teams/{id}/work-documents` (documents whose supports-parents are the team's items: tasks with team_id + items on the team's delivery board). Content size cap enforced (e.g. 256KB/page) with a clear validation error.
- **Web**: `/teams/:slug` fixed v1 layout — header, rendered Charter, Announcements (newest-first, pinned on top, post box for members), Members, Delivery board, Streams, Documentation tree, Work Documents panel. `/teams/:slug/pages/{path...}` renders/edits pages via the generalized editor (extract ContentEditor's save/merge behind a callback; markdown toolbar inserting syntax). All rendering through the existing XSS-safe pipeline (markdown.rs).
- **Editor honesty**: v1 is textarea + Edit/Preview + toolbar; markdown stays the source of truth.

## Decomposition (proposed)

1. **Schema + provisioning**: three tables + models + seeded scaffold on team creation + backfill migration; charter protection; schema-sync. (M)
2. **Pages + announcements API**: by-slug endpoint, pages CRUD/tree with permission model, announcements, size cap; integration tests incl. permission matrix + charter protection. (M)
3. **Derived work-documents**: `GET /api/teams/{id}/work-documents` + the landing-page panel; the PO's "see my team's docs" ask lands here. (S)
4. **Web: landing page v1**: fixed layout composing charter/announcements/members/board/streams/docs-tree/work-docs; by-slug fetch. (M)
5. **Web: page view/edit**: `/teams/:slug/pages/{path...}`, generalized editor + markdown toolbar + version-checked saves/merge dialog + history rows. (M)
6. **E2E + fixture wave**: seed demo team content; teampages.spec (scaffold, charter protection, edit/save/conflict, announcements one-way, work-docs panel); existing-spec fallout. (M)

## Exit Criteria

- [ ] Every team (new and pre-existing) has the seeded scaffold; the Charter cannot be deleted or moved.
- [ ] `/teams/:slug` renders the fixed v1 layout with all panels live, including the derived Work Documents panel.
- [ ] Pages are editable at stable path URLs with version-checked saves, a 409 merge dialog, and history rows.
- [ ] Announcements are append-only with no comment surface anywhere; pinned sort first.
- [ ] Permission matrix enforced (team members + org admins write; tenant reads) with a server-side content size cap.
- [ ] Full ladder green (unit, integration, e2e incl. a new team-pages spec).

## Status Updates

- 2026-08-29: Promoted from KAIROS-T-0079 on PO instruction; design/survey carried over; decomposition proposed for sign-off.