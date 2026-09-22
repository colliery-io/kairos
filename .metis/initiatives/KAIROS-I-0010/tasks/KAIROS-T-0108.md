---
id: plugin-repo-scoped-bootstrap
level: task
title: "Plugin: repo-scoped bootstrap, session hook, workflow skills, cross-team filing recipe"
short_code: "KAIROS-T-0108"
created_at: 2026-09-22T03:04:50.693862+00:00
updated_at: 2026-09-22T03:04:50.693862+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0107"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Plugin: repo-scoped bootstrap, session hook, workflow skills, cross-team filing recipe

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D6**. Decision: [[KAIROS-A-0019]] §3. Plugin conventions: [[KAIROS-A-0014]], `plugin/skills/meta/writing-great-skills`.

## Objective

An agent's session is scoped to the repository it is checked out in: bootstrap records which repo this is, the SessionStart hook injects that repo's queue, the workflow skills work against it and refuse to drift into another repo's tickets, and there is a documented recipe for filing work against another team's repo.

## Implementation Notes

- **`.claude/kairos.local.md` contract** (bootstrap SKILL.md + `hooks/session_start.py` `FRONTMATTER_KEYS`): keys become exactly `deployment_url`, `tenant`, `repository`, `delivery_stream`, `team_board`, `initiative_board`. `repository` is the slug. Board keys stay, now derived from the repo's owning team, written for backwards compatibility. Empty value when undiscovered, never omitted.
- **`skills/meta/bootstrap/SKILL.md`** step 3 "detect repo": `git remote get-url origin` → normalize ssh (`git@host:owner/repo.git`) and https forms to `(forge, full_name)` (GitHub/GitLab by host; anything else → `other`) → `list_repositories` match on forge + name → found: write slug, derive boards from its team → not found: offer `create_repository` (user's single team, or ask which) → declined/no remote: leave `repository:` empty and say what unblocks it. Re-run idempotency preserved.
- **`hooks/session_start.py`**: add `repository` to `FRONTMATTER_KEYS`; when set, the live-state hint becomes "call `get_repository <slug>` (read its description), then `board_items` for `<team_board>` with `repository=<slug>` for your queue"; when unset, today's text. Keep it dependency-free.
- **Skills**:
  - `workflow/implement`: read the repo scope; if the task's `repository` differs from the session's, stop and say so (pointer to the right checkout) rather than implementing.
  - `workflow/triage`: queue/search scoped to `repository`; duplicate search stays board-wide.
  - `workflow/decompose`: set `repository` on every task created; when the initiative spans repos, ask per task (or per group) which repo — never leave it unset when the session has one.
  - `engineering/code-review`: verify the PR's repo matches the originating item's `repository`; flag a mismatch as a finding.
  - `meta/kairos` (the umbrella reference): a short "Filing work against another team's repository" recipe: `list_repositories` → `get_repository` (read `description`) → `create_item` with `repository` + `parent`/`blocks` to the originating item → report the short code and that it sits in that team's Backlog awaiting their triage.
- `plugin/README.md` bootstrap section; `plugin/references/` already regenerated in T-0107.

### Dependencies
T-0107 (the MCP tools the skills call).

### Risk Considerations
Skills are prose; the acceptance test is the e2e/dogfood pass in T-0110 plus a manual `/kairos:bootstrap` run against the dev stack from this very repo (register `kairos` as a repo owned by the demo platform team).

## Acceptance Criteria

- [ ] `session_start.py` unit-testable parse + hint logic covered (there is a small test harness pattern in `plugin/hooks` — extend it, or add one).
- [ ] `/kairos:bootstrap` run against the dev stack from this repo writes `repository: kairos` and the derived boards; re-run is a no-op.
- [ ] Each touched SKILL.md passes the `writing-great-skills` checklist (frontmatter description states when to trigger; steps are imperative; no placeholders).
- [ ] Dogfood: with `repository` set, a fresh session's injected context names the repo and the hint uses `get_repository` + filtered `board_items`.
- [ ] `plugin/README.md` updated.

## Status Updates

*To be added during implementation*
