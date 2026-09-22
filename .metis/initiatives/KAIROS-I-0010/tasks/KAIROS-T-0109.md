---
id: gui-repo-filter-swimlane-on-boards
level: task
title: "GUI: repo filter + swimlane on boards, repo chips, team Repositories panel, admin Repositories page, task repo picker"
short_code: "KAIROS-T-0109"
created_at: 2026-09-22T03:04:52.321519+00:00
updated_at: 2026-09-22T03:04:52.321519+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0104", "KAIROS-T-0106"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] Repo chip visible on task cards and detail header when set; absent otherwise.
- [ ] Filter chip narrows the board; swimlane toggle appears only for >1 repo; selection survives a WS refetch and a reload (URL state).
- [ ] Task repo picker sets/clears/re-homes; a cross-team repo shows the 422 message inline.
- [ ] Team page Repositories panel renders the seeded demo repos with correct counts.
- [ ] Admin Repositories page: create, edit, connect webhook (secret shown once), disconnect, delete-refused-while-referenced.
- [ ] `angreal test e2e` smoke suite still green (new specs are T-0110's).
- [ ] `cargo fmt --check`, clippy `-D warnings` on `kairos-web`; `angreal build web` green.

## Status Updates

*To be added during implementation*
