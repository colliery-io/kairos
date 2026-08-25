---
id: data-model-scope-metadata
level: task
title: "Data model: scope metadata definitions per entity type; document lifecycle as a first-class column"
short_code: "KAIROS-T-0078"
created_at: 2026-08-16T14:57:03.064159+00:00
updated_at: 2026-08-25T03:29:28.113446+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] PATCH metadata with an out-of-scope definition is 422 with a naming message ("does not apply to task items"); the same write on a document succeeds (meta.rs test). Clears of out-of-scope values remain allowed for cleanup.
- [x] Template stamping enforced in kairos-db via `definition_applies_to` — write_path test smuggles a task-scoped definition into the prd template and proves it never stamps.
- [x] GET /api/metadata-definitions?entity_type=… filters (unscoped OR scoped-to-type; bad value 422); the web picker consumes exactly this filter; meta.rs asserts task/initiative catalogs incl. complexity-excludes-initiatives; e2e asserts both directions (task picker lacks Document Type; document carries it).
- [x] documents.lifecycle NOT NULL CHECK draft|review|published|archived DEFAULT draft; PATCH /api/documents/{code}/lifecycle (manage_documents-gated, free transitions, 422 bad value, 403 for no-grant users — meta.rs arc); interpretation recorded: activity row `lifecycle:{from}->{to}` + item_updated thin event, NO version bump (matches the T-0077 work_class precedent; write_path asserts version untouched and no-op logs nothing).
- [x] Guarded idempotent tenant migration with the full backfill mapping + no-status→published; retires the status definition/options/associations/values; stray out-of-scope rows deleted in a DO block with RAISE NOTICE count (recorded decision: delete + report).
- [x] SEED_SYSTEM_DEFAULTS_SQL seeds 3 definitions + system scope rows; paired PUBLIC migration retires system 'Document status' and creates system_metadata_definition_scopes; provisioning copies scopes; tenant_provisioning asserts convergence (3 copied, scope rows, 15 options, 6 template associations, 24 tenant tables).
- [x] Documents remain off all boards — untouched by this change; no document board level introduced (ADR records the rule).
- [x] Lifecycle renders as the dashed-outline `.kairos-lifecycle-badge` (state colors) + a dedicated "editorial state — not board status" panel, never in the metadata panel (lifecycle.spec asserts all three).
- [x] ADR KAIROS-A-0018 records the two vocabularies, documents-never-on-boards, entity-scoped metadata, seeded scopes, and the alternatives rejected.

## Resolved Decisions (PO review, 2026-08-24)

1. **Lifecycle enum includes `archived`**: draft | review | published | archived, default draft.
2. **Backfill for docs with no legacy status → `published`** (nothing adopted beyond the demo tenant; a live charter labeled "draft" would read as a regression). Legacy `status` maps draft→draft, review→review, approved→published.
3. **Lifecycle transitions are FREE** (any→any, Confluence-label style) — an editorial label, never a transition engine.
4. **Migration deletes stray out-of-scope metadata rows AND reports counts** in the migration/provisioning output.
5. **No scoping admin UI in v1** — only the demo tenant exists; seeding scopes for system definitions + exposing scope on the admin API suffices. UI when a real operator need appears.
6. **`complexity` gains a scope NOW, excluding initiatives** (scope rows: strategy, task, document, adr). The seeded definition is the same xs–xl vocabulary as initiatives' NATIVE complexity column (tenant.rs seeds; models/items.rs:92–94), so leaving it attachable to initiatives reproduces the exact duplication wart T-0066 fixed for Status. `priority` stays unscoped (applies to all).

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
- 2026-08-24: All six open questions resolved with the PO (recorded above); acceptance criteria updated to match (archived in the enum; unstated-status backfill → published; delete-with-counts migration). Ready to execute.
- 2026-08-25 (impl, server side done — workspace compiles):
  - Migrations: PUBLIC `2026-08-24-000000_system_metadata_scopes` (system scopes table + document_type→document, complexity→{strategy,task,document,adr}; deletes system 'Document status' + options + template associations; reversible down); TENANT `2026-08-24-000000_metadata_scopes_document_lifecycle` (scopes table + guarded hardcoded seed for system defs; documents.lifecycle TEXT CHECK draft|review|published|archived DEFAULT draft; backfill legacy status draft/review/approved→published + no-status→published; retires status definition; DO-block deletes stray out-of-scope rows with RAISE NOTICE count). Applied to org_demo; schema.rs regenerated (33 tables).
  - tenant.rs: SEED_SYSTEM_DEFAULTS_SQL drops 'Document status', seeds system scopes; provisioning copies scopes with definitions.
  - kairos-db: DocumentLifecycle text_enum + ActivityAction::Lifecycle (+tests); MetadataDefinitionScope model; Document.lifecycle; `definition_applies_to` (THE enforcement primitive — empty scopes = all); create_document stamping filters via it; `set_document_lifecycle` (activity `lifecycle:{from}->{to}` + ItemUpdated event, NO version bump — same interpretation as T-0077's work_class; AC "captured by versioning/history" read as activity+event, recorded here).
  - Server: metadata PATCH phase-1 scope check (422 on out-of-scope SET; clears of out-of-scope values still allowed for cleanup); definitions list gains entity_type filter (unscoped OR scoped-to-type; local query struct — serde_urlencoded can't flatten Pagination) + scopes in DTO + create/update accept entity_types (validated vocabulary, dedup); NEW PATCH /api/documents/{code}/lifecycle (manage_documents on the authorization board); openapi registered.
  - Clients: MetadataDefinition.entity_types, Create/Update entity_types, Document.lifecycle, SetLifecycleRequest, client.set_document_lifecycle(); MCP get_item shows `lifecycle:`; CLI documents table/fields show lifecycle.
  - NEXT: web (fetch_definitions entity_type param, document lifecycle badge+control), seed_demo/tenant_provisioning/meta test updates (+ re-pin upgrade-path sim to the NEW newest tenant migration), db stamping-filter test, e2e (picker scoping assertion + lifecycle spec), ADR, full ladder.
- 2026-08-25 (cont.): Web + tests + ADR authored; unit/build/lint green, integration running:
  - Web: Family::entity_type(); fetch_definitions(entity_type) — the picker consumes the same server filter the write path enforces; ItemDetail.lifecycle mirror (+decode test); header lifecycle badge (own `.kairos-lifecycle-badge` dashed-outline class + state colors: published=OK, review=GOLD, else MUTED); LifecyclePanel ("editorial state — not board status" caption, free-transition select + Set, server-gated); api::set_lifecycle.
  - Tests updated: tenant_provisioning (3 definitions copied, no 'status', scope rows asserted, 15 enum options, 24 tenant tables incl. metadata_definition_scopes, upgrade-path re-pinned to the T-0078 migration); write_path (stamp set minus status; lifecycle arc: born draft → published, activity row, no version bump, no-op logs nothing; smuggled task-scoped template association does NOT stamp — the kairos-db enforcement AC); meta.rs (document_type on task 422 with message, same write on document OK, catalog filter per type incl. complexity-excludes-initiatives, bad entity_type 422, lifecycle arc: draft default, publish, free transition back, bob 403 manage_documents, bad value 422) + client list_metadata_definitions_for.
  - e2e: smoke picker assertions updated for scoping (Complexity offered, Priority stamped as editor, Document Type absent); NEW lifecycle.spec.ts (badge draft → publish via panel, panel distinct from metadata, document carries Document Type as editor + Priority in picker).
  - ADR KAIROS-A-0018 created and populated: two status vocabularies, documents-never-on-boards, entity-scoped metadata, seeded scopes, migration/backfill consequences.
- 2026-08-25 (final): Full ladder green — unit, integration (30 targets), e2e (API golden path + MCP + 6 Playwright specs incl. the new lifecycle.spec, no retries). Integration iterations were all stale-count re-pins from retiring the 4th definition: models_roundtrip (4→3 system definitions), public_migrations (8→9 public tables), tenant_provisioning (template associations 9→6), write_path (D-code sequence shift from the new scope-filter fixture). UAT restore pending a Docker Desktop restart (unrelated environment hiccup at teardown) — will re-boot the demo when the daemon returns.