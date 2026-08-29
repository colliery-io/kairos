---
id: derived-team-work-documents
level: task
title: "Derived team work-documents: endpoint + landing-page panel"
short_code: "KAIROS-T-0084"
created_at: 2026-08-29T02:58:59.862728+00:00
updated_at: 2026-08-29T12:57:15.220487+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Derived team work-documents: endpoint + landing-page panel

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. The PO's direct UAT ask ("why can't I see a list of my documents for a team?"). Independent of the team_pages tables — can land before or alongside T-0082/T-0083; the PANEL goes on the current /teams/:slug page and is re-composed by T-0085.

## Objective

Surface the documents attached to a team's WORK from the team page: `GET /api/teams/{id}/work-documents` + a "Work documents" panel on `/teams/:slug`.

## Implementation Notes

- **Membership rule** (derived, no schema): a document belongs to a team's work when its `supports` PARENT (source of the supports edge, per the T-0018 contract) is (a) a task with `team_id = {team}`, or (b) any live item whose `board_id` is the team's delivery board. One grouped SQL query in kairos-db (graph.rs or teams.rs): join item_relationships (relationship='supports') → documents (live) on target, parent resolution via the tasks table + entity_directory for board placement; DISTINCT on the document (a doc could support two team items). Return (document short_code, title, lifecycle, parent short_code, parent title), ordered by document short_code.
- Docs supporting org-level items (e.g. a PRD under an org-wide initiative) deliberately do NOT appear — team attribution follows the parent item. Record this boundary in the panel's caption/empty-state so it reads as intended, not missing ("documents attached to this team's work items").
- **Endpoint**: `GET /api/teams/{id}/work-documents` (open tenant-wide reads, 404 unknown team), DTO in kairos-client + client method, utoipa.
- **Web**: a "Work documents" Panel on `/teams/:slug` (current teams.rs layout — T-0085 re-composes later): rows of `short_code — title` linking to `/items/{code}`, lifecycle badge chip (reuse the T-0078 state colors), parent item as a dimmed secondary line linking to its detail. Honest empty state.
- **Fixture note**: the demo seed's only document (PRD) supports the org-level signup initiative → both teams' panels are EMPTY on the stock seed. Integration tests create their own fixtures; the demo-visible example arrives with T-0087's fixture wave (a doc under a platform task).

## Acceptance Criteria

## Acceptance Criteria

- [x] `GET /api/teams/{id}/work-documents` returns exactly the derived set: docs supporting the team's tasks (team_id) or items on the team's delivery board; soft-deleted docs/parents excluded; DISTINCT; 404 unknown team.
- [x] Integration test builds both membership paths + a negative (doc under an org-level initiative absent) + dedup (doc supporting two team items appears once).
- [x] kairos-client DTO + method; openapi registered.
- [x] /teams/:slug renders the panel with detail links, lifecycle chips, parent attribution, and the scope-honest caption/empty state.
- [x] Ladder green for the touched tiers.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved, "ralph 1-3").
- 2026-08-29: COMPLETE. Shipped: `graph::team_work_documents` in kairos-db — ONE grouped `sql_query` (item_relationships supports → live documents on target, parents hydrated via `entity_directory` which is live-only, `LEFT JOIN tasks` for the team_id path, `DISTINCT ON (d.short_code)` with `ORDER BY d.short_code, p.short_code` so parent attribution is deterministic — lexicographically first). `GET /api/teams/{id}/work-documents` in org/teams.rs (open read, load_team 404, delivery_board_of feeds path b; no board → team_id path only), openapi registered; `TeamWorkDocument` DTO in types_team_pages + `list_team_work_documents` client method. Web: `WorkDocument` mirror + fetch in pages/teams/api.rs (decode test), "Work documents" panel on /teams/:slug — `code — title` → /items/{code}, lifecycle Pill (local `lifecycle_color` copy of the T-0078 mapping), dimmed "supports {parent}" attribution line linking to the parent, scope-honest caption ("documents attached to this team's work items") and empty state naming the org-level boundary.
- 2026-08-29: Verified in kairos-server/tests/team_pages.rs (extended): both membership paths (task team_id on a foreign board; team_id-less task on the team's delivery board), the combined path, org-level negative, dedup (doc supporting two team items appears once, parent = lexicographically first), soft-deleted doc AND soft-deleted parent excluded, second team sees only its board's doc, unknown team 404. Ladder green: `angreal test unit` + `angreal test integration` (32/32 targets ok), kairos-web compiles for wasm32. Demo-visible fixture arrives with T-0087 as planned.