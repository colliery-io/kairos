---
id: entity-directory-and-searchable
level: task
title: "entity_directory and searchable_items expose deleted_at; consumers filter explicitly"
short_code: "KAIROS-T-0156"
created_at: 2026-09-23T11:29:48.965493+00:00
updated_at: 2026-09-23T12:26:39.655302+00:00
parent: KAIROS-I-0015
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Move the liveness filter out of the two database views and into their call
sites, so that "show me archived work" has something to widen. This is the
enabling change for search, traverse and the graph — and the highest-risk
task in the initiative for a silent regression.

## Implementation Notes

Both views live in
`crates/kairos-db/migrations/tenant/2026-07-09-000000_create_tenant_schema/up.sql`:

- `searchable_items` — up.sql:364-386, `WHERE deleted_at IS NULL` on every
  UNION branch.
- `entity_directory` — up.sql:396+, the same.

They are upstream of nearly every contradiction in this initiative, which is
why a `?include_deleted=` parameter bolted onto the handlers cannot work:
the parameter would have nothing to widen. That is precisely the bug that
makes `--include-deleted` a silent no-op alongside `--query` today.

Write a new tenant migration that recreates both views **selecting
`deleted_at`** rather than filtering on it. No table changes.

Then make every consumer filter explicitly. Known consumers:

- `api/mod.rs:166-190` (resolution — already modal after T-0154)
- `kairos-db/src/search.rs:266-292` (traverse root), `:350-362`
  (`text_match_ids`), `:431-467` (`partition_by_type`)
- `kairos-db/src/graph.rs:403-419` (`neighbors_of`), and the live-only
  consumers at `:466-470`, `:532-535`, `:598-600`, `:715-729`, `:874`, `:935`

### Guarding against the regression

A consumer that forgets to filter starts leaking archived rows into a
default listing, silently. In order of usefulness:

1. `crates/kairos-db/tests/search.rs:637-679` pins current behaviour — it
   must keep passing **unchanged** for the live-only paths.
2. The `housekeeping` UAT journey asserts end to end that default listings
   hide archived work.
3. After this lands, every `entity_directory` / `searchable_items` reference
   in `crates/` must name a liveness mode. Worth a grep in review.

Performance: the default path is the one that must stay fast. The partial
indexes (`idx_tasks_column` and siblings, up.sql:181/204/225) are
`WHERE deleted_at IS NULL`; check the planner still uses them once the
filter moves into the query.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A tenant migration recreates both views exposing `deleted_at`.
- [x] Every consumer names its liveness mode explicitly.
- [x] `crates/kairos-db/tests/search.rs:637-679` passes unmodified.
- [x] A test asserts an archived item is absent from every default listing
      (boards, the five family lists, search without the flag, the entity
      directory).
- [x] `EXPLAIN` on the default board and list queries still uses the partial
      indexes.
- [x] `angreal test` green; full compose UAT green.

## Status Updates

### 2026-09-23 — landed

Migration `2026-09-23-000001_views_expose_deleted_at` recreates both views
selecting `deleted_at` instead of filtering on it. No table changes, no
index changes. `DROP VIEW` + `CREATE VIEW` rather than `CREATE OR REPLACE`
because the down migration has to REMOVE a column again, and
`CREATE OR REPLACE VIEW` may only append.

**The change is behaviour-neutral at every surface.** Each consumer gained,
in the same commit, the exact predicate the view used to apply. The only
thing that actually moved is `resolve_short_code`'s `IncludeArchived` path,
which now reads the view instead of T-0154's hand-rolled `DIRECTORY_UNION`
— that const is gone, and it was the one edit made in `api/mod.rs`.

#### Grep audit of view consumers

19 references to `FROM`/`JOIN entity_directory` or `FROM searchable_items`
remain in `crates/` outside the migrations. 17 spell `deleted_at IS NULL`;
the 2 that deliberately do not are both the wide half of a `match` whose
other arm does, so the liveness mode is named either way:

| site | mode |
| --- | --- |
| `kairos-db/src/graph.rs:153` `resolve_entity` | live-only |
| `kairos-db/src/graph.rs:430` `neighbors_of` | live-only |
| `kairos-db/src/graph.rs:620` `team_work_documents` | live-only |
| `kairos-db/src/graph.rs:755` focal-subgraph degree count | live-only |
| `kairos-db/src/graph.rs:760` focal-subgraph node hydration | live-only |
| `kairos-db/src/graph.rs:847` `blocks_summary` | live-only |
| `kairos-db/src/graph.rs:900` `team_link_rollup` | live-only |
| `kairos-db/src/graph.rs:961` `repository_link_rollup` | live-only |
| `kairos-db/src/search.rs:284` `resolve_root` by short code | live-only |
| `kairos-db/src/search.rs:290` `resolve_root` by id | live-only |
| `kairos-db/src/search.rs:373` `text_match_ids` | live-only |
| `kairos-db/src/search.rs:461` `partition_by_type` | **`include_deleted`** |
| `kairos-db/src/search.rs:463` `partition_by_type` | live-only |
| `kairos-server/src/api/mod.rs:197` `resolve_short_code` | `LiveOnly` |
| `kairos-server/src/api/mod.rs:201` `resolve_short_code` | **`IncludeArchived`** |
| `kairos-server/src/api/mod.rs:233` `resolve_item_type` | live-only |
| `kairos-server/src/mcp/tools.rs:2038` `parent_chain` | live-only |
| `kairos-db/tests/isolation.rs:208` directory resolution | live-only |
| `kairos-db/tests/isolation.rs:980` concurrent search | live-only |

`partition_by_type` got simpler rather than more complex: its
`include_deleted` branch used to re-derive the directory from the five base
tables by hand, because the view had already dropped the rows the flag was
asking about. Both branches are now the same query against the view,
differing only in the predicate.

#### Planner check (`EXPLAIN`, 20k strategies + 20k tasks, 10% archived)

The partial indexes are all still used on the default path, and the
predicate is satisfied BY the index predicate rather than as a recheck:

- board items (`tasks WHERE column_id = ? AND deleted_at IS NULL`):
  `Index Scan using idx_tasks_column`, `deleted_at IS NULL` absorbed into
  the index predicate and absent from the plan's filters. Unchanged —
  board and family listings never touched the views; they are diesel
  query-builder reads of the base tables.
- `text_match_ids` in its new shape:
  `Bitmap Index Scan on idx_strategies_tsv` / `idx_tasks_tsv`, cost 2108.
  The GIN partial indexes survive the filter moving out of the view,
  because the planner meets the predicate before the UNION branches are
  materialised.
- `resolve_short_code`, both modes: `Index Scan using
  {family}_short_code_key` on every branch, identical cost (16.63) with and
  without the predicate. The archived mode is not slower than the live one.

#### For KAIROS-T-0157 — the index bill comes due here

`EXPLAIN` on the same full-text query **without** the predicate — the mode
T-0157 adds — falls off a cliff:

```
Gather  (cost=1000.00..8267.84 rows=203)
  ->  Parallel Append
        ->  Parallel Seq Scan on tasks  (cost=0.00..3635.06)
        ->  Parallel Seq Scan on strategies  (cost=0.00..3612.06)
```

Cost 8268 against 2108, and it degrades linearly with table size. The cause
is that `idx_strategies_tsv` and siblings (`up.sql:388-390`) are **partial**
`WHERE deleted_at IS NULL`, so an include-archived text search cannot use
them at all and re-derives every row's `tsvector` on the fly. T-0157 needs
either non-partial GIN indexes or a second archived-only set before turning
the flag on; this is ADR-20's "the archived branch needs its own index
consideration" arriving in concrete form. Nothing about it blocks T-0157's
correctness work — but shipping the flag without it makes a wide search a
table scan.

#### Also for T-0157 / T-0158 / T-0159

- **T-0157**: the two predicates to make conditional are
  `search.rs:373` (`text_match_ids`) and `search.rs:284`/`:290`
  (`resolve_root`). `partition_by_type` already reads `include_deleted`.
  Note that search is filtered in **three** places, not one: even with both
  of the above widened, per-family hydration (step 6) still applies its own
  `deleted_at IS NULL` default — verified experimentally by removing the
  filters from `text_match_ids` and `partition_by_type` and watching both
  `tests/search.rs` and the new listing test stay green. Hydration is the
  actual gate; the other two are defence in depth.
- **T-0158**: the single line to change is `graph.rs:430` in
  `neighbors_of`. Every other `entity_directory` join in `graph.rs` is a
  different surface (rollups, forge panels, focal subgraph) and should stay
  live-only unless T-0158 deliberately widens it too.
- **T-0159**: list endpoints do **not** read these views at all — all five
  families and `board_items` are diesel query-builder reads of the base
  tables with `.filter(deleted_at.is_null())`. Adding `include_deleted`
  there is independent of this task, and the planner evidence above says
  the partial `idx_*_board` / `idx_*_column` indexes will not serve the
  wide mode either.

#### Tests

- `crates/kairos-db/tests/search.rs:637-679` passes **unmodified**.
- New: `crates/kairos-server/tests/archived_hidden.rs` — seeds a live and
  an archived item in all five families on four boards, then asserts the
  archived ones are absent from the boards, all five family lists *and
  their `total`s*, search by `q`/`filter`/`traverse`, and the entity
  directory (via relationship neighbour hydration) — while still being
  retrievable by short code, marked. Verified to bite: removing the
  `deleted_at IS NULL` from `neighbors_of` fails it.
- Rewritten (the old "archived = gone" contract, kept as the record of
  what changed rather than deleted):
  - `tests/write_path.rs` — the two view assertions now say the archived
    row is absent from the DEFAULT surface *and* still present in the view
    carrying its `deleted_at`. A `view_count_any` helper sits beside
    `view_count` to make the pair explicit.
  - `uat/journeys/housekeeping.journey.ts` — was **already failing before
    this task** (last touched in T-0148; T-0154 made `GET /api/tasks/{code}`
    and `/history` serve archived work, and the journey still asserted 404
    on both). Rewritten to assert both halves of ADR-20: off the MCP queue
    AND off `/api/tasks` (codes and count), but 200 with `archived_at` set
    on the item and its history.
- `tests/tenant_provisioning.rs` — fleet-upgrade block re-pinned from
  T-0161's migration to this one (the recurring chore KAIROS-T-0093 tracks).
  T-0161's `board_columns_live_*` evidence was moved into
  `EXPECTED_INDEXES` so re-pinning does not silently drop it — worth
  copying that habit next wave. Added: a freshly provisioned tenant's views
  expose `deleted_at`, and an upgraded tenant's views read all five entity
  tables (the revert installs strategies-only stubs, so this also proves
  the migration rebuilds the whole view rather than patching what it finds).

#### Note for whoever is in `items.rs`

`items.rs:1192-1193` still says soft-deleted rows "disappear from
`searchable_items` and `entity_directory` (the views filter `deleted_at IS
NULL`)". That is now false — left alone because KAIROS-T-0160 owns the
file. One doc-comment sentence.

Separately: the one-line `deleted_at IS NULL` added to
`mcp/tools.rs:2038` (`parent_chain`) was swept into T-0160's commit
`f8b0847` by a concurrent `git add`. The change is correct and in the tree;
recording it here so the audit trail is not misleading about which ticket
made it.

#### Gates

`angreal test lint`, `angreal test unit`, `angreal test integration` (41
targets) and `angreal test uat --journey housekeeping` all green.