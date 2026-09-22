---
id: gui-repo-filter-swimlane-on-boards
level: task
title: "GUI: repo filter + swimlane on boards, repo chips, team Repositories panel, admin Repositories page, task repo picker"
short_code: "KAIROS-T-0109"
created_at: 2026-09-22T03:04:52.321519+00:00
updated_at: 2026-09-22T04:49:58.556528+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0104, KAIROS-T-0106]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# GUI: repo filter + swimlane on boards, repo chips, team Repositories panel, admin Repositories page, task repo picker

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D7**. GUI conventions: [[KAIROS-A-0015]] (Leptos, Aurora Dark), fine-grained rendering rules from T-0074.

## Objective

Humans on a multi-repo team can see and slice their board by repository, find their team's repositories from the team page, and org admins manage repositories (and their webhooks) in one place.

## Implementation Notes

`crates/kairos-web`:

- **Board view**: a **Repository** multi-select chip in the existing team-lens bar (T-0069); filter is client-side over the already-fetched items (they carry `repository` from T-0104) so WS refetches keep the selection. Optional "group by repo" swimlane toggle when the team owns >1 repo — reuse the Planned/Support lane machinery (T-0077) parameterized on `repository.slug`, with a "No repository" lane last. Selection persisted in the URL query like the team lens.
- **Cards + detail header**: repo chip (slug, links to the repo URL) next to the type badge; the copy-link/short-code affordances (T-0076) unchanged.
- **Item detail (tasks)**: a repository picker (select over `list_repositories` narrowed to the task's team, plus "None") calling `PUT /api/tasks/{code}/repository`; 422 rule violations shown inline in the existing error slot.
- **Team landing page** (T-0085 layout): a **Repositories** panel between the charter and the in-flight rollup: slug, forge icon + link, open tasks, in-flight PR count (from `GET /api/repositories?team=`).
- **Org admin**: the forge-connections page becomes **Repositories**: list; create/edit drawer (slug, forge, full name, URL, default branch, owner team, description); per-row "Connect webhook" runs the existing connection flow (now posting `{ repository }`) and shows the derived secret once as today; "Disconnect" soft-deletes the connection, not the repo. Delete repo disabled with tooltip while referenced.
- Empty states for teams with no repos: one line pointing at the admin page / `/kairos:bootstrap`.

### Dependencies
T-0104 (task DTO + filter + set-repo endpoint), T-0106 (repository API). Can start once both are on `main`; runs in parallel with T-0107/T-0108.

### Risk Considerations
Board rendering has flaked before under WS refetch (T-0073/T-0074); keep the repo filter and swimlane state in signals outside the refetched item list so open menus and selections survive.

## Acceptance Criteria

- [x] Card chip (`.kairos-card__repo[data-repo]`, ICE pill) when bound, absent otherwise; detail header shows `repo: <slug>` among the type facts.
- [x] Repository lens (`[data-testid=repo-lens]`, one chip per slug on the board) narrows tasks only; "Group by repository" (`[data-testid=group-by-repo]`) appears only when >1 repo is present; both live in the URL (`?repo=`, `?by_repo=1`) via `query_signal`, so they survive WS refetches and reloads by construction.
- [x] Picker (`[data-testid=repository-control]`) sets / re-homes / clears via `PUT …/repository`; server 422s render inline in an Alert; gated on the same `manage_tasks` mirror as create (bob on the web team sees none — the team-lens spec's "no select in the Board panel" holds).
- [x] Team page Repositories panel (slug pill, forge · name link, "N open", webhooks/no webhooks), fed by `GET /api/repositories?team=`; the demo seed renders three.
- [x] `/admin/repositories`: register (forge, name, URL, team picker, optional slug/branch/description), edit + re-home, Connect webhook (URL + secret in a once-only panel, `[data-testid=webhook-secret]`), Disconnect (looks up the connection id via the detail), Delete (server 409 surfaces in the mutation notice). Tab + card added; route registered.
- [~] `angreal test e2e`: 9/10 green incl. smoke; the one failure is `forge.spec` on the old connection body — T-0106's deliberate break, fixed in T-0110.
- [x] fmt; `angreal web lint` clean; no NEW clippy findings in `kairos-web` (the 10 remaining are pre-existing under this toolchain); `cargo test -p kairos-web --lib` 65/65; `angreal web build` green.

## Status Updates

- 2026-09-22: Done and committed (`795e3e6`). Notes for T-0110:
  - Selectors: `.kairos-card__repo[data-repo=<slug>]`, `[data-testid=repo-lens] .kairos-board__lens-chip[data-repo=<slug>]` (+ `--on` when selected), `[data-testid=group-by-repo]`, `.kairos-board__lane--repo[data-repo-lane=<slug|''>]`, `[data-testid=repository-control]`, team page `[data-repo=<slug>]` rows, admin `[data-repo=<slug>]` rows and `[data-testid=webhook-secret]`.
  - The group-by view passes `RepoLane` into `LaneColumns`; drag/drop there transitions only (no lane axis), by design.
  - `RepositoryControl` reads the whoami context the item page already provides; anonymous or non-`manage_tasks` users never see the select.