---
id: edge-proposals-an-agent-proposes-a
level: task
title: "Edge proposals: an agent proposes a relationship, a human confirms it"
short_code: "KAIROS-T-0192"
created_at: 2026-09-24T02:28:01.844440+00:00
updated_at: 2026-09-24T02:28:01.844440+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0191]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0017
---

## Parent Initiative

[[KAIROS-I-0017]]

## Objective

Let the graph improve with use. Where the signal from [[KAIROS-T-0191]] is strong,
an agent may **propose** a `parent` or `blocks` edge; a human confirms it.
[[KAIROS-A-0021]] rule 6.

This is the compounding part. Every confirmation turns an implicit dependency
into an explicit one, which makes the next retrieval better, because graph
distance is half the signal.

## Implementation Notes

### Technical Approach

- storage for proposals: source, target, proposed relationship type, the evidence
  (score and *why* from the retrieval that produced it), who or what proposed it,
  when, and its state — pending, confirmed, rejected
- an MCP tool and REST endpoint to propose
- confirm and reject, which only a human-authorised caller may do
- confirming creates the real edge in `item_relationships`; rejecting records the
  rejection rather than deleting it, because a repeatedly-rejected proposal is a
  signal that the thresholds are wrong

### Where a human sees them

A proposal nobody sees is a row in a table. The GUI needs somewhere to show
pending proposals on an item and confirm or reject them — Leptos, aurora_dark
tokens, never raw colours, per `docs/gui-conventions.md`.

Scope judgement: the minimum is proposals surfaced **on the item they concern**,
not a separate review queue. A queue is a second product; start with the place a
person is already looking.

### Why confirmation is not optional

Agents write most of the content here and humans edit; that division is the
product's shape. Letting an agent write graph edges directly would let a
confident wrong similarity silently restructure a portfolio, and the blast radius
of a wrong `parent` edge is a board that reports the wrong thing to the wrong
people. A proposal costs a click. A wrong edge costs a conversation.

### Dependencies

[[KAIROS-T-0191]] — proposals are made of its evidence.

### Risk Considerations

- **Proposal spam.** An agent loop that proposes on every run will bury the
  signal. Deduplicate on the pair and type, do not re-propose a rejected pair
  without materially better evidence, and cap pending proposals per item.
- A cycle-creating `parent` proposal must be refused at confirmation time using
  whatever check the existing parent-assignment path already has — not a new one.
- Rejections are data. Report the confirm/reject ratio somewhere an operator can
  see, because it is the only honest measure of whether this feature works.

## Acceptance Criteria

- [ ] Proposals are stored with evidence, proposer, timestamp and state
- [ ] An agent can propose a `parent` or `blocks` edge via MCP and REST
- [ ] Only a human-authorised caller can confirm or reject
- [ ] Confirming creates the real relationship; rejecting is recorded, not erased
- [ ] Pending proposals are visible and actionable on the item they concern in the
      GUI, using design tokens only
- [ ] Duplicate and re-proposal are prevented, and pending proposals per item are
      capped
- [ ] A cycle-creating `parent` confirmation is refused by the existing check
- [ ] The confirm/reject ratio is observable
- [ ] `angreal test` green

## Status Updates

*To be added during implementation*
