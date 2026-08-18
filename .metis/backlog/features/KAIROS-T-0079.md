---
id: team-landing-pages-seeded
level: task
title: "Team landing pages: seeded documentation tree, charter, and one-way announcements"
short_code: "KAIROS-T-0079"
created_at: 2026-08-16T14:57:48.022857+00:00
updated_at: 2026-08-16T14:57:48.022857+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


exit_criteria_met: false
initiative_id: NULL
---

# Team landing pages: seeded documentation tree, charter, and one-way announcements

> **NOTE: initiative-sized (XL).** This backlog item carries the design; on acceptance it should be promoted to an initiative and decomposed (~6–8 tasks for v1).

## Objective

UAT verdict on `/teams/:slug`: "this is bare." Give teams a landing page they can make their own: a **folder-style system of markdown pages** with a simple WYSIWYG-style editorial mode, **opinionated seeded defaults**, strictly **one-way comms** (announcements, no comments), and always-present **Team Documentation** sections.

PO direction (treat as requirements):
- Folder-style system + markdown rendering as the display; simple "WYSIWYG"-style editorial mode.
- System stays flexible, but Kairos ships opinionated layouts and defaults.
- Comms are ONE-WAY in v1 — announcements/broadcast, no comments anywhere.
- Every team gets: **Team Charter** (always present), **Support Processes**, and **Documentation** subdivided into planning docs, diataxis-style docs (tutorials/how-to/reference/explanation), and design docs.

## Backlog Item Details

### Type
Feature (initiative candidate)

### Priority
P1 — UAT feedback; the team surface is a first-class lens (KAIROS-I-0006) with no content.

### Business Justification
- **User Value**: Teams get a home — who we are, how to engage us, where our docs live — instead of a roster and two link panels.
- **Effort Estimate**: XL (initiative-sized; v1 spans db + server + web plus seed/backfill).

## Current State

- `/teams/:slug` (pages/teams.rs:166–255) renders exactly: PageHeader + Members roster + a Delivery-board link + Delivery-streams list. Read-only by design; CRUD lives in /admin/teams. Slug resolution is a client-side scan of all teams (teams.rs:125–133) — no GET /api/teams/by-slug.
- `Team` model = id, name, slug, team_type, timestamps (models/teams.rs:23–31) — **no content field of any kind**. Team creation auto-creates the `{slug}-delivery` board in one transaction (org/teams.rs:5–10); writes need org-admin `manage_teams`; reads are tenant-wide.
- **No folder/tree/team-doc plumbing exists.** Documents have no parent_id/team_id (schema.rs:98–112); `POST /api/documents` REQUIRES a live strategy/initiative/task parent (supports edge + board-inherited authorization, documents.rs:1–17); the relationship graph is items-only (enums.rs:162–169) — a team cannot own a document today.
- **Reusable pieces**: XSS-safe markdown rendering is a free function (item/markdown.rs:20–34, pulldown-cmark, raw HTML escaped); `ContentEditor` (textarea + Edit/Preview + A-0004 optimistic concurrency with 409 merge dialog, editor.rs:100–248) is coupled to item endpoints but generalizable. No content size limits exist anywhere in the write path.

## What Other Tools Do (survey summary)

- **Atlassian Atlas**: fixed-schema team profile (members, mission, links, current work) — zero-maintenance, comparable across teams; inextensible. Its **updates** feed is the canonical one-way comms pattern (append-only broadcast, push/digest distribution).
- **Confluence**: template-seeded space overview + page tree — flexible, but blank-page rot without a librarian.
- **GitLab handbook**: folder-tree-of-markdown as single source of truth — diffable, agent-friendly; MR friction excludes non-technical writers.
- **Slab/Slite/Notion editors**: WYSIWYG where markdown syntax triggers live formatting — but their stored format is a block model; **deterministic round-tripping back to real markdown is the hard engineering bit** few tools attempt.
- **Diataxis** (tutorials/how-to/reference/explanation) is the standard docs taxonomy (Canonical et al.). Notion/Slite/Slab-style verification badges (owner + expiry) are the freshness pattern — separable, v2.

Best-fit synthesis: Atlas-style fixed charter layout + Confluence-style seeded defaults + GitLab's storage mental model + Atlas-style updates feed.

## Options Considered

**A. Extend the Document entity with team ownership** — nullable team_id/parent_id on documents. *Rejected*: forks the settled T-0018 parent contract and A-0006 board-inherited authorization; teams aren't graph nodes so "team parent" can't be a modeled edge; folder semantics leak into workflow-attached docs; short-code/search namespace mixes deliverables with wiki pages.

**B. New `team_pages` construct (RECOMMENDED)** — dedicated tables: `team_pages` (self-referential folder tree: team_id, parent_id, kind folder|page, slug, title, content, position, version, audit cols; unique (team_id, parent_id, slug)), `team_page_history`, `team_announcements` (append-only). *Pros*: clean permission model (team members write, tenant reads) independent of board ABAC; first-class folder tree with path-addressable URLs; documents contract untouched; markdown pipeline reused verbatim; scaffold seeding rides the existing transactional team-creation precedent. *Cons*: second content store (history/concurrency machinery paralleled — mitigate by generalizing the editor's save/merge behind a callback); team pages aren't graph nodes in v1.

**C. Minimal: charter column on teams + announcements table** — *Rejected*: fails the stated requirements (no folders, no doc sections); dead end requiring a content migration later; the "bare" complaint returns immediately.

## Recommendation (v1 sketch)

- **Schema**: team_pages / team_page_history (item_history mirror) / team_announcements (id, team_id, body, created_by, created_at, pinned). Charter page flagged protected (undeletable/unmovable).
- **Seeding**: scaffold created in the same transaction as team creation (alongside the auto delivery board): Team Charter page (templated headings), Support Processes folder + index, Documentation folder with Planning / Tutorials / How-to Guides / Reference / Explanation / Design Docs subfolders. Backfill migration seeds existing teams. Seed-on-creation over template-on-first-edit because "Charter always present" is a requirement.
- **API**: `GET /api/teams/by-slug/{slug}` (kills the client-side scan); pages CRUD + tree endpoint under `/api/teams/{id}/pages`; announcements GET/POST (append-only; delete own/org-admin; no edit). Writes: team members + org admins; reads: tenant-wide. Enforce a server-side content size cap (none exists today — set one, e.g. 256KB/page).
- **GUI**: `/teams/:slug` becomes a **fixed opinionated layout** in v1 (not configurable blocks): header, rendered Charter, Announcements panel (newest-first, pinned on top, post box for members), Members, Delivery board link, Streams, Documentation tree navigator. `/teams/:slug/pages/{path...}` renders/edits pages.
- **Editor v1 (honest scope)**: existing textarea + Edit/Preview generalized over a save callback, plus a markdown toolbar (heading/bold/list/link inserting syntax) and the A-0004 merge dialog. **Markdown stays the source of truth.** True WYSIWYG-over-markdown live formatting is v2 — deterministic round-tripping is the hard bit per the survey.
- **v2 wave**: hybrid live-formatting editor, configurable landing blocks, announcement push/digest distribution, verification badges, cross-linking team pages ↔ work items, move/rename with redirects, export.

## Acceptance Criteria (v1)

- [ ] Tenant migration adds team_pages, team_page_history, team_announcements; documents table and its parent contract untouched.
- [ ] Team creation seeds the full scaffold (Charter, Support Processes, Documentation with planning/tutorials/how-to/reference/explanation/design-docs) in the same transaction as the delivery board; backfill seeds existing teams.
- [ ] The Charter exists for every team and cannot be deleted or moved; other pages/folders support create, rename, move, soft-delete.
- [ ] GET /api/teams/by-slug/{slug} exists and TeamPage uses it (no fetch-all scan).
- [ ] `/teams/:slug` renders the fixed v1 layout (charter, announcements, members, board link, streams, docs tree); pages reachable at stable path URLs.
- [ ] Editing offers Edit/Preview + markdown toolbar; version-checked saves with 409 merge dialog; every save writes a history row.
- [ ] Announcements are append-only markdown posts by team members/org admins; **no comments, reactions, or threading anywhere**; pinned sort first.
- [ ] Permissions: team members + org admins edit/post; all tenant users read.
- [ ] All rendering goes through the existing XSS-safe markdown pipeline.
- [ ] Server-side content size limit enforced with a clear validation error.
- [ ] Page slugs unique within (team, parent); folder deletion with children behaves predictably (documented + tested).
- [ ] Tests cover seed transaction, charter protection, permission matrix, and the concurrent-edit conflict path.

## Open Questions

- Announcement authorship: team members only, or also org admins to any team? (Recommended: both.)
- Should team pages carry short codes so work items can reference them in v2, or stay path-addressed? (Affects whether v2 cross-linking needs a migration.)
- Content size cap value; configure a server-wide axum body limit at the same time?
- Backfill depth for existing teams: full scaffold or charter-only?
- Announcement retention/pinning scope for v1.
- Should v1 auto-derive a "current work" summary from the delivery board (Atlas-style), or is the board link sufficient until v2?

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
