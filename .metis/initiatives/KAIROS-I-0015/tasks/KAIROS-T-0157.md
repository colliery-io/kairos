---
id: search-honours-include-deleted
level: task
title: "Search honours include_deleted alongside a text query and from an archived root"
short_code: "KAIROS-T-0157"
created_at: 2026-09-23T11:29:51.232959+00:00
updated_at: 2026-09-23T12:54:22.735576+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0156]
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

Make `include_deleted` mean what it has always claimed to mean: findable by
full-text search, and traversable from an archived root. Today it is a
silent no-op whenever a text query is present.

## Implementation Notes

**Blocked by [[KAIROS-T-0156]].**

### The intersection bug

`crates/kairos-db/src/search.rs:159-181` builds candidate id sets from
`q`, traverse and metadata and **intersects** them. `q` matched only through
`searchable_items` (live-only), so archived ids were never in the `q` set and
the intersection dropped them **before** `include_deleted` was consulted at
`search.rs:187`. Nothing validated or rejected the combination — it just
quietly returned less.

With T-0156 the `q` set can contain archived ids, so the flag governs at the
point it always intended to. Verify the ordering explicitly rather than
assuming it falls out.

### `is_constraining`

`crates/kairos-core/src/search.rs:158-173` deliberately excludes
`include_deleted` from the "does this filter narrow anything" test, so a
filter carrying only that flag is a 400. Under [[KAIROS-A-0020]] "show me
archived work" is a legitimate whole query. Include it, and check the
at-least-one-capability rule still rejects a genuinely empty filter.

### Traverse root

`search.rs:266-292 resolve_root` resolves through the view and returns
`SearchError::TraverseRootNotFound` → 404 (`api/search.rs:88-94`). It should
honour the flag: traversing *from* archived work is exactly the audit
question ("what hung off this once?").

### Hydration

`search.rs:544/582/621/672/702` already apply `deleted_at IS NULL` only
`if !filter.include_deleted`; `:560/601/652/682/718` are the unconditional
live-only default. Confirm both still behave after the view change.

Results must mark archived hits — see the marking rule in the initiative.

## Acceptance Criteria

- [x] `q` + `include_deleted` returns archived matches; `q` alone does not.
- [x] `kairos search --query X --include-deleted` matches the API.
- [x] A filter carrying only `include_deleted` is accepted, not 400.
- [x] Traverse from an archived root returns its subgraph instead of 404.
- [x] Archived hits are marked in the response.
- [x] `crates/kairos-db/tests/search.rs:637-679` still passes.
- [x] The UAT `operations` journey's note about the limitation
      (`uat/journeys/operations.journey.ts:197-199`) is updated — it
      currently documents the no-op as expected behaviour.

## Status Updates

### 2026-09-23 — landed

Three predicates, one migration, and a validator that had been rejecting
the only way to ask the question.

#### The three predicates

`include_deleted` is read once in `execute_search` and now reaches all
three places search filters liveness, in the order the pipeline runs them:

| site | before | after |
| --- | --- | --- |
| `text_match_ids` (`search.rs:372`) | always `deleted_at IS NULL` | conditional |
| `resolve_root` (`search.rs:284/291`) | always `deleted_at IS NULL` | conditional |
| `partition_by_type` (`search.rs:466`) | already conditional (T-0156) | unchanged |
| per-family hydration (`:560/601/652/682/718`) | already conditional | unchanged |

Hydration was already right, which is exactly why the bug was invisible:
the flag governed the last gate but the `q` set — built live-only and
**intersected** at step 4 — had already dropped the archived ids before
hydration ever saw them. The caller was told "no matches" rather than "not
supported", which is the worse of the two failures. Ordering verified
explicitly rather than assumed: the new db test asserts `q` alone returns
nothing for the archived row and `q + include_deleted` returns exactly it.

`is_constraining` (`kairos-core/src/search.rs`) now counts
`include_deleted`. The old comment was right that it widens rather than
narrows — but the at-least-one-capability rule exists to reject a request
that asked for *nothing*, not one that asked for something broad, and
"show me the archived work" is a whole question under ADR-20. A genuinely
empty `filter {}` is still a 400; there is a test for each.

#### Index choice: (a), non-partial — measured

20k strategies + 20k tasks, 10% archived, PostgreSQL 16 in the compose
stack. **The corpus matters**: T-0156's numbers came from text where every
row matched the term, which measures nothing — the planner is right to
seq-scan that either way. This corpus spreads 200 topic words so a term is
selective (~0.5% of rows), which is the case a GIN index exists for.

| | partial (before) | non-partial (after) |
| --- | --- | --- |
| default mode | cost 593.91, 0.474 ms, 206 buffers | cost 644.14, 0.509 ms, 206 buffers |
| include archived | cost 8905.84, **133.281 ms**, 1875 buffers | cost 644.24, **0.094 ms**, 206 buffers |

Before: `Gather → Parallel Append → Parallel Seq Scan on tasks/strategies`,
re-deriving every row's tsvector, degrading linearly with the table.
After: `Bitmap Index Scan on idx_strategies_tsv` / `idx_tasks_tsv` in
**both** modes. 13.8x by cost, ~1400x by wall clock.

What the default path gives up is having `deleted_at IS NULL` absorbed INTO
the index predicate; the planner now applies it as a heap `Filter` instead.
That is +8.5% on a cost estimate, identical buffers, and no measurable
time. Index size grew 1528 kB → 1648 kB on strategies (+7.8%) — proportional
to the archived share, as expected. (Sizes compared after `REINDEX`: a GIN
index built before a bulk insert carries an unmerged pending list and reads
3x larger either way.)

Option (b), a second archived-only index set, was rejected on the argument
the notes anticipated: archived rows are read-only under ADR-20 / D5 —
every mutating path loads `LiveOnly` — so they are written exactly once, at
archive time, and the write-amplification case for keeping the hot index
narrow barely applies. One index per table beats ten. Option (c) is
indefensible at 133 ms and linear.

Migration `2026-09-23-000002_tsv_indexes_cover_archived` drops and recreates
the five `idx_*_tsv` without the predicate. The `to_tsvector` expression is
repeated verbatim: it must match `searchable_items`' own expression
character for character or the planner stops recognising the index.
Re-measured after provisioning a tenant through the migration rather than
by hand — same plan, both modes index-served.

#### Marking

The API already marked archived hits: every entity DTO carries
`archived_at` (T-0154/T-0155's `convert.rs`), and search returns the same
DTOs. Asserted rather than assumed now. The two surfaces that did NOT mark
them do now:

- **MCP** `render_search_results` — `[archived]` per line, all five groups.
  An agent that cannot tell retired work from live work will try to write
  to it and be refused by every path with no idea why.
- **CLI** `search`'s human table — `[archived]` on the title cell,
  alongside the banner `kairos <family> get` already prints.

#### Tests

- **New** in `kairos-db/tests/search.rs`: the `q` pair, `include_deleted`
  alone, and the traverse-root pair. `:637-679` passes **unmodified** —
  the block is inserted after it.
- **New** in `kairos-server/tests/search_endpoint.rs`: the same four
  behaviours over HTTP, plus `archived_at` present on the hit.
- **Rewritten** (old "archived = gone" contract, kept as the record of what
  changed): `search_endpoint.rs`'s "a widening-only filter does not count
  as a capability" is now "a filter that says NOTHING is still no
  capability", probed with `{"filter": {}}` on the wire.
- **`archived_hidden.rs` passes untouched** — the default path is not
  weakened anywhere.
- **Trap found**: `soft_delete_item` **cascades**. Archiving an initiative
  to test the traverse root also archived its children, which proves
  nothing about the ROOT lookup and broke a later pagination assertion.
  Both tests stamp `deleted_at` directly for that probe and say why.
- `uat/journeys/operations.journey.ts`: the `--include-deleted` + `--query`
  no-op note is gone, replaced by the behaviour. The same step also still
  asserted `404` on GET for every cascaded child — stale since T-0154, the
  seventh instance of the old contract — rewritten to `200` + `archived_at`.
  `angreal test uat --journey operations` green.
- `tests/tenant_provisioning.rs`: fleet-upgrade block re-pinned from
  T-0156's migration to this one. T-0156's five-table view check moved UP
  into the freshly-provisioned assertions rather than being deleted, and a
  note in the block tells the next re-pinner to do the same. This task's own
  evidence (`no idx_*_tsv is partial`) is asserted in BOTH places, since an
  index name alone does not carry its predicate and `EXPECTED_INDEXES` only
  holds names. (KAIROS-T-0093 is the root cause.)

#### Gates

`angreal test lint`, `angreal test unit`, full `cargo test --workspace
--test '*'` (all targets), and `angreal test uat --journey operations` all
green.

#### For KAIROS-T-0159 and KAIROS-T-0165

- **T-0159**: the partial-index problem is the same shape one table over.
  `idx_tasks_board` / `idx_tasks_column` and siblings are all
  `WHERE deleted_at IS NULL`, so an `include_deleted` list endpoint or
  board view will not use them either. The fix here is the precedent, but
  the trade is NOT identical: those indexes are on the hot default path for
  every board render, so measure before making them whole — a second,
  archived-only index may win where it lost for full text.
- **T-0159**: `kairos-client/src/types.rs:446` already carries
  `include_deleted` on the list query DTO; the server side is what is
  missing.
- **T-0165**: the MCP `search` tool description and `SearchFilterParams`
  now say `include_deleted` composes with everything and that hits come
  back marked. No tool count change (17 → 17); `restore_item` is still the
  18th the drift gate is waiting on.
- Anyone touching `mcp/tools.rs`: `archived_marker()` is there now. T-0158
  landed two inline copies of the same three lines in the same file while
  this was in flight — worth collapsing into one call when the dust
  settles.

## Notes carried in from [[KAIROS-T-0156]] — read before starting

**2026-09-23. Two findings that change this task's size.**

### 1. The index bill is unpaid, and it is the real work here

`idx_strategies_tsv`, `idx_initiatives_tsv`, `idx_tasks_tsv`,
`idx_documents_tsv`, `idx_adrs_tsv` (up.sql:386-390) are **partial** GIN
indexes, `WHERE deleted_at IS NULL`. So a text search that includes archived
work **cannot use them at all**: measured on 20k strategies + 20k tasks with
10% archived, the same query costs **8268 without the predicate versus 2108
with it**, falling back to a parallel seq scan that re-derives every row's
tsvector — and it degrades linearly with table size.

Turning the flag on without paying this makes archived text search a
tenant-wide performance hazard rather than a feature. Options:

- **(a) Make the five GIN indexes non-partial.** One index per table serving
  both modes. Costs index size (archived rows are typically a small
  fraction) and a little write amplification — but archived rows are
  read-only (KAIROS-A-0020 / D5), so they are written exactly once, at
  archive time. **Recommended** unless measurement says otherwise.
- **(b) A second, archived-only GIN index set.** Five more indexes; keeps
  the hot path's index smallest, at the cost of doubling the objects to
  maintain and reason about.
- **(c) Accept the seq scan.** Only defensible if archived text search is
  rare AND tenants stay small; the linear degradation says otherwise.

Measure and state which you chose. Whichever it is, `EXPLAIN` both modes on
a seeded table and record the numbers, as T-0156 did.

### 2. Search filters liveness in THREE places, not one

Widening `text_match_ids` and `partition_by_type` is **not sufficient** —
verified experimentally. Per-family **hydration** (step 6 in the pipeline,
`search.rs:544/582/621/672/702`, with the unconditional live-only defaults
at `:560/601/652/682/718`) applies its own filter, and is the actual gate.

Also from T-0156: `resolve_root` for traverse is at `search.rs:284/290`.