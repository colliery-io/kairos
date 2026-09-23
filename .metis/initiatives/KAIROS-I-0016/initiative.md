---
id: diataxis-documentation-a-book
level: initiative
title: "Diataxis Documentation - A Book Kairos Does Not Have Yet"
short_code: "KAIROS-I-0016"
created_at: 2026-09-23T21:59:37.423389+00:00
updated_at: 2026-09-23T21:59:37.423389+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/discovery"


exit_criteria_met: false
estimated_complexity: L
initiative_id: diataxis-documentation-a-book
---

# Diataxis Documentation - A Book Kairos Does Not Have Yet Initiative

# Diataxis Documentation - A Book Kairos Does Not Have Yet Initiative

## Context

Kairos has the Diátaxis *framework* and no Diátaxis *documentation*.

**What already exists.** [[KAIROS-S-0008]] is a complete Diátaxis
specification — the compass, a per-mode contract with citable rule IDs
(T1–T6 for tutorials, and the how-to / reference / explanation sets),
classification heuristics, and mode-mixing anti-patterns. It is rendered to
`plugin/references/diataxis.md` and powers the `diataxis-review` skill in
`plugin/skills/review/`. So a page can already be reviewed against the
framework, rule by rule. Nothing has been written to it.

**What passes for documentation today.**

| File | Lines | Mode it actually serves |
|---|---|---|
| `README.md` | 571 | all four at once |
| `docs/gui-conventions.md` | 279 | contributor reference |
| `docs/api/scim.md` | 116 | reference |
| `docs/api/events.md` | 87 | reference |
| `deploy/helm/kairos/README.md` | — | how-to (operator) |
| `uat/README.md` | 149 | contributor reference |
| `e2e/README.md` | 80 | contributor reference |
| `plugin/README.md` | 76 | contributor reference |

No site generator, no mode-organised tree, and **no tutorial anywhere** — the
one mode nothing in the repo attempts.

**The README is the problem in one file.** Its nine sections are
Development, CLI, Deployment, Service accounts & API keys, Teams,
Repositories, Git forge integration, User acceptance runs, CI. That is a
contributor setup guide, a CLI reference, an operator how-to, four
explanation-shaped feature tours, and a test-tier reference, in one page. By
S-0008's own anti-pattern catalogue it violates the governing principle in
nearly every section — not because it is badly written, but because a reader
arrives at it in one of four moments and it answers a different one.

### The house convention (surveyed 2026-09-23, 27 colliery-io repos)

This is not a greenfield decision. Four sibling repos already do the same
thing, and the newest work is the most consistent:

| repo | `docs/src/` |
|---|---|
| **squire-core** (updated 2026-09-22) | `tutorials/ how-to/ reference/ explanation/` + `SUMMARY.md`, `introduction.md` |
| **graphqlite** (2026-09-20) | the same four, clean |
| **arawn** | the same four, plus `contributing/` |
| **brokkr** | the four, plus older `api/ getting-started/ openapi/` accretions |

The three mkdocs repos are not a competing convention so much as a gradient
toward the same one: `fidius` has all four mode directories, `weir` has
three plus `guides/`, and `hlin` — the least far along — has only
`api/ getting-started/`. The repos that got furthest are the ones that
reached four modes, and the two most recent docs efforts are both mdBook.

**Publishing is settled too.** `squire-core/.github/workflows/docs.yml`
builds mdBook 0.5.2 on pushes touching `docs/**`, and its header comment
states the intent explicitly: *"decoupled from release tags so docs ship
independently"*. It pushes the rendered site to a public dist repo because
Squire's source is private; Kairos is already public, so it publishes to its
own Pages.

**Sizing signal.** squire-core's book is deliberately small: 2 tutorials,
~8 how-to guides, 2 reference pages, 2 explanation pages. A thin spine that
covers the real paths, not an exhaustive corpus. Worth copying — an
unfinished 60-page book is worse than a finished 15-page one, because a
reader cannot tell which pages are load bearing.

**One asset worth exploiting.** `/api/openapi.json` already exists
(KAIROS-T-0023), and `crates/kairos-server/tests/openapi.rs` writes a spec
artifact in CI. REST reference should be generated from it rather than
hand-maintained — a hand-written endpoint list is the reference page most
certain to rot.

### Notes on the shared theme

`colliery-io/mdbook-colliery` exists but was last updated 2026-03-14, and
**none of the four mdBook books actually reference it**: brokkr rolls its own
`theme/css/colliery.css`, and squire-core, graphqlite and arawn use stock
`rust` / `ayu` / `coal`. The shared theme is effectively dormant. This
initiative follows the repos rather than the theme, and does not attempt to
revive it.

## Goals & Non-Goals

**Goals:**
- An mdBook at `docs/`, laid out as the four Diátaxis modes, published to
  GitHub Pages on `docs/**` pushes.
- Every page serves exactly one mode and passes `diataxis-review` with cited
  rule IDs — the framework Kairos already owns, applied to Kairos.
- A tutorial exists. Today none does, which means there is no path for
  someone who has not used Kairos before.
- The README becomes a landing page, and stops being four documents.
- REST reference generated from the OpenAPI spec, not hand-maintained.

**Non-Goals:**
- Not an exhaustive corpus. A thin spine that covers the real paths, sized
  like squire-core's.
- Not reviving `mdbook-colliery`, and not designing a Colliery theme.
- Not migrating to mkdocs, and not relitigating the generator.
- Not touching the release pipeline — docs ship on `docs/**` pushes,
  deliberately decoupled from tags.
- Not rewriting `docs/gui-conventions.md`'s substance; where contributor
  docs live is a scope question below, not an invitation to rewrite them.

## Detailed Design

### D1 — Layout, following the convention rather than inventing one

```
docs/
├── book.toml
└── src/
    ├── SUMMARY.md          the spine
    ├── introduction.md     what Kairos is, and which mode to go to
    ├── tutorials/
    ├── how-to/
    ├── reference/
    └── explanation/
```

`book.toml` modelled on squire-core's: stock theme, `git-repository-url`,
`edit-url-template`, folded sidebar, search on. No custom theme.

### D2 — Audience is a second-level grouping, not a top-level split

Kairos has four audiences — operator (deploy, tenancy, OIDC), end user
(flight levels, boards, the vocabulary), agent author (MCP, service
accounts), contributor (angreal, test tiers, conventions). Diátaxis organises
the top level by **mode**, so audience groups *inside* a mode. squire-core
already does exactly this in its `SUMMARY.md`, subdividing how-to into "For
families" and "For self-hosters".

For Kairos that means how-to reads:

```text
# How-to guides
## For operators
## For people doing the work
## For agent authors
```

Tutorials do not subdivide: a tutorial is a lesson with one intended
learner, and two audiences means two tutorials.

### D3 — Content inventory: where the 571 lines go

Nothing is deleted without a destination. Each README section maps to a mode
by asking S-0008's two questions (action or cognition; study or work):

| README section | Destination | Mode |
|---|---|---|
| Development | contributor how-to (see D6) | how-to |
| CLI | `reference/cli.md` | reference |
| Deployment | `how-to/operators/*` + `tutorials/deploy-to-kubernetes.md` | split |
| Service accounts & API keys | `how-to/agent-authors/machine-access.md` | how-to |
| Teams — and winding one down | `how-to/work/wind-down-a-team.md` + `explanation/teams-and-boards.md` | split |
| Repositories | `explanation/repositories-as-execution-scope.md` + a how-to | split |
| Git forge integration | `how-to/operators/connect-a-forge.md` | how-to |
| User acceptance runs | contributor reference (see D6) | reference |
| CI | contributor reference (see D6) | reference |

`docs/api/scim.md` and `docs/api/events.md` move to `reference/` as-is —
they are already single-mode and correct, which is why they are the only
existing pages that need no reclassification.

The four sections marked **split** are the mode-mixing this initiative
exists to fix: each currently explains *why* and instructs *how* in the same
prose, which S-0008 §4 names as an anti-pattern in both directions.

### D4 — What the book contains (the thin spine)

**Tutorials** (2 — a guaranteed result, every step works every time):
- `run-kairos-locally.md` — from nothing to a board with your first piece of
  work on it, using the compose stack. The "does this thing do anything?"
  lesson.
- `deploy-to-kubernetes.md` — from nothing to a running deployment, using
  the published chart and image from v0.1.0.

**How-to** (~9, grouped by audience):
- operators: install with Helm, configure an OIDC issuer, provision a
  tenant, connect a git forge, back up and restore
- people doing the work: set up a team's board, move work between boards,
  wind down a team, find archived work
- agent authors: give an agent machine access, connect over MCP

**Reference** (5):
- `cli.md` — the `kairos` command tree
- `configuration.md` — every environment variable and Helm value
- `mcp-tools.md` — the 18 MCP tools
- `rest-api.md` — generated from `/api/openapi.json` (D5)
- `glossary.md` — the vocabulary, which matters more here than in most
  products because Kairos has two senses of "archived" and several of
  "board"

**Explanation** (5):
- `flight-levels.md` — why strategy / initiative / delivery, and what the
  levels are for
- `teams-and-boards.md` — Team Topologies types, board ownership, the
  lifecycle
- `capabilities-and-access.md` — the A-0006 board-scoped model, and why it
  is not roles
- `archiving.md` — [[KAIROS-A-0020]]: put away means hidden by default and
  nothing more. This page is why the initiative is worth doing now: the
  decision is fresh, load bearing, and currently recorded only in an ADR and
  a commit message.
- `repositories-as-execution-scope.md` — A-0019

### D5 — REST reference is generated, not written

`/api/openapi.json` exists (KAIROS-T-0023) and CI already writes a spec
artifact. A hand-written endpoint list is the reference page most certain to
rot — R4 demands completeness ("every parameter, return value, error,
default and constraint"), which is precisely what a human cannot keep true
by hand across 40-odd endpoints.

Generate it at build time. `mdbook-openapi`-style preprocessors exist but
add a dependency; a small script rendering the spec to markdown before
`mdbook build` is the simpler option, and the decision belongs to the task
that implements it, measured against whether the output is readable.

### D6 — Contributor docs: in the book, or left alone?

`arawn` keeps `contributing/` inside its book; the other three do not.
Kairos's contributor docs (`uat/README.md`, `e2e/README.md`,
`docs/gui-conventions.md`, `plugin/README.md`, and the README's Development
/ CI sections) are substantial and currently sit next to the code they
describe, which is where a contributor looks for them.

**Recommendation: leave them in place** and give the book one
`explanation/contributing.md` that says where they are and why they live
there. Moving `uat/README.md` away from `uat/` would make it less
discoverable for the person most likely to need it. This is an open question
for Dylan (below) rather than a settled call.

### D7 — S-0008 is the gate

Every page is reviewed with the `diataxis-review` skill before its task
closes, and the review cites rule IDs. The point is not ceremony: the spec
exists, the reviewer exists, and a Diátaxis book that was never checked
against either would be an odd thing to ship from this repo.

### D8 — Publishing

`.github/workflows/docs.yml`, modelled on squire-core's: mdBook 0.5.2,
triggered on pushes touching `docs/**` and the workflow itself, plus
`workflow_dispatch`, with a `docs-publish` concurrency group. Kairos is
public, so it publishes to its own Pages rather than to a dist repo.

An `angreal docs` task (`build`, `serve`) so the book is built the way
everything else in this repo is run.

## Alternatives Considered

- **mkdocs.** Three colliery repos use it, so it is not unreasonable — but
  the two most recent docs efforts are mdBook, the four-mode layout is
  cleanest in the mdBook repos, and Kairos is a Rust workspace where mdBook
  needs no Python toolchain. Rejected on consistency, not merit.
- **Keep everything in the README and just reorganise it.** Cheapest, and it
  fails the one thing Diátaxis is for: a single page cannot serve four
  reading moments, however well ordered.
- **An exhaustive corpus.** Rejected in favour of squire-core's sizing. An
  unfinished 60-page book is worse than a finished 15-page one, because the
  reader cannot tell which pages are load bearing.
- **Hand-written REST reference.** Rejected: R4 requires completeness across
  ~40 endpoints, which no human keeps true.
- **Revive `mdbook-colliery`.** Out of scope, and none of the four books use
  it — following a dormant theme would be following nothing.

## Implementation Plan

*Pending — three scope questions go to Dylan before decomposition.*

### Open questions

1. **Contributor docs (D6):** leave `uat/README.md`, `e2e/README.md` and
   `docs/gui-conventions.md` where they are with a pointer page, or move
   them into the book as `contributing/` the way arawn does?
2. **[[KAIROS-S-0008]] is in `discovery`.** The spec governing this work is
   not finalised. Promote it as part of this initiative, or write against it
   as-is?
3. **Does the README keep its Development section?** Every sibling repo
   keeps a README, but Kairos's is doing four jobs. The landing page needs a
   floor: bare pointer, or pointer plus enough for a contributor to build
   and test without leaving the repo?

## Progress Log

- 2026-09-23: Created after the v0.1.0 release, from Dylan's "we should stand
  up diataxis documentation next". Surveyed 27 colliery-io repos first: the
  layout, generator and publishing pattern are all existing house convention
  (squire-core, graphqlite, arawn, brokkr), so this initiative follows them
  rather than deciding them. Held in discovery pending the three scope
  questions above — no decomposition yet.
