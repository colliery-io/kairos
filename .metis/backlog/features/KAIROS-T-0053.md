---
id: helm-chart-for-kubernetes
level: task
title: "Helm chart for Kubernetes deployment"
short_code: "KAIROS-T-0053"
created_at: 2026-07-16T22:42:55.546664+00:00
updated_at: 2026-07-17T00:56:58.132059+00:00
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

# Helm chart for Kubernetes deployment

## Objective

Ship a v1 Helm chart at `deploy/helm/kairos/` so Kairos runs on Kubernetes — the deployment path A-0013 deferred as "a future initiative if customer demand appears." Pragmatic scope (no superseding ADR this pass, per Dylan): the chart honors the decided deployment surface (single OCI image, env-only config, external Postgres + external IdP per A-0016, `/healthz`+`/readyz`+`/metrics`).

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium (nice to have)

### Business Justification
- **User Value**: operators running K8s get a first-class install instead of adapting the compose reference by hand
- **Business Value**: unblocks the "customer deployment that outgrows a single node" review trigger in A-0013
- **Effort Estimate**: M

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: {Number/percentage of users affected}
- **Reproduction Steps**: 
  1. {Step 1}
  2. {Step 2}
  3. {Step 3}
- **Expected vs Actual**: {What should happen vs what happens}

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: {Why users need this}
- **Business Value**: {Impact on metrics/revenue}
- **Effort Estimate**: {Rough size - S/M/L/XL}

### Technical Debt Impact **[CONDITIONAL: Tech Debt]**
- **Current Problems**: {What's difficult/slow/buggy now}
- **Benefits of Fixing**: {What improves after refactoring}
- **Risk Assessment**: {Risks of not addressing this}

## Acceptance Criteria

## Acceptance Criteria

- [x] `deploy/helm/kairos/` chart: Chart.yaml (apiVersion v2, appVersion 0.1.0), values.yaml, _helpers.tpl, and templates for Deployment, Service, Ingress, ConfigMap (non-secret env), Secret (DATABASE_URL etc.), ServiceAccount, HPA (gated), ServiceMonitor (gated), a `helm test` connection pod, NOTES.txt, .helmignore, and a chart README
- [x] Deployment runs `ghcr.io/colliery-io/kairos:{appVersion}` (never latest), single container, liveness `/healthz` + readiness `/readyz`, configurable replicas/resources; env surface = the full A-0013 set (DATABASE_URL, OIDC_ISSUER_URL/AUDIENCE, KAIROS_WEB_CLIENT_ID, KAIROS_BASE_DOMAIN xor KAIROS_SINGLE_TENANT, KAIROS_DEPLOYMENT_ADMINS, KAIROS_LOG_*, KAIROS_OTEL_ENDPOINT, retention KAIROS_HISTORY_*/ARCHIVE_TARGET/RETENTION_MODE, KAIROS_DEV_UI default false)
- [x] External Postgres + external IdP per A-0016 (chart bundles NEITHER; DATABASE_URL from a Secret or `existingSecret`; docs point OIDC_* at the customer IdP). Public migrations run on boot (T-0007) — no migration Job needed; documented
- [x] Ingress supports BOTH tenancy modes: wildcard host (`*.base-domain` for subdomain tenancy, A-0005) and single-host (single-tenant mode); TLS via cert-manager annotations or provided secret
- [x] `helm lint` clean; `helm template` renders cleanly for three value sets (single-tenant minimal; wildcard-domain + ingress-TLS; all optionals on: HPA + ServiceMonitor + otel + custom retention); rendered manifests pass kubeconform (containerized: `docker run ghcr.io/yannh/kubeconform`) with zero errors — recorded
- [x] Chart README documents install (`helm install kairos deploy/helm/kairos -f my-values.yaml`), the values surface, the external-Postgres/IdP requirement, and how a fresh install bootstraps (deployment admin -> create tenant -> members)

## Implementation Notes

- helm v3.21 is installed natively (use it for lint/template). kind + kubeconform are NOT installed; run kubeconform via docker (`ghcr.io/yannh/kubeconform:latest-alpine`). A real kind cluster smoke is a BONUS if kind installs cleanly as a single static binary via curl (no Homebrew) — otherwise render+schema-validate is the gate and the kind smoke is documented as follow-up.
- Parity source: `deploy/docker-compose.yaml`, `deploy/.env.example`, `deploy/Caddyfile` (Caddy did wildcard-subdomain TLS in compose; the Ingress replaces it). A-0013 (deployment surface), A-0016 (no bundled IdP), A-0005 (subdomain tenancy), A-0015 (GUI served at / by the same binary — one container serves everything), T-0049 (`/metrics`, `/readyz`).
- Do NOT add a Postgres subchart — external only, keeps A-0016's "identity/state are the operator's" posture and avoids a stateful dependency in the chart.

## Verification Gate (KAIROS-A-0012 spirit)

`helm lint` clean · `helm template` renders for all three value sets · kubeconform passes on rendered output · every acceptance criterion demonstrated with command + recorded output in Status Updates.

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

### 2026-07-16 — Parity read complete, chart build starting

Confirmed the deployment surface against source (not memory):

- **Image**: `ghcr.io/colliery-io/kairos:{version}` from `.github/workflows/release.yml` line 199; never `latest`. Chart defaults `image.tag` to `.Chart.AppVersion` via helper.
- **Bind/port**: `crates/kairos-server/src/config.rs:11` `DEFAULT_BIND_ADDR = 127.0.0.1:8080`; compose overrides to `0.0.0.0:8080`. Chart sets `KAIROS_BIND_ADDR=0.0.0.0:{containerPort}`, containerPort default 8080, configurable.
- **Routes served** (`crates/kairos-server/src/app.rs:162-168`): `/healthz` (liveness), `/readyz` (readiness, DB+migration check), `/metrics` (Prometheus). Matches T-0049 / A-0013.
- **Env surface** (all confirmed real in code): `DATABASE_URL` (secret), `OIDC_ISSUER_URL`, `OIDC_AUDIENCE`, `KAIROS_WEB_CLIENT_ID` (default kairos-web), `KAIROS_BASE_DOMAIN` XOR `KAIROS_SINGLE_TENANT`, `KAIROS_DEPLOYMENT_ADMINS`, `KAIROS_LOG_LEVEL`/`KAIROS_LOG_FORMAT`, `KAIROS_DEV_UI` (default false). Retention (`crates/kairos-core/src/retention.rs:38-48`): `KAIROS_HISTORY_HOT_DAYS`(90), `KAIROS_HISTORY_KEEP_LATEST`(5), `KAIROS_ACTIVITY_RETENTION_DAYS`(365), `KAIROS_ARCHIVE_TARGET`, `KAIROS_RETENTION_MODE`(archive|discard|off). `KAIROS_OTEL_ENDPOINT` is in A-0013's documented surface (server wiring pending) — emitted only when set.
- **A-0016 / A-0013**: no bundled IdP, no Postgres subchart — external only. Migrations run on boot (T-0007) — no Job.
- **Tenancy** (A-0005 §2): wildcard subdomain `*.{baseDomain}` XOR single-tenant. Chart templates a `fail` guard: exactly one of baseDomain/singleTenant must be set.

Building chart at `deploy/helm/kairos/`.

### 2026-07-16 — Chart built and verified (GATE GREEN)

**File tree** (`deploy/helm/kairos/`): Chart.yaml, values.yaml, README.md, .helmignore, templates/{_helpers.tpl, deployment.yaml, service.yaml, ingress.yaml, configmap.yaml, secret.yaml, serviceaccount.yaml, hpa.yaml, servicemonitor.yaml, NOTES.txt, tests/test-connection.yaml}, ci/{single-tenant-minimal, wildcard-ingress-tls, all-optionals}-values.yaml.

**helm v3.21.0.** `helm lint` — clean for all three value files (only `[INFO] icon is recommended`, non-blocking):
```
1 chart(s) linted, 0 chart(s) failed   (× single-tenant, wildcard, all-optionals)
```

**`helm template` — all three render:**
- (a) single-tenant minimal → Secret(inline DATABASE_URL) + ConfigMap(KAIROS_SINGLE_TENANT=acme, KAIROS_BIND_ADDR=0.0.0.0:8080) + SA + Service + Deployment(image `ghcr.io/colliery-io/kairos:0.1.0`) + test Pod. No Ingress.
- (b) wildcard+ingress+TLS → NO Secret (existingSecret), Ingress hosts `kairos.example` + `*.kairos.example`, `secretName: kairos-tls`.
- (c) all optionals → ConfigMap has KAIROS_OTEL_ENDPOINT + retention overrides; DATABASE_URL `key: connection-string` (custom existingSecretKey); HorizontalPodAutoscaler (cpu 65 + memory 80); ServiceMonitor; single-host Ingress.

**Fail-guard (tenancy XOR)** — both directions error at render as designed:
```
BOTH set:    Error: config.tenancy: set EXACTLY ONE ... — both are set.
NEITHER set: Error: config.tenancy: set EXACTLY ONE ... — neither is set.
```

**kubeconform (containerized `ghcr.io/yannh/kubeconform:latest-alpine`, `-strict -summary`, `-schema-location default` + datreeio CRDs-catalog for the ServiceMonitor CRD):**
```
render-a: 6 resources - Valid: 6, Invalid: 0, Errors: 0, Skipped: 0   (exit 0)
render-b: 6 resources - Valid: 6, Invalid: 0, Errors: 0, Skipped: 0   (exit 0)
render-c: 8 resources - Valid: 8, Invalid: 0, Errors: 0, Skipped: 0   (exit 0)
```
Zero errors, zero skipped — the CRD catalog resolved the ServiceMonitor (render-c's 8th resource) rather than skipping it.

**BONUS — kind smoke DONE (not deferred).** `kind` v0.27.0 + `kubectl` v1.36.2 installed cleanly as single static binaries via curl (no Homebrew). Created cluster `kairos-smoke`, ran server-side dry-run applies against the real API server:
```
render-a: serviceaccount/secret/configmap/service/deployment.apps/pod  all "created (server dry run)"
render-b: serviceaccount/configmap/service/deployment.apps/ingress.networking.k8s.io/pod  all "created (server dry run)"
```
Ingress (networking.k8s.io/v1) and all core objects passed real API-server validation + admission. Cluster deleted after; the running kairos dev compose stack (kairos-postgres, kairos-dex) was untouched throughout. A full app-Ready smoke needs an external Postgres + IdP + the pullable release image, which are out of scope here — the server-side apply is the meaningful chart-level smoke.

**Result: helm lint + all three templates + kubeconform GREEN; fail-guard verified; kind server-side smoke passed. All 6 ACs met.**