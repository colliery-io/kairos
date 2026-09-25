---
id: more-user-journeys-the-stories-the
level: initiative
title: "More User Journeys - The Stories the UAT Tier Does Not Tell Yet"
short_code: "KAIROS-I-0014"
created_at: 2026-09-23T03:40:55.286890+00:00
updated_at: 2026-09-25T00:04:33.609637+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


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

### The arc: new user → mature org

Dylan (2026-09-22): *"keep adding uat stories until we've covered from new
user to mature org."* The journeys are ordered by where an organisation is
in its life, and the suite should read as that arc end to end. Waves 1–2
above cover the first half; these cover what happens once Kairos is load
bearing.

**J14 `growing-team` — "Someone joins, someone leaves"**
A new member is added, granted capabilities on a board, does a piece of
work; then a member leaves — removed from the team, their in-flight work
reassigned, and the org-admin guard that refuses removing the LAST admin.

*Catches:* work orphaned by an offboarding, and the last-admin guard.

**J15 `reorg` — "Two teams become one"**
The mature-org event nothing tests: a team is wound down and its work
absorbed. Repositories re-homed to the surviving team, live cards moved
board to board, the old team deleted once it is clear (KAIROS-I-0012's
rule under real load), and the agents scoped to the moved repository still
find their queue afterwards.

*Catches:* routing that goes stale after a re-home — a task bound to a
moved repository sitting on a board its owner no longer holds.

**J16 `quarterly-review` — "The leadership team reads the whole portfolio"**
Strategy → initiatives across several teams, progress rollups, the blocked
chain surfaced at the Initiative Board Review, and a stream's view of who
is working on what. Read-heavy, across every level at once, which is the
only way the flight-level story is actually exercised.

*Catches:* rollups that disagree with the boards they summarise.

**J17 `incident` — "Something breaks on a Friday"**
An unplanned support request arrives into the Support lane, is triaged
ahead of planned work, becomes a bug bound to a repository, is fixed with
a PR that links back, and closes — the unplanned-work path (KAIROS-T-0077
lanes + work_class) that the planned-work journeys never touch.

*Catches:* lane handling under a real transition sequence, and a support
item that never reaches the team's rollup.

**J18 `second-tenant` — "The deployment hosts more than one organisation"**
An operator provisions a second tenant, seeds it minimally, and proves the
seam: a member of one cannot see or reach the other's work through any
surface (GUI, API, MCP, CLI). Compose-only.

*Catches:* tenant isolation as a PERSON experiences it, rather than as the
integration suite's negative tests.

**J19 `housekeeping` — "Old work is put away"**
The end of the arc: completed work archived (soft delete), a board's
history growing, the retention posture checked, and a repository retired
once nothing references it. What a two-year-old organisation does every
quarter.

*Catches:* archive-then-delete interactions with the guards added in
KAIROS-I-0012.

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

One task per journey. The order walks the arc rather than the risk, so a
half-finished initiative still reads as a coherent stretch of an
organisation's life:

1. **J13 first-week** (a newcomer reads)
2. **J8 machine-access** (the first machine principal)
3. **J9 explorer** (asking where work came from)
4. **J10 decision-record** (decisions accumulate)
5. **J11 new-kind-of-work** (process evolves)
6. **J17 incident** (unplanned work arrives)
7. **J14 growing-team** (people join and leave)
8. **J15 reorg** (teams merge)
9. **J16 quarterly-review** (the portfolio is read)
10. **J12 operations** (the deployment is tended)
11. **J18 second-tenant** (a second organisation arrives)
12. **J19 housekeeping** (old work is put away)
13. **Close out:** both full runs recorded, READMEs updated with the arc,
    the drift gate still green.

Gates per task: `npx tsc --noEmit`, the journey green in compose, no
leftovers; the close-out task runs both modes end to end.

## Progress Log

- 2026-09-22: Created from Dylan's "write more journeys to test against".
  First draft framed it as an API/GUI coverage gate; Dylan clarified he
  meant user journeys, so the machinery was dropped and the initiative is
  six stories the product supports and the suite does not yet tell.
- 2026-09-22: Dylan extended the scope — *"keep adding uat stories until
  we've covered from new user to mature org"* — so the six became twelve
  and the suite is ordered as an arc rather than a set.
- 2026-09-23: **All twelve journeys written, green and committed**
  (KAIROS-T-0137..0148), three of them in parallel by subagents. The suite
  is 20 journeys. Close-out (KAIROS-T-0149) done:

  | Run | Mode | Result |
  |---|---|---|
  | `mudz86xn` | compose, fresh seed | 20 journeys, 20 passed, 0 failed |
  | `mudz23wx` | `--server` | 20 journeys, 20 passed, 8 compose-only steps skipped |
  | — | `angreal test e2e` | 10/10 golden path + MCP, 11 passed GUI smoke |

  Surface coverage: **MCP 17/17 tools, CLI 16/16 nouns, 0 allow-listed.**

- 2026-09-23: **What the twelve journeys found.** Every one of these is a
  place the product and the story disagreed, and in almost all of them the
  product turned out to be right — the story was rewritten:
  - **KAIROS-T-0152** (filed, bug): a soft-deleted item's metadata value
    can be neither read nor cleared, yet still counts against
    `DEFINITION_IN_USE` — one deleted card makes a field permanently
    unretirable. Found by the close-out, because the suite was leaking one
    definition per run while passing.
  - **KAIROS-T-0151** (filed, bug): an archived item and its `/history`
    both 404. Only the activity trail survives, so "what did that ticket
    say?" has no answer once the work is put away. Same shape as T-0152:
    rows that live on in the database with no surface serving them.
  - **KAIROS-T-0150** (filed, tech-debt): `kairos tasks create --board` is
    UUID-only while `move --to-board` and `--repo` take slugs.
  - **A metadata definition cannot be retired while anything references
    it** (`409 DEFINITION_IN_USE`). J11's planned ending — "alice retires
    the field, the item keeps its stamped value" — is not reachable; the
    story became the better one, taking the field off the *template* so new
    work stops collecting it while old work keeps what it has.
  - **ADRs are org-level**, so a team member cannot record one
    (`manage_adrs`); and **the editorial lifecycle is a document concept** —
    an ADR being superseded moves its board *column*, not a lifecycle.
  - **Draft → Decided is not a legal transition**; the graph insists on
    Discussion in between, and says so with the allowed targets.
  - **`task_type` is immutable**, so "a request becomes a bug" is really a
    bug raised *from* the request.
  - **Strategy traverse is blind to standing buckets**, and
    **`--include-deleted` is not combinable with `--query`**.
  - **A single-tenant deployment cannot exercise the cross-tenant seam**,
    which is why J18 is compose-only and the drift gate skips under
    `--server` rather than reporting a false uncovered surface.
- 2026-09-23: **Harness lesson worth keeping.** Two full runs passed while
  leaving residue, because journeys that retire their own work had those
  deletes reported as leaks (a 404 at teardown) — noise that buried the one
  real leak. `uat/run/ledger.ts` now treats a 404 as done. Also: `flock`
  does not exist on macOS, so the shared-stack lock I gave the parallel
  agents silently did nothing and all three raced; `until mkdir …` is the
  portable form.

**Ready for review.** The initiative is not transitioned — Dylan reviews.