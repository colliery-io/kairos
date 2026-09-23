---
id: list-endpoints-and-board-items
level: task
title: "List endpoints and board items gain an include_deleted opt-in"
short_code: "KAIROS-T-0159"
created_at: 2026-09-23T11:29:55.870120+00:00
updated_at: 2026-09-23T13:15:54.986793+00:00
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

Give "visible when asked for explicitly" somewhere to hang on the list
surfaces. Today the five family lists and the board items endpoint have no
opt-in at all.

## Implementation Notes

**Blocked by [[KAIROS-T-0156]].**

- Family lists: `api/tasks.rs:148/153` and the same pair in
  `strategies.rs:82/87`, `initiatives.rs:83/88`, `documents.rs:151/156`,
  `adrs.rs:79/84` — count and page both filter.
- Board items: `api/org/boards.rs:461-600`, with the per-family filters at
  `:507`, `:520`, `:533`, `:571`. `BoardItemsQuery` (`boards.rs:440-446`)
  carries only `repository`.

Add `include_deleted` (match the existing spelling on `SearchFilter` — a
product-wide rename is explicitly out of scope, see the initiative's
non-goals). **Default false everywhere.**

The count and the page must agree. A list that reports 40 results and
returns 12 is a worse bug than the one being fixed.

MCP `board_items` (`mcp/tools.rs:621-624`, rows at `:1712-1810`) and
`list_boards` counts (`column_item_counts`, `tools.rs:1825-1848`) take the
same argument. Adding an argument to an existing tool does not change the
drift gate's tool count.

CLI: the entity-family list commands in `commands/entities.rs` gain the flag.
A verb-level flag, not a new noun — the gate's noun count is unaffected.

## Acceptance Criteria

- [x] All five family lists and `/api/boards/{id}/items` accept
      `include_deleted`, default false.
- [x] Counts and pages agree under both settings.
- [x] Archived rows are marked in list payloads.
- [x] MCP `board_items` and the CLI list commands expose it.
- [x] A test asserts default behaviour is byte-identical to before.
- [x] `angreal test` green.

## Status Updates

**2026-09-23 — the shape of the opt-in.**

`include_deleted` is carried by a NEW DTO, `dto::ListQuery` (limit, offset,
include_deleted), not by `dto::Pagination`. `Pagination` is shared with six
listings that have no archived mode — boards, teams, members, tenants,
templates, streams — and widening it would have advertised a parameter
those endpoints ignore. That is worse than not having one, because a reader
would believe they had asked.

`impl From<Pagination> for ListQuery` means the ~40 existing
`list_tasks(Pagination::default())` call sites across the test suites keep
compiling and keep meaning exactly what they meant: live rows only. The
client method now takes `impl Into<ListQuery>`.

Server-side the flag becomes the existing `Liveness` enum at the door —
`clamp_list()` returns `(limit, offset, liveness)` as one value, so the
`count` and the paged `load` cannot be given different answers. Inside each
handler the predicate is a closure used twice:

```rust
let visible = || {
    let mut query = dsl::tasks.into_boxed();
    if liveness == Liveness::LiveOnly { query = query.filter(dsl::deleted_at.is_null()); }
    query
};
let total = visible().count()...;
let rows  = visible().order(...)...;
```

The CLI flag went on a new `EntityListArgs` for the same reason the DTO did:
the shared `ListArgs` also serves `boards|teams|members|streams|admin`.
`kairos boards list --help` is byte-identical to before; `kairos tasks list
--help` gained one line.

**2026-09-23 — two things the brief did not call out, both found by doing it.**

1. **Widening the cards without widening the COLUMNS drops the oldest audit
   rows silently.** An archived card keeps a `NOT NULL` FK to the column it
   was put away in, and that column may itself have been removed since
   (KAIROS-T-0161). Both listings bucket rows by column id, so a card whose
   column is missing from the map is fetched and then never rendered — no
   error, just absence. `board_items` now calls
   `load_columns_including_removed(.., include_deleted)`, and MCP grew a
   `board_columns_including_removed`.

2. **`column_item_counts` (the per-column numbers `list_boards` prints)
   stays live-only on purpose.** It shares `board_item_rows` with
   `board_items`, so the liveness had to become a parameter rather than
   widen by accident: ADR-20 rule 5 says archived work is not live work, and
   those counts answer "how much is on this board?". Same reasoning keeps
   the children-progress rollup and the blocks summary in
   `/api/boards/{id}/items` unwidened — the brief's "archived rows are
   marked, not disguised" cuts both ways.

Marking was free on the API (`archived_at` is already on all five DTOs and
already serialized by `IntoDto`, absent while live — confirmed on the list
path). MCP renders ` [archived]` after the row; the CLI appends
`[archived]` to the CODE cell rather than adding a column, so a default
table is unchanged.

**2026-09-23 — EXPLAIN, both modes, and an index change (landed, with numbers).**

Corpus: 200k tasks over 20 boards (10k each), 15% archived, scattered
UNIFORMLY. The first corpus keyed `deleted_at` off the same counter that
chose the board, so whole boards came out with zero archived rows and the
comparison measured nothing — the same methodological trap T-0157 found in
T-0156's benchmark. Numbers below are from the reseeded corpus, warm cache,
PostgreSQL 16, `EXPLAIN (ANALYZE, BUFFERS)`.

*Family lists — the widened mode is CHEAPER, no index question at all:*

| query | default (live only) | include_deleted |
|---|---|---|
| `count(*)` | 19.9 ms, 4878 buf | **11.2 ms**, 4878 buf |
| page, `ORDER BY short_code LIMIT 50` | 0.043 ms, 5 buf | **0.022 ms**, 5 buf |
| page, `OFFSET 100000` | 22.3 ms, 3324 buf | **16.7 ms**, 2827 buf |

Both modes use `tasks_short_code_key`; the live mode pays for a filter the
wide mode does not have. Nothing to do here.

*Board items — the one regression, and it was a full table scan:*

| | default | include_deleted |
|---|---|---|
| shipped (partial `idx_tasks_board`) | 3.97 ms, 2181 buf | 29.7 ms, **10440 buf** (Seq Scan) |
| (a) `idx_*_board` made non-partial | 3.97 ms, 2190 buf | 21.3 ms, 2848 buf |
| (b) partial KEPT + 2nd full index | 3.99 ms, 2181 buf | 20.1 ms, 2848 buf |

The partial index cannot serve a query that declines its predicate, so the
wide mode was O(tenant) where a board listing should be O(board) — the cost
of reading one team's archived cards grew with every other team's live ones.

(a) and (b) are identical within noise on BOTH paths. (b) costs an extra
index to maintain on every write plus 1400 kB; (a) costs 264 kB (1136 →
1400 kB, +23%) and nothing else. **Landed (a)** as
`migrations/tenant/2026-09-23-000003_board_indexes_cover_archived`, on
`idx_strategies_board` / `idx_initiatives_board` / `idx_tasks_board`.

T-0157's caution that these sit on the HOT path, unlike the tsv indexes,
was the right question and the answer came out "no cost": the default plan
is a Bitmap Heap Scan either way, so it was already fetching the heap tuple
and `deleted_at IS NULL` costs the same as a heap filter as it did folded
into the index predicate. 3.97 ms vs 3.97 ms, 2181 vs 2190 buffers.

`idx_*_column` stays partial deliberately — nothing queries by column (both
listings bucket in Rust), and leaving a narrow index available to the
planner costs the default path nothing. ADRs have no board index to widen;
that listing already scanned in both modes, unchanged by this task.

An archived-only index (`WHERE deleted_at IS NOT NULL`) was rejected on
mechanism, not cost: the widened query says nothing at all about
`deleted_at`, and Postgres does not reason that two complementary partial
indexes together cover a table. Neither half would ever be used.

**2026-09-23 — tests.**

`tests/archived_hidden.rs` extended, not weakened. Sections 1–5 are
untouched; three new sections:

- **6.** The five family lists under `include_deleted=true`: both rows come
  back, `archived_at` is set on exactly the archived one, `total` is 2, and
  `items.len() == total` is asserted per family under BOTH settings.
- **7.** The same on the board, per board, including that the widened board
  is a superset of the default one.
- **8.** The default is a no-op: `/api/{family}?limit=200` and
  `…&include_deleted=false` return identical bodies, and likewise for the
  board. That is the "byte-identical to before" gate — it proves the new
  parameter changes nothing when nobody asks.

`tests/tenant_provisioning.rs`: the fleet-upgrade rehearsal was re-pinned
from T-0157's migration to this one (the hand-maintenance chore
KAIROS-T-0093 exists to remove), and T-0157's evidence was carried up into
the freshly-provisioned assertions rather than deleted, per the note in
that block. Two new assertions: no `idx_*_board` is partial in a fresh
tenant, and all three are whole in an UPGRADED one.

`uat/journeys/housekeeping.journey.ts` gained a step: the quarter comes
back by asking for it, on `/api/tasks?include_deleted=true` (count and page
agree, marked) and on MCP `board_items` with `include_deleted: true`
(rendered `[archived]`). The existing "hidden by default" step is
unchanged. `npx tsc --noEmit` clean; the journey itself was NOT run (see
below).

Gates: `angreal test lint` green, `angreal test unit` green, `cargo test
--workspace --test '*' --no-fail-fast` green — 41/41 binaries. OpenAPI:
`include_deleted` appears on all five family lists and
`/api/boards/{id}/items` and on NOTHING else (`/api/boards`, `/api/teams`
still `[limit, offset]`); `registered_routes_and_spec_paths_match_exactly`
passes.

**2026-09-23 — two operational notes for whoever is next.**

- `angreal test integration` and `angreal test uat` tear the compose stack
  down **including the volume**, which kills a concurrent agent's suite.
  Used `cargo test --workspace --test '*' --no-fail-fast` directly instead,
  which is what the angreal task runs between its up and its down.
- **Adding a migration directory does not trigger a rebuild.**
  `embed_migrations!` expands at compile time and cargo does not know the
  macro depends on those files, so a new migration silently does not exist
  until something in `kairos-db` is touched. `touch
  crates/kairos-db/src/migrations.rs` after adding one — the symptom
  otherwise is a provisioning test failing as though the migration were
  never written.
- `cli_live.rs`'s Dex device-flow test is flaky when several test binaries
  run at once (it failed once, passed alone and on the clean full run).
  Pre-existing; nothing in this task touches CLI auth.

## Notes carried in from [[KAIROS-T-0156]]

**2026-09-23.** List endpoints **do not read the two views at all** — they
are diesel query-builder queries straight against the base tables — so this
task is independent of T-0156 rather than built on it.

Note the partial `idx_*_board` / `idx_*_column` indexes (up.sql:181/204/225)
are `WHERE deleted_at IS NULL` and so will **not serve a wide listing**
either. The same index question T-0157 faces for full-text applies here for
board and column listings; measure before assuming a wide list is cheap.