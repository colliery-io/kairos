---
id: reference-generate-the-rest-api
level: task
title: "Reference: generate the REST API page from the OpenAPI spec"
short_code: "KAIROS-T-0170"
created_at: 2026-09-23T22:11:16.382014+00:00
updated_at: 2026-09-23T22:11:16.382014+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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
