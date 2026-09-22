---
id: fix-web-stale-repo-lens-state
level: task
title: "Fix: web — stale repo lens state, group-by from effective selection, keyed repo lanes, shared power memo, projection tests, a11y, clippy backlog"
short_code: "KAIROS-T-0114"
created_at: 2026-09-22T09:53:04.194205+00:00
updated_at: 2026-09-22T09:53:04.194205+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: web — stale repo lens state, group-by from effective selection, keyed repo lanes, shared power memo, projection tests, a11y, clippy backlog

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

The board's repository lens honours URL state it cannot show: a stale `?repo=<slug no longer on the board>` blanks every task with no chip lit and no control to clear it, and `?by_repo=1` still groups when the toggle is hidden. Group-by lanes come from the unfiltered slug set, so chips + group-by yields empty lanes. Plus the smaller web findings and the ten pre-existing clippy errors.

## Implementation Notes

`crates/kairos-web`:
1. **Lens state** (`src/pages/boards.rs` BoardBody): prune `selected_repos` against `board_repos` in the memo (unknown slugs are ignored, and written back out of the URL); render the lens row whenever a query param is set even if `board_repos` is empty, with a "clear" chip; group-by lanes derive from the effective selection (selected slugs, or all when none) + the unbound remainder.
2. **Keyed lanes**: render repo lanes with a keyed `<For>` (key = slug) rather than `collect_view` inside `move ||`, so a WS refetch that changes the slug set doesn't rebuild every lane (T-0074 rule).
3. **Admin default team** (`src/pages/admin/repositories.rs` ~142-156): no signal write inside a tracked render — mirror `admin/boards.rs`'s empty-first-option approach or set the default in an `Effect`. Also: the disconnect action must report "no webhook to disconnect" when `connection_id` is `None`; inline `style` must use `var(--gold)` not the hex token (conventions §2).
4. **Shared power memo** (`src/pages/item.rs`): extract `fn board_power(board_slug, team_id, kind, pick) -> Memo<bool>` used by `MoveControl` and `RepositoryControl`; picker shows `repo_full_name` next to the slug; when `current` is not in `options` (re-homed away) say so instead of a silently disabled button; drop the dead `options` work before the empty early-return.
5. **Module home**: move the repository mirrors (`Repository`, `RepositoryRef`, `RepositoryTeam`, `list_repositories`, `set_repository`) out of `boards/data.rs` (they sit after `mod tests`) into `src/pages/repositories/api.rs`; admin/api.rs re-exports from there, not from boards.
6. **Projection tests**: `selected_repos` parsing/pruning, `RepoLane::admits`, the task-only retain filter — alongside the existing `card_lane`/`drop_effect`/`band_models` tests.
7. **a11y**: `aria-pressed` on lens chips and the group-by toggle.
8. **Team page**: fetch repositories concurrently with the other five loads (`futures::join!` or `join_all`) and degrade to an empty panel on error rather than failing the page.
9. **Clippy backlog** (pre-existing, all trivial): `search/data.rs:226` unused `group`; `boards/live.rs:50`, `boards.rs:276`, `:301` type_complexity → `type` aliases; `boards.rs:240`, `:246` collapsible_if; `search/graph.rs:394` clone-on-Copy; `teams/doc.rs:5-7` doc indentation. Then add `-p kairos-web` to the clippy gate in `.angreal/task_test.py` (or wherever the gate runs) so `-D warnings` covers the whole workspace.

## Acceptance Criteria

- [ ] Stale `?repo=` no longer hides everything: unknown slugs are dropped from the URL and a visible clear control exists; `?by_repo=1` with one repo renders the single-lane view sanely.
- [ ] Chips + group-by renders only selected lanes (+ unbound).
- [ ] Repo lanes are keyed; unit tests for the three projections pass.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean (first time) and is the gate.
- [ ] `angreal web lint`, `angreal web build`, `angreal test e2e` green.

## Status Updates

*To be added during implementation*
