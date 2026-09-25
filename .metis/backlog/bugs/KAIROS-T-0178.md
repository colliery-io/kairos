---
id: an-agent-cannot-create-a-bucket
level: task
title: "An agent cannot create a bucket initiative: MCP create_item lags the CLI"
short_code: "KAIROS-T-0178"
created_at: 2026-09-23T22:59:44.707706+00:00
updated_at: 2026-09-25T00:52:10.105249+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `bucket_type` and `decision_date` are either accepted by `create_item`
      or documented as deliberately excluded, with the reason.
- [x] `column` is documented as deliberately excluded, with the transition-graph
      reason, or accepted.
- [x] `docs/src/reference/mcp-tools.md` states the outcome.
- [x] A check that the three surfaces do not drift apart again, or a note in
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
### 2026-09-25 â the ticket's own defaults, taken as written

All three were pre-reasoned when this was filed, and re-reading them they hold:
**add `bucket_type`, add `decision_date`, keep `column` out and say so.**

### bucket_type and decision_date

Both were hardcoded `None` in `create_item`'s builder while `items::CreateInitiative`
and `items::CreateAdr` already accepted them â so this was three lines of wiring
plus the parse, which is now literally the same parse as `POST /api/initiatives`
and `POST /api/adrs`, error text included. Wrong-type use is refused by
`reject_field`, exactly as `complexity` and `decision_maker` already were.

What made the `bucket_type` gap worse than it looks: `get_item` has always
*reported* `bucket:` and `decision_date:`. The concepts were visible to an agent
and unreachable by it, which is the least helpful combination.

### column stays out, and the absence is now stated

An item always lands in its board's entry column, and `transition_item` is the
only way work moves. Accepting a column would let an agent place an item past
states the transition graph exists to enforce â something a person using the GUI
cannot do. So this is a rule, not a gap.

The point is that a rule and a gap are indistinguishable from the outside: an
agent reads the schema, sees no `column`, and cannot ask which it is. So it is
said in the tool `description` (where an agent will actually read it) and in
`reference/mcp-tools.md`, not only in a code comment.

### The drift check, which found something on its first run

`create_item_covers_every_rest_create_field` diffs `CreateItemParams`'s field
names against the union of the five REST create requests, with deliberate
differences in `DELIBERATELY_ABSENT` carrying a reason each. Adding an entry is a
decision someone wrote down; leaving one out fails.

It earned its place immediately: it failed on `parent_short_code`, which REST
calls that and MCP calls `parent`. A naming difference, not a capability
difference â but I had guessed the field was called `parent_id` when writing the
excuse list, so the test corrected me rather than the reverse. That is the
evidence that it reads the real surface.

Two companions: one asserts the two previously-unreachable fields by name, and
one asserts the `column` refusal is present in the **tool description** rather
than merely true of the struct.

### Verified through get_item, not the create response

The MCP integration journey now creates a `tech_debt` bucket initiative and a
dated ADR and reads both back with `get_item`, because the question is whether
the value was *persisted* â a create response echoing its own input would prove
nothing. It also asserts the two refusals: `bucket_type` on a task names the
field and the type it belongs to, and `26-10-1985` is refused for `YYYY-MM-DD`
rather than silently dropped.

That needed one grant in the fixture (`manage_adrs` on the ADR board). Added as
an explicit narrow grant in the same style as the rest â alice stays a plain
member with no admin bypass, which is what makes the ABAC assertions later in
that test mean anything. The activity-log count moved 2 â 4 with the two new
creates, which is the mutation-logging assertion doing its job.

### Gates

lint clean (after `cargo fmt`), **397 unit tests**, integration **47/47 exit 0**,
`angreal docs build` green. The REST reference is unchanged, correctly: MCP tool
descriptions are not part of the OpenAPI spec.