---
id: data-model-scope-metadata
level: task
title: "Data model: scope metadata definitions per entity type; document lifecycle as a first-class column"
short_code: "KAIROS-T-0078"
created_at: 2026-08-16T14:57:03.064159+00:00
updated_at: 2026-08-16T14:57:03.064159+00:00
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

# Data model: scope metadata definitions per entity type; document lifecycle as a first-class column

## Objective

UAT feedback: "document type is card metadata; document type is only for documents… status is 'where is it at in the process' never a document status like 'draft'." Make both rules **structural facts**: metadata definitions get entity-type scoping enforced on the write path, and document lifecycle (draft/review/published) becomes a typed column on documents — a different mechanism from board position, rendered differently.

PO constraints (hard requirements):
1. `document_type` exists ONLY on documents, never on tickets.
2. Ticket "status" always means board position; it is never a document-lifecycle value like "draft". Documents may keep a lifecycle, but it is a DIFFERENT concept, modeled and rendered as such.

## Backlog Item Details

### Type
Feature (data-model correctness)

### Priority
P1 — the T-0065/T-0066 GUI fixes papered over a hole that is still open at the data layer.

### Business Justification
- **User Value**: No more nonsense states (a task carrying "Document Type: PRD" or status "draft"); document lifecycle gets a real, visible home.
- **Effort Estimate**: L — spans all three crates + a guarded multi-tenant migration with backfill; decomposes into 3–4 tasks.

## Current State (the hole, precisely)

- Storage is already per-type: five typed tables sharing one UUID space (models/items.rs:1–7). Documents are the only entity with **no board_id/column_id** ("Documents do not live on boards", items.rs:199–217), the only one with `template_id`, and creation requires a live work-item parent with a `supports` edge + inherited authorization (api/documents.rs:164–221).
- **Metadata definitions have NO entity-type scoping**: `MetadataDefinition` is just id/name/slug/field_type/is_system_default (models/templates.rs:62–70). 'Document Type' (enum: prd|system_context|architecture|charter|social_contract|vision) and 'Document status' (slug `status`: draft|review|approved) are seeded unscoped (tenant.rs:147–177).
- **The write path enforces nothing**: `PATCH /api/{entity_type}/{short_code}/metadata` validates the value against the definition's type/options only — no check that a definition belongs to the entity type (api/meta/metadata.rs:100–124). `document_type` and `status` ARE attachable to a task today. Template stamping (kairos-db/src/items.rs:672–695) is a second unguarded write path.
- **KAIROS-T-0065's fix was GUI-only**: the panel renders editors only for stamped fields, but the "(add a field…)" picker still offers the full unscoped catalog to any entity (pages/item/metadata.rs:91–135; blanket GET /api/metadata-definitions, item/api.rs:318–322).
- **Document lifecycle is a free-form metadata string**: the `status` row (default 'draft') stamped by prd/system_context/architecture_framing templates only (tenant.rs:198–213); charter/social_contract/vision templates stamp none. No transitions, no enforcement, editable like any string. T-0066 only renamed the definition ('Status' → 'Document status').

## What Other Tools Do (survey summary)

Every surviving tool keeps documents off boards and gives them either no lifecycle or a lightweight editorial one — never the work-item workflow engine. **Jira+Confluence**: hard type split; Confluence content status is a colored label, not a transition engine; Jira fields are scoped per issue-type. **Linear**: docs attach to projects/initiatives with zero status — the container carries state; boards render issues only. **Notion**: schema-by-container with no enforcement — the convention-erosion failure mode Kairos has today. **Shortcut**: docs are peers with owner/access, no workflow states. **Height** (everything-is-a-task counterexample): shut down 2025. Consensus: two enforced lifecycle vocabularies; type-scoped metadata; documents contextualize containers rather than flowing through them.

## Options Considered

**A. Schema-level fix (RECOMMENDED)** — `metadata_definition_scopes` join table on a new entity_type enum (empty scope set = applies to all, so priority/complexity need no migration); enforcement in BOTH write paths (PATCH validation + template stamping, at the core/db layer); document lifecycle promoted to a NOT NULL `documents.lifecycle` enum column (draft|review|published) with a dedicated endpoint and a distinct badge rendering; guarded migration backfills lifecycle from legacy `status` metadata (approved→published) then deletes the `status` definition. *Pros*: closes the hole at the data layer; models the two-vocabulary rule exactly; extends the existing per-type-table split. *Cons*: largest surface (enum + join table + column + backfill + two enforcement points + endpoint + badge).

**B. API+validation layer only** — hard-coded scope map in the server, entity_type filter on the definitions listing. *Rejected*: the distinction lives in server code, not the model; template stamping (a kairos-db path) stays uncovered; operator-created definitions can't be scoped; lifecycle stays a free-form string in the generic panel — repeats the T-0065/T-0066 papering-over one layer down.

**C. Documents become board citizens** (a 'document' board level with draft/review/published columns). *Rejected*: models document lifecycle as exactly the concept the PO forbids it from being (board position); unanimously contradicted by the surveyed field; does nothing about unscoped metadata.

## Recommendation (v1 sketch)

- **Schema (kairos-db)**: `entity_type` Postgres enum ('strategy','initiative','task','document','adr'); `metadata_definition_scopes(definition_id, entity_type)` with empty-set-means-unscoped; `documents.lifecycle` document_lifecycle enum NOT NULL DEFAULT 'draft'; guarded tenant migration — backfill lifecycle from `status` item_metadata (draft→draft, review→review, approved→published), delete `status` rows/definition/template associations, seed document_type's scope row; update SEED_SYSTEM_DEFAULTS_SQL accordingly.
- **API (kairos-server)**: scope check in PATCH metadata validation (metadata.rs:104–124); scope filter in template stamping (kairos-db items.rs:672–695 — enforced in kairos-db, not the handler); `entity_type` filter on GET /api/metadata-definitions; `PATCH /api/documents/{short_code}/lifecycle` participating in existing history/versioning.
- **GUI (kairos-web)**: metadata panel passes its entity type so the add-field picker only offers in-scope definitions; document pages render lifecycle as a distinct badge/select near the title, styled apart from anything board-status-like; boards untouched — documents stay off them.
- **ADR**: record the two-vocabulary decision (ticket status = board position; document lifecycle = typed editorial state; documents never occupy board columns; metadata definitions are entity-scoped). This constrains every future entity type — exactly what the ADR machinery is for.

## Acceptance Criteria

- [ ] PATCH metadata with an out-of-scope definition (e.g. document_type on a task) returns a validation error; API-layer test.
- [ ] Template stamping never writes a definition outside its scope, enforced in kairos-db with a test.
- [ ] GET /api/metadata-definitions supports entity_type filtering; the add-field picker only offers in-scope definitions.
- [ ] `documents.lifecycle` NOT NULL enum (draft|review|published) default draft; dedicated endpoint updates it; captured by existing versioning/history.
- [ ] Guarded, idempotent tenant migration: backfill lifecycle from legacy `status` (approved→published), remove the `status` definition/options/template associations/item_metadata rows; stray out-of-scope rows handled per the migration decision recorded here.
- [ ] SEED_SYSTEM_DEFAULTS_SQL no longer seeds 'Document status'; seeds document_type scoped to documents; fresh and existing tenants converge.
- [ ] Documents remain absent from all boards (no 'document' board level; entity_directory board_id stays NULL; no transition route).
- [ ] Lifecycle renders as a visually distinct badge on document pages, never in the shared metadata panel, never in any board column.
- [ ] An ADR documents the two status vocabularies, entity-scoped metadata, and documents-never-on-boards.

## Open Questions

- Include 'archived' in the lifecycle enum in v1, or three states enough?
- Charter/social_contract/vision docs have no legacy status — backfill to 'draft' or 'published'?
- Lifecycle transitions free (any→any, Confluence-label style — recommended) or ordered (draft→review→published only)?
- Migration handling of existing out-of-scope rows (document_type already on a task): delete silently or delete + report counts?
- Do any tenants have operator-created definitions today? (Decides whether v1 needs a scoping admin UI or admin API only.)
- Should complexity gain a scope (initiatives already have a complexity *column*, items.rs:92–94), or stay unscoped until a real conflict?

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
