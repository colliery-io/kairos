---
id: fix-cross-team-story-end-to-end
level: task
title: "Fix: cross-team story end to end — blocks/parent edge permissions, create_item parent gate, task repository in MCP reads"
short_code: "KAIROS-T-0111"
created_at: 2026-09-22T09:52:58.941427+00:00
updated_at: 2026-09-22T10:08:38.990120+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: cross-team story end to end — blocks/parent edge permissions, create_item parent gate, task repository in MCP reads

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

Make the A-0019 §4 cross-team story actually executable by the principal it was written for: a non-admin member files a task against another team's repository, links it `blocks` back to their own item, and an agent can see a task's repository over MCP. Today step 3 of the recipe 403s (`link_items` is org-admin-only), `create_item` writes a `parent` edge with no check at all, and MCP reads never render `repository`.

## Implementation Notes

1. **Edge permission rule** (`crates/kairos-server/src/mcp/tools.rs` `link_items`/`unlink_items` ~1010-1018; `api/meta/relationships.rs` ~361; and `create_item_impl`'s `parent` write ~2144). Replace the org-admin-only gate for `blocks` and `parent` with: allowed when the caller holds `manage_<family>` on the board of the **target** item (the item receiving the edge) OR on the board of the source, OR the caller **created** the source item (covers a `file_backlog` filer linking what they just filed). Other relationship types (`supersedes`, `supports` on documents) keep today's rule. Put the decision in one server helper (`require_edge_capability`) used by all three entry points; encode the pure part (which types are "collaborative") in `kairos_core::abac`.
2. **`create_item` + `parent`**: route through the same helper — a `parent` edge from a foreign item must satisfy the rule (a filer parenting their task under the OTHER team's initiative is allowed only when they hold manage there; parenting under their OWN initiative is allowed via "created source"/manage-on-source). Test both.
3. **MCP reads render repository**: `get_item` (task branch, ~649-735) adds `repository: <slug> (<owner team>)` or `(none)`; `board_items` rows carry `[repo:<slug>]`; `search` compact listing likewise. Reuse `attach_repositories`/`repository_ref` from `api/convert.rs`.
4. `tests/file_backlog.rs`: extend with — filer links `blocks` from filed task to her own item (200), filer `parent`s under the other team's initiative (403), filer `parent`s under her own initiative (200), admin-only types still 403. `tests/mcp.rs`: `get_item`/`board_items`/`search` show the repository.
5. Update the ADR §4 wording only if the rule differs from "blocks edge from the new task to the originating item" (it should not).

## Acceptance Criteria

- [x] Filer links `blocks` from her filed task to her own item (HTTP create + delete of her own edge; MCP link_items via manage-on-target); two foreign items → 403; `supersedes` → 403 even on her own items.
- [x] `create_item`+`parent` gated by `require_edge_capability_on` BEFORE the insert: foreign initiative → refused with no orphan (task count asserted equal); an initiative she authored → 200 with the edge.
- [x] `get_item` prints `repository: <slug> (owner: <team>)` or `(none)`; `board_items` and `search` rows carry `[repo:<slug>]` (one slug query per call). Asserted in `tests/mcp.rs`.
- [x] fmt, clippy `-D warnings` on core/db/client/server/cli/soak, unit, integration 37/37.
- [ ] `repositories.spec.ts` asserting the edge as carol is T-0115's (now unblocked).

## Status Updates

- 2026-09-22: Done, `21f8a05`. Rule as shipped: collaborative = `parent`, `blocks`; allowed when admin, or manage_<family> on source's board, or on target's board, or caller created the source. Removal is gated identically. `meta.rs`/`mcp.rs` probes that relied on "any relationship write is admin-only" now use `informs`; alice (who manages both boards in those fixtures) is asserted ALLOWED for `parent`/`blocks`, and bob (no grants) is the 403 case for delete. The A-0006 note in `api/meta/mod.rs` docs records the carve-out; the ADR text ("blocks edge from the new task to the originating item") did not need to change.