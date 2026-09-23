---
id: the-archived-item-page-read-it
level: task
title: "The archived item page: read it, read its history, restore it"
short_code: "KAIROS-T-0164"
created_at: 2026-09-23T11:30:07.479107+00:00
updated_at: 2026-09-23T11:30:07.479107+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0154, KAIROS-T-0160]
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

The screen this whole initiative exists for: open a piece of archived work,
read what it said, read its history, and put it back. Closes
[[KAIROS-T-0151]].

## Implementation Notes

**Blocked by [[KAIROS-T-0154]] and [[KAIROS-T-0160]].**

Routes are in `crates/kairos-web/src/app.rs:55-80`. Both relevant pages
consume API routes that 404 on archived work today, so both render an error:

- `/items/:code` — the item page.
- `/activity/history/:code` — **the audit answer**, and the single most
  valuable screen here. Copy-forward history (KAIROS-A-0004) was built to
  reconstruct what a record said at a point in time, and it currently goes
  dark exactly when that matters.

Needed:

- an unmistakable banner on an archived item — when it was put away and by
  whom (the activity trail has the actor);
- the **Restore** action, wired to [[KAIROS-T-0160]]'s endpoint, shown only
  to someone who holds the capability, and surfacing its refusals (missing
  board / column / team / repository) as a readable message naming what is
  gone rather than a bare error;
- write affordances hidden or disabled, since archived work is read-only —
  disabled with an explanation beats absent, so the page does not look
  broken;
- history renders normally, since the rows were always intact.

No archive/trash *route* is required — archived work is reached by short
code and by search ([[KAIROS-T-0163]]), which matches "hidden by default"
better than a recycle-bin page would. Note this decision on the task so it
is not re-litigated.

The same vocabulary hazard as T-0163: `item.rs:269,315` uses "archived" for
the document editorial lifecycle. On the item page both words can appear at
once — an editorially-archived document that is also put away. They must
read as different things.

## Acceptance Criteria

- [x] `/items/:code` renders an archived item with a clear banner instead of
      an error, for all five families.
- [x] `/activity/history/:code` renders an archived item's history.
- [x] Restore is present for a capable user, absent otherwise, and its
      refusals name what is missing.
- [x] Write affordances are visibly disabled, not silently broken.
- [x] The two senses of "archived" are distinguishable on one screen.
- [x] [[KAIROS-T-0151]] can be closed.
- [x] `angreal test` green; `angreal test e2e` green.

## Status Updates

**2026-09-23 — the column-name gap: option 2, narrowed to a query flag.**

The note offered two shapes and preferred `deleted_at` on the column DTO
with the board page filtering. Taken, with one change: the flag is
**opt-in per request** rather than a new default.

Making `GET /api/boards/{id}` return removed columns unconditionally would
have reversed a contract [[KAIROS-T-0161]] had just landed — its
`load_columns` doc and its `org_endpoints.rs` assertion both say *"a
removed column must not render on a live board"* — and pushed the filter
out to four independent GUI consumers (item page, admin board config,
search column picker, board data layer), where forgetting one silently
re-renders a removed column on a live board. That is the exact regression
T-0161 fixed, re-introduced as a maintenance burden.

So: `BoardColumn` gains `removed_at` (not `archived_at` — the entity
DTOs' `archived_at` is the ADR-20 work-item state, and a column is not
work), and `GET /api/boards/{id}?include_removed_columns=true` is the one
way to see removed columns. Default reads are byte-identical to before;
T-0161's test still passes unchanged. The item page is the only caller
that passes the flag, and it labels such a column *"{name} (column since
removed)"*. Option 1 (`column_name` on the entity DTO) was rejected on
cost: `into_dto` is pure, so it would have needed an `attach_*` batch fill
per handler — a denormalised field on five DTOs and every list endpoint,
to serve one panel.

**2026-09-23 — what landed in the GUI.**

- `/items/:code` renders an archived item of any family: a gold banner
  (`data-testid="archived-banner"`) with when it was put away and, from
  the activity trail, by whom (best-effort — the banner renders without
  the actor if the trail is unreadable, never the other way round).
- Write affordances are disabled with the reason beside them: Delete and
  New document (page header), Save + the whole textarea/toolbar (the
  editor renders the markdown instead), Save metadata + every field
  control, the lifecycle Set button, and the move/lane/board/repository
  controls (replaced by *"Placement is frozen…"*, since placement is the
  record of where the work sat).
- Restore sits in the banner, gated by a `manage_<family>` client mirror
  (`boards::holds_capability`, host-tested) that narrows to the item's own
  board and widens for off-board items, whose authorization board the
  server resolves through a parent the client cannot see. The 422
  `RESTORE_BLOCKED` refusal renders as prose naming what is gone.
- `/activity/history/:code` carries the same banner and disables rollback
  (a copy-forward write) with the reason; reading and diffing are
  untouched — that is the audit answer this initiative exists for.

**Vocabulary collision, recorded rather than renamed.** The ADR-20 state
is called **"put away"** on every surface of these two pages (badge,
banner, disabled-control copy); the KAIROS-T-0078 editorial state keeps
its `lifecycle: …` prefix. When a document is BOTH (editorially archived
and put away), the banner adds an explicit paragraph naming the two and
saying which is which. The collision is commented at
`pages/item/api.rs` (`ItemDetail::lifecycle`), `pages/item.rs`
(`ItemLoaded`) and `pages/activity.rs`.

**No archive/trash route** — reaffirmed, not re-litigated: archived work
is reached by short code and by search ([[KAIROS-T-0163]]), which is what
"hidden by default" means. A bin page would be a second place work lives.

**2026-09-23 — gates, all green.** `angreal test lint`, `angreal web lint`
(tokens only), `angreal web build`, `cargo test -p kairos-web` (80 tests,
incl. new mirror / capability-mirror / timestamp-format cases), and
`angreal test e2e` — 14 GUI specs, the three new ones in
`e2e/tests/archived.spec.ts`:

1. an archived task reads, its write affordances are disabled with the
   reason, its history reads with rollback off, and Restore puts it back;
2. **all five families** archived (strategy, initiative, task, ADR,
   document) render with the banner — and an editorially-archived
   document that is ALSO put away shows both markers plus the
   disambiguation paragraph;
3. an archived card in a removed column still names the column, and its
   restore is refused with "its board column (removed)".

Fixture discipline worth keeping: an earlier draft created its own
delivery board and broke three unrelated specs at once (`smoke` counts 5
board tiles, `team-lens` the same, and a second live delivery board on a
team makes `POST /api/tasks` routing ambiguous, 422). The spec now creates
no board, and everything it does create ends up archived or removed —
invisible by construction, which is the state under test. Also learned:
A-0001 cascades along PARENT edges only, so a document hanging off a task
by `supports` has to be archived in its own right.

## Notes carried in from other tasks

**2026-09-23, from [[KAIROS-T-0161]] — a gap this task must close.**
`crates/kairos-web/src/pages/item.rs:469-474` resolves an item's column name
out of `BoardDetail.columns`, which T-0161 made **live-only**. So an
archived card sitting in a column that has since been removed renders
"unknown column" — which is exactly the audit fact the column soft-delete
was designed to preserve, lost again at the last surface.

MCP solved it with an unfiltered `column_label` lookup (`mcp/tools.rs`); the
GUI has no equivalent. Two options, decide when implementing:

- a `column_name` on the item DTO (simplest for this page, one more
  denormalised field on the wire), or
- `deleted_at` on the column DTO, with the board page filtering it out
  (keeps one source of truth, touches the board page too).

Prefer the second unless it turns out the board page cannot cheaply filter.
