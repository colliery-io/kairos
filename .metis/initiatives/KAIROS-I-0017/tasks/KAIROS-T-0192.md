---
id: edge-proposals-an-agent-proposes-a
level: task
title: "Edge proposals: an agent proposes a relationship, a human confirms it"
short_code: "KAIROS-T-0192"
created_at: 2026-09-24T02:28:01.844440+00:00
updated_at: 2026-09-24T21:52:12.841080+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0191]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

- [x] Proposals are stored with evidence, proposer, timestamp and state
- [x] An agent can propose a `parent` or `blocks` edge via MCP and REST
- [x] Only a human-authorised caller can confirm or reject
- [x] Confirming creates the real relationship; rejecting is recorded, not erased
- [x] Pending proposals are visible and actionable on the item they concern in the
      GUI, using design tokens only
- [x] Duplicate and re-proposal are prevented, and pending proposals per item are
      capped
- [x] A cycle-creating `parent` confirmation is refused by the existing check
- [x] The confirm/reject ratio is observable
- [x] `angreal test` green

## Status Updates

*To be added during implementation*

## Status Updates

### 2026-09-24 — done: an agent proposes, a human decides

`edge_proposals`, an MCP tool to propose, REST endpoints to decide, and a panel
on the item where deciding happens.

### The rule lives in the service, not the surfaces

`confirm` and `reject` refuse a service account in `kairos_db::proposals`
itself — not in the MCP handler, not in the REST handler. A rule implemented in
two places is a rule the third place will not have, and the third place is the
one that silently restructures a portfolio. Asserted directly: the agent that
filed a proposal cannot confirm its own.

### The graph's own checks are inherited, not re-implemented

Confirming creates the edge through `graph::link_items`, so the cycle check, the
rule matrix and the activity log are **the existing ones**. Two refusals are
tested, both at confirmation time rather than proposal time — an agent
suggesting something is a suggestion, and the graph is what gets to say no:

- a `blocks` edge that would close a loop → `GraphError::CycleDetected`
- a `parent` edge task → task → `GraphError::Rule`, the same refusal a human gets

A refused confirmation **leaves the proposal pending**. It is not a decision, and
the proposal is still there for a human to reject on its merits.

### Only `parent` and `blocks` are proposable

Those two change what a board reports and what an agent picks up next.
`supports`, `informs` and `supersedes` are editorial and a wrong one is cheap to
undo, so they stay a human's to draw — there is no sense building a review
workflow around a decision nobody would get badly wrong.

### Rejections are kept

A rejected proposal is the only honest measurement this feature has. If a pair
keeps being proposed and keeps being rejected, retrieval is wrong about
something, and `confirm_rate()` is where that shows. Deleting rejections would
delete the evidence that the feature is not working.

`confirm_rate` returns `None` until something is decided — "no data" and "nobody
agrees" are different, and reporting `0.0` for an untouched deployment would
raise a false alarm.

### Spam, prevented in the schema rather than in every caller

A partial unique index on `(source, target, relationship) WHERE state =
'pending'`: re-proposing something already waiting is a no-op at the database
level. Partial on purpose — a **rejected** pair may be proposed again, because a
rejection judges the evidence at the time and better evidence deserves another
hearing; a confirmed one is history.

Plus a cap of ten pending per item, in either direction. An agent loop would
otherwise bury the signal under its own output, and a human looking at thirty
suggestions on one card reads none of them. At the cap the answer is to decide
some, not to make room.

### The GUI: on the item, not in a queue

A review inbox is a second product, with its own notifications and its own
backlog of things nobody opens. A proposal shown where the work already is gets
seen by the person already looking at it.

The panel renders **nothing at all** when nothing is pending — a permanent "no
suggestions" box trains people to stop looking at the place suggestions appear.
It states plainly that nothing has changed yet, shows the agent's reasoning
verbatim rather than a label, and phrases each one from the reader's side ("this
blocks DEMO-T-0007" / "DEMO-T-0007 blocks this") because they are already looking
at one of the two. `token::VIOLET` for the claim pill: distinct from the status
colours, and not `OK`/`BAD`, which would imply a verdict it has not had.
`angreal web lint` clean — no raw colours.

### Two gates caught things, as designed

- `tenant_provisioning` needed `edge_proposals` in `EXPECTED_TABLES`, and its
  fleet-upgrade block re-pinned to this migration — the **third** re-pin in two
  days, which keeps making the case for [[KAIROS-T-0093]]. The upgrade-path
  assertion also checks the partial index came back, because a table without it
  passes a shape check and fails in production.
- The MCP surface assertion needed `propose_edge` added, with a note that there
  is deliberately **no** confirm/reject tool: deciding is not an agent's to do.

### One thing deferred, deliberately, and allow-listed

`angreal test uat`'s drift gate failed on `related_work` and `propose_edge` being
unexercised by any journey. That is [[KAIROS-T-0193]]'s work — it extends
`multi-repo-agent` with ask → prior art → propose → confirm — so both carry a
**pending** ALLOW entry naming that ticket, which the README sanctions while a
ticket is open. The gate fails on stale entries, so they cannot outlive it.

### Gates

fmt and clippy clean; `angreal web lint` clean; unit; integration **47 targets**;
e2e 16; uat 22 journeys and the drift gate green.

Also fixed on the way: `db index-embeddings` carried `risk_level="caution"`,
which angreal does not accept — it warned and silently defaulted to `safe`. An
`ALTER TABLE` rewrite taking an `ACCESS EXCLUSIVE` lock is not safe, so it is now
`destructive`. With only three levels that overstates the consequence (nothing is
destroyed) and understates nothing, which is the right way round: a needless
confirmation costs a click, a lock on production costs an outage. Flagged to
Dylan as a judgement call he may want to reverse.