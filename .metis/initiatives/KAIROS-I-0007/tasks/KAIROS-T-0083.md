---
id: team-pages-announcements-api-by
level: task
title: "Team pages + announcements API: by-slug, CRUD/tree, permissions, size cap"
short_code: "KAIROS-T-0083"
created_at: 2026-08-29T02:58:56.249857+00:00
updated_at: 2026-08-29T12:07:11.237497+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Team pages + announcements API: by-slug, CRUD/tree, permissions, size cap

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. Depends on KAIROS-T-0082 (schema/models/scaffold).

## Objective

The server surface for team pages and announcements: by-slug team lookup, pages CRUD + tree, append-only announcements, the team-membership permission model, and the tenant's first server-side content size cap.

## Implementation Notes

- **Permission model** (differs deliberately from board ABAC): writes require the caller to be a MEMBER of the team OR an org admin; reads are open tenant-wide (matches org/teams.rs reads). Implement one `require_team_member_or_admin(conn, tenant, team_id, user)` helper in the server (membership via team_members; org role via the existing context) — 403 names the requirement.
- **Endpoints** (kairos-server api/org/teams.rs or a new api/org/team_pages.rs, registered in openapi):
  - `GET /api/teams/by-slug/{slug}` → TeamDetail (404 unknown) — also lets the web kill its fetch-all scan.
  - `GET /api/teams/{id}/pages` → the TREE (nested or flat-with-parent_id list in position-then-title order; pick flat + parent_id for a dumb client, record choice).
  - `POST /api/teams/{id}/pages` (kind, parent_id?, slug, title, content?) → 201; slug conflicts 422; parent must be a folder of the same team (422).
  - `GET /api/teams/{id}/pages/{page_id}` → full page.
  - `PATCH /api/teams/{id}/pages/{page_id}` — title/content edits are VERSION-CHECKED (A-0004 pattern: expected version, 409 with details.current carrying the server copy) and write team_page_history rows; also rename (slug) + move (parent_id/position). Charter (is_protected): rename/move/delete refused 422; content edits allowed.
  - `DELETE /api/teams/{id}/pages/{page_id}` — soft delete; folders with live children refused 422 (`FOLDER_NOT_EMPTY`, counts in message — the column-removal precedent).
  - `GET /api/teams/{id}/announcements` (pinned first, newest first) / `POST` (body, pinned?) / `DELETE /{announcement_id}` (author or org admin). NO edit route — append-only.
- **Size cap**: reject page content and announcement bodies over 256 KiB with 422 naming the limit (constant in kairos-core or server config). This is the tenant's first content cap — keep it a named const.
- **DTOs** in kairos-client (types_org or new types_team_pages) + client methods; utoipa registrations for all routes.
- Activity/events: log_activity rows for page create/edit/delete + announcement create (entity_type "team_page"/"team_announcement" — extend the documented action set only if needed; reuse Create/Delete + content edits via history). Thin events NOT required for v1 (no live team-page views yet) — record the decision.

## Acceptance Criteria

## Acceptance Criteria

- [x] All routes above exist, registered in openapi, with kairos-client DTOs + methods.
- [x] Permission matrix integration-tested: team member writes OK; non-member member 403 (naming team membership); org admin OK; all tenant users read.
- [x] Page content PATCH is version-checked: stale version → 409 with details.current; every content save writes a history row.
- [x] Charter protection: rename/move/delete 422; content edit OK. Folder delete with live children 422 with counts.
- [x] Announcements append-only (no edit route), pinned-first ordering, delete by author/org-admin only.
- [x] Size cap enforced on pages and announcements with a clear 422; integration test covers it.
- [x] `GET /api/teams/by-slug/{slug}` returns the team (404 unknown slug).

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved, "ralph 1-3").
- 2026-08-29: COMPLETE. Shipped: kairos-db page/announcement services in `team_pages.rs` (load/list/create/update_page_content/rename_move/soft_delete/list_announcements, `MAX_CONTENT_BYTES` = 256 KiB named const, typed `TeamPageError`); server module `api/org/team_pages.rs` (8 routes + `require_team_member_or_admin` whose 403 names team membership, `map_page_error` with 409 `details.current` per A-0004); `GET /api/teams/by-slug/{slug}` in teams.rs; router merged in org/mod.rs; all 9 paths in openapi (self-maintaining route==spec gate passes); kairos-client `types_team_pages.rs` DTOs + 9 methods. Integration test `kairos-server/tests/team_pages.rs` covers the full permission matrix, 409 merge shape, charter protection, FOLDER_NOT_EMPTY with count, size cap on both families, append-only pinned-first announcements, author-or-admin delete, by-slug 404, and activity rows. `angreal test unit` + `angreal test integration` (32 targets) green.
- 2026-08-29: Decisions recorded: (1) pages list is FLAT with parent_id (dumb client nests; position-then-title order). (2) PATCH is EITHER content(+version) OR structure (slug/parent_id/move_to_root/position) — mixing 422s; `move_to_root: bool` disambiguates "move to root" from "unchanged" since JSON null can't. (3) Activity: Create/Delete rows for pages (entity_type `team_page`) + Create for announcements (`team_announcement`); content edits are NOT activity-logged — `team_page_history` is their record (items.rs A-0004 precedent). (4) No thin WS events for v1 (no live team-page views yet). (5) Announcement bodies share `MAX_CONTENT_BYTES` rather than a second constant.