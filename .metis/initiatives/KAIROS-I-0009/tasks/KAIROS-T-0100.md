---
id: links-read-api-item-detail
level: task
title: "Links read API + item-detail Development panel"
short_code: "KAIROS-T-0100"
created_at: 2026-09-01T23:12:36.803347+00:00
updated_at: 2026-09-02T10:24:44.952033+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Links read API + item-detail Development panel

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. Depends on KAIROS-T-0097 (schema); usable as soon as KAIROS-T-0099 is landing data.

## Objective

Surface the links where the work is: `GET /api/{family}/{code}/links` plus a Development panel on the item detail page showing branches and PRs with their state.

## Implementation Notes

- **Endpoint**: `GET /api/{entity_type}/{short_code}/links` in `crates/kairos-server/src/api/meta/relationships.rs` — the family-generic module that already hosts `/relationships`, `/children-progress`, and `/graph`, and already has `resolve_family_item` for the 404-on-family-mismatch behaviour. Open tenant-wide like its siblings. Register in openapi.
- Response rows carry what the panel renders: `kind`, `external_id`, `title`, `url`, `state`, `author`, `repo_full_name` + `forge` (joined from `forge_connections`), `forge_updated_at`. Order: PRs before branches, then newest `forge_updated_at` first — put the ordering in SQL so every client agrees.
- **DTOs** in `crates/kairos-client/src/types_forge.rs` + a client method, plus the partial mirror + decode test in the web crate (`pages/item/api.rs`), per gui-conventions §4.
- **Web panel** (`crates/kairos-web/src/pages/item.rs`): a `Panel` titled "Development", rendered **only when non-empty** — no always-rendered empty panel (the standing rule since the T-0089 explorer rework). Each row: forge + repo (mono, dimmed), the PR/branch title as an `Anchor` to the forge URL, and a state `Pill`.
- **State colors**: `open` → ICE, `merged` → OK/VIOLET, `closed` → MUTED, `draft` → MUTED. **Red stays reserved** for violated/at-risk (the standing convention from KAIROS-I-0008) — a closed PR is not an error state. Tokens only; `angreal web lint` enforces.
- External links leave the app: add `rel="noopener noreferrer"` and `target="_blank"` (first place Kairos links off-origin — check whether aurora's `Anchor` supports it, and if not use a plain `<a>` with the existing anchor classes).
- Live refresh: the panel refetches on the `item_links_changed` WS event from KAIROS-T-0099, reusing the item page's existing reload handle.

## Acceptance Criteria

## Acceptance Criteria

- [x] `GET /api/{family}/{code}/links` returns the item's branches and PRs with repo/forge context; 404 on family mismatch like its sibling endpoints; open tenant-wide; openapi registered.
- [x] Ordering is server-side and stable (PRs first, newest first).
- [x] kairos-client DTOs + method; web mirror with a decode test.
- [x] Item detail renders a Development panel only when links exist; rows link out to the forge with `noopener noreferrer`; state chips use tokens with no red.
- [x] An ingested webhook updates the open item view without a manual refresh.
- [x] Unit + lint + build green; server integration coverage for the endpoint.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
- 2026-09-02: COMPLETE. `GET /api/{entity_type}/{short_code}/links` added to meta/relationships.rs beside `/relationships`, `/children-progress`, and `/graph` — same `resolve_family_item` 404 behaviour, open tenant-wide, openapi registered. Ordering is SQL-side in `links_for_item` (`kind DESC` puts pull_request before branch, then `forge_updated_at DESC`). `types_forge::ItemLink` + `client.item_links()`; partial mirror + decode test in `pages/item/api.rs`.
- 2026-09-02: Web: `DevelopmentPanel` between Metadata and Relationships, rendering **nothing** when there are no links — including while loading or on error, since the panel is additive context and never the page's job (the standing no-empty-panels rule from the T-0089 rework). Rows: state Pill (open=ICE, merged=VIOLET, draft/closed=MUTED — **no red**, a closed PR is a normal outcome), the PR title as a plain `<a class="cl-anchor" target="_blank" rel="noopener noreferrer">` (aurora's `Anchor` has no target/rel props, and this is the first place Kairos links off-origin), and `forge · owner/repo` mono-dimmed.
- 2026-09-02: Live refresh required NEW plumbing, not a reuse: the item page had **no WS subscription at all**. Wired the panel to `subscribe_all_events` (the whole-tenant helper generalized for the graph view in KAIROS-T-0090), refetching when an event names this short code, with the guard in `StoredValue::new_local` + `on_cleanup` per the boards drop-guard discipline.
- 2026-09-02: Verified: `forge_webhook` extended to assert the endpoint returns what ingestion wrote (state, kind, external_id, forge, repo) plus a family-mismatch 404 — test green. `angreal test unit` clean, `angreal web lint` clean, `angreal web build` succeeds. Browser proof of the live path rides KAIROS-T-0102's spec.