---
id: an-agent-cannot-create-a-bucket
level: task
title: "An agent cannot create a bucket initiative: MCP create_item lags the CLI"
short_code: "KAIROS-T-0178"
created_at: 2026-09-23T22:59:44.707706+00:00
updated_at: 2026-09-23T22:59:44.707706+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# An agent cannot create a bucket initiative: MCP create_item lags the CLI

## Objective

Decide whether MCP `create_item` should accept the fields the CLI accepts,
and close the gap or record the asymmetry deliberately.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [ ] P0 - Critical
- [ ] P1 - High
- [x] P2 - Medium (an agent can work around it via REST, but should not have to)

### Impact Assessment

- **Affected users**: agents, and the people who write them. Kairos treats
  agents as a first-class audience — that is what the MCP surface is for — so
  a field an agent cannot set is a capability an agent does not have.
- **Reproduction**: `CreateItemParams`
  (`crates/kairos-server/src/mcp/tools.rs:195-229`) carries `item_type`,
  `title`, `board`, `parent`, `template`, `content`, `task_type`,
  `repository`, `work_class`, `hypothesis`, `complexity` and
  `decision_maker`. It does **not** carry:
  - `bucket_type` — so **an initiative created over MCP can never be a
    bucket**. The CLI has it
    (`crates/kairos-cli/src/commands/entities.rs:659`), and `is_bucket` is
    derived from it, so the distinction is unreachable from MCP entirely.
  - `column` / `column_id` — an item always lands in the board's entry
    column; an agent cannot place it.
  - `decision_date` — an ADR's date, which the CLI and REST both accept.
- **Expected vs actual**: parity, or a stated reason for the difference.
  Today the difference is silent: an agent reads the tool schema, sees no
  `bucket_type`, and has no way to know whether that is a deliberate
  restriction or an omission.

### The question behind the bug

This may be a product decision rather than a defect, which is why it is filed
as a question with a default rather than a patch:

- **`bucket_type`** looks like a straightforward omission. Buckets are a
  planning construct (KAIROS-A-0002) and an agent doing planning work has the
  same claim on them as a human. **Default: add it.**
- **`column`** is arguably right to withhold. Placing an item in an arbitrary
  column bypasses the transition graph, and the entry-column rule is what
  makes `transition_item` the only way work moves. An agent that could place
  freely could skip states the board is designed to enforce. **Default: leave
  it out, and say so in the tool description** — a documented refusal is
  better than a silent absence.
- **`decision_date`** is a small gap with no obvious rationale.
  **Default: add it.**

Whatever is decided, the outcome belongs in the tool's `description` and in
`docs/src/reference/mcp-tools.md`, because the reader who needs it is an agent
that cannot ask.

## Acceptance Criteria

- [ ] `bucket_type` and `decision_date` are either accepted by `create_item`
      or documented as deliberately excluded, with the reason.
- [ ] `column` is documented as deliberately excluded, with the transition-graph
      reason, or accepted.
- [ ] `docs/src/reference/mcp-tools.md` states the outcome.
- [ ] A check that the three surfaces do not drift apart again, or a note in
      the tool module saying they are allowed to and why.

## Status Updates

**2026-09-23 — filed.** Found by [[KAIROS-T-0169]] while writing
`reference/mcp-tools.md` for [[KAIROS-I-0016]]: documenting every tool's
arguments against the `*Params` structs surfaced what the structs do not
carry. Verified independently before filing.

Worth noting as a pattern — this is the second defect this initiative found by
writing reference rather than by testing ([[KAIROS-T-0177]] is the other).
Completeness (R4) forces someone to enumerate a surface, and enumerating a
surface is when you notice what is missing from it.
