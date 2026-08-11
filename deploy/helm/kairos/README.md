# Kairos Helm chart

Deploys [Kairos](https://github.com/colliery-io/kairos) — Flight Levels work
management — to Kubernetes. Kairos is a single stateless binary that serves the
GUI (`/`), REST API (`/api`), MCP (`/mcp`), SCIM (`/scim/v2`), and the ops
endpoints (`/healthz`, `/readyz`, `/metrics`) from **one container**
(KAIROS-A-0015). This chart deploys that image and nothing else.

## What this chart does NOT include

Per **KAIROS-A-0016** (identity is external) and **KAIROS-A-0013** (Postgres is
the operator's sole state), the chart bundles **neither a database nor an
identity provider**. You must bring:

- **An external PostgreSQL** reachable via `DATABASE_URL`. The server applies
  pending public migrations on boot (KAIROS-T-0007) before it reports ready —
  **there is no migration Job to run.**
- **An external OIDC issuer** (Okta, Entra ID, Auth0, Keycloak, Dex, …). Point
  `OIDC_ISSUER_URL` / `OIDC_AUDIENCE` at it and register Kairos there as an OIDC
  app (plus an optional SCIM app for user lifecycle).

There is deliberately no Postgres subchart — a stateful dependency in the chart
would contradict A-0016's "state and identity are the operator's" posture.

## Install

```sh
helm install kairos deploy/helm/kairos -f my-values.yaml
```

Minimal `my-values.yaml` for a single-tenant evaluation:

```yaml
database:
  url: "postgres://kairos:PASSWORD@my-postgres:5432/kairos"
config:
  oidc:
    issuerUrl: "https://idp.example.com/"
    audience: "kairos"
  tenancy:
    singleTenant: "acme"     # exactly one of singleTenant / baseDomain
  deploymentAdmins: "oidc-sub-of-the-first-admin"
```

Production (wildcard subdomain tenancy + ingress + cert-manager TLS), with the
database URL kept out of values via an existing Secret:

```sh
kubectl create secret generic kairos-db \
  --from-literal=DATABASE_URL='postgres://kairos:PASSWORD@my-postgres:5432/kairos'
```

```yaml
database:
  existingSecret: kairos-db          # chart renders no Secret of its own
config:
  oidc:
    issuerUrl: "https://idp.example.com/"
    audience: "kairos"
  tenancy:
    baseDomain: "kairos.example"     # acme.kairos.example -> tenant acme
  deploymentAdmins: "oidc-sub-of-the-first-admin"
ingress:
  enabled: true
  className: nginx
  tenancyMode: wildcard              # routes kairos.example + *.kairos.example
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  tls:
    enabled: true
    secretName: kairos-tls
```

Upgrade / rollback are `helm upgrade` / `helm rollback` — the server is
stateless, so rolling to a previous image tag is a clean rollback.

## Tenancy: exactly one mode

Set **exactly one** of `config.tenancy.baseDomain` or
`config.tenancy.singleTenant`. Setting both, or neither, fails the render (and
the server would fail fast at boot anyway).

| Mode | Value | Ingress | DNS / TLS |
|------|-------|---------|-----------|
| Wildcard subdomain (KAIROS-A-0005 §2) | `config.tenancy.baseDomain` | `ingress.tenancyMode: wildcard` routes the apex **and** `*.baseDomain` | Wildcard DNS + wildcard cert (cert-manager DNS-01, or a provided secret) |
| Single-tenant | `config.tenancy.singleTenant` | `ingress.tenancyMode: single` routes `ingress.host` | One A/AAAA record + one cert |

In the compose reference (`deploy/docker-compose.yaml`) Caddy terminated TLS and
did the wildcard routing; on Kubernetes the Ingress replaces Caddy.

## Values surface

### Image & scheduling

| Key | Default | Purpose |
|-----|---------|---------|
| `image.repository` | `ghcr.io/colliery-io/kairos` | Release image (KAIROS-T-0047). |
| `image.tag` | `""` → chart `appVersion` | Never `latest` (KAIROS-A-0013). |
| `image.pullPolicy` | `IfNotPresent` | |
| `imagePullSecrets` | `[]` | Private-registry pull secrets. |
| `replicaCount` | `2` | Stateless server; ignored when autoscaling on. |
| `resources` | 100m/128Mi … 1/512Mi | Requests/limits. |
| `nodeSelector` / `tolerations` / `affinity` / `topologySpreadConstraints` | `{}` / `[]` | Scheduling. |
| `podSecurityContext` / `containerSecurityContext` | non-root, read-only rootfs, drop ALL caps | Hardened defaults. |

### Service, ingress, autoscaling, metrics

| Key | Default | Purpose |
|-----|---------|---------|
| `service.type` / `service.port` | `ClusterIP` / `80` | In-cluster Service. |
| `containerPort` | `8080` | Pod listen port; sets `KAIROS_BIND_ADDR=0.0.0.0:<port>`. |
| `ingress.enabled` | `false` | Create an Ingress. |
| `ingress.className` | `""` | IngressClass. |
| `ingress.tenancyMode` | `wildcard` | `wildcard` or `single` (see table above). |
| `ingress.host` | `kairos.example.com` | Host for `single` mode. |
| `ingress.annotations` | `{}` | e.g. cert-manager issuer. |
| `ingress.tls.enabled` / `ingress.tls.secretName` | `false` / `""` | TLS block. |
| `autoscaling.enabled` | `false` | HPA (min/max/target CPU/memory). |
| `metrics.serviceMonitor.enabled` | `false` | Prometheus Operator ServiceMonitor scraping `/metrics`. |

### Application config (env surface, KAIROS-A-0013)

Non-secret keys become a ConfigMap loaded via `envFrom`; `DATABASE_URL` is a
Secret loaded via `valueFrom`.

| Key | Env var | Default | Notes |
|-----|---------|---------|-------|
| `database.url` / `database.existingSecret` (+`existingSecretKey`) | `DATABASE_URL` | — | **Secret.** Set one. |
| `config.oidc.issuerUrl` | `OIDC_ISSUER_URL` | — | Required (external IdP). |
| `config.oidc.audience` | `OIDC_AUDIENCE` | — | Required. String or list; a list (or comma-separated string) is an `aud` allow-list for per-client-audience IdPs like Google Workspace (KAIROS-T-0055). |
| `config.webClientId` | `KAIROS_WEB_CLIENT_ID` | `kairos-web` | GUI PKCE client id. |
| `config.apiBearer` | `KAIROS_API_BEARER` | `access_token` | `access_token` (Dex/Keycloak) or `id_token` (opaque-access-token IdPs, e.g. Google Workspace). |
| `config.webClientSecret` / `webClientSecretExistingSecret` (+`…Key`) | `KAIROS_WEB_CLIENT_SECRET` | `""` | **Secret.** Confidential GUI client secret; **required for Google Workspace** ("Web application" clients). Empty for public clients. |
| `config.tenancy.baseDomain` | `KAIROS_BASE_DOMAIN` | `""` | Wildcard tenancy. |
| `config.tenancy.singleTenant` | `KAIROS_SINGLE_TENANT` | `""` | Single-tenant. |
| `config.deploymentAdmins` | `KAIROS_DEPLOYMENT_ADMINS` | `""` | Comma-separated OIDC subs. |
| `config.log.level` | `KAIROS_LOG_LEVEL` | `info` | |
| `config.log.format` | `KAIROS_LOG_FORMAT` | `json` | `json` or `pretty`. |
| `config.otelEndpoint` | `KAIROS_OTEL_ENDPOINT` | `""` | Emitted only when set. |
| `config.retention.historyHotDays` | `KAIROS_HISTORY_HOT_DAYS` | server `90` | Emitted only when set. |
| `config.retention.historyKeepLatest` | `KAIROS_HISTORY_KEEP_LATEST` | server `5` | Emitted only when set. |
| `config.retention.activityRetentionDays` | `KAIROS_ACTIVITY_RETENTION_DAYS` | server `365` | Emitted only when set. |
| `config.retention.archiveTarget` | `KAIROS_ARCHIVE_TARGET` | `""` | Path or `s3://…`. |
| `config.retention.mode` | `KAIROS_RETENTION_MODE` | server `archive` | `archive` \| `discard` \| `off`. |
| `config.devUi` | `KAIROS_DEV_UI` | `false` | Swagger UI; keep off in prod. |
| `extraEnv` | (verbatim) | `[]` | Extra `EnvVar`s. |

## Probes and migrations

- **Liveness** hits `/healthz` (process only).
- **Readiness** hits `/readyz` (DB connectivity + pending-migration check).
- An optional **startupProbe** (`startupProbe.enabled: true`, default on) hits
  `/readyz` and gives boot migrations up to `failureThreshold*periodSeconds`
  before liveness starts counting.

## Bootstrapping a fresh install

1. Set `config.deploymentAdmins` to your own OIDC `sub` — that grants the
   cross-tenant admin routes (`/api/admin/tenants`).
2. Log in via your IdP, then `POST /api/admin/tenants` to create your first
   tenant.
3. Add members: SCIM push from your IdP, or `POST /api/members` by email
   (JIT-on-first-login also works for small orgs).

## Verify the release

```sh
helm test kairos            # runs a Pod that curls /healthz
```

## Follow-up

A real `kind` cluster smoke test is a documented follow-up (see the task's
Status Updates). The CI gate for this chart is `helm lint` + `helm template`
across three value sets + `kubeconform` schema validation on the rendered
manifests.
