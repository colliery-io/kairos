---
id: repositories-e2e-fixtures-docs
level: task
title: "Repositories e2e + fixtures + docs: repositories.spec, cross-team filing, forge.spec fallout, operator and plugin docs"
short_code: "KAIROS-T-0110"
created_at: 2026-09-22T03:04:53.931224+00:00
updated_at: 2026-09-22T03:04:53.931224+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0107", "KAIROS-T-0108", "KAIROS-T-0109"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Repositories e2e + fixtures + docs: repositories.spec, cross-team filing, forge.spec fallout, operator and plugin docs

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D8**; closes the initiative. Testing tiers: [[KAIROS-A-0012]]. Docs structure: [[KAIROS-S-0008]] (Diataxis).

## Objective

End-to-end proof that the repo-scoped loop works through the real GUI and API against seeded data, the forge e2e adjusted to the re-keyed connection body, and operator/plugin documentation that lets someone set up repositories and cross-team filing without reading the code.

## Implementation Notes

`e2e/` (Playwright, run via `angreal test e2e`; helpers in `e2e/helpers` — `createForgeConnection`, `githubPullRequest` per the code index):

- `repositories.spec.ts`:
  1. Org admin creates a repository for team A on the admin page; it appears on team A's landing page panel.
  2. Team board: repo filter chip and swimlane behave per T-0109 AC; card chip present.
  3. Cross-team filing: as a member of team B (not A), create a task via the API with `repository` = A's repo and no board → it appears in A's Backlog; B's user cannot transition it (403); A's member can. Assert the `blocks` edge when created with `parent`/`blocks`.
  4. Forge link-back: `createForgeConnection` on A's repo, fire a `githubPullRequest` webhook whose title carries the cross-team task's short code → the PR shows in that task's Development panel and in team A's in-flight rollup.
- `forge.spec.ts` + `createForgeConnection` helper: body `{ repository }`; assertions on `repository.slug` instead of the dropped fields.
- Seeded fixtures: T-0103's demo repos are the fixtures; if the e2e seed path differs from `seed_demo`, extend it identically.
- Fallout sweep: `teampages.spec`, `graph.spec`, smoke — anything asserting on task DTO shape or the forge-connection response.

Docs (`docs/`, Diataxis):

- How-to: "Register repositories and connect forge webhooks" replaces the forge setup page from T-0102 (repo first, webhook second).
- How-to: "File work against another team's repository" (human via GUI/CLI, agent via the plugin recipe).
- Reference: `/api/repositories`, task `repository` field and `PUT …/repository`, MCP `list_repositories`/`get_repository`, `.claude/kairos.local.md` keys, `file_backlog` in the ABAC reference.
- Explanation: one short page linking A-0019 — "Boards plan, repositories execute".
- `plugin/README.md` bootstrap section (if T-0108 left it), CHANGELOG/release notes entry noting the `POST /api/forge-connections` body change and `board_id` now optional on task create.

### Dependencies
T-0107, T-0108, T-0109 (everything).

### Risk Considerations
The cross-team spec needs two authenticated users on different teams; the dev Dex config (T-0050) and seed users must cover that — check the seeded demo users' team memberships first and extend `seed_demo` if needed.

## Acceptance Criteria

- [ ] `repositories.spec.ts` passes locally via `angreal test e2e` and in CI.
- [ ] `forge.spec.ts` passes against the re-keyed connection API.
- [ ] Full e2e suite green (no fallout left).
- [ ] Docs pages above exist, are linked from the docs index, and the diataxis-review skill's checklist passes on them.
- [ ] Release notes entry present.
- [ ] Initiative I-0010 Goals all demonstrably met; initiative Status/Progress log updated and ready to transition to completed.

## Status Updates

*To be added during implementation*
