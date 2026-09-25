---
id: generalize-the-tenant-provisioning
level: task
title: "Generalize the tenant_provisioning upgrade-path test — stop re-pinning per migration"
short_code: "KAIROS-T-0093"
created_at: 2026-08-29T15:51:44.469179+00:00
updated_at: 2026-09-25T01:58:06.499754+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#tech-debt"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] The upgrade-path simulation selects the newest tenant migration programmatically; no migration name is hardcoded in the test.
- [x] Landing a new tenant migration requires NO edit to the upgrade-path portion of tenant_provisioning.rs (table-count constants may still move).
- [x] The newest migration's `down.sql` is exercised (if option (a) is chosen), or the chosen alternative is recorded here with rationale.
- [x] `angreal test integration` green.

## Status Updates

- 2026-08-29: Ticketed from the KAIROS-I-0007/I-0008 waves (the re-pinning chore bit three times this month).
### 2026-09-25 — the target is derived, and the assertion got stronger

Two changes, and the second matters more than the ticket asked for.

**Which migration to unwind is now derived.** `revert_last_migration(TENANT_MIGRATIONS)`
replaces a hardcoded `DROP TABLE` plus a hardcoded version string. It runs the
newest migration's own `down.sql` and removes its bookkeeping row in one step,
under a `SET LOCAL search_path` pinned to the tenant — the same way
`provision_tenant` pins it. Option (a) from the implementation notes, chosen for
the reason given there: it also **exercises the down script**, which nothing did.
Down scripts are written, reviewed, committed and never run.

Verified the derivation works rather than assuming it: the test prints
`simulated an old tenant by reverting 20260925000002 in org_widgets`, which is the
migration added earlier today, not the one the old code named.

**What it asserts afterwards changed, and that is the real fix.** The block used
to assert specific tables and indexes by name, which is exactly why it needed
re-pinning: those names *are* the migration. It now asserts that an upgraded
tenant's schema is indistinguishable from a freshly provisioned one, using
`globex` as the reference — already provisioned in the same run, and already
asserted to have applied nothing.

That is migration-agnostic, so a new migration gets upgrade-path coverage the
moment it lands whether it adds a table, an index, a column or a predicate. It is
also a stronger question than the old one: *does the upgrade path produce the same
schema as fresh provisioning?* No amount of per-migration table-name checking
answers that, and it is the failure that actually hurts.

`schema_shape` compares tables, views, sequences, column types/nullability/defaults,
and **full index definitions** rather than index names. That last one is deliberate:
two of this month's defects ([[KAIROS-T-0161]], [[KAIROS-T-0184]]) were a plain
unique where a partial one was meant, and a name-only comparison cannot see the
difference.

### Verified by breaking it, and then fixed the failure output

Appending a stray `CREATE INDEX` to the newest migration's `down.sql` — down-script
residue, a real risk and the thing this test now uniquely covers — fails the test,
naming `stray_residue_idx` as present in the upgraded tenant and absent from the
fresh one.

The first version of that failure dumped both schemas: **over four hundred lines
per side**, leaving the reader to find the differing row. For a test whose whole
purpose is to stop being a maintenance burden, that would have replaced one chore
with another. It now reports the symmetric difference only, so the output was one
line, plus a note that down-script residue is among the usual causes.

### Evidence that was carried, and stays

Earlier re-pins moved assertions up into the freshly-provisioned section rather
than deleting them (T-0156's five-table views, T-0159's board indexes, T-0186's
weighting, T-0187's embedding tables). Those stay. The schema comparison covers
their class generically, but a named assertion records WHY a table matters, and a
set difference cannot say that. The comments that described the re-pinning chore
are updated to past tense rather than removed, since they explain why that section
looks the way it does.

### Gates

lint clean, integration **47/47 exit 0**.