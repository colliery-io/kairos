---
id: initiative-level-overview
level: specification
title: "Initiative Level Overview"
short_code: "KAIROS-S-0002"
created_at: 2026-03-05T02:50:24.938915+00:00
updated_at: 2026-03-05T02:50:24.938915+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#specification"
  - "#phase/discovery"


exit_criteria_met: false
initiative_id: NULL
---

# Initiative Level Overview

## The Board

The coordination layer. One board, owned by a **coordinator**. Only **Initiatives** (epics) live here — concrete projects that deliver against strategies. Tasks live on delivery boards.

The **Social Contract** informs this board but doesn't live on it. This is the "how we work together" — shared agreements across teams about communication, commitments, and coordination norms. It sets the boundaries for how the group collaborates.

Columns: **Discovery → Design → Ready → Decompose → Active → Monitoring → Completed**. Ready items are pulled JIT into Decompose — the buffer where teams break work into tasks and surface last-mile dependencies. Feature work rolls out during low-activity periods. Monitoring is the post-delivery support phase through the high-activity season (break-fixes, adjustments, tuning). An initiative isn't done until it's stabilized.

**Bucket initiatives:** Not all work is a planned epic. "Bucket" initiatives exist for ongoing work that still needs to be visible and tracked against capacity: **Tech Debt**, **Bugs**, and **Ad-Hoc/Service Requests** (for teams that regularly take requests from others). Buckets are recreated quarterly as a coarse capacity tracking mechanism. Tasks flow through delivery boards underneath them like any other initiative.

## Ceremonies

### Board Review — Weekly or Biweekly

**Who leads:** Coordinator
**Who participates:** Delivery team leads

A public forum. The single point of coordination. Walk the board right-to-left. This meeting forces conversations into the open — it doesn't have them. All real work happens out of band.

**Actions that come out:**

- Monitoring items assessed — stable (move to Completed) or break-fix work flagged to delivery boards
- Active items assessed — on track, at risk, or blocked
- Cross-team blockers explicitly called out — "I need X team to unblock me" happens here, publicly
- Ready items pulled into Decompose (JIT replenishment)
- Discovery/Design progress checked, next steps committed to
- Strategy alignment confirmed

### Working Sessions — Out of Band, As Needed

**Who leads:** Coordinator or relevant lead
**Who participates:** Relevant delivery team leads

Scheduled when the board review surfaces that action is needed.

- **Discovery/Design** — scope the initiative, define the approach, identify teams
- **Decomposition** — break initiative into tasks, assign owners, map dependencies
- **Break-fix coordination** — address issues surfaced during Monitoring

**Actions that come out:**

- Initiatives progressed through board columns as work completes
- Tasks created with acceptance criteria and owners assigned
- Dependencies identified and communicated to affected teams