---
id: m4-gui-activity-and-history-views
level: task
title: "M4: GUI - activity and history views"
short_code: "KAIROS-T-0044"
created_at: 2026-07-10T22:02:50.657288+00:00
updated_at: 2026-07-15T22:15:17.219269+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0039]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: GUI - activity and history views

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

Activity and history per A-0015.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Item history view: version list, view any version, diff two versions (text diff client-side), rollback via the documented flow (copy-forward as new version)
- [x] Activity feed: filterable (entity, actor, action, since), paginated; entity links to detail

## Implementation Notes

References A-0004, T-0020 history/activity endpoints.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-14: Active. Read gui-conventions.md (THE LAW), A-0015, A-0004 (rollback = copy-forward via the standard versioned save), S-0005 history/activity sections, T-0039 code + status updates (CDP verification method), server handlers (api/meta/history.rs, api/meta/activity.rs) and DTOs (kairos-client types_meta: HistoryVersion/HistorySnapshot/ActivityEntry/ListEnvelope). PLAN: (1) new module `crates/kairos-web/src/pages/activity.rs` (pages.rs stays a file; Rust allows src/pages/activity.rs as its submodule — avoids restructuring the shared file while T-0040..43 agents are concurrent) exporting ActivityPage + ItemHistoryPage; pages.rs stub replaced in place with `pub mod activity; pub use activity::{...};` (single-line insertions at my stub, per lane rules); app.rs gains one route line `activity/history/:code` → ItemHistoryPage + one route-map doc line. ROUTE CONTRACT for T-0041's "history link": `/activity/history/{short_code}`. (2) Diff crate = `similar` (pure Rust, wasm-compatible, line-level TextDiff) — workspace dep, single-line insertions in root + crate Cargo.toml. (3) Activity feed: GET /api/activity with entity_id/actor_id/action/since + limit/offset; entity SHORT CODE filter resolved client-side (code letter → family → GET /api/{family}/{code} → id); actor filter from GET /api/members (open tenant-wide) dropdown; entity links resolved via a client-side id→short_code directory built from the five family list endpoints (documented limit; unresolvable ids render as plain text). (4) History view: version list (GET history), view any version (?version=N), diff two selected versions client-side, rollback = fetch snapshot → fetch current version → PATCH {title,content,version} (copy-forward, A-0004), 409 surfaced with reload guidance per conventions. (5) Local patch_json helper lives in my module NOT api.rs — deliberate lane-isolation deviation (T-0041 will need the same sibling; consolidation into api.rs is a follow-up once concurrent lanes land). (6) Verify: own server :8084 on scratch DB kairos_t0044 (dex allows any localhost port for public clients — redirect_uri derived from window.origin), curl PKCE edits to build versions, headless-Chrome-over-raw-CDP Node script for browser evidence per T-0039's method.
- 2026-07-15: IMPLEMENTED. Shipped crates/kairos-web/src/pages/activity.rs (ActivityPage + ItemHistoryPage + mirrors + 8 host unit tests); pages.rs stub swapped for `mod activity; pub use ...` (same pattern T-0040/41 used); app.rs +1 route line `activity/history/:code` + route-map doc line; `similar = 2.7.0` added (workspace + crate, single lines). CORRECTION to plan item (6): dex does NOT allow arbitrary localhost ports when a public client has registered redirectURIs (verified: authorize with redirect_uri :8084 → "Unregistered redirect_uri"); only http://localhost:8080/callback is registered and dex config is shared/untouchable. Workaround: official instance on :8084 (per lane instructions) + a SECOND instance of the same binary/scratch-DB on :8080 solely for browser evidence; curl PKCE flow uses redirect_uri :8080 and exchanges via the :8084 relay. STATE: scratch DB kairos_t0044 migrated + seeded; alice token minted via curl PKCE; DEMO-T-0001 grown to 4 content versions via PATCH (409-versioned saves); crate green across the whole concurrent fan-out; `cargo test -p kairos-web` 40 passed (incl. my 8); `angreal web lint` clean; `cargo clippy -p kairos-web --all-targets -D warnings` clean; `cargo fmt -p kairos-web --check` clean; `angreal web build --release` green (dist refreshed). NEXT: run scratchpad/t0044-verify.mjs (CDP; 12 screenshots + DOM checks), then curl-verify rollback created v5, gate, cleanup.
- 2026-07-15 08:40: GATES PRE-RUN while the harness safety classifier is down (non-allowlisted Bash like `node …` is temporarily blocked; allowlisted cargo/angreal commands still run): `angreal test unit` green across workspace; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean; `angreal web lint` clean; `angreal web build --release` green. CONSOLIDATION: a concurrent lane added the conventions' `post_json/patch_json/delete_json` siblings to api.rs — my module now calls `api::patch_json` for rollback and the local duplicate was deleted (deviation from plan item 5 resolved; recorded follow-up no longer needed). Remaining: browser evidence (t0044-verify.mjs blocked on classifier availability), rollback curl verification, cleanup.
- 2026-07-15 10:18: STILL BLOCKED on the permission-classifier outage for non-allowlisted Bash (`node t0044-verify.mjs`), retrying since 08:05 with paced waits. Everything else is done and green. DB facts pre-verified via allowlisted psql: DEMO-T-0001 has versions 1..4; activity_log has 32 rows (18 create / 14 relationship_add; alice 26, bob 3, carol 3) so the 25-row page + Next and the bob actor-filter (3 rows) assertions are deterministic. T-0040..43 all completed; T-0050 (register extra localhost callback ports in dev dex) was filed by another lane for the shared redirect-URI constraint. My two servers (:8084 official, :8080 browser-evidence twin) still up; scratch DB intact.
- 2026-07-15 11:30: BROWSER VERIFICATION EVIDENCE (unblocked once Dylan switched permission mode to manual approval). Method: T-0039's — headless system Chrome over raw CDP, Node 22, no installs; scripts scratchpad/t0044-verify.mjs (activity feed) + t0044-verify-history.mjs (history view; separate script because a hard navigation drops the in-memory session by design, so part 2 deep-links /activity/history/DEMO-T-0001 and logs in onto it — also proving T-0041's history-link target works as a deep link). Screenshots 01-12 + results JSONs in scratchpad/t0044-evidence/; 01/02/05/08/10/12 visually inspected (aurora dark shell, alice+demo+admin header, Activity nav active). ACTIVITY FEED — 8/8 DOM checks PASS: login → /activity (return_to honored); feed renders 25 rows "1–25 of 32" [02]; Next → "26–32 of 32" [03]; Previous → back; action=create filter → "1–18 of 18", every pill create [04]; entity short-code filter DEMO-T-0001 (client-side letter→family→id resolution) → exactly the create row, entity cell is an <a href=/items/DEMO-T-0001> link [05]; actor filter bob → exactly his 3 rows [06]; since=2030-01-01 → honest Empty state [07]. HISTORY VIEW — checks PASS (one scripted assertion was itself wrong — it demanded editor=alice on all rows, but v1's editor is CAROL, the seeded creator; screenshot 08 shows the editor column correctly resolving carol vs alice, which is stronger evidence than the assertion): version list v4..v1 newest-first with editor + edited-at, current pill on v4, Roll back disabled on the current row [08]; View v1 → full snapshot panel (title + content) [09]; A=v1/B=v4 radios → client-side diff panel: red '-' deletion + green '+' insertions over full snapshots (`similar` line diff) [10]; Roll back v2 → confirm modal [11] → green banner "the v2 snapshot was copied forward as new version v5", list refreshed with v5 current [12]. ROLLBACK INDEPENDENTLY VERIFIED (curl vs :8084 + psql): GET item → version 5 updated_by alice; history total 5 [5,4,3,2,1]; snapshot v5 content == snapshot v2 content byte-for-byte (psql: v5_content_equals_v2 = t). FINAL GATE RE-RUN post-evidence: `angreal web lint` clean; `cargo fmt --check` clean (workspace); `cargo clippy -p kairos-web --all-targets -- -D warnings` clean; earlier same-code runs: `cargo clippy --workspace --all-targets -- -D warnings` clean, `angreal test unit` green (all crates incl. kairos-web 40/40 with my 8: family mapping, query encoding, timestamp formatting, action colors, similar diff signs, history/activity/member/item-head mirror decode locks), `angreal web build --release` green. DEVIATION on record (shared-services instruction): `angreal test integration|e2e` not run — deferred to the full gate. DIFF-CRATE DECISION: `similar` 2.7.0 (pure Rust, wasm32-clean, line-level Myers/LCS) over hand-rolled LCS. FOLLOW-UP on record: server-side `short_code` on ActivityEntry would retire the client-side id→short_code directory (5 list fetches, first 200/family, documented limit). CLEANUP: both kairos-server instances killed, scratch DB kairos_t0044 dropped (WITH FORCE), no scratch DBs remain, kairos-postgres + kairos-dex confirmed UP (healthy), dex config untouched. Task complete.