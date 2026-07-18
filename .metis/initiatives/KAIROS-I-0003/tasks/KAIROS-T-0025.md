---
id: m2-scim-2-0-provisioning-endpoint
level: task
title: "M2: SCIM 2.0 provisioning endpoint"
short_code: "KAIROS-T-0025"
created_at: 2026-07-10T09:02:14.230121+00:00
updated_at: 2026-07-10T22:53:38.622367+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0019]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: SCIM 2.0 provisioning endpoint

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Implement the inbound SCIM 2.0 server per KAIROS-A-0016 (decided): per-tenant `/scim/v2/Users` and `/scim/v2/Groups` so enterprise IdPs (Okta, Entra, Auth0, …) push user/group lifecycle into Kairos — including the deprovisioning path JIT cannot cover.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] SCIM token management: org-admin API to create/list/revoke per-tenant SCIM bearer tokens; tokens hashed at rest; SCIM requests authenticate by token only (tenant-scoped by the token, not subdomain); new tenant-schema table via migration
- [x] `/scim/v2/Users`: POST (create/link user + membership), GET by id, GET list with `filter=userName eq "..."` (the filter subset IdPs actually send), PATCH (RFC 7644 ops incl. `active: false` → membership revoked immediately), PUT replace, DELETE (revoke membership; users row retained for audit integrity); SCIM error envelope (RFC 7644 §3.12) throughout
- [x] Identity join: SCIM `externalId`/`userName` → `public.users.external_id` (OIDC sub) with email fallback; mapping contract documented in the module docs and operations docs
- [x] `/scim/v2/Groups`: org-membership group (role mapping documented) and optional team-mapped groups; membership add/remove reflected in `organization_members`/`team_members`; last-admin guard honored (422-equivalent SCIM error)
- [x] ServiceProviderConfig/Schemas/ResourceTypes discovery endpoints served per RFC 7643
- [x] Deprovision → active sessions: documented semantics (tokens remain valid to TTL per A-0010; membership loss takes effect immediately at the tenant middleware — integration-tested: deprovisioned user's valid token gets 403)
- [x] Integration tests simulating an IdP: full user lifecycle (provision → login binds by external_id → deprovision → 403), group sync, filter queries, malformed SCIM payloads → spec-compliant errors; activity logging on all lifecycle mutations

## Implementation Notes

References: KAIROS-A-0016 (decided — the contract), A-0010 (JIT coexists; join key = sub), A-0006 (membership is the authorization substrate), S-0005 Organization Membership addendum (manual path SCIM automates). Scope is the pragmatic IdP subset, not full RFC coverage — anything unsupported returns proper SCIM errors rather than pretending. New migration goes in the tenant tree (scim_tokens) or public (decide by tenancy of token — likely tenant schema; document).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created from KAIROS-A-0016 (identity externalized: BYO OIDC + SCIM).
- 2026-07-10: ACTIVE (shared-services lane). Design decisions before implementation:
  - **Token format / tenant resolution**: `kairos_scim_<slug>_<64-hex-secret>` — the tenant discriminator is embedded in the token (A-0016: "tenant-scoped by the token, not by subdomain"). Parsing is unambiguous: strip the `kairos_scim_` prefix, split at the LAST `_` (the secret is fixed-alphabet hex, never contains `_`; slugs may). Auth = SHA-256(full token) looked up in `org_{slug}.scim_tokens` where `revoked_at IS NULL` — O(1), no cross-schema iteration. Unknown slug / no hash match / revoked → SCIM 401 error envelope (uniform, non-leaking).
  - **scim_tokens** is a TENANT table (new migration `2026-07-10-000000_scim_tokens`), so `tenant_provisioning.rs`'s EXPECTED_TABLES grows to 22 (+ a fleet-upgrade simulation: drop table + bookkeeping row in one tenant, `migrate_all_tenants` re-applies). schema.rs gets the matching `diesel::table!` (print-schema style); db models/helpers in NEW `kairos-db/src/scim.rs`.
  - **Identity join contract**: externalId → `users.external_id`; miss → userName → `external_id`; miss → first email value (or userName if it contains `@`) → `users.email` (link-by-email retains the existing row's external_id — it is the OIDC sub JIT wrote); miss → create user with `external_id = externalId // userName`. Ops docs must tell IdP admins to map the OIDC sub into externalId (or userName), otherwise a later JIT login mints a second user row.
  - **SCIM resource set = org membership**: a User resource exists iff an `organization_members` row exists; `active:false`/DELETE revoke membership immediately (users row retained), after which GET is 404 and re-activation is a fresh POST (documented RFC-subset boundary).
  - **Groups contract**: `kairos-admins` (id = org UUID) maps role admin/member in `organization_members`; `kairos-team-<slug>` (id = team UUID) maps `team_members`. POST Groups creates a team (+ its delivery board, mirroring /api/teams). LAST_ADMIN → SCIM 400 `scimType: "mutability"` (modification incompatible with resource state).
  - **SCIM writes run outside the OIDC stack** (mounted like the admin router); /api/scim-tokens (create/list/revoke, org-admin, secrets shown once, hashed at rest) rides the normal auth → tenant stack. Activity rows: actor = the token's created_by, details prefixed `scim`.
- 2026-07-10: IMPLEMENTED + VERIFIED. Files: `crates/kairos-db/migrations/tenant/2026-07-10-000000_scim_tokens/`, `crates/kairos-db/src/scim.rs` (+ `pub mod scim;` in lib.rs, `scim_tokens` in schema.rs), `crates/kairos-server/src/scim/{mod,auth,error,discovery,users,groups,tokens}.rs` (+ lib.rs mod line, two app.rs mounts: /scim/v2 outside the OIDC stack after the mcp merge; /api/scim-tokens inside the protected stack), `crates/kairos-server/tests/scim.rs`, ops doc `docs/api/scim.md`, tenant_provisioning.rs extended (22 tables + fleet-upgrade simulation: drop scim_tokens/bookkeeping in one tenant → `migrate_all_tenants` re-applies exactly there). New deps: sha2 0.10.9, rand 0.9.4 (already in lockfile transitively).
  **Acceptance-criterion evidence (commands + outputs):**
  - `cargo fmt --check` → clean; `cargo clippy --workspace --all-targets -- -D warnings` → `Finished 'dev' profile` (no warnings).
  - `cargo test -p kairos-db` → 11 unit + all 10 integration binaries ok, incl. `tenant_provisioning_lifecycle ... ok` (22 tenant tables asserted; fleet upgrade applies the new migration only to the out-of-date tenant: `[("acme",0),("globex",0),("widgets",1)]`) — token-table migration + migrate-tenants criterion.
  - `cargo test -p kairos-server --test scim` → `scim_provisioning_against_live_stack ... ok` (live compose Postgres + Dex, scratch db `kairos_scim_t0025_test`). One IdP-simulation test covers, in order: token 403s for non-admin/non-member; POST /api/scim-tokens 201 with `kairos_scim_acme_…` secret once + activity row; list shows metadata only (no token/hash fields); uniform SCIM-401 envelope for missing/garbage/short/foreign-slug tokens; ServiceProviderConfig/Schemas/ResourceTypes payload assertions; POST Users creates user+membership (SCIM id == users.id, external_id == userName) + activity; duplicate POST → 409 uniqueness; EMAIL-FALLBACK link to JIT alice retaining her Dex sub; missing userName/underivable email → 400 invalidValue; broken JSON → 400 invalidSyntax; GET by id + 404s; filter userName/externalId eq (incl. 0-hit) + `co` → 400 invalidFilter; startIndex/count paging; PATCH displayName; PATCH without PatchOp schema → invalidSyntax; unsupported path → invalidPath; PUT profile replace + join-key change → 400 mutability; **deprovision proof**: alice 200 /api/whoami → SCIM PATCH active:"False" (Entra string form) revokes membership → SAME OIDC token now 403 MEMBERSHIP_REQUIRED, GET 404, users row retained, re-POST re-provisions, DELETE 204; Groups: kairos-admins (id=org uuid) listed with svc; POST kairos-team-platform 201 creates team + delivery board (+activity), naming contract → invalidValue, duplicates → uniqueness; displayName filter; member remove via `members[value eq]` + add; unprovisioned member → invalidValue; rename → mutability; promote bob/demote svc via admins group reflected in organization_members; LAST_ADMIN via group remove AND via Users deactivate → 400 mutability; PUT admin-set swap without false LAST_ADMIN; admins DELETE refused, team DELETE 204→404; token revoke → SCIM 401, double revoke 409, revoke activity row.
  - `cargo test -p kairos-server --lib` → 29 ok (7 new scim unit tests: token parse/round-trip/underscore slugs/malformed set, hash stability, filter parser, pagination clamps, error envelope).
  **NOTE — deferred full gate:** `angreal test unit` / `angreal test integration` NOT run here per shared-services lane discipline (concurrent agents own kairos-client + existing server tests); the orchestrator runs the full A-0012 gate afterward. Services left UP.