---
id: fix-web-stale-repo-lens-state
level: task
title: "Fix: web — stale repo lens state, group-by from effective selection, keyed repo lanes, shared power memo, projection tests, a11y, clippy backlog"
short_code: "KAIROS-T-0114"
created_at: 2026-09-22T09:53:04.194205+00:00
updated_at: 2026-09-22T10:41:41.991351+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Stale `?repo=` no longer hides everything: unknown slugs are dropped from the URL and a visible clear control exists; `?by_repo=1` with one repo renders the single-lane view sanely. — `selected_repos` = `parse_repo_query` pruned against `board_repos` (`prune_repos`), an `Effect` writes the pruned form back (history **replace**, so a stale entry is not a back-button trap); the lens row renders whenever a lens param is set, with a `Clear` chip (`data-testid="clear-repo-lens"`); the group-by toggle stays visible while grouping is on so a one-repo `?by_repo=1` (one repo lane + unbound) can be turned off.
- [x] Chips + group-by renders only selected lanes (+ unbound). — `repo_lanes(selected, known)`: the effective selection in board order (all when none), then `None`.
- [x] Repo lanes are keyed; unit tests for the three projections pass. — keyed `<For>` over `Memo<Vec<LaneKey>>` (key = `Option<slug>`); tests `selected_repos_parse_and_prune_against_the_board`, `repo_lane_admits_by_binding`, `lens_filter_narrows_tasks_only`, `repo_lanes_follow_the_effective_selection` (71 kairos-web lib tests pass).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` is clean (first time) and is the gate. — ten backlog errors retired; the angreal tasks had no clippy step at all, so `angreal test lint` (fmt --check + workspace clippy `-D warnings`) was added and `angreal test all` now starts with it (CI's gate 2 already ran `--workspace`).
- [x] `angreal web lint`, `angreal web build`, `angreal test e2e` green. — all green; 11/11 Playwright specs, no e2e selector changes needed (repositories.spec/forge.spec untouched).

## Status Updates

**2026-09-22** — implemented in `231aa25`. All nine implementation notes done. Downstream notes:

- Repository mirrors now live in `crates/kairos-web/src/pages/repositories/api.rs` (`Repository`, `RepositoryRef`, `RepositoryTeam`, `list_repositories`, `set_repository`); `admin::api` re-exports `Repository`/`list_repositories` from there. `boards::data` no longer defines them.
- `item.rs` gained `board_power(board_slug, team_id, kind, pick) -> Memo<bool>` — use it for any further whoami-gated affordance on the detail page rather than re-deriving from `use_context`.
- The repository picker is a raw `<select class="cl-input cl-select">` with `(slug value, "slug · repo_full_name" label)` pairs because aurora's `Select` is value == label; the e2e `selectOption('platform-infra')` keeps matching by value.
- Lens query writes (`repo`, `by_repo`) use `query_signal_with_options` with `replace: true, scroll: false`.
- `kairos-web` now depends on `futures-util` (0.3.31, same as server/client) for the team page's `join!`/`join_all`.
- Gate results: `cargo fmt --all --check` PASS; `angreal web lint` clean; `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (verified in an isolated worktree at HEAD + this change, because the shared working tree carried a concurrent, uncommitted KAIROS-T-0115 edit to `kairos-server/tests/task_repositories.rs` that did not compile); `cargo test -p kairos-web --lib` 71 passed; `angreal web build` OK; `angreal test e2e` PASSED (golden path 10/10, 11 Playwright specs).