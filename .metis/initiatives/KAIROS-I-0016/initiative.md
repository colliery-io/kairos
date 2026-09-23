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

`docs/api/scim.md` and `docs/api/events.md` move to `reference/`.

**Correction, 2026-09-23.** This paragraph originally read "as-is — they are
already single-mode and correct, which is why they are the only existing pages
that need no reclassification." That was an assumption, and it was wrong.
`scim.md` measures **~65% reference, ~20% how-to, ~15% explanation**: its
`## Setup (org admin)` section is a numbered imperative procedure with per-IdP
conditionals, which is S-0008 §4.3's "reference that instructs" exactly.
[[KAIROS-T-0179]] splits it.

The lesson is worth more than the fix: **the only two pages this plan exempted
from review were the two nobody had reviewed.** An exemption granted on the
strength of a glance is where mode-mixing survives a documentation project.

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

### D6 — Contributor docs are out of scope (decided)

**Dylan, 2026-09-23: out of scope entirely.** The book serves three
audiences — operator, end user, agent author. `uat/README.md`,
`e2e/README.md`, `docs/gui-conventions.md` and `plugin/README.md` are
untouched and unreferenced by it.

That is a narrower scope than `arawn`'s (which keeps `contributing/` inside
its book) and it is the right one here: those four files sit beside the code
they describe, which is where the person who needs them is already looking,
and "contributor" is an audience rather than a Diátaxis mode — a
`contributing/` directory would be the one top-level entry not organised by
mode.

The consequence to accept knowingly: **the book is not the whole of Kairos's
documentation, and should not claim to be.** `introduction.md` says what the
book covers, not "everything is here". The README keeps the contributor
quickstart (D9), which is what stops a new contributor being stranded.

### D7 — S-0008 is the gate, and gets promoted first

**Dylan, 2026-09-23: promote it as part of this work.** It becomes the
standard every page is reviewed against, and writing a book against a spec
that is itself unfinished is the wrong order — rule IDs could shift halfway
through and invalidate earlier reviews.

So the first task reviews S-0008, walks it out of `discovery`, and
re-renders `plugin/references/diataxis.md` from it. Everything after cites
rule IDs against a settled spec.

Every page is reviewed with the `diataxis-review` skill before its task
closes, and the review cites rule IDs. The point is not ceremony: the spec
exists, the reviewer exists, and a Diátaxis book that was never checked
against either would be an odd thing to ship from this repo.

### D9 — The README becomes a landing page plus a contributor quickstart

**Dylan, 2026-09-23: pointer plus contributor quickstart.** Roughly 80
lines: what Kairos is, a link to the book, how to install the CLI, and
enough to clone / build / test without leaving the repo (`angreal services
up`, `angreal test all`). Everything user-facing moves into the book.

Keeping the quickstart is what makes D6's narrower scope safe: contributor
docs are out of the book, so the README is the entry point that stops a new
contributor being stranded. It is the one place a second reading moment is
accepted deliberately, because a repo landing page has two unavoidable
audiences — someone evaluating the product and someone about to build it.

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

Ten tasks. The first two are strictly ordered — the spec is the gate, and
the scaffold is what everything else writes into. After that the four modes
fan out, and the close-out is last because it reviews the whole book.

1. **Promote [[KAIROS-S-0008]]** out of `discovery` and re-render
   `plugin/references/diataxis.md` (D7). Gate for everything after.
2. **Scaffold the book and publish it** (D1, D8): `docs/book.toml`,
   `src/SUMMARY.md`, `introduction.md`, the four directories, an
   `angreal docs` task, and `.github/workflows/docs.yml` on Pages. Ships an
   empty-but-live book, so every later task has somewhere to land.
3. **Reference: CLI and configuration** — `reference/cli.md`,
   `reference/configuration.md`.
4. **Reference: MCP tools and glossary** — `reference/mcp-tools.md`,
   `reference/glossary.md`, and migrate `docs/api/scim.md` /
   `docs/api/events.md` into `reference/` unchanged.
5. **Reference: REST, generated from the OpenAPI spec** (D5) —
   `reference/rest-api.md` plus its generation step.
6. **Explanation: all five pages** (D4) — flight levels, teams and boards,
   capabilities and access, archiving, repositories as execution scope.
7. **How-to: for operators** — install with Helm, configure an OIDC issuer,
   provision a tenant, connect a git forge, back up and restore.
8. **How-to: for people doing the work, and for agent authors** — set up a
   board, move work between boards, wind down a team, find archived work;
   give an agent machine access, connect over MCP.
9. **Tutorials: the two lessons** — run Kairos locally, deploy to
   Kubernetes.
10. **Close out**: README reduced to a landing page plus contributor
    quickstart (D9), `diataxis-review` run over every page with rule IDs
    cited, cross-links checked, book builds and publishes.
11. **Reference: capabilities and error codes** ([[KAIROS-T-0176]], added
    2026-09-23) — a gap in this plan, not a late addition. D4 named five
    reference pages and none of them is the home for the capability
    vocabulary or the S-0005 error codes, so the explanation pages ended up
    citing `KAIROS-A-0006` — a document outside the book — which is the
    "documented elsewhere only" R4 forbids. Found by [[KAIROS-T-0171]],
    because E6 forces the question "where does this fact actually live?" for
    every fact an explanation page is tempted to state.

Gates per task: the book builds (`angreal docs build`), and every page the
task adds passes `diataxis-review` with its rule IDs cited in the task's
Status Update.

## Progress Log

- 2026-09-23: Created after the v0.1.0 release, from Dylan's "we should stand
  up diataxis documentation next". Surveyed 27 colliery-io repos first: the
  layout, generator and publishing pattern are all existing house convention
  (squire-core, graphqlite, arawn, brokkr), so this initiative follows them
  rather than deciding them. Held in discovery pending the three scope
  questions above — no decomposition yet.
- 2026-09-23: **Scope decided by Dylan.** Contributor docs out of scope
  entirely (so the book covers three audiences and does not claim to be all
  of Kairos's documentation); [[KAIROS-S-0008]] promoted out of `discovery`
  as the first task, since it is the gate; README reduced to a landing page
  plus a contributor quickstart, which is what makes the narrower scope safe.
  Decomposed into ten tasks.
- 2026-09-23: **[[KAIROS-S-0008]] published and the scaffold live** at
  https://colliery-io.github.io/kairos/ (T-0166, T-0167). The spec review
  corrected the initiative on generated reference: the strain is **R1**
  (predictable structure), not R4 (completeness) — generation beats a human on
  completeness precisely because it is mechanical. [[KAIROS-T-0170]] then
  measured it: one generated page came to 3694 lines and was complete and
  unnavigable, so the REST reference is **nine** pages grouped by the
  product's own shape.
- 2026-09-23: **An eleventh task, [[KAIROS-T-0176]]**, for a gap in this
  plan's own reference list — see item 11 above.
- 2026-09-23: **A twelfth, [[KAIROS-T-0179]]**, because D3's claim that the
  two existing API pages were "already single-mode and correct" was an
  assumption that did not survive measurement (see the correction in D3).
- 2026-09-23: **Independent review is not optional for reference pages.**
  [[KAIROS-T-0169]]'s own self-review passed every structural rule (R1–R3)
  and was weak on exactly R4 and R6 — the independent review then found six
  factual defects, one of them blocking: `search` refuses an unresolvable
  `traverse.from` with `NOT_FOUND`, on a page that had just stated the rule
  "a reference that does not resolve is VALIDATION", so an agent would have
  branched on the wrong code. The generalisable reason: **missing refusals are
  invisible to whoever wrote the page.** You cannot notice the absence of a
  case you never thought of. Every remaining reference task in this initiative
  gets an independent review, not a self-review.

- 2026-09-23: **All twelve tasks complete.** The book is live at
  **https://colliery-io.github.io/kairos/** — 36 pages: 1 tutorial, 12 how-to,
  17 reference (8 written, 9 generated), 5 explanation. `SUMMARY.md` has 35
  uncommented entries; the one commented line is the deferred Kubernetes
  tutorial.

  Gates: `angreal docs build` clean, zero broken file links and zero broken
  anchors across all 36 pages, `angreal test lint` clean, `actionlint` clean,
  `render-openapi.py --check` passing, `release.yml` untouched, docs publish
  green.

- 2026-09-23: **What writing the documentation found that testing had not.**
  Eight defects, filed rather than papered over. The pattern is worth naming:
  **R4 forces someone to enumerate a surface, and enumerating a surface is when
  you notice what is missing from it.**

  - [[KAIROS-T-0177]] — Compose silently drops the two variables Google
    Workspace deployments *require*. `.env.example` explains both at length;
    the compose file forwards neither; the server reads both.
  - [[KAIROS-T-0178]] — MCP `create_item` lags the CLI, so an initiative
    created over MCP can never be a bucket.
  - [[KAIROS-T-0180]] — the amd64-only image blocks **every** ARM evaluation,
    including a local `kind` cluster on the most common laptop. Recorded at
    release time as an accepted limitation; it only survived contact with
    somebody following the documented path.
  - [[KAIROS-T-0182]] — `configure_templates` and `configure_metadata` can be
    granted, display as granted, and authorise nothing. Two of the four
    non-`manage_*` capabilities are enforced and two are not, which is why it
    was invisible.
  - [[KAIROS-T-0183]] — `manage_*` also grants `manage_members`, so a glob that
    reads as a work-item family grants board administration.
  - [[KAIROS-T-0184]] — four SCIM defects, two breaking real integrations: a
    `userName` PUT refused forever on an Okta-shaped deployment, and a
    soft-deleted team **permanently burning its slug**.
  - Two stale doc comments corrected in passing: "the 14 frozen tools" (there
    are 18) and `ThinEvent.event` listing 7 of 9 values.

- 2026-09-23: **What the process found, for the next documentation initiative.**

  - **Independent review is not optional for reference pages.** A self-review
    passed every structural rule (R1–R3) and was weak on exactly R4/R6; the
    independent pass then found six defects, one blocking. Later the full
    review found seven blocking, and **every one was a missing refusal or an
    under-stated `details`** — you cannot notice the absence of a case you
    never thought of. Evidence, twice, not principle.
  - **Exemptions are where mode-mixing survives.** The only two pages this plan
    exempted from reclassification were the two nobody had reviewed, and one of
    them ([[KAIROS-T-0179]]) was 20% how-to.
  - **Generated reference strains R1, not R4.** Generation beats a human on
    completeness precisely because it is mechanical; what it fails is
    predictable structure. Measured: one page came to 3694 lines, complete and
    unnavigable.
  - **E6 is what finds missing reference pages.** Forbidding an explanation page
    from being a fact's only home forces the question "where does this live?",
    which is how [[KAIROS-T-0176]] was discovered.
  - **Two pages faithful to two sources can contradict each other.** The
    capability vocabulary says what may be granted; the handler annotations say
    what is enforced. Both were right; the product was wrong.
  - **`git add` does not isolate concurrent agents**, and a private
    `GIT_INDEX_FILE` built from a stale tree is worse — it silently reverts.
    The recipe needs `git read-tree HEAD` immediately before staging, and for a
    file several agents edit, building the blob from `HEAD` plus your own lines
    via `hash-object` + `update-index --cacheinfo`. It caught live reverts three
    times.

**Ready for review.** The initiative is not transitioned — Dylan reviews.
