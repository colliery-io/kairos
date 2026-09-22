---
id: fix-pre-existing-forge-abac
level: task
title: "Fix: pre-existing forge/ABAC hygiene — transactional rotate, config check ordering, CAPABILITY_VOCABULARY dedupe, dead helpers, loose cross-tenant assertion"
short_code: "KAIROS-T-0116"
created_at: 2026-09-22T09:53:07.848633+00:00
updated_at: 2026-09-22T10:30:11.305624+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: pre-existing forge/ABAC hygiene — transactional rotate, config check ordering, CAPABILITY_VOCABULARY dedupe, dead helpers, loose cross-tenant assertion

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

Small pre-existing defects the review surfaced while reading the touched modules. None is new to I-0010; all are cheap and adjacent.

## Implementation Notes

1. `api/org/forge.rs` `rotate_connection`: the comment says "same transaction" but `BlockingTenantPool::run` wraps nothing — delete-then-create autocommit separately, so a failure between them loses the connection. Wrap with `run_in_transaction` (the helper `org/teams.rs` already uses).
2. `api/org/forge.rs` `create_connection`: `signing_key` is checked before the write, `public_url` after — a missing `KAIROS_PUBLIC_URL` persists the connection then returns 501. Check both up front.
3. `api/org/mod.rs` `CAPABILITY_VOCABULARY` duplicates `kairos_core::abac::{CAPABILITIES, GLOBS}`; derive it from core.
4. Dead public helpers: `kairos_db::repositories::find_by_forge_name`, `kairos_db::forge::find_connection_by_repo`, `kairos_core::abac::is_computed` — delete, or wire `find_by_forge_name` into a `GET /api/repositories?forge=&name=` lookup if bootstrap (T-0115) wants server-side remote matching (preferred: keep + expose).
5. `kairos-db/src/forge.rs` `create_connection` collapses DB errors into `RepositoryNotFound` → map `RepositoryError` variants properly.
6. `api/org/forge.rs` `get_repository`-style `internal(e.to_string())` on a `ForgeError` → use `map_error`.
7. `tests/file_backlog.rs` cross-tenant probe accepts `Validation | Forbidden | NotFound`; pin it to the one status the middleware actually returns.
8. `mcp/tools.rs` whoami re-queries `team_members` for ids already loaded; reuse.
9. Reject UUID-parsable slugs in `kairos_core::repositories::is_valid_slug` (a UUID-shaped slug is unreachable by slug because `resolve` parses ids first).

## Acceptance Criteria

- [~] Rotate wrapped in `run_in_transaction` (the helper teams.rs already relies on). No fault-injection hook exists in the server, so the "fail between delete and create" case is asserted by construction (one transaction) rather than by a test; adding an injection seam for one test was judged not worth it.
- [x] Missing `KAIROS_PUBLIC_URL` → 501 and `has_webhook` stays false — tested with a second unconfigured server on the same database (`forge_connections.rs`).
- [x] Vocabulary derived from core; `is_computed` and `find_connection_by_repo` deleted; `find_by_forge_name` exposed as `GET /api/repositories?forge=&name=` (tested: hit, miss, half-pair 422) for T-0115's bootstrap.
- [x] Cross-tenant probe pinned to `Validation` (with the reason recorded in the test); UUID-shaped slug → 422 (core unit + API test).
- [x] fmt (my crates), clippy `-D warnings` on core/db/client/server/cli/soak, unit, integration 38/38.

## Status Updates

- 2026-09-22: Done, `42d7cd4`. Also: `ForgeError::RepositoryNotFound` became `ForgeError::Repository(RepositoryError)` so a DB error is no longer reported as "does not exist"; the repository detail maps `ForgeError` through `forge::map_error` (now `pub(crate)`); MCP whoami no longer re-queries team ids.