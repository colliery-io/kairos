---
id: more-user-journeys-the-stories-the
level: initiative
title: "More User Journeys - The Stories the UAT Tier Does Not Tell Yet"
short_code: "KAIROS-I-0014"
created_at: 2026-09-23T03:40:55.286890+00:00
updated_at: 2026-09-23T03:42:44.251409+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/design"


exit_criteria_met: false
estimated_complexity: M
initiative_id: more-user-journeys-the-stories-the
---

# More User Journeys - The Stories the UAT Tier Does Not Tell Yet Initiative

## Context **[REQUIRED]**

The UAT tier has eight journeys (KAIROS-I-0011, KAIROS-I-0013). They cover
onboarding, planning, the agent loop, cross-team filing, the audit trail,
team knowledge, board setup, and a smoke. Dylan: *"write more journeys to
test against"* — more of the product's real stories walked by a persona,
not more machinery.

What a person or a machine can do with Kairos today that no journey tells:

- **A CI system works without a human.** Service accounts, API keys,
  rotation, revocation — the A-0017 machine-principal story. J3's agent
  gets a key handed to it; nobody ever rotates or revokes one.
- **Someone asks a question and follows the answer.** The `/search` page
  and the relationship graph explorer: filters, traverse, walking out to a
  neighbour and back. J2 touches the graph tab on one item; nobody
  explores.
- **A decision gets recorded, and later superseded.** ADRs: draft →
  decided, a `supersedes` edge, the lifecycle badge. J2 creates one ADR
  from the CLI and never opens it.
- **A team makes a new kind of work first-class.** Templates and metadata
  definitions used as an admin actually uses them — stamp an item from a
  template, retire a field.
- **An operator checks the deployment is healthy**, and deletes something
  big after previewing the damage (`/healthz`, `/readyz`, `/metrics`,
  cascade preview before a delete).
- **A newcomer finds their way around** — the reading paths: boards
  overview by flight level, an item's children progress, a team's
  in-flight rollup. Read-only, and the least tested part of the product
  because nothing writes.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- Five or six new journeys, each a story a person would recognise, in the
  existing harness (personas, ledger, narrated steps, report).
- Each one self-cleaning and green in both compose and `--server` modes.
- The existing drift gate stays green (new tools/nouns would fail it, and
  these journeys use what is already there).

**Non-Goals:**
- No new harness concepts, no coverage machinery. KAIROS-I-0014 started as
  an API/GUI-route gate; Dylan's clarification dropped that — journeys are
  the point.
- Not duplicating the integration or e2e tiers: a journey exists when a
  PERSON does the thing, not to tick an endpoint.
- Not testing error branches a person would never trigger.

## Detailed Design **[REQUIRED]**

Six journeys. Each names its cast, its story, and what it would catch that
nothing else would.

### J8 `machine-access` — "A CI system works, then loses its key"

alice registers a service account for CI, mints a key, and the machine
uses it (MCP + CLI, no browser). Then the key is rotated — the old one
stops working mid-story, which is the part worth testing — and finally
revoked, after which the machine is locked out cleanly rather than
half-working. alice sees the machine's activity in the feed as a
principal, not as a person.

*Catches:* an auth path that only machines use, and a revocation that
leaves a stale session usable.

### J9 `explorer` — "Someone asks where a piece of work came from"

carol wants to know why a task exists. She searches for it by text,
filters to her team, opens it, then walks the relationship graph out to
its initiative and the strategy above that — and back down to a sibling
task. Her last step is the traverse query from the CLI, the same question
asked a different way, with the same answer.

*Catches:* the graph explorer drifting from the search page, and traverse
returning something different from what the GUI drew.

### J10 `decision-record` — "A decision is made, then superseded"

The team records an ADR, moves it draft → decided, and a document is
attached to the initiative it justifies. Months later (minutes here) a
second ADR supersedes the first: the edge is created, the old one's
lifecycle moves, and both are readable with the relationship between them
visible.

*Catches:* the `supersedes` edge type and the editorial lifecycle, neither
of which any journey writes.

### J11 `new-kind-of-work` — "A team makes support requests first-class"

alice creates a template for a recurring kind of work and a metadata
field it needs, bob raises an item from the template (content and fields
pre-stamped), works it, and alice later retires the field — the item keeps
its stamped value, which is the bit people get wrong.

*Catches:* template stamping, and what happens to stamped values when a
definition is retired.

### J12 `operations` — "An operator checks the deployment, then deletes something big"

An operator checks `/healthz`, `/readyz` and `/metrics`, reads
`/api/config` to confirm what the deployment believes about itself, then
previews a cascade delete of an initiative with children — reads the
preview, and only then deletes. The children are gone; the audit trail
says who.

*Catches:* a cascade preview that disagrees with the delete that follows.

### J13 `first-week` — "A newcomer finds their way around without writing anything"

bob's first week: the boards overview by flight level, an initiative's
children-progress, a team's in-flight rollup, the repositories panel, his
own queue narrowed to his repository. He writes nothing. Every assertion
is "the thing a new person needs is where they would look for it".

*Catches:* read paths, which nothing else tests because every other
journey writes.

### Conventions (unchanged)

Existing harness: `journey()` / `step(persona, …)`, the ledger for
everything created, per-run `uat-<run>-` names, compose-only steps marked,
`npx tsc --noEmit` plus the journey green in both modes as the gate.
Distinct fixture suffixes where a journey creates a team (J1 `mobile`,
J3 `ios`, J7 `infra` are taken).

## Alternatives Considered **[REQUIRED]**

- **Extend the drift gate to API paths and GUI routes** (this
  initiative's first draft). Dropped on Dylan's clarification: it would
  have produced a worklist of endpoints, and endpoints are not stories.
  The MCP/CLI gate from I-0013 stays as it is.
- **One big "everything else" journey.** Rejected: journeys are stories,
  and a story with eleven unrelated acts is a checklist.
- **Skip the read-only journey (J13).** Tempting — nothing breaks
  loudly — which is exactly why it is worth having.

## Implementation Plan **[REQUIRED]**

One task per journey, in this order (each independent; the order is by how
much each would embarrass us if broken):

1. **J8 machine-access**
2. **J12 operations**
3. **J9 explorer**
4. **J10 decision-record**
5. **J11 new-kind-of-work**
6. **J13 first-week**
7. **Close out:** both full runs recorded, READMEs updated to list the
   journeys, the drift gate still green.

Gates per task: `npx tsc --noEmit`, the journey green in compose, no
leftovers; the close-out task runs both modes end to end.

## Progress Log

- 2026-09-22: Created from Dylan's "write more journeys to test against".
  First draft framed it as an API/GUI coverage gate; Dylan clarified he
  meant user journeys, so the machinery was dropped and the initiative is
  six stories the product supports and the suite does not yet tell.