---
id: fix-e2e-plugin-and-docs-coherence
level: task
title: "Fix: e2e, plugin and docs coherence — de-flaked slugs, bob proves the team gate, blocks edge asserted, router sync, recipe placement, wire naming, README split"
short_code: "KAIROS-T-0115"
created_at: 2026-09-22T09:53:06.058473+00:00
updated_at: 2026-09-22T09:53:06.058473+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0111]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: e2e, plugin and docs coherence — de-flaked slugs, bob proves the team gate, blocks edge asserted, router sync, recipe placement, wire naming, README split

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

Make the prose and the specs say exactly what the code does. `repositories.spec.ts` proves less than it claims and can fail deterministically on retry; three skills point agents at things they cannot reach; the wire naming for repository references is inconsistent; the README mixes Diataxis modes under one heading.

## Implementation Notes

**e2e** (`e2e/tests/repositories.spec.ts`, `forge.spec.ts`, `helpers/api.ts`):
1. Suffix every slug the specs register with a per-run id (`test.info().workerIndex` + timestamp) so a `retries: 1` re-run never 409s on "slug taken"; clean up nothing (fresh seed per run) but never collide.
2. "The owning team can transition it" must be **bob** (platform member, non-admin, `bob-password`), not alice; keep alice for admin-only steps.
3. Assert the `blocks` edge from the filed task to a carol-owned item, created **as carol** (needs T-0111); assert it renders in the Relationships panel.
4. Replace the one-shot `count()` comparison with `toHaveCount` on the seeded number of payments-api chips before/after filtering.
5. Drop the seed-coupled `data-repo-lane=""` and `/webhooks/github/demo/` assertions or derive the tenant slug the way `forge.spec` does.

**plugin**:
6. `/kairos` router (`plugin/skills/meta/kairos/SKILL.md`): update the `decompose` and `triage` bullets (repository binding; repo-slice grooming) per the sync rule; the cross-team recipe stays here AND is inlined (or a shared `references/cross-team-filing.md` both point at) in `implement`, which is model-invoked and cannot reach the router.
7. `bootstrap`: say plainly that registering an unknown repo needs the CLI (`kairos repos create`) or an admin, since there is no MCP `create_repository`; match remotes against `list_repositories` (carries forge), not `whoami` (renders no forge); write `team_board` as the board **slug** (the hook's hint calls `board_items` with it) and fix `hooks/session_start.py` + its tests accordingly.
8. `implement`/`triage`/`code-review`: the repo-mismatch guards read the repository from `get_item` (T-0111 renders it); binding an unbound task is `kairos repos bind` (CLI) — say so.

**wire naming** (client + server + CLI): one convention — a repository reference field is `repository` (slug|UUID) everywhere. Rename `CreateTaskRequest.repository_id` → `repository` (keep `repository_id` accepted as a serde alias for one release), make HTTP search `filter.repository` accept slug|UUID (resolve in the handler like board items), `kairos search --repo` takes slug|UUID. Update OpenAPI, README, tests.

**docs** (`README.md`): split the Repositories section into explanation (first paragraph), how-to (register, file, cross-team, bind), reference (routes/CLI table), and move "Upgrade notes" into a new `## Upgrade notes` section near Deployment. Mention `kairos search --repo`. True up `initiative.md` D5/D6/D8 wording to what shipped (`repos bind/unbind`, README not a docs page, no reference regeneration).

## Acceptance Criteria

- [ ] `angreal test e2e` green twice in a row (proves the retry/slug story) with bob transitioning and the blocks edge asserted as carol.
- [ ] Every MCP tool/param named in the skills exists; router bullets match skill behaviour; `implement` carries the recipe.
- [ ] `repository` is the reference field name everywhere on the wire (alias kept); README + OpenAPI updated.
- [ ] README Repositories section passes a Diataxis read: no mode mixing under one heading.

## Status Updates

*To be added during implementation*
