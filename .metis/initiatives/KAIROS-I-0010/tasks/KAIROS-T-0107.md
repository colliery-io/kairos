---
id: mcp-client-cli-list-repositories
level: task
title: "MCP + client + CLI: list_repositories, get_repository, repository on create_item/board_items/search/whoami, kairos repos commands"
short_code: "KAIROS-T-0107"
created_at: 2026-09-22T03:04:49.197406+00:00
updated_at: 2026-09-22T03:04:49.197406+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0104", "KAIROS-T-0105", "KAIROS-T-0106"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# MCP + client + CLI: list_repositories, get_repository, repository on create_item/board_items/search/whoami, kairos repos commands

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D5**. Decision: [[KAIROS-A-0019]] §3.

## Objective

An agent can discover repositories (owner, board, conventions, in-flight PRs), scope its reads to one repo, and create a task against any repo — including another team's — from the MCP surface. The CLI gets the same surface for humans and scripts.

## Implementation Notes

MCP (`crates/kairos-server/src/mcp/tools.rs`, following the existing `#[tool]` + `Params` struct pattern; every tool gets a description that tells an agent *when* to use it, per S-0006):

- `list_repositories { team?: String }` → per §D5 shape. Description: "Directory of repositories in this tenant: who owns each, which delivery board its tickets land on, open work. Call before filing work against a codebase you are not checked out in."
- `get_repository { repository: String }` → §D5 shape incl. `description` and `in_flight`. Description mentions reading `description` before working in an unfamiliar repo.
- `CreateItemParams.repository: Option<String>` (tasks only; `reject_field` for other types). `board` becomes optional when `repository` is given (delegates to the §D2 resolution in the shared server helper from T-0104/T-0105 — do not reimplement routing in MCP). Description gains: "Any member may create a task against another team's repository; it lands in that board's Backlog."
- `BoardItemsParams.repository`, `SearchFilterParams.repository` (slug or UUID).
- `whoami` gains `repositories: [ {slug, team} ]` for the caller's teams and the `implicit` capability list from T-0105 (if T-0105 put it on the API DTO, MCP just passes it through).
- Regenerate `plugin/references/` tool docs (angreal task if one exists; check `angreal tree`).

kairos-client (`crates/kairos-client`): already has the HTTP methods from T-0104/T-0106; this task only adds what MCP/CLI need that is missing.

CLI (`crates/kairos-cli`): `kairos repos list [--team]`, `kairos repos get <slug>`, `kairos repos create --slug --forge --name --url --team [--default-branch] [--description]`, `kairos repos update <slug> [...]`, `kairos repos delete <slug>`; `kairos tasks create --repo <slug>` (board becomes optional), `kairos tasks list --repo`, `kairos tasks set-repo <code> <slug|none>`. Table + `--json` output like the neighbours.

### Dependencies
T-0104, T-0105, T-0106.

### Risk Considerations
The MCP `create_item` path and the HTTP path must share the routing + capability helper; a divergence here is exactly the class of bug T-0096 recorded for `set_metadata`. Add an MCP-level test for cross-team filing so the two paths are covered independently.

## Acceptance Criteria

- [ ] MCP integration tests: `list_repositories`/`get_repository` shapes; `create_item` with `repository` and no `board` routes correctly; cross-team filing via MCP lands in Backlog; `board_items`/`search` with `repository` filter.
- [ ] `whoami` (MCP) shows `repositories` and `implicit`.
- [ ] `plugin/references/` regenerated and committed.
- [ ] CLI commands exercised in the client/CLI integration suite (at least create/list/get/set-repo).
- [ ] fmt/clippy/unit/integration green.

## Status Updates

*To be added during implementation*
