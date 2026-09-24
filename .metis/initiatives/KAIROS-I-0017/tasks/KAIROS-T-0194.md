---
id: uat-a-coding-agent-works-across
level: task
title: "UAT: a coding agent works across three repositories and keeps them straight"
short_code: "KAIROS-T-0194"
created_at: 2026-09-24T03:25:58.636287+00:00
updated_at: 2026-09-24T03:26:42.455601+00:00
parent: KAIROS-I-0017
blocked_by: []
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

A UAT journey in which one coding agent works across **three repositories** and
keeps them straight. Dylan asked for "basically *Claude uses Kairos for a few
repos*", and the existing [[KAIROS-T-0136]]-era `agent-loop` journey is one
agent, one repository, one ticket — which is not the shape agents are deployed
in. The same agent moves between codebases all day, and every interesting
failure is about crossing between them.

It is also where relevance ranking ([[KAIROS-T-0186]]) gets exercised at the
surface that matters. "Search before you file" is the one habit that stops an
agent re-solving solved work, and before T-0186 it returned the best answer
wherever it happened to fall in creation order.

## Implementation Notes

`uat/journeys/multi-repo-agent.journey.ts`, eleven narrated steps.

### It creates no repositories

The seed already has three across two owner teams, with genuinely different
instructions — `payments-api` ("`cargo test` before every PR"),
`platform-infra` ("Terraform + Helm, plan output in every PR description") and
`portal-web` ("`trunk build` must pass; no hardcoded colors"). The only thing
the journey creates is **one service account that is a member of both teams**,
which is the premise: what an agent may do in a repository is decided by its
team membership, not by what it happens to have checked out.

### The claims it makes

1. **The estate is the union of its memberships** — `whoami` reports two teams
   and all three repositories; `list_repositories` agrees.
2. **Instructions are per repository, and do not carry over.** The three "How to
   work here" openings are asserted pairwise distinct. An agent that reads one
   and assumes the rest runs the wrong test command in two of three codebases
   and nothing tells it off.
3. **The best match comes first without being asked.** Two overlapping tickets
   are raised on one repo, the same two words in one's *title* and the other's
   *body*; a `q` search with no `sort` ranks the title match first. The same
   query with an explicit `created_at desc` returns the **opposite** order, so
   the first assertion cannot be passing by accident of creation order.
4. **Relevance to nothing is refused** — `sort.field: relevance` without `q`
   comes back `VALIDATION: sort.field relevance requires q`, not a silent
   chronological list the agent would believe was ranked.
5. **Work is filed where it belongs, not where the agent was standing.** Working
   in `portal-web` it finds a billing defect and files against `payments-api`;
   the task lands on `platform-delivery / Backlog`, and the journey asserts it is
   *not* on the web board.
6. **It draws the cross-repository edge** it can see and the humans cannot —
   `link_items blocks` from the platform task to its own portal task. This is
   precisely the edge [[KAIROS-T-0192]] will later propose rather than require.
7. **It disturbs only the repository it is working in.** Every repository's
   contents are snapshotted before and after a `transition_item`, and the two it
   did not touch must compare **equal**, not merely plausible.
8. **A repository-scoped read returns that repository only** — asserted
   pairwise, because a three-repo agent that cannot tell whose work it is looking
   at is worse than a one-repo agent.

### Two fixes the journey forced

- `get_item` prints board and column on **one** line (`- board: x / column: y`),
  so `field(item, '- column')` finds nothing. The first run failed on exactly
  that, which is the journey earning its place before it was even finished.
- `whoami` lists repositories in the same `- slug — name` shape as teams one
  section further down, so a naïve regex counted five "teams". Scoped to the
  `## Teams` section.

### Relevance was not reachable from the agent surface

Found while writing this: [[KAIROS-T-0186]] added `relevance` to core, the REST
handler and the DB layer, but the MCP `sort.field` vocabulary and the CLI
`--sort` help still listed only three fields, so an agent could not ask for it
and the MCP tool schema advertised a stale default. Plumbed through
`SearchSortParams`, `search_to_core`, the client DTO, the CLI help and both
reference pages. The OpenAPI drift gate caught the schema page, as designed.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A journey in which one agent works across three repositories, in the
      existing narrated-step shape with a readable report
- [x] It creates no repositories and no teams; teardown leaves the tenant as it
      was found
- [x] Per-repository instructions asserted distinct
- [x] Ranking asserted at the MCP surface, with chronology disagreeing on the
      same query so the assertion cannot pass by accident
- [x] `relevance` without `q` asserted refused
- [x] Cross-repository filing asserted to route by repository owner
- [x] The untouched repositories asserted **equal** before and after
- [x] `relevance` reachable from MCP and the CLI, and documented in both
      reference pages
- [x] `npm run typecheck`, the whole UAT suite (22 journeys) and the surface
      drift gate green

## Status Updates

### 2026-09-23 — done

`angreal test uat` — **22 journeys, 22 passed, 0 failed**, plus the surface
coverage gate. The new journey runs in 395 ms.

The report line that makes the point, from run `mueyw2eb`:

> | 5 | agent | searches before filing anything else, and the best match comes
> back first without being asked | ranked: DEMO-T-0012, DEMO-T-0013;
> newest_first: DEMO-T-0013, DEMO-T-0012; sort_asked_for: none |

The two orders are opposite, which is the whole evidence: ranking is doing the
work, not creation order.

Running the full suite mattered more than running the new journey. Changing the
default sort changes every caller that named none, and `explorer`,
`quarterly-review` and `cross-team` all search. None broke.

### A broken gate found on the way out

Running the one tier this work had not touched, `angreal test e2e`, failed — on
`capture-docs-images.spec.ts`, the documentation-screenshot capture script from
[[KAIROS-T-0185]]. Its own header says it is "@docs-tagged and excluded from
`angreal test e2e`, so it never runs as part of CI", and **nothing did the
excluding**: neither `e2e/playwright.config.ts` nor the angreal task filtered it.
So the e2e tier has failed since T-0185 landed earlier the same day, on a script
that is not a test — it needs state a caller sets up around it and it writes PNGs
into the book.

Fixed by passing `--grep-invert @docs` in the angreal task rather than setting
`grepInvert` in the Playwright config, because a config-level exclusion would
fight the documented deliberate invocation
(`npx playwright test capture-docs-images --grep @docs`). The spec's header no
longer claims an exclusion that does not exist.

`angreal test e2e`: 16 passed.

This is the second thing in this task found by running something rather than
reasoning about it, and both were claims in comments that were not true of the
code. Worth remembering when the next header says a thing is covered.

### Not done

The journey asserts the agent *can* draw a cross-repository edge; it does not
assert anything about **finding** one it was not told about. That is
[[KAIROS-T-0191]]'s signal and [[KAIROS-T-0192]]'s proposal, and
[[KAIROS-T-0193]] extends this journey with the ask → prior art → propose →
confirm arc once they exist. This journey is deliberately the foundation that
extension builds on rather than a second journey beside it.