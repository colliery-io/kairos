---
id: reference-generate-the-rest-api
level: task
title: "Reference: generate the REST API page from the OpenAPI spec"
short_code: "KAIROS-T-0170"
created_at: 2026-09-23T22:11:16.382014+00:00
updated_at: 2026-09-23T22:32:18.437554+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

`reference/rest-api.md`, **generated** from the OpenAPI spec rather than
written. A hand-maintained endpoint list is the reference page most certain
to rot, and R4 demands completeness across forty-odd endpoints.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].**

The spec already exists: `/api/openapi.json` (KAIROS-T-0023), aggregated from
the `#[utoipa::path]` annotations on every handler.
`crates/kairos-server/tests/openapi.rs` writes a spec artifact in CI, and its
`registered_routes_and_spec_paths_match_exactly` test already guarantees the
spec matches the router — so the spec is trustworthy input, which is the part
that usually is not.

### The design call

Two options; measure rather than assume:

- **A preprocessor** (`mdbook-openapi` and similar) — less code, one more
  dependency, and the output is whatever it gives you.
- **A small script** rendering the spec to markdown before `mdbook build` —
  more code, full control of the output, no new dependency in the book.

Judge on whether the output is actually readable. A wall of generated tables
that nobody can navigate satisfies R4 and fails the reader, which is the
trade to watch. If generation produces something unreadable, a generated
*index* plus hand-written orientation is a legitimate third answer — say so
rather than shipping something unusable.

Wire it into `angreal docs build` so a local build and CI produce the same
page, and make sure the generation step fails loudly rather than silently
emitting an empty page.

## Acceptance Criteria

## Acceptance Criteria

- [ ] `reference/rest-api.md` is generated from the OpenAPI spec, not
      hand-written.
- [ ] Generation runs in `angreal docs build` and in the docs workflow, and
      fails loudly on an empty or malformed spec.
- [ ] The page is navigable by a human, not only complete.
- [ ] The chosen approach and the readability judgement are recorded.
- [ ] `diataxis-review` passes; rule IDs cited, with any tension between R4
      and generated output noted for [[KAIROS-T-0166]]'s spec review.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.**

### The design call: a script, and NINE pages rather than one

Measured before deciding, as the task asked. A single generated page came to
**3694 lines**, of which the schema tables were **1501**. mdBook puts only
chapter titles in the sidebar, so a reader landing there got a wall of text
and a hand-written table of contents: **complete (R4) and unnavigable (R1)**
— precisely the failure [[KAIROS-T-0166]]'s spec review predicted for
generated reference.

Split by group, the sidebar mirrors the product and every page is a sane
size:

| page | lines |
|---|---|
| `rest-api.md` (index) | 24 |
| `rest/work-items.md` | 568 |
| `rest/boards-and-teams.md` | 788 |
| `rest/across-any-work-item.md` | 245 |
| `rest/tenant-configuration.md` | 167 |
| `rest/execution-scope.md` | 165 |
| `rest/machine-access.md` | 136 |
| `rest/the-deployment-itself.md` | 135 |
| `rest/schemas.md` | 1506 |

**Ordering is the R1 answer.** The renderer does not walk `paths` in spec
order — it groups by the operation's OpenAPI tag and orders the groups the way
the product is shaped: the five entity families, then the operations that work
across all of them, then boards and teams, execution scope, tenant
configuration, machine access, and the deployment's own endpoints. A tag not
in that list lands under "Other", which is a deliberate signal that a new
surface was added and the ordering needs review.

Chose a **script** (`scripts/render-openapi.py`) over an `mdbook-openapi`
preprocessor: it sits beside the existing `scripts/render-references.sh`, adds
no dependency to the book, and — the deciding factor — full control of output
is what made the R1 grouping possible at all. A preprocessor would have given
whatever ordering it gives.

### Generated but committed, with CI as the anti-rot guarantee

The pages are **committed**, and `ci.yml` runs
`python3 scripts/render-openapi.py --check` right after the step that already
produces `target/openapi.json`. Reasoning: the docs workflow needs no Rust
toolchain and builds in seconds, so putting generation there would have made
every prose fix wait on a server build. Committing keeps it fast, and the CI
check makes rot impossible — change a handler's `#[utoipa::path]` without
re-rendering and CI says so, naming `angreal docs api`.

`--check` also flags **orphaned** pages, so removing an endpoint group from
the product cannot leave a page behind.

### Failing loudly

Verified by experiment rather than assumed: fed a spec with empty `paths`, the
renderer exits 1 with *"refusing to write an empty reference page"* rather
than emitting a plausible empty page. Missing file and malformed JSON are
handled the same way, each naming the command that fixes it.

### Gates

- `angreal docs api` regenerates; `angreal docs build` clean.
- `--check` passes on the committed pages.
- `actionlint` clean on `ci.yml`.
- Spot-checked rendered output: `POST /api/{entity_type}/{short_code}/restore`
  comes out with both path parameters, all four responses, and working
  `schemas.md#…` links.

### For [[KAIROS-T-0175]]

`SUMMARY.md` nests the eight generated pages under `REST API`. If a future
endpoint group appears, `angreal docs api` writes the page but **`SUMMARY.md`
is not generated** — the new page needs a line adding by hand, and `--check`
will not catch that. Worth a note in the close-out's link check.
