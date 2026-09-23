---
id: j6-team-knowledge-team-directory-a
level: task
title: "J6 team-knowledge: team directory, a page written and merged after a 409, an announcement, charter refusal, my_boards"
short_code: "KAIROS-T-0134"
created_at: 2026-09-23T02:59:16.521209+00:00
updated_at: 2026-09-23T03:07:15.027670+00:00
parent: KAIROS-I-0013
blocked_by: [KAIROS-T-0131]
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# J6 team-knowledge: team directory, a page written and merged after a 409, an announcement, charter refusal, my_boards

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D4: a new journey `uat/journeys/team-knowledge.journey.ts` — "A team writes down how it works". KAIROS-I-0007 has no journey at all.

## Implementation Notes

### Technical Approach

Cast: bob (platform member, the author), carol (non-member, the negative case), alice (admin). Read `e2e/tests/teampages.spec.ts` first — it has the selectors for the page tree, the editor, the 409 merge and the charter protection; the journey tells the same story as a person rather than asserting the widgets.

1. **bob (GUI)** `/teams` — the directory lists the teams; he opens platform.
2. **bob (GUI)** creates a page under the docs tree and writes it (title + markdown), saves. Ledger its delete (`DELETE /api/teams/{id}/pages/{page_id}`).
3. **carol (API)** patches the same page between bob's load and save → bob's save 409s and he merges (the same A-0004 contract as items). Use `e2e/helpers/api.ts::patchTeamPage` as the shape for a `surfaces/` helper if one is missing.
4. **bob (GUI)** posts an announcement; **alice (GUI)** sees it on the platform team page.
5. **carol (GUI)** is refused editing platform's charter (non-member) — assert the refusal as she would see it.
6. **bob (MCP)** `my_boards` — "where do I work" — assert his delivery board is listed with its columns.
- Delete `mcp:my_boards` from ALLOW.

### Dependencies

T-0131.

## Acceptance Criteria

## Acceptance Criteria

- [ ] `angreal test uat --journey team-knowledge` green in both modes; report shows the page, the merge, the announcement and the refusal.
- [ ] `mcp:my_boards` gone from ALLOW, full run green.
- [ ] No `uat-` leftovers (pages and announcements deleted).

## Status Updates

*To be added during implementation*