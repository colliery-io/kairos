---
id: search-honours-include-deleted
level: task
title: "Search honours include_deleted alongside a text query and from an archived root"
short_code: "KAIROS-T-0157"
created_at: 2026-09-23T11:29:51.232959+00:00
updated_at: 2026-09-23T11:29:51.232959+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0156]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] `q` + `include_deleted` returns archived matches; `q` alone does not.
- [ ] `kairos search --query X --include-deleted` matches the API.
- [ ] A filter carrying only `include_deleted` is accepted, not 400.
- [ ] Traverse from an archived root returns its subgraph instead of 404.
- [ ] Archived hits are marked in the response.
- [ ] `crates/kairos-db/tests/search.rs:637-679` still passes.
- [ ] The UAT `operations` journey's note about the limitation
      (`uat/journeys/operations.journey.ts:197-199`) is updated — it
      currently documents the no-op as expected behaviour.

## Status Updates

*To be added during implementation*

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
