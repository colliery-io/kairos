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
  - "#phase/todo"


exit_criteria_met: false
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

- [ ] `reference/mcp-tools.md` covers all 18 tools with arguments, defaults
      and refusals; the count agrees with the drift gate.
- [ ] `reference/glossary.md` disambiguates the two senses of "archived"
      across all three names, plus board, delete, and the rest.
- [ ] `docs/api/scim.md` and `events.md` are in `reference/`, unchanged in
      substance, with links updated.
- [ ] `diataxis-review` passes; rule IDs cited.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
