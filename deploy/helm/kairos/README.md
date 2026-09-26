# Kairos Helm chart

Deploys [Kairos](https://github.com/colliery-io/kairos) — Flight Levels work
management — to Kubernetes. Kairos is a single stateless binary that serves the
GUI (`/`), REST API (`/api`), MCP (`/mcp`), SCIM (`/scim/v2`), and the ops
endpoints (`/healthz`, `/readyz`, `/metrics`) from **one container**
(KAIROS-A-0015). This chart deploys that image and nothing else.

## What this chart does NOT include

Per **KAIROS-A-0016** (identity is external) and **KAIROS-A-0013** (Postgres is
the operator's sole state), the chart bundles **no identity provider**, and
bundles a database only as an evaluation convenience you can decline (see below).
You must bring:

- **An external PostgreSQL, carrying `pgvector`** for any real deployment,
  reachable via
  `DATABASE_URL`. The server applies pending public migrations on boot
  (KAIROS-T-0007) before it reports ready — **there is no migration Job to
  run** — and the first of those migrations installs the `vector` extension.
  A database that cannot install it is refused at startup with a message
  naming it (KAIROS-A-0021 rule 2, KAIROS-T-0187); pgvector ships with RDS,
  Cloud SQL and Azure Database for PostgreSQL, and the
  `pgvector/pgvector:pg16` image is stock Postgres with it added.
- **An external OIDC issuer** (Okta, Entra ID, Auth0, Keycloak, Dex, …). Point
  `OIDC_ISSUER_URL` / `OIDC_AUDIENCE` at it and register Kairos there as an OIDC
  app (plus an optional SCIM app for user lifecycle).

### The bundled database, and what it is not

Since **KAIROS-A-0021 rule 2** the chart *can* stand a PostgreSQL up for you, and
`helm install` with nothing but the two OIDC values produces a running
deployment. That exists because Kairos now requires the `pgvector` extension, and
"bring your own Postgres, and it must have an extension your provider may not
have enabled" is a poor first afternoon.

**It is for evaluation.** One replica, one PVC, no backups, no failover, and a
password that lives in values and in your release history. A production
deployment sets `postgresql.enabled: false` and points `database.url` at a
managed instance — RDS, Cloud SQL and Azure Database for PostgreSQL all offer
pgvector.

The earlier posture is not reversed so much as narrowed. State is still the
operator's; what changed is that the absolute became a default you can decline.
A-0013 and A-0016 record the amendment.

`postgresql.enabled` has **three** states rather than two:

| value | effect |
|---|---|
| *(unset, the default)* | bundled — **unless** you set `database.url`/`existingSecret` |
| `true` | bundled, and naming an external database is an error |
| `false` | no bundle; exactly the chart as it was before |

The first row is what makes `helm upgrade` safe for a release that already
exists: those all name a database, so they keep it, and you need change nothing.
Asking for both explicitly is refused at render time rather than quietly
resolved — an upgrade that picked one for you would either abandon your database
or stand a second one up beside it, and you would find out later.

## Install

From the published chart (each release is pushed to GHCR as an OCI artifact;
`--version` pins it, since the chart never resolves `latest`):

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos \
  --version 0.4.0 -f my-values.yaml
```

Or from a checkout, which is what you want when changing the chart itself:

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
| `postgresql.enabled` | — | *(unset)* | Bundled evaluation database. Unset = on unless `database.*` is set; `false` = off. **Not for production.** |
| `postgresql.auth.*`, `postgresql.persistence.size` | — | `kairos` / `8Gi` | Bundled database only. `size: 0` uses an emptyDir and loses the data. |
| `database.url` / `database.existingSecret` (+`existingSecretKey`) | `DATABASE_URL` | — | **Secret.** Set one, or leave both empty for the bundled database. |
| `embeddings.provider` / `url` / `model` | `KAIROS_EMBED_*` | — | Semantic retrieval. The local model ships in the image; set `url` for an OpenAI-compatible endpoint. |
| `embeddings.apiKey` / `embeddings.existingSecret` (+`existingSecretKey`) | `KAIROS_EMBED_API_KEY` | — | **Secret**, handled like `database.url`. A local Ollama needs none. |
| `config.oidc.issuerUrl` | `OIDC_ISSUER_URL` | — | Required (external IdP), **unless** the chart bundles a Dex (`dex.enabled`) or `config.localAuth.enabled` is on. Rendering fails when none of the three provides a way to log in, rather than letting the pod crash-loop on a config the server refuses. |
| `config.oidc.audience` | `OIDC_AUDIENCE` | — | Required. String or list; a list (or comma-separated string) is an `aud` allow-list for per-client-audience IdPs like Google Workspace (KAIROS-T-0055). |
| `config.webClientId` | `KAIROS_WEB_CLIENT_ID` | `kairos-web` | GUI PKCE client id. |
| `config.apiBearer` | `KAIROS_API_BEARER` | `access_token` | `access_token` (Dex/Keycloak) or `id_token` (opaque-access-token IdPs, e.g. Google Workspace). |
| `config.webClientSecret` / `webClientSecretExistingSecret` (+`…Key`) | `KAIROS_WEB_CLIENT_SECRET` | `""` | **Secret.** Confidential GUI client secret; **required for Google Workspace** ("Web application" clients). Empty for public clients. |
| `config.tenancy.baseDomain` | `KAIROS_BASE_DOMAIN` | `""` | Wildcard tenancy. |
| `config.tenancy.singleTenant` | `KAIROS_SINGLE_TENANT` | `""` | Single-tenant. |
| `config.deploymentAdmins` | `KAIROS_DEPLOYMENT_ADMINS` | `""` | Comma-separated OIDC subs. |
| `config.log.level` | `KAIROS_LOG_LEVEL` | `info` | |
| `config.log.format` | `KAIROS_LOG_FORMAT` | `json` | `json` or `pretty`. |
| `config.otel.endpoint` | `KAIROS_OTEL_ENDPOINT` | `""` | OTLP/**HTTP** traces endpoint, e.g. `http://collector:4318/v1/traces`. Empty disables tracing entirely. |
| `config.otel.sampleRatio` | `KAIROS_OTEL_SAMPLE_RATIO` | `""` | Head sampling 0.0–1.0; server default `1.0`. Emitted only when set. |
| `config.auth.maxFailures` | `KAIROS_AUTH_MAX_FAILURES` | `""` | Failed authentications before a lockout; server default `5`. Emitted when set, **including `0`**, which turns throttling off. |
| `config.auth.failureWindowSecs` | `KAIROS_AUTH_FAILURE_WINDOW_SECS` | `""` | How long failures accumulate; server default `300`. Emitted only when set. |
| `config.auth.lockoutSecs` | `KAIROS_AUTH_LOCKOUT_SECS` | `""` | How long a lockout lasts; server default `60`. Emitted only when set. |
| `config.localAuth.enabled` | `KAIROS_LOCAL_AUTH` | `false` | Accept local password accounts in addition to the issuer. Off means `/api/login` is not routed at all. Accounts are admin-created; there is no self-service sign-up and no reset email. |
| `config.localAuth.sessionTtlSecs` | `KAIROS_SESSION_TTL_SECS` | `""` | Session bearer lifetime in seconds; server default `1209600` (14 days). Emitted only when set. |
| `config.localAuth.bootstrapAdmin` | `KAIROS_BOOTSTRAP_ADMIN` | `""` | A first-boot admin, created only on a boot that finds no users at all and inert afterwards. Also becomes a deployment admin, since a fresh deployment has no organization either. Remove it once the account exists. |
| `config.localAuth.bootstrapPasswordHash` | — | `""` | Its password as a PHC hash (`kairos-server hash-password`), rendered into a Secret, never the ConfigMap. Use a values file, not `--set`: a PHC string contains commas. |
| `config.localAuth.bootstrapPasswordExistingSecret` | — | `""` | A Secret holding it instead; wins over the above. |
| `config.auth.trustedProxy` | `KAIROS_TRUSTED_PROXY` | `""` | Trust `X-Forwarded-For` for the client address. Empty **follows `ingress.enabled`** — behind an Ingress the socket peer is the ingress controller, so the header is the only real client address; without one it is caller-supplied and trusting it would remove the throttle rather than weaken it. |
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

## Bundled Dex (evaluation only)

| Value | Env / effect | Default | Notes |
|---|---|---|---|
| `dex.enabled` | — | *(unset)* | Tri-state. Unset: on **unless** `config.oidc.issuerUrl` is set **or** `config.localAuth.enabled` is on. `true`: on, and naming an issuer is refused at render time. `false`: off. The local-auth clause matters: asking for password accounts must not silently also hand you an identity provider with a static password in your values file. |
| `dex.image.repository` / `.tag` | — | `ghcr.io/dexidp/dex` / `v2.43.1` | Pinned, like every image in this chart. |
| `dex.adminEmail` | the one static user | `""` | Required when bundled. |
| `dex.adminPasswordHash` | its bcrypt hash | `""` | **Required** when bundled — the chart ships no default, because a default is a known credential in every install. `docker run --rm httpd:2.4 htpasswd -bnBC 10 "" 'pw' \| tr -d ':\n'` |
| `dex.extraUsers` | more static users | `[]` | `{email, username, userID, hash}`. |
| `dex.resources` | — | 25m / 64Mi | |

Requires `ingress.enabled`: the issuer URL is stamped into every token's `iss`, so
the browser and the server must use the same one and an in-cluster Service name is
not reachable from a browser. The chart routes Dex at `/dex` on your ingress.

In-memory storage, so a restart invalidates every token. No user lifecycle, no
password reset, no MFA, no audit trail. See
[Install with Helm](https://colliery-io.github.io/kairos/how-to/install-with-helm.html).
