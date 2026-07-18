---
id: serve-metrics-and-verify-readyz
level: task
title: "Serve /metrics (and verify /readyz) per A-0013"
short_code: "KAIROS-T-0049"
created_at: 2026-07-14T23:05:42.156477+00:00
updated_at: 2026-07-16T00:47:35.249417+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Serve /metrics (and verify /readyz) per A-0013

## Objective

Ship the A-0013 observability surface that never got an owning task: Prometheus `/metrics` (and confirm `/readyz` exists with its documented semantics). Found by the T-0046 soak harness, which scrapes `/metrics` and currently reports it UNAVAILABLE.

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification
- **User Value**: operators get the decided A-0013 baseline (HTTP histograms, pool gauges, per-tenant counters); the soak harness's pool-stability assertions arm themselves (`thresholds.require_metrics`)
- **Business Value**: production-readiness claim of A-0013 becomes true
- **Effort Estimate**: S

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `GET /metrics` (unauthenticated, per Prometheus convention — outside the auth stack like /healthz; document) serves Prometheus text format: HTTP request histograms labeled by route/method/status, connection-pool gauges (bb8 + the blocking r2d2 bridge), per-tenant request counters
- [x] `GET /readyz` verified present with A-0013 semantics (DB connectivity + pending-migration check) — implement if missing
- [ ] Soak harness re-run (10-min smoke) with `require_metrics` armed: pool-stability assertions active and green — **FOLLOW-UP** (see Status Updates: solo-infeasible under shared-services mode; armed path now satisfiable, lighter pool-gauge proof done)
- [x] Integration test: scrape parses, expected metric families present, counters advance across requests

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

### 2026-07-15 — active, plan + decisions

**Metrics crate choice: hand-rolled Prometheus text exporter (NO `metrics`/`metrics-exporter-prometheus`).**
Rationale: (a) `metrics-exporter-prometheus` installs a *process-global* recorder; the suite runs many in-process routers per process (`Router::oneshot`), so a global recorder would let per-tenant counters bleed across tests and make "counters advance" assertions non-deterministic. A per-`AppState` `Arc<Metrics>` registry is isolated per router → deterministic. (b) Pool gauges are point-in-time (`bb8::Pool::state()` / `r2d2::Pool::state()`) and render naturally at scrape time. (c) Zero new workspace deps (lighter). The soak scraper (`kairos-soak/src/prom.rs`) parses lenient `name{labels} value` text and only needs pool metrics whose name contains `pool`/`connection`.

**Design:**
- `crates/kairos-server/src/metrics.rs`: `Metrics` registry (`Mutex<BTreeMap>` histograms keyed (method,route,status) + tenant counters), `track_metrics` outer middleware (records duration histogram + per-tenant counter from response-extension `TenantContext`), `metrics_handler` (renders http histograms + tenant counters + live pool gauges), `readyz` handler.
- `AppState` gains `metrics: Arc<Metrics>` (built in `build_state` + `state_with`).
- `app.rs`: register `/metrics` + `/readyz` (both NON-/api, mounted beside `/healthz` outside auth) and add `.layer(track_metrics)` around the whole router.
- `middleware/tenant.rs`: stamp resolved `TenantContext` onto the *response* extensions so the outer metrics layer can attribute the tenant (request extensions set by inner layers are invisible to an outer layer).
- `blocking.rs`: additive `run_public` (sync public conn for the migration check) + `pool_state()` accessor for the r2d2 gauge.
- `kairos-db/migrations.rs`: additive `has_pending_public_migrations(conn)` for the `/readyz` pending-migration check.
- bb8 gauge via existing `TenantPool::raw_pool().state()` — no `pool.rs` change needed.

**/readyz semantics (A-0013):** async-pool connectivity (`public_conn` + `SELECT 1`) AND pending-public-migration check (embedded tree vs applied) via the sync pool. 200 `ready` only when reachable and no pending migrations; 503 `not ready: ...` otherwise.

**openapi test:** `/metrics` + `/readyz` are non-`/api` literals → the route scanner's `path.starts_with("/api")` filter skips them.

Metric families: `http_request_duration_seconds{_bucket,_sum,_count}` (labels method/route/status), `kairos_db_pool_connections{pool,state}` gauges, `http_requests_by_tenant_total{tenant}`.

### 2026-07-15 — implemented + verified, completing

**Files:** new `crates/kairos-server/src/metrics.rs`; `app.rs` (metrics field + `/metrics` + `/readyz` routes + `track_metrics` layer); `middleware/tenant.rs` (response-extension `TenantContext` stamp); `blocking.rs` (`run_public` + `pool_state`); `kairos-db/src/migrations.rs` + `lib.rs` (`has_pending_public_migrations`); new `crates/kairos-server/tests/metrics.rs`. **No workspace Cargo.toml change** — hand-rolled exporter needs zero new deps. No `pool.rs` change (bb8 gauge via existing `raw_pool().state()`).

**Self-checks (all green):** `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test -p kairos-server` = 34 lib + all integration incl. `tests/metrics.rs` (new) and `tests/openapi.rs` (still 5/5 green — `/metrics` + `/readyz` non-`/api` so the route scanner skips them); `cargo test -p kairos-db` green.

**Live proof** (own server, port 18110, scratch DB `kairos_t0049_live`, dropped after): `GET /readyz` → `200 ready`; `GET /metrics` → Prometheus text with per-route HTTP histograms (route labels are matched-path patterns — `MatchedPath` works under the global `.layer`, so cardinality is bounded), both pool gauges, and `http_requests_by_tenant_total{tenant="acme"} 50` after a 50-request authed burst + 40-way concurrent load; async pool gauge grew 1→16 (POOL_SIZE) under load then settled. `/readyz` not-ready-on-pending-migrations path proven deterministically in the integration test.

**AC status:**
- AC1 `/metrics` (unauth, histograms + pool gauges + per-tenant counters): **PASS**
- AC2 `/readyz` (DB connectivity + pending-migration): **PASS** (implemented — only `/healthz` existed before)
- AC4 integration test (parses, families present, counters advance): **PASS**
- AC3 soak 10-min smoke with `require_metrics` armed: **FOLLOW-UP.** The soak driver needs solo-infeasible coordination (seed-demo fixture, deployment-admin bystander, the `svc` service identity, MCP/WS workforce) and `angreal test soak` is off-limits under shared-services mode (it cycles compose). Lighter proof done per brief: concurrent-load pool-gauge movement above. The armed path is now satisfiable — `/metrics` returns 200 and the soak's `is_pool_metric` filter matches `kairos_db_pool_connections` (name contains both `pool` and `connection`), so the prior UNAVAILABLE breach is resolved. **Follow-up:** run `angreal test soak` smoke profile with `thresholds.require_metrics = true` in a dedicated (non-shared) window.