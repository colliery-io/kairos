---
id: flaky-org-endpoints-early-setup
level: task
title: "Flaky org_endpoints early-setup failure under full suite"
short_code: "KAIROS-T-0048"
created_at: 2026-07-13T12:21:12.857895+00:00
updated_at: 2026-07-16T02:11:37.420081+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Flaky org_endpoints early-setup failure under full suite

## Objective

Diagnose and fix the intermittent `org_endpoints` integration-test failure observed under full-suite runs.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P2 - Medium (nice to have)

### Impact Assessment
- **Affected Users**: CI/developers — test flake only; no production surface implicated yet (but the root cause could be a real setup race)
- **Reproduction Steps**:
  1. `angreal services up`
  2. `cargo test -p kairos-server --no-fail-fast` (full suite, all targets concurrently)
  3. Observed ONCE during KAIROS-T-0035's gate: `org_endpoints` failed in ~0.15s during early setup (before any assertions), then passed standalone and in two subsequent full-suite runs
- **Expected vs Actual**: suite deterministic under concurrency; actual = rare early-setup failure, most likely a scratch-DB name/creation race or connection-pool contention when many test binaries provision tenants simultaneously

## Acceptance Criteria

## Acceptance Criteria

- [x] Root cause identified — not reproducible after heavy stress (560 concurrent CREATE DATABASE + 180 concurrent 12-binary runs, all green; PG16's WAL_LOG strategy avoids the classic template-lock race). Diagnosed as test-harness fragility: `recreate_scratch_db` single-shot `.expect()`s every step, so any transient shared-Postgres blip panics before assertions — matching the observed ~0.15s signature. No product race (provisioning is one all-or-nothing transaction).
- [x] Fix lands — bounded exponential-backoff retry (50ms base, 2s cap) around the transient scratch-DB lifecycle ops in `tests/common/mod.rs` (+73/-16); no assertion weakened, product code untouched.
- [x] Green streak recorded — agent's 180 concurrent binary runs + orchestrator's 15/15 org_endpoints loop + full `angreal test integration` pass, all green; fmt/clippy clean.

## Test Cases **[CONDITIONAL: Testing Task]**

{Delete unless this is a testing task}

### Test Case 1: {Test Case Name}
- **Test ID**: TC-001
- **Preconditions**: {What must be true before testing}
- **Steps**: 
  1. {Step 1}
  2. {Step 2}
  3. {Step 3}
- **Expected Results**: {What should happen}
- **Actual Results**: {To be filled during execution}
- **Status**: {Pass/Fail/Blocked}

### Test Case 2: {Test Case Name}
- **Test ID**: TC-002
- **Preconditions**: {What must be true before testing}
- **Steps**: 
  1. {Step 1}
  2. {Step 2}
- **Expected Results**: {What should happen}
- **Actual Results**: {To be filled during execution}
- **Status**: {Pass/Fail/Blocked}

## Documentation Sections **[CONDITIONAL: Documentation Task]**

{Delete unless this is a documentation task}

### User Guide Content
- **Feature Description**: {What this feature does and why it's useful}
- **Prerequisites**: {What users need before using this feature}
- **Step-by-Step Instructions**:
  1. {Step 1 with screenshots/examples}
  2. {Step 2 with screenshots/examples}
  3. {Step 3 with screenshots/examples}

### Troubleshooting Guide
- **Common Issue 1**: {Problem description and solution}
- **Common Issue 2**: {Problem description and solution}
- **Error Messages**: {List of error messages and what they mean}

### API Documentation **[CONDITIONAL: API Documentation]**
- **Endpoint**: {API endpoint description}
- **Parameters**: {Required and optional parameters}
- **Example Request**: {Code example}
- **Example Response**: {Expected response format}

## Implementation Notes **[CONDITIONAL: Technical Task]**

{Keep for technical tasks, delete for non-technical. Technical details, approach, or important considerations}

### Technical Approach
{How this will be implemented}

### Dependencies
{Other tasks or systems this depends on}

### Risk Considerations
{Technical risks and mitigation strategies}

## Status Updates **[REQUIRED]**

### 2026-07-15 — Investigation start (active)

**Environment**: kairos-postgres + kairos-dex UP and healthy (SHARED with T-0049/T-0050). Postgres 5432 + Dex 5558 reachable. cargo 1.93.0.

**Ruled out**: scratch-DB *name* collision across binaries. Every test binary uses a distinct `SCRATCH_DB` constant. No two binaries create the same DB name.

**Leading hypothesis**: The early-setup failure is in `common::recreate_scratch_db` (tests/common/mod.rs). Under full-suite concurrency, ~12 test binaries each run `DROP DATABASE ... WITH (FORCE)` + `CREATE DATABASE <unique>` against the shared Postgres near-simultaneously at suite start. Postgres `CREATE DATABASE` copies from `template1`; two concurrent `CREATE DATABASE` calls collide on the template with the classic transient error `source database "template1" is being accessed by other users` (or a lock/`tuple concurrently updated` on pg_database). This matches every symptom: ~0.15s (before the slow Dex round-trips), before any assertion, rare (needs overlap), only under full-suite concurrency. TEST-HARNESS race, not a product provisioning race.

**Plan**: (1) reproduce via a stress loop running many binaries' create-DB concurrently; capture the actual error. (2) Fix in tests/common/mod.rs by retrying DROP/CREATE with bounded backoff on transient errors (no assertion weakened). (3) 20 consecutive green runs recorded.

### 2026-07-15 — Reproduction attempts (NOT reproduced)

Note on runner reality: `cargo test --workspace --test '*'` (the `angreal test integration` path) runs each integration **binary sequentially**; libtest only parallelizes *within* a binary, and each of these integration binaries holds a single `#[tokio::test]`. So the reporter's "all targets concurrently" mental model isn't cargo's actual behavior — genuine cross-binary overlap only happens if binaries are launched in parallel by hand. I therefore built the harshest faithful stressor possible.

Stress performed against the live shared stack (no compose cycling; direct `cargo test --no-run` build, then ran compiled binaries directly; scratch DBs are per-binary unique and dropped):
1. **Raw Postgres race**: 40 rounds × 14 concurrent `DROP DATABASE ... FORCE` + `CREATE DATABASE <unique>` (=560 concurrent creates). No `template1 is being accessed`, no `tuple concurrently updated`, no error. PG16 uses the `WAL_LOG` CREATE DATABASE strategy by default, which does not take the exclusive template lock that produced this classic flake on PG<15.
2. **Full-binary max concurrency**: all 12 integration binaries launched simultaneously, 15 rounds (=180 full binary runs, 12-way concurrent — far more overlap than cargo ever produces). Every round green. Peak `pg_stat_activity` during the storm = **47 / 100** connections (`superuser_reserved=3`), so connection-pool exhaustion is also ruled out with wide headroom.

**Conclusion**: Not reproducible after a substantial stress loop (560 concurrent creates + 180 concurrent binary runs). Consistent with the ticket's "observed once, P2" nature. The two mechanisms the reporter hypothesized (create-DB race, pool contention) are both disproven in this environment.

**Root-cause assessment**: The single observed failure (~0.15s, before any assertion) can only be an early-setup panic in `common::recreate_scratch_db` — the connect / `DROP DATABASE` / `CREATE DATABASE` sequence — or the immediately-following `establish` + `run_public_migrations`. That code path currently single-shots every step with `.expect(...)`, so ANY momentary blip on the shared Postgres (a burst-load `CREATE DATABASE`, a transient connect refusal while another binary's `DROP ... FORCE` terminates backends, a brief `too many clients` spike) panics the whole test instantly — exactly the observed signature. It is a test-harness fragility, not product code.

**Decision**: Harden defensively (as the task authorizes when repro fails) — add bounded exponential-backoff retry around the transient scratch-DB lifecycle ops in `tests/common/mod.rs`. Product code (`kairos-db/src/tenant.rs`) is untouched: no product race was found, and provisioning already runs in a single all-or-nothing transaction.