---
id: fix-repositories-migration
level: task
title: "Fix: repositories migration hardening — slug collision handling, SQL/Rust slug parity, populated backfill and down-migration tests"
short_code: "KAIROS-T-0113"
created_at: 2026-09-22T09:53:02.350168+00:00
updated_at: 2026-09-22T10:16:29.942171+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: repositories migration hardening — slug collision handling, SQL/Rust slug parity, populated backfill and down-migration tests

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

The `2026-09-22-000000_repositories` migration has a real failure mode the upgrade test cannot see: it runs on an empty `forge_connections`. Two live connections that differ only by forge or case (`github:acme/foo` + `gitlab:acme/foo`, `Acme/Foo` + `acme/foo`) both derive slug `acme-foo` and abort on `idx_repositories_slug` with a raw unique violation mid-fleet-upgrade, bypassing the fail-loud RAISE. The SQL slug expression also doesn't trim edge hyphens or cap at 63 bytes like `kairos_core::repositories::slug_from_full_name`.

## Implementation Notes

1. **up.sql**: derive slugs collision-free — `btrim(regexp_replace(lower(full_name), '[^a-z0-9]+', '-', 'g'), '-')` capped to 63; on collision suffix `-<forge>` then `-2`, `-3`… (a `DO` block loop or a window-numbered CTE). Extend the pre-flight RAISE to name the colliding pairs when suffixing would exceed 63 bytes. Guard the soft-deleted no-team fallback: if no live team exists, skip the row (leave `repository_id` NULL and drop the `SET NOT NULL`? no — instead RAISE naming it: a dead connection with no owner in a tenant with no teams is unrecoverable and rare).
2. **Rust parity**: `slug_from_full_name` documents that the SQL mirrors it; add a core unit test table of full names → slugs and a db integration test that inserts the same names into a pre-migration-shaped scratch tenant and asserts the migrated slugs equal the Rust derivation.
3. **Populated backfill test** (`crates/kairos-db/tests/tenant_provisioning.rs` upgrade-path block, or a new `repositories_migration.rs`): simulate the pre-migration shape WITH rows — live connections (two colliding across forges, one mixed-case), a soft-deleted one with no team, a link on each — run `migrate_all_tenants`, assert: repositories created with the expected slugs, `repository_id` set on every connection incl. soft-deleted, links still resolve through the join, the team-less LIVE connection variant RAISEs with its id.
4. **down.sql executed**: the same test runs the down migration (via `diesel_migrations` revert or by executing the file) and asserts the old columns are restored with the right values, then re-runs up to prove re-runnability.

## Acceptance Criteria

## Acceptance Criteria

- [ ] Colliding names migrate to distinct slugs; slugs pass `is_valid_slug`; SQL and Rust derivations agree on the test table.
- [ ] Team-less live connection RAISEs naming the id (tested).
- [ ] Down migration is executed by a test and restores the pre-shape; up re-applies cleanly after it.
- [ ] fmt / clippy / unit / integration green; `angreal db schema-sync` produces no diff.

## Status Updates

*To be added during implementation*