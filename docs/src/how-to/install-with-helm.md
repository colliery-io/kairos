# Install Kairos on Kubernetes with Helm

Get a Kairos 0.1.1 deployment serving on a cluster you already run.

**Before you start**, have all four:

- A cluster and Helm 3.8 or newer (the chart is distributed as an OCI artifact).
- **A PostgreSQL the cluster can reach, with `pgvector` available.** The chart
  bundles no database and there is no Postgres subchart. The extension is a
  requirement, not an option: the first migration installs it, and a database
  that cannot is refused at startup with a message saying so. Managed Postgres
  offers pgvector on RDS, Cloud SQL and Azure; `pgvector/pgvector:pg16` is the
  upstream image with it added.
- **An OIDC issuer with Kairos registered as an app.** The chart bundles no
  identity provider. If you have not registered the clients yet, do
  [Configure an OIDC issuer](configure-an-oidc-issuer.md) first — you need the
  issuer URL and audience before the chart will render.
- Your own OIDC `sub`, which becomes the deployment admin that provisions the
  first tenant.

Every value named below is specified in
[Configuration → Helm chart values](../reference/configuration.md#helm-chart-values).

## 1. Choose the tenancy mode

Set **exactly one** of these, and the choice drives DNS and TLS:

| Use | Set | Ingress | You need |
|---|---|---|---|
| One organization per host | `config.tenancy.baseDomain: kairos.example` — `acme.kairos.example` resolves to tenant `acme` | `ingress.tenancyMode: wildcard` routes the apex and `*.kairos.example` | Wildcard DNS and a wildcard certificate |
| A single organization | `config.tenancy.singleTenant: acme` | `ingress.tenancyMode: single` routes `ingress.host` | One A/AAAA record and one certificate |

Setting neither fails the render, and so does setting both:

```text
Error: execution error at (kairos/templates/deployment.yaml:1:4): config.tenancy:
set EXACTLY ONE of baseDomain (wildcard subdomain tenancy) or singleTenant
(single-tenant mode) — neither is set.
```

In wildcard mode, create the wildcard DNS record and issue the wildcard
certificate now — a DNS-01 cert-manager issuer, or a Secret you provide. The
rollout in step 4 will succeed without them and be unreachable.

## 2. Write a values file

Keep the database URL out of values by creating the Secret yourself:

```sh
kubectl create secret generic kairos-db \
  --from-literal=DATABASE_URL='postgres://kairos:PASSWORD@my-postgres:5432/kairos'
```

```yaml
# my-values.yaml
database:
  existingSecret: kairos-db          # the chart renders no Secret of its own
config:
  oidc:
    issuerUrl: "https://idp.example.com/"
    audience: "kairos"
  tenancy:
    baseDomain: "kairos.example"
  deploymentAdmins: "oidc-sub-of-the-first-admin"
ingress:
  enabled: true
  className: nginx
  tenancyMode: wildcard
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  tls:
    enabled: true
    secretName: kairos-tls
```

For an evaluation install, `database.url` inline and `config.tenancy.singleTenant`
are enough, and `ingress.enabled` can stay `false`.

Three values are load-bearing and the chart refuses to render without them —
each failure names itself, so you will not get a half-configured Deployment:

- `config.oidc.issuerUrl` — `config.oidc.issuerUrl is required (KAIROS-A-0016:
  bring your own OIDC issuer)`
- `config.oidc.audience` — the same, for the audience
- one of `database.url` / `database.existingSecret` — `database: set EITHER
  database.url (inline) OR database.existingSecret`

Then apply whichever of these describe your deployment:

- **Your issuer mints a distinct `aud` per OAuth client** (notably Google
  Workspace): make `config.oidc.audience` a YAML list of the client ids. A
  token matching any listed audience validates; there is no "any audience"
  mode.
- **Your GUI client is confidential** (again Google Workspace): set
  `config.webClientSecretExistingSecret` rather than `config.webClientSecret`,
  so the secret stays out of values and release history.
- **Your issuer's access token is opaque:** set `config.apiBearer: id_token`.
- **You front Kairos with your own gateway:** leave `ingress.enabled: false`
  and route to the Service yourself.
- **You want `KAIROS_PUBLIC_URL` or `KAIROS_WEBHOOK_SIGNING_KEY`** (needed for
  [Connect a git forge](connect-a-git-forge.md)): neither has a dedicated chart
  value. Set them through `extraEnv`.
- **You scrape with the Prometheus Operator:** set
  `metrics.serviceMonitor.enabled: true`, which scrapes `/metrics`. Liveness
  probes go at `/healthz`, readiness at `/readyz`; the chart wires both already.

Leave `image.tag` empty so it tracks the chart's `appVersion`. Do not pin
`latest`; the chart never resolves it. Leave `config.devUi` at `false` — it
mounts the Swagger UI at `/api/docs`.

## 3. Install

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos \
  --version 0.1.1 -f my-values.yaml
```

`--version` is not optional in practice: the chart publishes no floating tag.

## 4. Wait for the rollout

```sh
kubectl rollout status deploy/kairos
```

The container applies pending **public** migrations on boot, before it binds,
so there is no migration Job to run and no ordering for you to arrange. While
they are outstanding `/readyz` answers 503 with `not ready: pending database
migrations`.

The startup probe covers that window. If a first boot against a large database
outlasts it, the pod is killed before it ever reports ready — raise
`startupProbe.failureThreshold`
([Probes](../reference/configuration.md#probes)).

## 5. Confirm the release

```sh
helm test kairos          # runs a Pod that curls /healthz
```

## 6. Provision the first tenant

A fresh install has no organizations. Log in through your IdP, then follow
[Provision a tenant](provision-a-tenant.md) — the `config.deploymentAdmins`
value from step 2 is what grants you the cross-tenant admin routes.

## Upgrading and rolling back

```sh
helm upgrade kairos oci://ghcr.io/colliery-io/charts/kairos \
  --version <new> -f my-values.yaml
helm rollback kairos
```

`helm rollback` rolls the process back, not the database: migrations are
forward-only. Take a dump before every upgrade, and roll the database back with
it — see [Back up and restore](back-up-and-restore.md).

**After any upgrade, run the tenant migrations yourself:**

```sh
kubectl exec deploy/kairos -- kairos-server migrate-tenants
```

Boot migrates the public schema only, and `/readyz` checks only the public
schema, so a release that adds a tenant migration comes up reporting ready with
the per-tenant schemas still behind. Nothing tells you; run it every time.

## Related

- [Configuration](../reference/configuration.md) — every value and variable
- [Configure an OIDC issuer](configure-an-oidc-issuer.md)
- [Provision a tenant](provision-a-tenant.md)
- [Back up and restore](back-up-and-restore.md)
