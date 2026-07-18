---
id: m4-gui-search-and-relationships
level: task
title: "M4: GUI - search and relationships views"
short_code: "KAIROS-T-0042"
created_at: 2026-07-10T22:02:48.357961+00:00
updated_at: 2026-07-15T12:05:11.750097+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0039]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: GUI - search and relationships views

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

Search and relationship exploration per A-0015.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Search page: text query + filter builder (entity types, board, column, task type, metadata key/values, date range) + optional traverse root/direction/depth; results grouped by type, paginated; result rows link to detail
- [x] Relationships view on item detail expanded: parent chain breadcrumb, children, blockers/blocking (both directions), supporting documents/ADRs; org-admin link/unlink affordances with the API's typed errors surfaced

## Implementation Notes

References A-0007/S-0005 search shape, T-0020 relationships endpoints.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-14: Active (shared-services mode, four concurrent GUI agents). Read gui-conventions.md, A-0015, A-0007 (POST /api/search shape), kairos-client types_search.rs + types_meta.rs (T-0020/T-0021 wire types), server relationships handler (typed 422s: RELATIONSHIP_RULE / CYCLE_DETECTED / ALREADY_LINKED), seed.rs, T-0039's kairos-web code + status updates. PLAN: (1) RELATIONSHIPS PLACEMENT DECISION - conventions §3 says "add sub-routes under your own path segment only", and /items/* belongs to T-0041 (concurrent agent, module untouchable), so the relationships explorer mounts at /search/relationships/:code (T-0042's segment), implemented as an exported reusable component `pages::search::relationships::RelationshipsView` that the T-0041 agent can later mount inside ItemDetail (handoff recorded here; their module NOT edited). Search result rows link to /items/{code} (detail) AND to the relationships route. (2) Module layout: pages.rs stub replaced with `pub mod search;` + re-exports (pages.rs stays a file - Rust resolves child modules at src/pages/search.rs, no directory conversion, no collision with concurrent stub owners); new files src/pages/search.rs (SearchPage), src/pages/search/data.rs (mirror DTOs + domain fetchers, kept in T-0042's module to avoid concurrent api.rs churn), src/pages/search/relationships.rs. Shared-file edits limited to: pages.rs stub block, app.rs one route line + route-map doc line, api.rs append post_json/delete_json siblings (sanctioned extension point per conventions §4). (3) Search page per AC: q + filter builder (entity-type chips, board Select from GET /api/boards, dependent column Select from GET /api/boards/{id}, task-type chips, metadata k=v rows, created after/before dates) + optional traverse (root short code, relationship chips, direction SegmentedControl, depth), POST /api/search, results grouped by type, pagination over total/limit/offset. (4) Relationships view: ancestor breadcrumb via iterative incoming-parent walk, children (outgoing parent), blocked-by/blocks, supporting docs/ADRs (supports/informs/supersedes both directions), org-admin (whoami role) link/unlink surfacing typed 422 codes via Alert per conventions error pattern. (5) Evidence: own server :8085, scratch DB kairos_t0042, seeded via seed-demo; Dex only registers localhost:8080/callback for kairos-web, so CDP verification uses Fetch-domain request interception (rewrite redirect_uri 8085->8080 on the authorize request, fulfill localhost:8080/callback with a 302 to 8085, rewrite redirect_uri in the relay POST body) - dex config untouched.
- 2026-07-14: IMPLEMENTED. New files: crates/kairos-web/src/pages/search.rs (SearchPage: text query, entity-type/task-type chips, board+dependent-column Selects from /api/boards, metadata k=v For-rows, created after/before dates, traverse Switch block with from/relationship-chips/direction SegmentedControl/depth NumberInput, page-size Select; POST /api/search via submitted-request signal; results grouped per type with code->/items/{code} links + relationships links; Showing x-y of total + Prev/Next editing offset in place), src/pages/search/data.rs (mirrors + fetchers + 4 host unit tests: family_of letters, request field-name lock incl. {} for empty, response group decode, relationships decode), src/pages/search/relationships.rs (RelationshipsPage route wrapper + standalone RelationshipsView: lineage breadcrumb via bounded cycle-guarded incoming-parent walk, Children, Blocked by, Blocks, Supporting material [supports/informs/supersedes both directions with direction-labelled pills], whoami-gated org-admin Link form [role SegmentedControl source/target, other short code, relationship Select] + per-row Unlink, mutations set a feedback signal rendered as Alert-ok / ErrorState [which prints 'code: CYCLE_DETECTED' etc.] and bump a reload counter to refetch). Shared-file edits: pages.rs stub -> `mod search;` + re-export (same pattern the concurrent agents used for boards/item/activity), app.rs +1 route line /search/relationships/:code + route-map doc row, api.rs post_json/delete_json siblings + shared decode_response tail (NOTE: a concurrent agent added a duplicate post_json mid-flight; merged to one canonical copy - signatures/behavior identical). `angreal web lint` clean. Scratch DB kairos_t0042 created+migrated+seeded (16 short codes, 14 edges). Full-crate cargo check currently blocked ONLY by T-0041's in-flight pages/item module errors (their lane); waiting on green to run tests + trunk build. CDP script ready at scratchpad/t0042-verify.mjs (headless Chrome :9242, Fetch-interception login bridge, 12 DOM assertions + 9 screenshots planned: text search 'sign-up', bug filter, traverse S-0001 w/ page-size 5 pagination both directions, relationships on I-0001 breadcrumb/children/PRD, admin link I-0001 supports A-0001, unlink it, cycle attempt T-0003 blocks T-0002 -> CYCLE_DETECTED, parent task->task -> RELATIONSHIP_RULE).
- 2026-07-15: BROWSER VERIFICATION EVIDENCE (T-0039's CDP method: headless system Chrome driven over raw CDP by a Node 22 script, no installs; script scratchpad/t0042-verify.mjs, screenshots 01-09 in session scratchpad t0042-evidence/, 01/05/08 visually inspected - aurora dark shell, alice+demo+admin header, all sections rendered). Stack: shared compose postgres+dex UNTOUCHED, own scratch DB kairos_t0042 (migrate + seed-demo: 16 items, 14 edges), debug server 127.0.0.1:8085 with OIDC_AUDIENCE=kairos-web KAIROS_SINGLE_TENANT=demo KAIROS_WEB_DIST=dist. Dex-redirect-URI mismatch (only :8080/callback registered) bridged purely in the test harness via CDP Fetch interception (authorize redirect_uri rewrite, local 302 fulfillment of :8080/callback back to :8085, relay POST body rewrite) - production code and dex config untouched. RESULT 14/14 PASS: (1) PKCE login lands /search; (2) whoami alice; (3) text search "sign-up" -> grouped Initiatives/Tasks/Documents incl. DEMO-T-0001 + DEMO-D-0001 [01-text-search.png]; (4) filter task_type=bug -> exactly DEMO-T-0007 "Showing 1-1 of 1" [02]; (5) traverse DEMO-S-0001 outbound parent depth 3 -> 8 descendants (2 initiatives + 6 tasks) [03]; (6) pagination page-size 5: Next -> "Showing 6-8 of 8" [04]; (7) Prev -> back to 1-5; (8) relationships DEMO-I-0001: breadcrumb strategy DEMO-S-0001 "Self-serve customer onboarding" > DEMO-I-0001; (9) children = 4 seeded tasks each w/ explore + admin Unlink; (10) supporting material = DEMO-D-0001 PRD with "supports ->" pill [05]; (11) admin link Create: DEMO-I-0001 supports DEMO-A-0001 -> success alert + row appears on refetch [06]; (12) admin Unlink of that edge -> success, seed state restored [07]; (13) cycle attempt on DEMO-T-0003 (blocks DEMO-T-0002, reverse of seeded edge) -> gold "Invalid request" ErrorState w/ message "...would create a cycle" + "code: CYCLE_DETECTED" [08]; (14) parent task->task -> "code: RELATIONSHIP_RULE" [09]. VERIFICATION GATE: `angreal web lint` clean; `angreal web build` AND `--release` green (one transient wasm-opt copy race with a concurrent agent's trunk build, clean on retry); `cargo test -p kairos-web` 40 passed incl. my 4 new host tests (family_of letters, S-0005 request field-name lock, response-group decode, relationships decode); `angreal test unit` green (9x "test result: ok", 0 failures); `cargo clippy -p kairos-web --all-targets -- -D warnings` clean; `cargo fmt --check` clean for all pages/search* files (workspace-wide fmt/clippy state owned by the four concurrent lanes jointly). DEVIATION (per shared-services instruction): `angreal test integration|e2e` NOT run - deferred to the full gate. HANDOFF for T-0041: `pages::search::relationships::RelationshipsView` takes only `short_code: String` (auth via context) and can be mounted inside ItemDetail as-is; route /search/relationships/:code stays regardless. Cleanup: scratch DB kairos_t0042 force-dropped (DROP DATABASE ... WITH (FORCE)), headless verification Chrome exited (script kills it on completion), shared services (kairos-postgres, kairos-dex) confirmed UP, dex config untouched.
- 2026-07-15: RESIDUAL: the scratch kairos-server process (pid 99654, 127.0.0.1:8085) could not be killed before session end - the harness Bash permission classifier was down for 25+ minutes and every `kill`/`pkill` attempt (retried across ~15 windows) was rejected by the outage, while allowlisted commands (docker exec psql, docker ps, echo, cat, ls) still worked, which is how the DB drop landed. The process is inert: its database no longer exists (all API calls 5xx), it binds only the T-0042-assigned local port 8085, and it touches no shared services. `kill 99654` clears it.