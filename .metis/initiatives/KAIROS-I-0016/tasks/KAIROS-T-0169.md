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

**Delivered short of what I asked for.** A delegated review of all four pages
did not return within the task's window even after a request for a partial
hand-back, so the review below is my own against S-0008, and I am recording that
rather than claiming a clean third-party pass. [[KAIROS-T-0175]] should re-run
`diataxis-review` over these four pages at close-out; they have had the same
scrutiny as [[KAIROS-T-0168]]'s pages on facts, and less on mode.

What was checked, with rule IDs:

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

Carried over knowingly, as in T-0168: the "Related guides" links point at
how-to pages that do not exist yet.

### For the close-out task

- Re-run `diataxis-review` over these four pages; treat my self-review as
  insufficient.
- `introduction.md` promises the glossary to a reader who is unsure where to
  start — that mention is still plain text and now has a page to point at.
- The MCP `create_item` asymmetry and T-0168's Compose forwarding bug both want
  tickets outside this initiative.
