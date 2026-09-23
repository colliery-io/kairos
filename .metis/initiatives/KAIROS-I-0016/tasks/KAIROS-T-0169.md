---
id: reference-the-mcp-tools-the
level: task
title: "Reference: the MCP tools, the glossary, and the two API pages that already exist"
short_code: "KAIROS-T-0169"
created_at: 2026-09-23T22:11:12.898219+00:00
updated_at: 2026-09-23T22:11:12.898219+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: true
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

`reference/mcp-tools.md` and `reference/glossary.md`, plus moving the two
existing API pages into the book unchanged.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].**

### `reference/mcp-tools.md`

18 tools. Source of truth is `crates/kairos-server/src/mcp/tools.rs` — the
`#[tool(description = …)]` attributes and the `*Params` structs, which are
the schema agents actually receive. The UAT drift gate asserts the inventory
is exactly 18, so `uat/checks/zz-surface-coverage.check.ts` and
`crates/kairos-server/tests/mcp.rs` both carry the authoritative list.

Per tool: what it does, its arguments with types and defaults, and what it
refuses. The refusals matter more here than in most reference material
because an agent cannot ask a follow-up question — `RESTORE_BLOCKED`,
`DEFINITION_IN_USE`, `INVALID_TRANSITION` and the read-only refusals on
archived work are the cases an agent will actually hit.

### `reference/glossary.md`

This page carries more weight in Kairos than in most products, because the
vocabulary genuinely collides:

- **"archived" means two unrelated things** — the document editorial
  lifecycle (`draft | review | published | archived`, KAIROS-T-0078) and
  [[KAIROS-A-0020]]'s put-away state. A published document can be
  editorially archived while being perfectly live. The GUI calls the second
  one **"put away"** (KAIROS-T-0163/0164) and the wire calls it
  `archived_at` / `include_deleted`. All three names, and which surface uses
  which.
- **"board"** — flight-level boards, a team's delivery board, the ADR board.
- **"delete"** is a soft delete everywhere, and is the same act as archiving.
- Team Topologies types, flight levels, capability vs role, bucket,
  work class vs task type, repository as execution scope.

### The two pages that already exist

`docs/api/scim.md` and `docs/api/events.md` move to `reference/` **as-is**.
They are the only existing pages that are already single-mode and correct,
which is why they need no reclassification — resist improving them here.
Update any inbound links.

## Acceptance Criteria

- [x] `reference/mcp-tools.md` covers all 18 tools with arguments, defaults
      and refusals; the count agrees with the drift gate.
- [x] `reference/glossary.md` disambiguates the two senses of "archived"
      across all three names, plus board, delete, and the rest.
- [x] `docs/api/scim.md` and `events.md` are in `reference/`, unchanged in
      substance, with links updated.
- [x] `diataxis-review` passes; rule IDs cited.
- [x] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

**2026-09-23 — done.** `docs/src/reference/mcp-tools.md` (~420 lines) and
`docs/src/reference/glossary.md` (~300 lines) written; `events.md` and
`scim.md` moved into `reference/` with `git mv`, content untouched; four
`SUMMARY.md` lines uncommented; `angreal docs build` clean.

### `mcp-tools.md`

Written from `mcp/tools.rs` — the `#[tool(description = …)]` attributes and the
`*Params` structs, which are the schema an agent actually receives — not from
the S-0006 spec. Two mechanical checks rather than assertions:

- the page's 18 `### <tool>` headings are **identical** to the inventory list in
  `crates/kairos-server/tests/mcp.rs` that the drift gate asserts;
- every field of every `*Params` struct appears on the page.

The refusals took the most work and are where the errors were. They are not in
one place: a refusal is assembled from `load_item`'s liveness, then
`authorize_item_write` / `require_capability_explained`, then whichever of
`map_item_error` / `map_board_error` / `map_link_error` /
`map_repository_error` the service error lands in — and the two repository
mappers **disagree on purpose**. `api::org::repositories::map_error` returns
`NOT_FOUND`, `api::tasks::map_repository_error` returns `VALIDATION`, and which
one a tool uses depends on whether the repository is the subject of the call or
a filter on it. First draft got four of these wrong (`list_repositories`,
`board_items`, `search`, `create_item` all said `NOT_FOUND` where the code says
`VALIDATION`; `unlink_items` said `VALIDATION` where a missing edge is
`NOT_FOUND`). Corrected, and the refusal table now states the rule explicitly:
**a reference that does not resolve is `VALIDATION`; the call's own subject not
existing is `NOT_FOUND`.**

Two things the brief expected that the code does not support:

- **`DEFINITION_IN_USE` is not reachable over MCP.** It guards
  `DELETE /api/metadata-definitions/{id}`, and no MCP tool deletes a
  definition. Documented as a REST refusal with a link there, rather than
  listed as something an agent can hit.
- **`INVALID_TRANSITION` is reachable from `transition_item` only**, not from
  `move_item`, which has its own four (`SAME_BOARD`, `NOT_DELIVERY_BOARD`,
  `NO_ENTRY_COLUMN`, `REPOSITORY_OWNER_MISMATCH`).

One asymmetry found and documented, because an author comparing surfaces will
otherwise assume parity: **MCP `create_item` has no `column`, no `bucket_type`
and no `decision_date`**, all of which the CLI's `create` verbs accept. An
initiative created over MCP can never be a bucket. Worth a look as a product
question, not just a docs note.

### `glossary.md`

Alphabetical, because for a glossary that *is* the product's structure (R1 —
predictable location). Every enum variant was checked against
`models/enums.rs`; the sweep caught a real R4 gap — the five relationship types
had no entry, only incidental mentions of `parent` and `supports` — so
`relationship` is now an entry with all five.

The two senses of **archived** lead the page and carry a surface-to-name table:
GUI "put away", wire `archived_at` / `include_deleted`, database `deleted_at`,
versus the document editorial lifecycle `draft | review | published |
archived`. The sentence that does the real work is the one saying the two are
independent — a published document can be editorially archived and perfectly
live. `api/documents.rs` already carries that warning in a doc comment, which
is where the wording came from.

**Scope correction mid-task:** the capability entry originally enumerated the
whole A-0006 vocabulary, the glob forms and the team-implied set. Per the
coordinator that belongs to [[KAIROS-T-0176]], so the entry now defines the
term, distinguishes it from `role`, and delegates the enumeration.

### `events.md` and `scim.md`

Moved with `git mv`, **not edited** — the premise being that they are already
single-mode. Confirmed by reading both: `events.md` is a wire-format reference
(fields table, thin-event shape, subscribe filter, delivery semantics) and
`scim.md` likewise. Neither contains a relative link, so the move broke nothing
internally.

Inbound references updated, all four in Rust rather than Markdown: two doc
comments in `crates/kairos-server/src/api/openapi.rs` and two in
`crates/kairos-web/src/pages/boards/live.rs`. One of the openapi.rs ones is the
spec's `info.description`, so I checked `scripts/render-openapi.py` before
touching it — the renderer uses `info.title` and `info.version` only, so the
generated REST pages do not change and the CI `--check` cannot trip.
`cargo check -p kairos-server --lib` clean. The remaining `docs/api` matches are
in `.metis` history, which is a record and was left alone. `docs/api/` is now
gone.

### `diataxis-review`

Ran twice: my own pass, then the delegated review, which landed after the first
commit. **The delegated review found six factual defects my pass had missed**,
one of them blocking. All are fixed in a follow-up commit. The lesson is worth
recording: self-review caught the structural rules (R1–R3) but was much weaker
on R4/R6 — the missing refusals were exactly the things I had not thought to
look for, which is what an independent pass is for.

#### Defects the delegated review found, all now fixed

- **R4 + R6, blocking** — `search` refuses an unresolvable `traverse.from` with
  **`NOT_FOUND`**, not `VALIDATION`: `map_search_error` maps
  `SearchError::TraverseRootNotFound` to `not_found`. Worse than an omission,
  because the page's own rule ("a reference that does not resolve is
  `VALIDATION`") would have led an agent to branch on the wrong code. The rule
  now names `traverse.from` as its one exception, with the reason: a traversal's
  root is the subject of that traversal.
- **R4 + R6** — `traverse.depth` is `Option<u32>`, so it is **optional in the
  schema** the agent receives, and a traversal without it is refused
  (`TraverseDepthRequired`). The page said "required" and listed only the
  out-of-range refusal. The field is deliberately not defaulted so that a
  missing depth is a typed refusal rather than a silent choice; documented that
  way.
- **R6** — `delete_item`: an *absent* `confirm` is a schema violation, not a
  `VALIDATION` refusal. `confirm: bool` has no serde default, so the call never
  reaches the body and carries no Kairos code. Only `confirm: false` refuses.
- **R4** — `link_items` omitted the self-link refusal (`GraphError::SelfLink` →
  `VALIDATION`), which is a plausible agent mistake.
- **R4** — `get_history`'s `limit` is `unwrap_or(20).clamp(1, 200)`: it
  **clamps silently** rather than refusing, so `limit: 1000` returns 200 rows
  and no error. Now stated, and contrasted with `search.limit`, which refuses.
- **R6, both pages** — "stored label" misdescribed the mechanism. Nothing is
  denormalized onto an archived card: a removed column is soft-deleted, the card
  keeps a `NOT NULL` FK to the surviving row, and `column_label` reads through it
  without filtering `deleted_at`. Reworded on both pages.
- **R3 + R2, `glossary.md`** — the `board` entry said "three kinds" and then
  listed four levels. Now "four levels grouping into three kinds".
- **R4, `glossary.md`** — the `metadata definition` entry omitted that an
  **archived carrier still counts** against deletion. ADR-0020 rule 6 decides
  exactly this, and the omission read as the opposite alongside this page's own
  "archived means hidden" entry.
- **R2, `mcp-tools.md`** — one imperative ("use `transition_item`"), restated
  declaratively.
- **S4, `mcp-tools.md`** — stated two conclusions whose rationale now has homes
  without linking either. Added.

The review also confirmed independently that the 18-tool set is
character-for-character the drift gate's, that every argument's type,
requiredness and default matches the `*Params` structs with the `depth`
exception, that the `VALIDATION`/`NOT_FOUND` split is right on all four traps I
had corrected, and that **`glossary.md` has zero enum or vocabulary defects** —
every variant, wire string and cardinality exact.

**One finding not acted on.** S4, degrading: the three dead `how-to` links. The
brief directs reference to link how-to by filename ahead of those pages
existing, and [[KAIROS-T-0175]] verifies links at close-out. Recorded rather
than silently kept.

**Two informational findings on the moved pages, for a later task.** These are
outside this task by design — the pages moved unchanged — but the premise that
they are already single-mode turns out to be half right:

- `events.md` — Reference ~80% / Explanation ~20%. The `Why this page exists`
  blockquote and `## Purpose` argue a design position. Cosmetic.
- **`scim.md` — Reference ~65% / How-to ~20% / Explanation ~15%. The premise
  does not hold.** `## Setup (org admin)` is a numbered imperative procedure
  with per-IdP conditionals — §4.3 "reference that instructs", violating R2
  outright. The proposed split is `how-to/provision-users-with-scim.md` plus a
  pure-reference remainder. Worth a ticket; deliberately not done here, since
  the task says to resist improving these pages.

#### My own earlier pass, for the record



- **R1, both, pass.** `mcp-tools.md` groups by what the tool surface does —
  orientation, reading, searching, writing, moving, relationships, archiving —
  rather than by the source file's declaration order, which is the failure
  §2.3 predicts for a page derived from code. `glossary.md` is alphabetical.
- **R2, both, pass.** Mechanically checked: zero occurrences of "you", "your",
  "we" or "should", and no step sequences, across both pages.
- **R3, both, pass.** One argument-table schema
  (`Argument | Type | Required | Default | Description`) for all 18 tools, and
  a uniform "Refuses:" sentence closing every tool entry.
- **R4** — the two mechanical completeness checks above, plus the relationship
  gap found and closed.
- **R6** — the five refusal-code corrections and the `get_history` `limit`
  clamp (1–200, which the first draft omitted).
- **S4** — both pages link out to the explanation pages, to each other, and to
  `rest/tenant-configuration.md` for the one refusal that lives on the REST
  surface.

### For the close-out task

- `introduction.md` promises the glossary to a reader who is unsure where to
  start — that mention is still plain text and now has a page to point at.
- Three tickets wanted outside this initiative: **`scim.md` needs splitting**
  (its Setup section is a how-to living in reference), the **MCP `create_item`
  asymmetry** with the CLI, and T-0168's **Compose forwarding bug**.
- Minor, noticed in passing and not fixed: `crates/kairos-server/src/mcp/tools.rs:1`
  and `crates/kairos-server/tests/mcp.rs:21` both still say "the 14 frozen
  tools". There are 18.
