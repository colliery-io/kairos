---
id: archived-work-stays-readable
level: initiative
title: "Archived Work Stays Readable - Implementing ADR-20 Across Every Surface"
short_code: "KAIROS-I-0015"
created_at: 2026-09-23T11:21:34.421605+00:00
updated_at: 2026-09-23T11:21:34.421605+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/discovery"


exit_criteria_met: false
estimated_complexity: L
initiative_id: archived-work-stays-readable
---

# Archived Work Stays Readable - Implementing ADR-20 Across Every Surface Initiative

# Archived Work Stays Readable - Implementing ADR-20 Across Every Surface Initiative

## Context

[[KAIROS-A-0020]] decided what archiving means: **this is old, so it is
hidden by default, and nothing more.** Today the product implements a much
stronger thing — archived work is *unreachable* — and this initiative closes
that gap.

### The ground truth (surveyed 2026-09-23)

There is exactly **one** soft-delete mechanism: a nullable `deleted_at`,
filtered with `.is_null()` at ~221 call sites in `crates/`. There is exactly
**one** "include deleted" parameter in the entire product —
`SearchFilter.include_deleted` (`crates/kairos-core/src/search.rs:155`) —
and it reaches exactly **one** code path, the search pipeline. Everything
else resolves through live-only views and 404s.

Families with `deleted_at` (`crates/kairos-db/src/schema.rs`): `adrs`:37,
`boards`:96, `delivery_streams`:108, `documents`:124, `forge_connections`:136,
`initiatives`:157, `repositories`:251, `strategies`:280, `tasks`:299,
`team_pages`:359, `teams`:371. Notably **hard-deleted** (no `deleted_at` at
all): `metadata_definitions`, `templates`, `item_metadata`,
`item_relationships`, `board_columns`, `api_keys`, `activity_log`,
`item_history`.

**Two database views are the choke points**, both in
`crates/kairos-db/migrations/tenant/2026-07-09-000000_create_tenant_schema/up.sql`:
`searchable_items` (up.sql:364-386) and `entity_directory` (up.sql:396+),
each with `WHERE deleted_at IS NULL` on every UNION branch. Almost every
contradiction below traces back to one of these two.

### Where the product contradicts ADR-20

Ranked by how badly they break the rule — these are the worklist:

1. **Item GET by short code 404s.** `api/mod.rs:166-190 resolve_short_code`
   (queries `entity_directory`) and `api/mod.rs:211-215 short_code_not_found`,
   whose message literally reads *"no live {entity_type} with short code"*.
   Plus five per-family `load()` fns: `api/tasks.rs:114`,
   `api/strategies.rs:52`, `api/initiatives.rs:53`, `api/documents.rs:111`,
   `api/adrs.rs:49`.
2. **`/history` 404s** — `api/meta/history.rs:60` via
   `api/meta/mod.rs:205-219`. Note the 404 comes purely from *resolution*:
   the `item_history` rows are never liveness-filtered (history.rs:63-88).
   This is the cheapest high-value fix in the initiative.
3. **MCP `get_item` / `get_history` error out** — `mcp/tools.rs:1377-1520`
   and `:804`.
4. **The GUI has no archived view whatsoever.** `pages/search/data.rs:67-69`
   says the filter builder cannot even express `include_deleted`; `app.rs:55-80`
   has no archive route. `/items/:code` renders an error for archived work.
5. **Full-text search can never see archived items.** `q` matches only
   through `searchable_items` (live-only), and the candidate sets are
   *intersected* (`kairos-db/src/search.rs:159-187`), so archived ids are
   dropped **before** `include_deleted` is consulted at search.rs:187. The
   flag is a silent no-op whenever `q` is set — nothing validates or rejects
   the combination. **This is the mechanism behind the
   `--include-deleted` + `--query` limitation** the UAT arc found.
6. **Traverse from an archived root 404s** — `kairos-db/src/search.rs:266-292`
   (`resolve_root` → `entity_directory`), not overridable by the flag.
7. **Archived items vanish from LIVE items' relationship lists** —
   `graph.rs:403-419 neighbors_of` JOINs `entity_directory`. Worth calling
   out separately: this degrades the *live* side of the record, not just the
   archived side. "What did this initiative contain?" silently loses rows.
8. **Metadata on an archived item is unreadable and unclearable, yet still
   counted** — `api/meta/metadata.rs:61` and `:100`, against the
   `DEFINITION_IN_USE` guard at `api/meta/definitions.rs:463-486` (which
   joins no entity table, so it considers liveness not at all). This is
   [[KAIROS-T-0152]].
9. **`count_items_in_column` counts archived rows** —
   `kairos-db/src/boards.rs:868-885`, called from `boards.rs:695`. An
   archived card permanently pins its board column. Deliberate once (the
   note at `api/org/mod.rs:117-118` reasons that re-parenting rows is a
   different invariant), but it deserves re-examination under ADR-20.
10. **No `include_deleted` on any list endpoint** — the five family lists
    (`tasks.rs:148/153` and siblings) and `/api/boards/{id}/items`
    (`org/boards.rs:440-446`) have no opt-in at all, so "visible when asked
    for explicitly" has nowhere to hang.

### What already complies

- `GET /api/activity` (`api/meta/activity.rs:81-118`) — no liveness filter
  at all, which is why it was the only thing left after archiving.
- `POST /api/search` for **filter-only** requests
  (`kairos-db/src/search.rs:431-467`, hydration at :544/:582/:621/:672/:702).

### Two things the survey changed about the decision

- **The retention sweeper does not run.** `spawn_retention_loop`,
  `sweep_all_tenants` and `sweep_tenant` (`kairos-db/src/retention.rs:293-523`)
  have **zero references in `crates/kairos-server/`** — only db tests call
  them, and the doc comment says server wiring is an M2 task. Even wired, it
  purges `item_history` and `activity_log` only, never soft-deleted entity
  rows. So nothing destroys archived content today, and ADR-20 makes that
  permanent by design rather than by accident.
- **"Archived" is already taken.** The document editorial lifecycle
  (KAIROS-T-0078) is `draft | review | published | archived`, unrelated to
  `deleted_at` — a published document can be editorially archived while
  staying live. `kairos-web/src/pages/item.rs:269,315` uses it in that
  sense. The user-facing noun for this state is therefore still open.

### Constraints worth knowing before designing

- **ABAC resolves through live-only lookups.** `abac.rs:339/355/394/425`
  means `resolve_authorization_board` returns `None` for an archived item,
  so capability resolution silently falls back to the tenant-wide org-admin
  policy. Serving archived items without fixing this would make archived
  work *more* restricted than live work, violating ADR-20 rule 4.
- **`SearchFilter::is_constraining()`** (`kairos-core/src/search.rs:158-173`)
  deliberately excludes `include_deleted` from the "does this narrow
  anything" test, so a filter carrying only that flag is a 400 today.
- **Archived items cannot be mutated at all** — `items.rs:325` (content
  update), `:650`, `:702`, `:792` (field setters) all require liveness.
  ADR-20 rule 4 and [[KAIROS-T-0152]]'s first criterion require at least
  `set_metadata` to reach archived work; how much further write access goes
  is a design question, not a given.
- Existing test that pins current search behaviour:
  `crates/kairos-db/tests/search.rs:637-679`.

## Goals & Non-Goals

**Goals:**
- An archived item and its history are retrievable, marked as archived, on
  every surface a person or agent uses: API, MCP, CLI, GUI.
- Archived work is findable by search — including full-text — when asked
  for explicitly, and stays out of results when not.
- Default listings (boards, queues, directories) are byte-for-byte
  unchanged in behaviour and no slower.
- `DEFINITION_IN_USE` names its blockers and they can be cleared.
- The `housekeeping` UAT journey flips from asserting 404 to asserting
  retrievability, and the arc's claims about archiving become true.

**Non-Goals:**
- Renaming "delete" to "archive" on the wire, in CLI nouns or in GUI copy.
  Real, but separate, and complicated by the lifecycle collision above.
- Wiring the retention sweeper. Adjacent and now more important, but it is
  a different question (erasure), not this one (visibility).
- Changing KAIROS-I-0012's team/repository guards. ADR-20 rule 5 keeps them
  counting live rows; they are already correct.

## Detailed Design

*Pending — the scope questions below go to Dylan before this is decomposed.*

### Open questions for the decision maker

1. **How far in one initiative?** The read path alone (items, history, MCP,
   CLI) is the smallest thing that makes audit work and needs no migration.
   Search and the graph need the two views changed, which is a migration.
   The GUI needs a way to ask, which is new UI.
2. **The column guard** (`boards.rs:868-885`): should a column holding only
   archived cards be removable? Today it is pinned forever. Keeping it
   pinned is consistent with "archived work is still content"; unpinning it
   is consistent with "archived work is not live work". Both readings of
   ADR-20 are available and they disagree.
3. **How much write access does archived work get?** Rule 4 says archiving
   is not a permission boundary, which argues for full editability.
   [[KAIROS-T-0152]] only strictly needs `set_metadata` to reach it.

## Alternatives Considered

*Pending design.*

## Implementation Plan

*Pending design.*

## Progress Log

- 2026-09-23: Created from [[KAIROS-A-0020]]. Survey of all ~221 `deleted_at`
  call sites complete and recorded above; the contradiction list is the
  worklist. Held in discovery pending Dylan's answers to the three scope
  questions — no decomposition yet.
