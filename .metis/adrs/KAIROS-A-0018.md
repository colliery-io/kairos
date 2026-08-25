---
id: 001-two-status-vocabularies-board
level: adr
title: "Two Status Vocabularies - Board Position for Work, Editorial Lifecycle for Documents"
number: 1
short_code: "KAIROS-A-0018"
created_at: 2026-08-25T03:01:07.053625+00:00
updated_at: 2026-08-25T03:01:07.053625+00:00
decision_date: 2026-08-25
decision_maker: Dylan Storey
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/draft"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Two Status Vocabularies - Board Position for Work, Editorial Lifecycle for Documents

## Context

UAT on the v1 clients surfaced a modeling hole (KAIROS-T-0078): "document type" and a draft/review "Document status" existed as **unscoped tenant metadata definitions**, attachable to any entity. A task could carry `document_type: prd` or status `draft` — nonsense states that GUI-only fixes (KAIROS-T-0065/T-0066) hid but did not prevent: the metadata PATCH validated only value shape, and template stamping was a second unguarded write path.

Underneath sat a conceptual ambiguity: Kairos had one word — "status" — for two different things. A work item's status is **where it sits in the process** (its board column, governed by the A-0002 transition rules). A document's status is **editorial** (draft → published), and documents deliberately do not live on boards (A-0001/S-0004: no `board_id`/`column_id` on `documents`).

The surveyed field is unanimous (Jira+Confluence, Linear, Shortcut, Asana): documents stay off boards and carry either no lifecycle or a lightweight label — never the work-item workflow engine. Notion's unenforced schema-by-convention is the failure mode Kairos was reproducing; Height, the everything-is-a-task counterexample, shut down in 2025.

## Decision

Kairos has **two status vocabularies, structurally enforced**:

1. **Work-item status IS board position.** A strategy/initiative/task's state is its `column_id`, moved only through the board's transition rules (A-0002). No metadata definition may model a parallel status (the T-0066 lesson).
2. **Document lifecycle is a typed column** — `documents.lifecycle: draft | review | published | archived` (default `draft`) — an **editorial label with free transitions** (any state to any state), never a transition engine, never a board column. It is mutated by a dedicated endpoint (`PATCH /api/documents/{code}/lifecycle`, gated by `manage_documents` on the authorization board), activity-logged (`lifecycle:{from}->{to}`), and rendered as a distinct badge — never in the shared metadata panel.
3. **Documents never occupy boards.** No 'document' board level exists; `entity_directory` reports `board_id NULL` for documents; there is no document transition route. Documents contextualize work items via `supports` edges (T-0018 contract).
4. **Metadata definitions are entity-scoped.** `metadata_definition_scopes(metadata_definition_id, entity_type)` lists the types a definition applies to; **no rows = applies to all**. Scoping is enforced in the data layer (`kairos_db::items::definition_applies_to`) at BOTH write paths — the metadata PATCH and template stamping — and mirrored in the read catalog (`GET /api/metadata-definitions?entity_type=…`), so pickers can only offer what the write path would accept. Seeded scopes: `document_type` → document only; `complexity` → everything **except** initiatives (their native `complexity` column is the source of truth there); `priority` unscoped.

## Consequences

- A task structurally cannot carry `document_type` or any draft/published state; the classes of nonsense UAT found are unrepresentable, not merely hidden.
- The legacy 'Document status' metadata definition is retired everywhere (system tables and tenants) by paired public/tenant migrations; existing values backfilled into `documents.lifecycle` (draft→draft, review→review, approved→published; documents with no legacy status → published, per PO decision — nothing beyond the demo tenant existed).
- Lifecycle changes are not content edits: no version bump, no `item_history` row (the A-0004 contract covers title/content), matching the `work_class` precedent (KAIROS-T-0077).
- Every future entity type must declare its scoping posture when added to the entity-type vocabulary (DDL CHECKs on both scopes tables + the server-side vocabulary).
- Stray out-of-scope rows are deleted at migration time **with counts reported** (RAISE NOTICE), so operators can see what a tenant lost.
- Operator-created definitions can be scoped via the admin API (`entity_types` on create/update); a scoping admin UI is deliberately deferred until a real operator need appears.

## Alternatives Considered

- **Server-side hard-coded scope map, no schema** — rejected: repeats the papering-over one layer down; template stamping (a kairos-db path) stays uncovered; operator definitions can't be scoped.
- **Documents as board citizens** (a 'document' board level with draft/review/published columns) — rejected: models document lifecycle as exactly the concept this ADR separates it from, against the unanimous surveyed field.
- **Lifecycle as ordered transitions** (draft→review→published engine) — rejected: editorial states are labels; a transition engine invites WIP limits and assignees on prose.

## References

- KAIROS-T-0078 (execution), KAIROS-T-0065/T-0066 (the GUI-only precursors), KAIROS-T-0077 (`work_class` — the no-version-bump precedent)
- KAIROS-A-0001 (data model), KAIROS-A-0002 (board columns/transitions), KAIROS-A-0003 (typed metadata), KAIROS-A-0004 (versioning)
