---
id: 001-deployment-and-operations-baseline
level: adr
title: "Deployment and Operations Baseline"
number: 1
short_code: "KAIROS-A-0013"
created_at: 2026-07-08T11:28:50.652468+00:00
updated_at: 2026-07-08T15:00:45.336463+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Deployment and Operations Baseline

## Context

The vision commits to progressive adoption ("simple to self-host — Docker Compose — and simple to operate") and a stateless server whose only state is PostgreSQL. With one binary serving GUI + API + MCP (A-0009, A-0011, A-0015) and Keycloak as a hard dependency (A-0010), the deployment story, configuration surface, and observability baseline need fixing before agents build them.

## Decision

**One OCI image, a reference Docker Compose deployment (Caddy + Kairos + Postgres 16; identity external per KAIROS-A-0016), 12-factor env-var configuration, JSON tracing logs, health endpoints, and Prometheus metrics.**

### Build artifacts
- A single multi-stage Dockerfile: builds the Leptos/WASM assets (`cargo-leptos`) and the release server binary, ships a minimal runtime image. Frontend assets are embedded/served by the binary — no separate frontend container
- Versioned image tags per release; `latest` never used in reference deployments

### Reference deployment (compose) — amended by KAIROS-A-0016 (2026-07-10): no bundled IdP
```
caddy       # TLS termination + wildcard subdomain routing (*.kairos.example → kairos)
kairos      # the server binary: / (GUI), /api, /mcp, /scim/v2, /healthz, /readyz, /metrics
postgres:16 # sole state; named volume
```
- **Identity is external** (A-0016): the operator points `OIDC_ISSUER_URL`/`OIDC_AUDIENCE` at their IdP (Okta, Entra, Auth0, Keycloak, …) and registers Kairos there as an OIDC app + optional SCIM app. Docs include per-IdP registration notes and a one-container Dex quickstart labeled *example, not product*
- The repo's existing `.angreal/docker-compose.yaml` grows into this reference; `angreal services up` remains the dev entry point; the dev/test profile runs **Dex** as its issuer fixture (A-0010/A-0012)
- **Single-tenant mode**: `KAIROS_SINGLE_TENANT=<slug>` skips subdomain resolution and pins one schema — evaluation installs need no wildcard DNS

### Configuration (env vars only, no config files)
`DATABASE_URL`, `KAIROS_BASE_DOMAIN`, `KAIROS_SINGLE_TENANT` (optional), `OIDC_ISSUER_URL`, `OIDC_AUDIENCE`, `KAIROS_BIND_ADDR`, `KAIROS_LOG_LEVEL`, `KAIROS_LOG_FORMAT` (json|pretty), `KAIROS_OTEL_ENDPOINT` (optional). Fail fast at startup on missing required config with an explicit message naming the variable.

### Observability
- `tracing` + `tracing-subscriber`: structured JSON logs by default; every request span carries tenant slug, user id, route, status, latency
- `/healthz` (process liveness, no dependencies) and `/readyz` (DB connectivity + pending-migration check)
- Prometheus metrics at `/metrics`: HTTP request histograms by route/status, connection-pool gauges, per-tenant request counters
- OpenTelemetry trace export only when `KAIROS_OTEL_ENDPOINT` is set — off by default

### State and backup
PostgreSQL is the only state (vision constraint). The operations doc ships `pg_dump`-based backup/restore guidance covering all `org_*` schemas plus public. No server-local files to back up; identity state lives entirely in the customer's IdP (A-0016).

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Compose reference + single image (chosen) | Matches vision's self-host promise; trivially runnable; one artifact to version | Manual scaling story; no orchestration primitives | Low | Low |
| Kubernetes/Helm as primary | Production-grade orchestration | Wildly over-scoped for v1 adoption profile; steep operator burden | Medium | High |
| Managed SaaS only | No operator burden for customers | Contradicts self-host principle and progressive adoption | High | High |
| Separate frontend container/CDN | Independent asset scaling | Second artifact, CORS surface, contradicts served-from-root decision | Low | Medium |

## Rationale

1. **The vision already made this call** ("Docker Compose, simple to operate") — this ADR just makes it buildable: exact services, exact config surface, exact endpoints.
2. **One image, one binary** keeps the version matrix trivial (GUI/API/MCP cannot skew) and makes rollback `docker compose pull` with a previous tag.
3. **Env-only config with fail-fast** is the least surprising contract for operators and the easiest for agents to implement correctly.
4. **Caddy in the reference stack** solves the wildcard-subdomain TLS problem (A-0005's known deployment cost) with the least configuration of any proxy.

## Consequences

### Positive
- `angreal services up` gives contributors and agents the full production topology locally
- Health/readiness endpoints make compose, CI, and future orchestrators all use the same probes
- Single-tenant mode removes the DNS barrier for evaluations and demos

### Negative
- Horizontal scaling beyond one node is out of scope (stateless server makes it straightforward later, but nothing ships)
- ~~Keycloak adds real memory footprint~~ — resolved by KAIROS-A-0016: the minimum deployment is Caddy + Kairos + Postgres

### Neutral
- Helm/K8s manifests are a future initiative if customer demand appears — nothing in this design blocks them
- CI publishes the image; release process details live with implementation tasks

## Review Schedule

### Review Triggers
- First customer deployment that outgrows a single node (introduce orchestration guidance)
- Keycloak resource footprint becomes an adoption objection (evaluate lighter OIDC dev-mode alternative)