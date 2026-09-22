---
id: repositories-e2e-fixtures-docs
level: task
title: "Repositories e2e + fixtures + docs: repositories.spec, cross-team filing, forge.spec fallout, operator and plugin docs"
short_code: "KAIROS-T-0110"
created_at: 2026-09-22T03:04:53.931224+00:00
updated_at: 2026-09-22T04:55:57.123216+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0107, KAIROS-T-0108, KAIROS-T-0109]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] `repositories.spec.ts` passes via `angreal test e2e` (team panel → board chips/lens/URL state/group-by → carol files cross-team over the API, lands in platform Backlog, 403 on her transition / 200 on the owner's, card chip → item picker re-homes and back → signed PR on a freshly connected repo links back to the filed task, directory shows the webhook → admin page registers `notifier` under web and connects it, secret shown once). CI runs the same `angreal test e2e`.
- [x] `forge.spec.ts` registers `acme/checkout-api` under platform first, then connects by slug; every prior assertion unchanged.
- [x] Full e2e suite 11/11 green.
- [~] Docs live in `README.md` (the project keeps operator docs there; `docs/` holds only the WS/SCIM protocol pages and GUI conventions — no docs index to link from). New "Repositories — where tickets are issued and executed" section (explanation → how-to: register, file incl. cross-team, bind/unbind → reference: routes, CLI, `file_backlog`) and the forge section rewritten repository-first. Self-checked against the Diataxis split (imperative how-tos, reference bodies, one explanation paragraph); the diataxis-review skill was not run end to end.
- [x] "Upgrade notes (KAIROS-I-0010)" in the README records the two contract changes and the migration's fail-loud case; the CLI quickstart mentions `kairos repos list`.
- [x] I-0010 Goals: repositories directory (T-0103/0106) ✓, `tasks.repository_id` + routing + filters (T-0104) ✓, repo-scoped agent loop + MCP tools (T-0107/0108) ✓, cross-team filing + PR link-back (T-0105, proven in e2e) ✓, GUI (T-0109) ✓, V-0001 amended + A-0019 decided ✓. Progress log updated; initiative left in `active` for Dylan's review per the loop contract.

## Status Updates

- 2026-09-22: Done. Commits `7192431` (e2e) and `b8eb956` (docs). One gotcha worth keeping: Dex keeps a single refresh token per user+client, so an e2e spec must mint its API tokens BEFORE the browser logs in, or the next full navigation bounces to Dex (the "in-app navigation only" convention in the older specs was working around the same thing).