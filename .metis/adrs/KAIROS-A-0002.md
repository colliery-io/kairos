---
id: 001-configurable-board-columns-and
level: adr
title: "Configurable Board Columns and Transitions"
number: 1
short_code: "KAIROS-A-0002"
created_at: 2026-03-04T01:30:15.544230+00:00
updated_at: 2026-07-08T15:00:12.625265+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-2: Configurable Board Columns and Transitions

## Context

Kairos implements three Flight Levels, each with boards that have distinct phase sequences. The vision defines default sequences:

- Strategy: Draft -> Review -> Active -> Monitoring -> Completed
- Initiative: Discovery -> Design -> Ready -> Decompose -> Active -> Monitoring -> Completed
- Delivery: Backlog -> Todo -> Blocked -> Active -> Completed

However, organizations differ. A team might want a "Spike" column on their initiative board. A delivery team might skip "Blocked" as a column. A strategy board might not need "Monitoring" if they don't do hypothesis validation yet.

The predecessor system (Metis) hardcoded phase sequences as enums in application code. Adding or changing phases required code changes and database migrations. This doesn't work for a multi-tenant system where different organizations (or even different boards within the same org) need different workflows.

## Decision

**Boards are first-class entities with configurable columns and explicit transition rules.**

### Schema

```
boards
  - id          (uuid)
  - name        (text)
  - slug        (text)
  - board_level (strategy|initiative|delivery)
  - team_id     (uuid, nullable - for delivery boards)
  - created_at  (timestamp)
  - updated_at  (timestamp)

board_columns
  - id          (uuid)
  - board_id    (uuid, FK -> boards)
  - name        (text, e.g., "Draft", "Active", "Monitoring")
  - position    (integer, display ordering)
  - created_at  (timestamp)
  - updated_at  (timestamp)

board_transitions
  - id              (uuid)
  - board_id        (uuid, FK -> boards)
  - from_column_id  (uuid, FK -> board_columns)
  - to_column_id    (uuid, FK -> board_columns)
```

### How It Works

- Workflow items (`strategies`, `initiatives`, `tasks`) reference `board_id` and `column_id` instead of a hardcoded `phase` enum.
- A phase transition is: "move item from column A to column B." The system checks `board_transitions` for a matching `(board_id, from_column_id, to_column_id)` row. If it exists, the move is allowed. If not, rejected.
- Columns are ordered by `position` for display (left-to-right on a board view), but ordering does not determine valid transitions - only `board_transitions` rows do.
- The system ships with default board configurations per level that match the vision's phase sequences. These are created when a new tenant or board is provisioned.

### Default Configurations

Created automatically for new boards:

**Strategy board defaults:**
Draft -> Review -> Active -> Monitoring -> Completed (forward-only)

**Initiative board defaults:**
Discovery -> Design -> Ready -> Decompose -> Active -> Monitoring -> Completed (forward-only)

**Delivery board defaults:**
Backlog -> Todo -> Active -> Completed (forward-only), plus:
- Todo <-> Blocked (bidirectional)
- Active <-> Blocked (bidirectional)

**ADR board defaults:**
Draft -> Discussion -> Decided -> Superseded (forward-only)

### Customization

Board owners (governed by ABAC) can:
- Add columns to a board
- Remove columns (if no items are currently in that column)
- Reorder columns (change `position`)
- Add or remove transitions
- Items in a removed column must be moved first (enforced)

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **A: Hardcoded phase enums** | Simple, no config tables, compile-time safety | No customization, code changes for new phases, doesn't work across tenants with different needs | Low | Low |
| **B: Ordered columns, forward-only + blocked escape** | Simple transition logic (position-based), easy to reason about | Can't model bidirectional transitions beyond blocked, can't add non-linear paths | Low | Low |
| **C: Explicit transition rules** (chosen) | Full flexibility, any workflow shape, per-board customization, transition rules are data not code | More tables, transition validation is a DB lookup not a code check, possible to create invalid/orphaned board configs | Low | Medium |

## Rationale

1. **Different organizations need different workflows.** A startup and an enterprise have different ceremony needs. Hardcoded phases force one workflow on everyone.

2. **Even within an org, boards differ.** A platform team's delivery board may have different columns than a stream-aligned team's. Initiative boards for different coordinators may emphasize different stages.

3. **Transition rules as data, not code.** Adding a column or transition is a database operation, not a code deployment. This is essential for a multi-tenant system.

4. **Sensible defaults reduce setup burden.** Most boards will use the defaults. Customization is available but not required.

5. **Option B was too restrictive.** Forward-only with a blocked escape hatch doesn't model real workflows. Some teams need bidirectional transitions (e.g., Active -> Design when scope changes), non-linear paths, or domain-specific columns.

## Consequences

### Positive
- Any workflow shape can be modeled per board
- Adding/removing/reordering columns is a data operation, no code changes
- Different tenants and teams can have different workflows simultaneously
- Default configurations mean zero-config for standard Flight Levels usage
- Transition validation is a simple existence check in `board_transitions`

### Negative
- Board configuration is an additional management surface (mitigated by defaults)
- Invalid configurations are possible (e.g., a column with no outbound transitions creating a dead end)
- Metrics and reporting need to be column-aware rather than assuming fixed phases
- Migration of items when columns are restructured needs careful handling

### Neutral
- The concept of "phase" is replaced by "column" throughout the system - terminology shift from Metis
- Board configuration becomes part of the tenant setup/onboarding process