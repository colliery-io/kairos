---
id: blocked-by-blocks-board-card
level: task
title: "Blocked-by/blocks board-card badges: rollup, web rendering, click-through"
short_code: "KAIROS-T-0091"
created_at: 2026-08-29T14:25:05.686953+00:00
updated_at: 2026-08-29T15:30:31.035865+00:00
parent: KAIROS-I-0008
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0008
---

# Blocked-by/blocks board-card badges: rollup, web rendering, click-through

## Parent Initiative

[[KAIROS-I-0008]] — Focal Flight-Level Graph View. Option B's strongest piece, pulled in by PO decision. Independent of the canvas tasks except for click-through targets; can run after KAIROS-T-0088 lands.

## Objective

Jira-style dependency badges on board cards: blocked-by and blocks counts visible at a glance on every board, clicking through to the item's graph.

## Implementation Notes

- **Server rollup**: extend the board-items payload (`GET /api/boards/{id}/items`) with per-item `blocked_by`/`blocks` counts — one grouped query over `item_relationships` (relationship = 'blocks') for the board's item id-set, counting live endpoints only (soft-deleted neighbors excluded, matching the T-0080 children-progress precedent `board_children_progress`). Riding the existing payload beats a sibling endpoint (the board view already fetches it; no extra round-trip) — record the choice on this task if reality disagrees.
- **DTOs**: counts on the board-item DTO in kairos-client + the kairos-web board mirror (decode test).
- **Web** (boards.rs cards): render nothing when both counts are zero; otherwise small neutral-accent pills — `⛔ blocked by N` / `⛓ blocks N` textual equivalents in the existing Pill vocabulary (MUTED/ICE accents — NO red; red stays reserved). Clicking a badge navigates to the item's graph (`/search/relationships/:code` until T-0090's tab lands, then the canonical target — coordinate with T-0090 on the final href).
- **WS**: counts refresh with the board's existing ItemUpdated refetch — no new subscription work.
- **Watch**: fine-grained board rendering (T-0074) — badges must not break the open-menu-survives-refetch behavior; stale-count tests in seed_demo/e2e ride KAIROS-T-0092.

## Acceptance Criteria

## Acceptance Criteria

- [x] Board items payload carries blocked_by/blocks counts; soft-deleted neighbors excluded; server integration test covers both directions and the exclusion.
- [x] Cards show the badges only when nonzero, in neutral accents (no red anywhere); the seeded blocks web renders visible badges on the stock demo.
- [x] Badge click navigates to the item's graph view.
- [x] Counts update live with the board's existing WS refetch.
- [x] kairos-client DTOs + web mirror decode test; unit + lint + build green; ladder green for touched tiers.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0008 decomposition (design PO-approved; badges pulled into the initiative).
- 2026-08-29: COMPLETE. Choice confirmed: counts RIDE the board-items payload (`blocks_summary: BTreeMap<short_code, BlocksCounts>` beside T-0080's `children_progress` — same shape, `#[serde(default)]` so old payloads decode). db: `graph::blocks_summary(conn, ids)` — one grouped query with `COUNT(*) FILTER` per direction and a live-only `entity_directory` join on the OTHER endpoint (soft-deleted neighbors never count); items with no live blocks edges get no entry. Web: `CardModel.blocks` (in the card diff key, so WS refetches rebuild exactly the changed cards — the T-0074 discipline holds), badges render only when nonzero as GOLD "blocked by N" / ICE "blocks N" pills (no red) wrapped in anchors to `/items/{code}?view=graph` (T-0090's canonical target). Counts refresh with the board's existing WS refetch — zero new subscription work.
- 2026-08-29: Verified: kairos-db graph test extended (t2 blocks t3 then soft-deleted → t3 blocked_by counts only t1; no-edges items absent), meta.rs asserts both directions on the live board payload, boards/data.rs decode test covers the absent-default and populated shapes. `angreal test unit` clean, web lint/build clean, `angreal test integration` 32/32 ok. The stock demo seed's two blocks edges make badges demo-visible (browser-asserted in T-0092).