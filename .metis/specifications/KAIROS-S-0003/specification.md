---
id: delivery-level-overview
level: specification
title: "Delivery Level Overview"
short_code: "KAIROS-S-0003"
created_at: 2026-03-05T02:50:25.529179+00:00
updated_at: 2026-03-05T02:50:25.529179+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#specification"
  - "#phase/discovery"


exit_criteria_met: false
initiative_id: NULL
---

# Delivery Level Overview

> Teams are empowered to work however is best for them and their cadences. The only requirements: work is trackable and clearly linked to initiatives so that flow is visible across levels.

The **Team Charter** informs this board but doesn't live on it. This is the "what we own and how we work together" — the team's scope of ownership, working agreements, and internal norms. It sets the boundaries for how the team operates.

## The Board

This is where work gets done. One board per team, owned by the **team lead**. Three item types: **Task**, **Bug**, and **Tech Debt**. Initiatives always live on the initiative board — even single-team efforts — because that's where capacity is tracked.

Columns: **Backlog → Todo → Blocked → Active → Completed**. Work enters from two sources: upstream tasks decomposed from initiatives, and team-internal items (bugs, tech debt). WIP-limit Active to roughly one item per team member.

## Ceremonies

### Daily Standup — Daily

**Who leads:** Team lead (or rotating facilitator)
**Who participates:** All team members

Walk the board right-to-left. Focus on Active and Blocked columns. This is not a status report — it's about flow.

**Actions that come out:**

- Blocked items surfaced with owners assigned to unblock
- Items stuck in Active longer than expected flagged for pairing or re-scoping
- Completed items acknowledged and moved off the board
- Upstream tasks with updates flagged for coordinator visibility

### Backlog Grooming / Triage — Weekly (Kanban) or Per Sprint

**Who leads:** Team lead
**Who participates:** Team members (full team or rotating subset)

Refine the backlog. Prioritize bugs and tech debt. Estimate effort. For kanban teams this is JIT — groom just enough to keep Todo stocked. For sprint teams, align with sprint cadence.

**Actions that come out:**

- Backlog items refined with clear acceptance criteria and effort estimates
- Bugs triaged and prioritized (P0s pulled immediately; P1–P3 ordered in backlog)
- Tech debt items assessed against standing capacity allocation (~20% of team time)
- Stale backlog items (90+ days untouched) archived or deleted
- Todo column restocked with the next highest-priority items

### Note on Upstream Sync

There is no separate upstream sync meeting. Delivery team leads attend the **Initiative Board Review** at the initiative level. That's where cross-team blockers are raised publicly — "I need X team to unblock me." Progress gets reported, new decomposed tasks get handed off, and cross-team dependencies are resolved.