---
id: reference-the-capability
level: task
title: "Reference: the capability vocabulary and the error codes, which have no home"
short_code: "KAIROS-T-0176"
created_at: 2026-09-23T22:54:08.816241+00:00
updated_at: 2026-09-23T23:00:50.784444+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

Two reference pages the initiative's decomposition missed:
`reference/capabilities.md` and `reference/errors.md`. Both were found by
[[KAIROS-T-0171]] while writing explanation, because E6 forced it to ask where
each fact it was tempted to state actually lives — and for these, the answer
was nowhere.

## Why this task exists

This is a **gap in [[KAIROS-I-0016]]'s D4 plan**, not a nice-to-have. The plan
named five reference pages (cli, configuration, mcp-tools, rest-api, glossary)
and none of them is the natural home for either of these. The explanation
pages currently cite `KAIROS-A-0006` — a Metis document outside the book — as
a stopgap, which is exactly the "documented elsewhere only" that R4 forbids.

### `reference/capabilities.md` — the larger gap

The capability surface appears today only incidentally, per endpoint, in the
generated REST pages. Nothing states the vocabulary as a whole. Needed:

- **The capability names**: `manage_strategies`, `manage_initiatives`,
  `manage_tasks`, `manage_documents`, `manage_adrs`, `transition_items`,
  `configure_*`, `manage_members`, `file_backlog`. Source:
  `crates/kairos-core/src/abac.rs` — the constants and
  `TEAM_IMPLIED_CAPABILITIES`.
- **Glob matching semantics**: how `*` and `manage_*` match, and — the part
  worth stating precisely — that a stored capability containing a literal `%`
  or `_` does **not** act as a SQL wildcard. `crates/kairos-db/tests/abac.rs`
  pins that truth table; the matcher is
  `kairos_core::abac::is_authorized`.
- **What is computed rather than granted**: the org-admin bypass, the
  team-implied set (KAIROS-T-0072), and `file_backlog`
  (KAIROS-T-0105) with its bounds.
- **How a document resolves its board** (through the `supports` edge to its
  parent) and what happens when there is none — the org-admin-only fallback.
- **Archived items resolve the same capabilities as live ones**
  ([[KAIROS-T-0153]]), which is a fact about the surface a reader will want
  confirmed rather than inferred.

Reference mode: state the vocabulary and the rules. Granting and revoking are
tasks, so link to a how-to rather than instructing (R2). Note that
`reference/cli.md` says grants are not part of the CLI surface, so the how-to
target may be the admin API in the REST pages.

### `reference/errors.md` — the smaller one

Every S-0005 error code in one table, which is what an agent needs and what
no page currently provides. `RESTORE_BLOCKED` is the concrete miss:
`reference/rest/across-any-work-item.md` documents the 422 and
`details.missing` but never names the code, because the code is not in the
handler's `#[utoipa::path]` description and so cannot be generated.

Codes to cover — grep `ApiError::unprocessable`, `::validation`,
`::conflict`, `::not_found`, `::forbidden` across
`crates/kairos-server/src/`, and `crates/kairos-server/src/error.rs` for the
envelope shape: `VALIDATION`, `CONFLICT`, `NOT_FOUND`, `FORBIDDEN`,
`INTERNAL`, `INVALID_TRANSITION`, `ITEM_NOT_ON_BOARD`, `COLUMN_NOT_EMPTY`,
`BOARD_NOT_EMPTY`, `DEFINITION_IN_USE`, `RESTORE_BLOCKED`, plus the SCIM
envelope's own shape (`crates/kairos-server/src/scim/error.rs`).

Per code: the HTTP status, what it means, and **what `details` carries** —
`details.current` for a version conflict, `details.allowed_targets` for an
invalid transition, `details.missing` for a blocked restore,
`details.items` / `details.templates` for a definition in use. The `details`
payloads are the part an agent can act on and the part most likely to be
undocumented.

## Two smaller gaps, also from [[KAIROS-T-0171]]

- **Per-level default column sequences** are named nowhere.
  `reference/rest/boards-and-teams.md` says a board is seeded with its level's
  defaults without listing them. Source:
  `public.system_board_defaults` (the seed migration). Put them in
  `reference/configuration.md` if that page has a natural section, otherwise
  here.
- **The six ADR-20 rules are cited by number** from the generated REST pages
  (ten times) and numbered nowhere in the book. Decide and record: either
  `explanation/archiving.md` numbers them — defensible, since the rules are
  the decision's argument rather than a limit or a signature, so E6 does not
  bar it — or they get a home here. **Prefer the explanation page**; a reader
  following "rule 3" from an endpoint wants the reasoning, not a table.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Mode is reference throughout: R1–R6, and
R2 especially — no instruction, link to how-to for tasks.

Uncomment your two lines in `docs/src/SUMMARY.md`. **Note `SUMMARY.md` is
shared with concurrent tasks** — build the blob from current `HEAD` plus only
your own lines rather than `git add`-ing your working copy; the recipe is in
[[KAIROS-T-0171]]'s Status Update and it caught a live revert.

## Acceptance Criteria

## Acceptance Criteria

- [ ] `reference/capabilities.md`: the full vocabulary, glob semantics
      including the literal-`%`/`_` case, what is computed vs granted, document
      board resolution, and the archived-items fact.
- [ ] `reference/errors.md`: every code with status, meaning and what
      `details` carries; `RESTORE_BLOCKED` among them.
- [ ] The per-level default columns are listed somewhere in reference.
- [ ] The ADR-20 rule numbering has a home, and the decision is recorded.
- [ ] Neither page instructs; task-shaped content links out.
- [ ] `diataxis-review` passes; R-rule IDs cited per page.
- [ ] `angreal docs build` clean; `SUMMARY.md` updated without disturbing
      other tasks' lines.

## Status Updates

*To be added during implementation*