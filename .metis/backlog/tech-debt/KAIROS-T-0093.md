---
id: generalize-the-tenant-provisioning
level: task
title: "Generalize the tenant_provisioning upgrade-path test — stop re-pinning per migration"
short_code: "KAIROS-T-0093"
created_at: 2026-08-29T15:51:44.469179+00:00
updated_at: 2026-08-29T15:51:44.469179+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#tech-debt"


exit_criteria_met: false
initiative_id: NULL
---

# Generalize the tenant_provisioning upgrade-path test — stop re-pinning per migration

## Objective

Make `crates/kairos-db/tests/tenant_provisioning.rs`'s upgrade-path simulation self-maintaining: it should derive "the newest tenant migration" from the embedded migration list instead of being hand-re-pinned every time one lands.

## Backlog Item Details

### Type
Tech Debt

### Priority
P3 — pure test-maintenance friction; nothing user-facing.

### Technical Debt Impact
- **Current Problems**: The upgrade-path test simulates "an existing tenant catches up": it drops the tables of ONE hardcoded migration and asserts re-provisioning restores them. Every new tenant migration requires re-pinning the test by hand (org_widgets → team_pages across T-0065/T-0077/T-0082…) — it has been touched in at least three waves this month, and forgetting it means the test silently exercises a stale migration instead of the newest one.
- **Benefits of Fixing**: New tenant migrations get upgrade-path coverage automatically; one recurring review-time gotcha disappears from every schema wave.
- **Risk Assessment**: Low risk of not fixing (the suite still passes) but the coverage quietly decays: the simulation stops representing the migration that most needs it — the newest one.

## Implementation Notes

- The embedded migrations are available at runtime (diesel `embed_migrations!` / the migration harness kairos-db already uses to run them); the newest tenant migration's name is derivable — no hardcoding needed.
- The hard part is dropping "the newest migration's objects" generically. Options to weigh at pickup: (a) run `down.sql` for the newest migration against the provisioned schema, then assert re-provisioning restores `EXPECTED_TABLES` (down scripts already exist and are maintained); (b) diff `pg_tables` before/after the newest migration on a scratch schema to learn its table set, then drop those. Option (a) is the honest one — it also verifies the down script actually works, which nothing tests today.
- Keep the EXISTING assertions (EXPECTED_TABLES etc.) — only the "which migration to unwind" selection generalizes.

## Acceptance Criteria

- [ ] The upgrade-path simulation selects the newest tenant migration programmatically; no migration name is hardcoded in the test.
- [ ] Landing a new tenant migration requires NO edit to the upgrade-path portion of tenant_provisioning.rs (table-count constants may still move).
- [ ] The newest migration's `down.sql` is exercised (if option (a) is chosen), or the chosen alternative is recorded here with rationale.
- [ ] `angreal test integration` green.

## Status Updates

- 2026-08-29: Ticketed from the KAIROS-I-0007/I-0008 waves (the re-pinning chore bit three times this month).