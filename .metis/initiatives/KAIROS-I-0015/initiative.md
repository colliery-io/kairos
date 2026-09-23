---
id: archived-work-stays-readable
level: initiative
title: "Archived Work Stays Readable - Implementing ADR-20 Across Every Surface"
short_code: "KAIROS-I-0015"
created_at: 2026-09-23T11:21:34.421605+00:00
updated_at: 2026-09-23T20:11:26.946340+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: archived-work-stays-readable
---

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

Dylan's scope calls (2026-09-23): **everything including the GUI**; a column
holding only archived cards **becomes removable**; archived work is
**read-only, plus an un-archive verb**.

### The organising idea

Almost every contradiction is one of two shapes: a *resolution* that refuses
to find the row, or a *view* that never emitted it. So the work is not 221
edits. It is: make resolution archived-aware, move the two views' liveness
filter from the view body to the call sites, and then let the surfaces ask.

Two rules hold everywhere:

- **The default never changes.** Every existing call site keeps its current
  behaviour by passing the live-only mode explicitly. A reviewer should be
  able to check that no default listing changed by looking for call sites
  that *omit* the parameter.
- **Archived rows are marked, not disguised.** Anything that serves an
  archived row says so in the payload (`archived_at`), in the rendered MCP
  text, and in the GUI. An auditor must never mistake archived work for
  live work.

### D1 — Resolution becomes archived-aware

`api/mod.rs:166-190 resolve_short_code` is the single chokepoint behind
contradictions 1, 2, 6, 8 and most of the MCP surface. It gains a mode:

```rust
enum Liveness { LiveOnly, IncludeArchived }
```

`short_code_not_found` (api/mod.rs:211-215) stops saying *"no live …"* — a
message that is currently correct and will become a lie. The five per-family
`load()` fns (`tasks.rs:114`, `strategies.rs:52`, `initiatives.rs:53`,
`documents.rs:111`, `adrs.rs:49`) take the same mode.

GET by short code passes `IncludeArchived`; every mutating handler keeps
`LiveOnly`, which is what makes "read-only" fall out of the design rather
than needing a second guard (see D5).

### D2 — ABAC must resolve for archived items (prerequisite, not optional)

`abac.rs:339/355/394/425` resolve the authorization board through live-only
lookups, so an archived item yields `None` and capability resolution falls
back to the tenant-wide org-admin policy. Serving archived items before
fixing this would make archived work **more** restricted than live work —
precisely inverting ADR-20 rule 4. This lands before or with D1, and the
test that proves it is a non-admin team member reading their own team's
archived card.

### D3 — The two views stop filtering, the call sites start

`entity_directory` and `searchable_items`
(`migrations/tenant/…/up.sql:364-386` and `:396+`) expose `deleted_at`
instead of filtering on it. Every consumer then filters explicitly. This is
a tenant migration; both views are recreated, no table changes.

This is the highest-risk step for a silent regression, because a consumer
that forgets to filter starts leaking archived rows into a default listing.
Mitigations, in order of usefulness:

1. The UAT drift gate and the `housekeeping` journey already assert that
   default listings hide archived work end to end.
2. `crates/kairos-db/tests/search.rs:637-679` pins current behaviour and
   must keep passing unchanged for the live-only paths.
3. Grep discipline: after this lands, every `entity_directory` /
   `searchable_items` reference must name a liveness mode.

### D4 — Search, traverse and the graph

- **The intersection bug.** `kairos-db/src/search.rs:159-187` intersects the
  candidate sets from `q`, traverse and metadata, and `q` only ever matched
  live ids, so archived candidates were dropped *before* `include_deleted`
  was read at :187. With D3 the `q` set can contain archived ids and the
  flag governs, as it always claimed to. The silent no-op becomes real
  behaviour — no new flag, no error case.
- **`is_constraining()`** (`kairos-core/src/search.rs:158-173`) currently
  excludes `include_deleted` from the "does this narrow anything" test, so a
  filter carrying only that flag 400s. It should count, since "show me
  archived work" is a legitimate whole query.
- **Traverse root** (`search.rs:266-292`) resolves through the view; with D3
  it honours the flag instead of 404ing.
- **`neighbors_of`** (`graph.rs:403-419`) includes archived neighbours,
  marked. Worth restating why this one matters: it degrades the **live**
  side of the record. "What did this initiative contain?" silently loses
  rows today, and no opt-in can currently recover them.

### D5 — Read-only, and the un-archive verb

Read-only needs no new guard: every mutating path already loads with
`LiveOnly` (`items.rs:325`, `:650`, `:702`, `:792`), so leaving those call
sites alone *is* the freeze. The design note is to state that deliberately,
so a later reader does not "fix" the inconsistency by making them uniform.

The new verb restores a row by clearing `deleted_at`:

- **API**: `POST /api/{family}/{short_code}/restore`, on all five families.
- **Capability**: the same one that archived it. Restoring is the inverse of
  deleting, not a new privilege.
- **MCP**: a `restore_item` tool. **This trips the drift gate** (17 tools →
  18) until a journey exercises it, which is the gate working as intended.
- **CLI**: a `restore` verb on the existing entity-family macro
  (`commands/entities.rs`) — a verb, not a noun, so the gate's noun count is
  unaffected.
- **GUI**: a Restore action on the archived item view (D7).

**Restore refuses when its home is gone**, naming what is missing rather
than silently re-homing: a task whose board, column or owning team was
removed, or whose repository was retired. Precedent for the shape is
`live_board_item_codes` — refuse, and name the blockers. The user then
moves it (`POST /api/tasks/{code}/move` already exists from I-0012).

### D6 — Columns, and the foreign key nobody has had to think about

`column_id` is `UUID NOT NULL REFERENCES board_columns(id)` with **no
`ON DELETE` clause** (up.sql:170, :190, :213, :250). So today's behaviour is
not merely the policy at `boards.rs:868-885` — the database itself refuses,
and `remove_column`'s doc comment says exactly that: *"soft-deleted rows
included, since they still reference the column"*. Dropping the count alone
would convert a clean 422 into a foreign-key violation.

So: **`board_columns` gets a `deleted_at` of its own**, and removal becomes
a soft delete. ADR-20 applied one level down, which is why it needs no new
concept:

- the FK stays satisfied, so an archived card still renders with its real
  column name — the audit answer stays intact;
- `count_items_in_column` (`boards.rs:868-885`) counts live rows only, so a
  column holding only archived cards is removable, as Dylan asked;
- live board rendering, `board_transitions` and the column rules filter to
  live columns, so nothing on a working board changes;
- a restore into a removed column is one of the refusals in D5.

`board_transitions` FKs are `ON DELETE CASCADE` (up.sql:92-93), so a
soft-deleted column must have its edges filtered rather than cascaded —
that is the one place this is more than mechanical.

### D7 — The GUI

`pages/search/data.rs:67-69` says the filter builder cannot even express
`include_deleted`, and `app.rs:55-80` has no route. Needed:

- the search filter builder can express it, and the search page has a
  visible toggle (not a URL-only parameter — "visible for audit" means
  discoverable);
- `/items/:code` renders an archived item with a clear banner and the
  Restore action, instead of an error;
- `/activity/history/:code` renders archived history, which is the single
  most valuable screen in this initiative: it is the audit answer.

**Vocabulary hazard:** `pages/item.rs:269,315` already uses "archived" for
the *document editorial lifecycle* (`draft|review|published|archived`,
KAIROS-T-0078), which is unrelated — a published document can be editorially
archived while perfectly live. The GUI must not use one word for both. This
initiative does not rename anything; it picks GUI copy that distinguishes
them and records the collision for whoever does the rename.

### D8 — `DEFINITION_IN_USE` (closes KAIROS-T-0152)

`api/meta/definitions.rs:463-486` counts `item_metadata` rows without
joining the owning entity, so liveness is not considered at all. With
archived items reachable, counting them is defensible — but the refusal must
**name** the carriers, archived ones marked, the way `live_board_item_codes`
does for the team guard. A bare count the user cannot act on is the actual
defect. `set_metadata` on an archived item stays refused (D5); the admin
restores, clears, re-archives.

## Alternatives Considered

- **Serve archived content to admins only.** Rejected in ADR-20 — it makes
  archiving a permission boundary.
- **A `?include_deleted=true` query parameter bolted onto today's handlers,
  leaving the views alone.** Tempting and much smaller, and it is what the
  existing search flag does. Rejected because it cannot work: the views are
  upstream of resolution, so the parameter would have nothing to widen —
  exactly the bug that makes `--include-deleted` a silent no-op alongside
  `--query` today. Fixing the views is the whole job.
- **Re-parent archived cards to a synthetic column on removal**, instead of
  soft-deleting columns. Rejected: it mutates the archived record, so the
  card no longer says which column it was in when it was put away, which is
  the audit fact worth keeping. It also contradicts read-only.
- **Denormalise the column name onto the item at archive time.** Same
  objection in a cheaper form, plus a second source of truth.
- **Hard-delete on archive after a retention window.** Out of scope, and
  currently impossible anyway: the sweeper is unwired and purges only
  `item_history` / `activity_log`.

## Implementation Plan

Five waves. Waves 1 and 2 are strictly ordered (D2 before D1 before D3);
after that the work fans out.

**Wave 1 — resolution (no migration, strictly ordered)**
1. **ABAC resolves archived items** (D2). Prerequisite for everything; alone
   it changes no surface behaviour.
2. **Archived-aware resolution: item GET and `/history`** (D1) —
   `resolve_short_code`, the five `load()`s, `archived_at` in the payload,
   the `"no live"` message. History is pure resolution, so it comes free
   with the chokepoint and carries the highest audit value in the wave.
3. **MCP and CLI read archived** (D1) — `get_item` / `get_history` with an
   archived marker in the rendered text, and the CLI entity families.

**Wave 2 — the views (migration; 4 before the rest)**
4. **Views expose `deleted_at`; all consumers filter explicitly** (D3).
5. **Search honours `include_deleted` alongside `q`; `is_constraining`
   counts it; traverse from an archived root** (D4).
6. **Graph neighbours include archived, marked** (D4).
7. **`include_deleted` opt-in on the five list endpoints and
   `/api/boards/{id}/items`** (D3).

**Wave 3 — writes and guards (independent of each other)**
8. **Un-archive across API, MCP and CLI, with the refuse-and-name guards**
   (D5).
9. **`board_columns` soft delete; `count_items_in_column` goes live-only**
   (D6).
10. **`DEFINITION_IN_USE` names its carriers** (D8) — closes
    [[KAIROS-T-0152]].

**Wave 4 — the GUI**
11. **Search toggle and filter-builder support** (D7).
12. **Archived item page with Restore, and archived history** (D7) — closes
    [[KAIROS-T-0151]].

**Wave 5 — close out**
13. **UAT and docs**: `housekeeping` flips from asserting 404 to asserting
    the audit answer; a journey covers `restore_item` so the drift gate
    reads 18/18; README and the arc table updated; both full runs recorded.

Gates per task: `angreal test` (unit + integration) green, and for anything
touching a default listing, the `housekeeping` journey green in compose.

## Progress Log

- 2026-09-23: Created from [[KAIROS-A-0020]]. Survey of all ~221 `deleted_at`
  call sites complete and recorded above; the contradiction list is the
  worklist. Held in discovery pending Dylan's answers to the three scope
  questions — no decomposition yet.
- 2026-09-23: **Scope decided by Dylan** — everything including the GUI; a
  column holding only archived cards becomes removable; archived work is
  read-only plus an un-archive verb. Design written against those answers.

  Designing the column answer turned up the thing the survey had not: the
  guard at `boards.rs:868-885` is not the only thing holding a column down.
  `column_id` is `NOT NULL REFERENCES board_columns(id)` with **no
  `ON DELETE` clause** (up.sql:170/190/213/250), so the database itself
  refuses — and `remove_column`'s own doc comment says so. Dropping the
  count alone would turn a clean 422 into a foreign-key violation. The
  answer is ADR-20 one level down: `board_columns` gets its own
  `deleted_at`. The FK stays satisfied, the archived card keeps rendering
  with the real column name it was put away in, and the column vanishes
  from live boards. No new concept.

  Also settled while designing: read-only needs **no new guard**. Every
  mutating path already loads live-only (`items.rs:325/650/702/792`), so
  leaving those call sites alone *is* the freeze — recorded in D5 so nobody
  later "fixes" the inconsistency.

- 2026-09-23: **All thirteen tasks complete.** Close-out (KAIROS-T-0165):

  | Run | Mode | Result |
  |---|---|---|
  | `mue4t6ai` | compose, fresh seed | 21 journeys, 21 passed, no teardown residue |
  | `mue4ud4b` | `--server` | 21 journeys, 21 passed, 8 compose-only steps skipped |
  | — | `angreal test e2e` | 10/10 golden path + MCP, 16 GUI specs |
  | — | `cargo test --workspace --test '*'` | 41/41 binaries |

  Surface coverage: **MCP 18/18 tools, CLI 16/16 nouns, 0 allow-listed.**

  Closes [[KAIROS-T-0151]] (archived content unreachable) and
  [[KAIROS-T-0152]] (a deleted item's metadata pinning its definition).

- 2026-09-23: **What the implementation found that the design did not.**
  Each of these is a place the code knew something the survey did not:

  - **The column guard was a foreign key, not a policy.** `column_id` is
    `NOT NULL REFERENCES board_columns(id)` with no `ON DELETE` clause, so
    dropping the count filter would have turned a clean 422 into an FK
    violation. Giving `board_columns` its own `deleted_at` also required
    converting two composite `UNIQUE`s into **partial unique indexes** —
    otherwise a removed column's name and position stay reserved forever.
    That is the general landmine for any future soft delete under a
    composite unique.
  - **The graph was already broken, not merely narrow.** `item_subgraph`
    walks `item_relationships` directly, so it already hopped *through*
    archived work; hydration then deleted the middle of each path, leaving
    the far side floating with no route back to the focus. An archived root
    dropped out of its own subgraph. Omitting archived nodes was not a
    smaller picture — it was a wrong one.
  - **The benchmark that justified the index change was wrong twice.** Two
    successive corpora measured nothing: the first had every row matching
    the search term, the second keyed `deleted_at` off the same counter as
    the board so whole boards had no archived rows. Only the third — a
    selective term, uniform scatter — produced real numbers. Worth
    remembering that a plausible benchmark is the easiest thing to get
    wrong in this codebase.
  - **The graph explorer had been rendering black-on-black since T-0089.**
    Its CSS referenced five custom properties aurora-dark does not define;
    an undefined var invalidates the declaration and `fill` inherits.
    `angreal web lint` bans raw colour literals, so a plausible-looking
    token name that does not exist passes clean. Found only because a new
    marker was correct and invisible.
  - **Widening the cards without widening the columns silently drops the
    oldest audit rows.** Both board listings bucket by column id; an
    archived card whose column was removed is fetched and then never
    rendered, with no error.
  - **Seven assertions across four test binaries and two UAT journeys
    encoded "archived = gone".** Each was rewritten rather than deleted —
    they are the clearest record of what changed. The count is the best
    argument that the decision was worth making explicit.

- 2026-09-23: **Process findings, for the next parallel run.**

  - **The git index is shared between agents in one tree, and `git add` does
    not isolate them.** Staged hunks were swept into the wrong commit three
    times. The workaround — committing through a private `GIT_INDEX_FILE` —
    then caused a worse failure: an index built before another agent's
    commit wrote a tree that **silently reverted seven files** of already
    landed work (`c38e565` over `0089ddc`, restored in `388256d`). Git saw
    an ordinary change and raised nothing. The recipe needs a third step:
    `git read-tree HEAD` into the private index immediately before staging,
    and `git reset` to resync the shared one afterwards. Verify with
    `git show --stat` that a commit contains only the intended paths **and**
    that files owned by others are absent entirely.
  - **`angreal test integration` and `angreal test uat` tear the compose
    stack down including the volume**, killing any concurrent run — once
    producing 32 spurious "connection refused" failures that looked like
    real breakage. Agents sharing a tree must use specific
    `cargo test -p <crate> --test <name>` binaries instead.
  - **`embed_migrations!` expands at compile time and cargo does not know it
    depends on the migration files.** Adding a migration directory does not
    trigger a rebuild; `touch crates/kairos-db/src/migrations.rs` is needed,
    or the test fails as though the migration was never written.
  - The `tenant_provisioning.rs` fleet-upgrade block is pinned to the newest
    tenant migration and was re-pinned three times in this initiative.
    **Carry the previous migration's evidence forward into
    `EXPECTED_INDEXES` when re-pinning**, or each re-pin silently deletes
    the last one's only coverage. Root cause is KAIROS-T-0093.

**Ready for review.** The initiative is not transitioned — Dylan reviews.
- 2026-09-23: **Reviewed and completed by Dylan.** Work pushed to
  `origin/main` (`2788a21..b1a044a`, 168 commits).

  Bookkeeping note: the initiative sat in `discovery` for the whole of
  implementation — the Ralph loop transitions tasks, not initiatives, and
  nothing advanced the parent. It was walked discovery → design → ready →
  decompose → active → completed at review time. The phase never reflected
  reality while the work was in flight, which is worth knowing if anyone
  reads this log expecting the phase history to be a timeline.
