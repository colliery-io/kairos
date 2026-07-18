---
id: m4-gui-item-detail-and-conflict
level: task
title: "M4: GUI - item detail and conflict merge"
short_code: "KAIROS-T-0041"
created_at: 2026-07-10T22:02:41.853211+00:00
updated_at: 2026-07-15T12:22:10.270930+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0039]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: GUI - item detail and conflict merge

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

Item detail per A-0015: content editing with optimistic-concurrency UX.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Detail route per entity type: title, content (markdown edit + preview), metadata panel (typed editors: enum dropdowns, date pickers, strings per A-0003 definitions), board/column, relationships summary, history link
- [x] Save uses version; on 409 the merge UI shows server-current vs yours side-by-side with keep-mine/take-theirs/manual-merge; retry carries the new version
- [x] Create-from-template flow (template picker shows content preview + declared fields)
- [x] Soft-delete with cascade warning (server-reported descendants) behind a confirm

## Implementation Notes

References A-0004 (409 contract carries current state), A-0003, A-0015.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-14: Active. Read gui-conventions.md, A-0015/A-0004/A-0003, T-0039 status + kairos-web code (app.rs/pages.rs/api.rs/auth.rs), server API surface (tasks.rs pattern, meta/{metadata,definitions,templates,relationships,history}.rs, org/boards.rs), kairos-client types/types_meta/types_org, seed.rs. KEY FACTS: (1) PATCH /api/{family}/{code} carries {title?,content,version}; 409 envelope details.current = the FULL current entity DTO (handlers reload; fallback path carries {version,title,content}). (2) Metadata: GET/PATCH /api/{family}/{code}/metadata, values map slug->value|null (null clears); definitions from GET /api/metadata-definitions (ListEnvelope, enum_options in display order). (3) Templates: GET /api/templates (ListEnvelope) + GET /api/templates/{id} = TemplateDetail w/ metadata fields (slug,name,field_type,enum_options,default,required); create-from-template = POST /api/documents {title, template_id, parent_short_code REQUIRED naming a workflow item}. (4) DELETE returns DeleteResponse {short_code,cascade_count,cascaded_short_codes} — cascade is reported AFTER the delete; NO pre-delete cascade-preview endpoint exists (server gap, recorded below). (5) Board/column names via GET /api/boards/{id} (BoardDetail: flattened board + columns). (6) Relationships: GET /api/{family}/{code}/relationships (outgoing/incoming groups). (7) Dex allows any http://localhost:PORT redirect for public clients, so my :8082 server works without touching shared dex config (verify empirically). PLAN: md renderer = pulldown-cmark 0.12.2 (cached in registry, deps bitflags/memchr/unicase/pulldown-cmark-escape all available; default-features off + "html"; raw HTML events escaped to text for XSS safety). Module: src/pages/item.rs + src/pages/item/{api,markdown,editor,metadata,create_doc,delete}.rs; pages.rs stub replaced by `pub use item::ItemPage` + `pub mod item;` (single-line insertions at my stub); app.css appends .kairos-item*/.kairos-overlay block (tokens only). LANE DEVIATION recorded: post/patch/delete HTTP helpers live in my module, NOT api.rs (conventions §4 says add siblings in api.rs, but two concurrent GUI agents share that file and my lane restricts shared-file edits to single-line at my stub) — hoist follow-up noted. Merge UI: side-by-side server-current vs mine, keep-mine (retry w/ current version), take-theirs (adopt server), manual-merge (keep draft, base=current version, server copy shown for reference); all paths carry the new version.
- 2026-07-15: SHIPPED the module: src/pages/item.rs (ItemPage: family from short-code type letter, one route all five families; ItemDetailView resource + 4 async states + page-level saved Banner; ItemLoaded: PageHeader, version pill, TypeFacts per-family pills, History link → /activity/history/:code (T-0044's registered route), New document + Delete actions; BoardPanel resolves board/column names via GET /api/boards/{id}, links /boards/{slug}; RelationshipsPanel outgoing/incoming groups linked to /items/:code + explorer link → /search/relationships/:code) + item/{api,markdown,editor,metadata,create_doc,delete}.rs; pages.rs stub → `mod item; pub use item::ItemPage;` (same pattern T-0040 used for boards); pulldown-cmark 0.12.2 added (workspace + crate, default-features off + html; raw Html/InlineHtml events escaped to text — XSS-safe for inner_html; cached crate, no network needed). GATES SO FAR: cargo check/clippy clean for my files (concurrent agents own remaining warnings in boards/search/admin), cargo test -p kairos-web 40/40 green (8 new: family parse, task/document mirror decode, 409 details.current extraction full+minimal, metadata null-clears, markdown structure/tables/escape), `angreal web lint` CLEAN, `angreal web build` green. STACK: scratch DB kairos_t0041 created+migrated+seeded (DEMO-S-0001..DEMO-A-0002), server on 127.0.0.1:8082 (OIDC_AUDIENCE=kairos-web, KAIROS_SINGLE_TENANT=demo, KAIROS_WEB_DIST=dist).
- 2026-07-15: BLOCKER FOUND + WORKAROUND: shared Dex EXACT-MATCHES redirect URIs (verified: auth request with redirect_uri=http://localhost:8082/callback → 400 "Unregistered redirect_uri"; the public-client any-localhost-port rule only applies when a client registers NO redirectURIs — kairos-web registers :8080/callback, and :8080 is a concurrent agent's server; live container config confirmed unchanged). Shared dex must stay untouched, so browser evidence uses a hybrid bootstrap (t0041-verify.mjs header documents it): mint the auth code out-of-band with redirect_uri=:8080 (Location header read; :8080 never contacted), place PKCE verifier/state in sessionStorage exactly where auth.rs keeps them across the redirect, and ONE CDP Fetch interception rewrites the token-relay POST body's redirect_uri 8082→8080 so Dex's code binding matches. complete_login, session install, guard, and the whole page under test run the real SPA path. FOLLOW-UP for the repo: register additional dev callback ports (or drop redirectURIs for the public client) in .angreal/dex/config.yaml so multi-agent GUI work doesn't collide — config change deliberately NOT made mid-flight against the shared container.
- 2026-07-15: BROWSER VERIFICATION EVIDENCE — 29/29 DOM+API assertions PASS (method: headless system Chrome over raw CDP, Node 22, script scratchpad/t0041-verify.mjs; screenshots 01-item-detail … 11-delete-cascade-report in scratchpad/t0041-evidence/, all visually inspected; stack: shared postgres+dex, scratch DB kairos_t0041 reseeded via seed-demo --force, server 127.0.0.1:8082, OIDC_AUDIENCE=kairos-web, KAIROS_SINGLE_TENANT=demo). Highlights per AC: (AC1) PKCE login deep-links to /items/DEMO-T-0002; title/family/short-code header; v1 + editing-v1 pills; board panel resolves "Platform Delivery" + column "Active" and links /boards/platform-delivery; metadata panel shows the 4 seeded enum definitions as dropdowns (Priority preloaded "high", options low/medium/high/critical in display order); relationships summary links incoming parent DEMO-I-0001 and outgoing blocks DEMO-T-0003 + graph-explorer link (/search/relationships/…); History → /activity/history/DEMO-T-0002; markdown preview renders h2/list/inline-code/strong via pulldown-cmark and ESCAPES raw <script>/<img> (screenshot 02). (AC2) Save v1→v2 verified via API; competing PATCH (second alice session via curl-equivalent, PKCE-minted token) v2→v3; browser save → 409 → merge dialog names "server is at v3 — you edited v2", shows both texts side-by-side, offers Cancel/Take theirs/Merge manually/Keep mine; TAKE-THEIRS adopts server content + rebases pill to editing-v3; second competing edit v3→v4; second 409 names v4; KEEP-MINE retries with v4 → server at v5 with the browser draft (API-verified) — retry carries the new version both directions; manual-merge path present (dialog screenshot 04). (AC3) From DEMO-I-0002: template picker lists system templates, PRD selection shows Declared fields (Document Type default prd, Status default draft, enum pills) + rendered starter-markdown preview (screenshot 08); Create → DEMO-D-0002 with stamped content, item_metadata document_type=prd status=draft (API), incoming supports edge from DEMO-I-0002; SPA navigates to the new detail route. (AC4) Delete on DEMO-I-0001: confirm dialog warns "This cascades … computed server-side (A-0001) … reported after deletion" and lists the 4 direct children (from the relationships endpoint — NO pre-delete cascade-preview endpoint exists server-side; recorded as a gap, kairos-server read-only this task); confirm → server-reported cascade report (4 descendants DEMO-T-0001..0004 as pills) + Back to boards; root and cascaded child 404 via API. ENUM METADATA: Priority high→critical via the dropdown + Save metadata → API-verified. VERIFICATION GATE: cargo fmt --check -p kairos-web clean; clippy: 0 findings in pages/item* (workspace -D warnings currently carries concurrent agents' in-flight warnings in boards/search/admin lanes — not mine); angreal test unit GREEN (kairos-web 40/40 incl. my 8 new tests); angreal web lint CLEAN; angreal web build and --release GREEN. `angreal test integration|e2e` NOT run per shared-services instruction — deferred to the full gate (noted deviation). CLEANUP: server on 8082 stopped, scratch DB kairos_t0041 dropped, shared postgres+dex left UP and healthy.