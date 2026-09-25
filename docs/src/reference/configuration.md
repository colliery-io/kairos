# Configuration

Every value that changes how Kairos behaves, in one place. Kairos 0.1.0.

Configuration has four surfaces, and they are layered rather than alternative:
the server reads **environment variables** only; the **Helm chart** and the
**reference Compose deployment** are two ways of setting those variables; the
**CLI** has a small configuration surface of its own, unrelated to the
server's.

## Server environment variables

Sixteen variables are read once at startup. For those sixteen: a required
variable that is unset, or any variable set to an unparseable value, fails
startup with a message naming the variable — invalid values are never silently
replaced by defaults — and an empty or whitespace-only value is treated as
unset.

The five retention variables are not among them. Nothing in the server reads
them; their own parsing rules differ, and are stated in that section.

### Required

| Variable | Type | Description |
|---|---|---|
| `DATABASE_URL` | Postgres connection URL | The external PostgreSQL, which is the deployment's sole state. Example: `postgres://kairos:kairos@localhost:41432/kairos`. Also read directly by every server subcommand, including `migrate`, before the rest of the configuration is resolved. |
| `OIDC_ISSUER_URL` | URL | The OIDC issuer. Discovery and JWKS endpoints are derived from it. A trailing slash is stripped. |
| `OIDC_AUDIENCE` | string, or comma-separated list | The `aud` claim bearer tokens must carry. A comma-separated allow-list is accepted for issuers that mint a distinct `aud` per OAuth client, such as Google Workspace; a token matching any listed audience validates. Enforced non-empty at startup. |

### Network and logging

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_BIND_ADDR` | socket address | `127.0.0.1:8080` | Where the HTTP server listens. Rejected at startup if it does not parse as a socket address. Both packaged deployments override it so the container is reachable from outside: the Helm chart to `0.0.0.0:<containerPort>`, the Compose file to `0.0.0.0:8080`. |
| `KAIROS_LOG_LEVEL` | tracing filter directive | `info` | Passed to the tracing filter; accepts per-target directives, e.g. `info,kairos_server=debug`. |
| `KAIROS_LOG_FORMAT` | `json` \| `pretty` | `json` | Log encoding. Any other value fails startup. |
| `KAIROS_PUBLIC_URL` | URL | unset | The deployment's externally reachable base URL. Required to render the webhook delivery URL an operator pastes into a forge. A trailing slash is stripped. Not inferred from the request `Host` header. |

### Tenant resolution

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_BASE_DOMAIN` | domain | unset | Enables Host-subdomain tenant resolution: `acme.<base>` resolves to tenant `acme`. |
| `KAIROS_SINGLE_TENANT` | tenant slug | unset | Pins one tenant and skips subdomain and header resolution entirely. |

Exactly one of the two applies. With neither set, only the `X-Tenant` header
resolves a tenant.

The two packaged deployments treat this differently. The Helm chart requires
exactly one: rendering fails when both are set and also when neither is. The
Compose file enforces nothing — it forwards both variables unconditionally, and
the only guard is a comment in `deploy/.env.example`.

### Identity and the browser client

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_WEB_CLIENT_ID` | string | `kairos-web` | The public OAuth client id the GUI uses for its PKCE flow. |
| `KAIROS_API_BEARER` | `access_token` \| `id_token` | `access_token` | Which OIDC token the GUI and CLI send as the `/api` bearer. Any other value fails startup. Server-side validation is identical either way — it validates whatever RS256 JWT arrives — so this only tells the clients which token to send. `id_token` is for issuers whose access token is opaque and therefore unvalidatable, notably Google and Google Workspace. |
| `KAIROS_WEB_CLIENT_SECRET` | string | unset | The OAuth client secret for a confidential GUI client. Unset keeps public-client behaviour, which suits Dex and Keycloak. When set, the server-side token relay presents it on the code and refresh exchanges. Required for Google Workspace. Server-side only: it is never sent to the browser and never appears in `/api/config`. |
| `KAIROS_DEPLOYMENT_ADMINS` | comma-separated OIDC `sub`s | empty | Principals — human users or service accounts — allowed to call the cross-tenant `/api/admin/tenants` routes. Entries are trimmed and empty entries dropped. Empty or unset means those routes always return 403. |

### Forge webhooks

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_WEBHOOK_SIGNING_KEY` | string | unset | The deployment secret every per-connection webhook secret is derived from. When unset, forge connections can be neither created nor verified: the feature is off rather than degraded. |

Neither `KAIROS_WEBHOOK_SIGNING_KEY` nor `KAIROS_PUBLIC_URL` has a dedicated
Helm value or `.env.example` entry; both are set through the chart's
`extraEnv` or the container environment directly.

### Development

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_DEV_UI` | `true` \| `1` \| `false` \| `0` | `false` | Mounts the Swagger UI at `/api/docs`. At the default the route does not exist. Any other value fails startup. |
| `KAIROS_WEB_DIST` | directory path | unset | Serve the GUI from this directory instead of the assets embedded in the binary. When both are present, the directory wins. |

### Retention — recognised but inert

**In 0.1.0 the server reads none of the five variables below, and they have no
effect.** The retention sweeper's scheduler, `spawn_retention_loop`, is not
called anywhere in the server binary, and its own documentation records that
wiring as a later milestone. Nothing is compacted, offloaded or pruned
regardless of these values. The Helm chart surfaces them, so they are listed
here; the types and defaults below are the ones the parser implements and the
ones that will take effect once the loop is wired in.

Their parsing differs from the sixteen variables above: empty is not treated as
unset. An empty `KAIROS_HISTORY_HOT_DAYS`, `KAIROS_HISTORY_KEEP_LATEST`,
`KAIROS_ACTIVITY_RETENTION_DAYS` or `KAIROS_RETENTION_MODE` is a parse error
rather than a fallback to the default. `KAIROS_ARCHIVE_TARGET` is the one
exception: empty means no target.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_HISTORY_HOT_DAYS` | unsigned integer (days) | `90` | Hot window. `item_history` snapshots edited within this window are never touched. |
| `KAIROS_HISTORY_KEEP_LATEST` | unsigned integer | `5` | Latest-N guard. The newest N versions of an item are never prune candidates regardless of age. |
| `KAIROS_ACTIVITY_RETENTION_DAYS` | unsigned integer (days) | `365` | `activity_log` window. Rows older than this are archive-then-delete candidates, with no compaction tiers. |
| `KAIROS_ARCHIVE_TARGET` | filesystem path or `s3://…` URL | unset | Where prune candidates are offloaded as NDJSON. A `file://` prefix is stripped. An `s3://` value parses to a distinct S3 target that the shipped code rejects with a typed not-implemented error, rather than being misread as a directory name. Empty means no target. |
| `KAIROS_RETENTION_MODE` | `archive` \| `discard` \| `off` | `archive` | `archive` offloads before pruning and, with no target configured, prunes nothing and logs a warning instead. `discard` prunes without archiving. `off` disables the sweeper entirely. Case-insensitive; any other value fails to parse. |

Past the hot window, compaction keeps the first and last snapshot per item per
UTC calendar month and treats the intermediate versions as prune candidates.
See [Archiving](../explanation/archiving.md).

## Default board configurations

Provisioning a tenant seeds one configuration per flight level, and creating a
board copies its level's. These are the defaults; a board's columns and
transitions are data and may be changed afterwards.

| Level | Columns | Transitions |
|---|---|---|
| `strategy` | Draft, Review, Active, Monitoring, Completed | Draft → Review → Active → Monitoring → Completed |
| `initiative` | Discovery, Design, Ready, Decompose, Active, Monitoring, Completed | Discovery → Design → Ready → Decompose → Active → Monitoring → Completed |
| `delivery` | Backlog, Todo, Blocked, Active, Completed | Backlog → Todo → Active → Completed, plus Todo ↔ Blocked and Active ↔ Blocked |
| `adr` | Draft, Discussion, Decided, Superseded | Draft → Discussion → Decided → Superseded |

The delivery graph is the only one that is not a straight line: Blocked is
reachable from Todo and from Active, and returns to whichever it came from.
The strategy, initiative and ADR graphs are forward-only, so an item cannot be
moved back a column — a correction is a new item, not a reversal.

Stored in `public.system_board_defaults`, one row per level.

## CLI configuration

The CLI reads no server variables. Its whole configuration surface is where it
keeps cached credentials.

| Variable | Description |
|---|---|
| `KAIROS_CONFIG_DIR` | The config directory, taken verbatim. Highest precedence. |
| `XDG_CONFIG_HOME` | Used as `$XDG_CONFIG_HOME/kairos` when `KAIROS_CONFIG_DIR` is unset or empty. |
| `HOME` | Used as `$HOME/.config/kairos` when neither of the above is set. |

With none of the three set, any command that reaches the API fails with exit 1
and a message naming all three.

The directory is created mode `0700`, and `credentials.json` inside it is
written mode `0600` — both at creation and on every rewrite, including the
token-refresh path. Writes are atomic: a temporary file is written and then
renamed.

### `credentials.json`

```json
{
  "version": 1,
  "deployments": {
    "https://kairos.example.com": {
      "access_token": "eyJ…",
      "refresh_token": "…",
      "expires_at": 1793491200,
      "issuer": "https://idp.example.com",
      "client_id": "kairos-cli",
      "tenant": "acme",
      "api_bearer": "access_token"
    }
  }
}
```

| Field | Type | Description |
|---|---|---|
| `version` | integer | Store format version. `1`. Defaults to `1` when absent. |
| `deployments` | object | One entry per deployment, keyed by the normalized deployment URL — whitespace and trailing slashes trimmed. Serialized in sorted key order. |
| `deployments.*.access_token` | string | The token sent as the API bearer. Holds whichever token `api_bearer` names. |
| `deployments.*.refresh_token` | string, optional | Omitted when the issuer declined `offline_access`. |
| `deployments.*.expires_at` | integer | Access-token expiry, in Unix seconds. |
| `deployments.*.issuer` | string | The OIDC issuer the tokens came from. Refreshes go back to it. |
| `deployments.*.client_id` | string | The OAuth client id used for the device grant and for refresh. |
| `deployments.*.tenant` | string, optional | Sent as `X-Tenant`. Omitted when no tenant was cached at login. |
| `deployments.*.api_bearer` | `access_token` \| `id_token` | Which token `access_token` holds. Absent in files written before the field existed, which resolve to `access_token`. |

A token is refreshed when its `expires_at` is within 30 seconds of now, so a
token never expires mid-request. A file that does not parse as JSON is an
authentication error (exit 2) naming the file.

## Helm chart values

`deploy/helm/kairos/values.yaml`. Most values under `config` become entries in a
ConfigMap that the Deployment loads with `envFrom`. Two exceptions: the
`config.webClientSecret*` values never reach the ConfigMap — the secret is
Secret-sourced, like `DATABASE_URL` — and the five `config.retention.*` values
are emitted only when non-empty. The chart provides no identity provider, and
provides PostgreSQL only as an evaluation convenience you can decline
(`postgresql.enabled`).

### Bundled database

| Value | Type | Default | Description |
|---|---|---|---|
| `postgresql.enabled` | bool, optional | *(unset)* | Tri-state. Unset: on **unless** `database.url`/`existingSecret` is set. `true`: on, and naming an external database is refused at render time. `false`: off. Evaluation only — one replica, no backups. |
| `postgresql.image.repository` / `.tag` | string | `pgvector/pgvector` / `pg16` | Must carry `pgvector`; the plain `postgres` image does not. |
| `postgresql.auth.database` / `.username` / `.password` | string | `kairos` / `kairos` / `kairos-evaluation-only` | The password is in values and in release history by construction. Fixed rather than generated so an upgrade does not change it out from under the volume. |
| `postgresql.persistence.size` | string | `8Gi` | `0` uses an `emptyDir`, which loses the data when the pod restarts. |
| `postgresql.persistence.storageClass` | string | `""` | Cluster default when empty. |

### Semantic retrieval

The local model ships inside the image, so retrieval works with none of these
set. `KAIROS_EMBED_API_KEY` is Secret-sourced like `DATABASE_URL` and never
reaches the ConfigMap.

| Value | Env var | Default | Description |
|---|---|---|---|
| `embeddings.provider` | `KAIROS_EMBED_PROVIDER` | — | `local` \| `remote` \| `none`. Empty means local. `none` disables embeddings; search still works from text alone. |
| `embeddings.url` | `KAIROS_EMBED_URL` | — | OpenAI-compatible base URL. Setting it selects the remote provider on its own. |
| `embeddings.model` | `KAIROS_EMBED_MODEL` | — | Model name to request from that endpoint. |
| `embeddings.apiKey` / `embeddings.existingSecret` (+`existingSecretKey`) | `KAIROS_EMBED_API_KEY` | — | **Secret.** A local Ollama needs none. |

The server reads two more that the chart does not surface, because a deployment
should not normally need them: `KAIROS_EMBED_CACHE` (where the local model lives;
the image sets it to `/var/lib/kairos/models`) and `KAIROS_EMBED_REFRESH_SECS`
(how often the background refresher looks for work, default 10, `0` disables).

### Workload

| Value | Type | Default | Description |
|---|---|---|---|
| `replicaCount` | integer | `2` | Server replicas. Ignored when `autoscaling.enabled` is true. |
| `image.repository` | string | `ghcr.io/colliery-io/kairos` | Image repository. |
| `image.tag` | string | `""` | Empty tracks the chart's `appVersion`. An explicit value pins a published release, e.g. `"0.1.0"`. The chart never pins `latest`. |
| `image.pullPolicy` | string | `IfNotPresent` | Image pull policy. |
| `imagePullSecrets` | list | `[]` | Pull secrets, e.g. `[{name: ghcr-creds}]`. |
| `nameOverride` | string | `""` | Overrides the chart name used in resource names. |
| `fullnameOverride` | string | `""` | Overrides the fully-qualified release name used in resource names. |
| `resources.requests.cpu` | quantity | `100m` | CPU request. |
| `resources.requests.memory` | quantity | `128Mi` | Memory request. |
| `resources.limits.cpu` | quantity | `"1"` | CPU limit. |
| `resources.limits.memory` | quantity | `512Mi` | Memory limit. |
| `podAnnotations` | map | `{}` | Extra pod annotations. |
| `podLabels` | map | `{}` | Extra pod labels. |
| `nodeSelector` | map | `{}` | Node selector for pod scheduling. |
| `tolerations` | list | `[]` | Tolerations for pod scheduling. |
| `affinity` | map | `{}` | Affinity rules for pod scheduling. |
| `topologySpreadConstraints` | list | `[]` | Topology spread constraints, e.g. spreading replicas across zones. |
| `extraEnv` | list of EnvVar | `[]` | Appended verbatim to the container. The route for any variable the chart does not surface. |
| `extraVolumes` | list | `[]` | Extra pod volumes. |
| `extraVolumeMounts` | list | `[]` | Extra container volume mounts. |

### Security context

| Value | Type | Default | Description |
|---|---|---|---|
| `serviceAccount.create` | bool | `true` | Create a dedicated ServiceAccount for the pods. |
| `serviceAccount.name` | string | `""` | Empty uses the generated fullname when `create` is true, and `default` when it is false. |
| `serviceAccount.annotations` | map | `{}` | ServiceAccount annotations, e.g. IRSA or Workload Identity roles. |
| `serviceAccount.automountServiceAccountToken` | bool | `false` | Auto-mount the ServiceAccount token. Kairos does not call the Kubernetes API. |
| `podSecurityContext.runAsNonRoot` | bool | `true` | Pod-level security context. |
| `podSecurityContext.runAsUser` | integer | `65532` | Pod-level security context. |
| `podSecurityContext.runAsGroup` | integer | `65532` | Pod-level security context. |
| `podSecurityContext.fsGroup` | integer | `65532` | Pod-level security context. |
| `podSecurityContext.seccompProfile.type` | string | `RuntimeDefault` | Pod-level seccomp profile. |
| `containerSecurityContext.allowPrivilegeEscalation` | bool | `false` | Container-level security context. |
| `containerSecurityContext.readOnlyRootFilesystem` | bool | `true` | Container-level security context. The server writes no local files. |
| `containerSecurityContext.runAsNonRoot` | bool | `true` | Container-level security context. |
| `containerSecurityContext.capabilities.drop` | list | `[ALL]` | Linux capabilities dropped from the container. |

The read-only root filesystem is compatible with a filesystem
`KAIROS_ARCHIVE_TARGET` provided the path is a writable volume mounted through
`extraVolumes` and `extraVolumeMounts`.

### Networking

| Value | Type | Default | Description |
|---|---|---|---|
| `containerPort` | integer | `8080` | The container's listen port. The chart sets `KAIROS_BIND_ADDR` to `0.0.0.0:<containerPort>`. Also the Service `targetPort`. |
| `service.type` | string | `ClusterIP` | Service type. ClusterIP plus an Ingress is the intended topology. |
| `service.port` | integer | `80` | The port the Service exposes in-cluster. |
| `service.annotations` | map | `{}` | Service annotations. |
| `ingress.enabled` | bool | `false` | Create an Ingress. |
| `ingress.className` | string | `""` | IngressClass, e.g. `nginx` or `traefik`. Empty uses the cluster default. |
| `ingress.annotations` | map | `{}` | Ingress annotations. For cert-manager TLS, e.g. `cert-manager.io/cluster-issuer`. |
| `ingress.tenancyMode` | `wildcard` \| `single` | `wildcard` | `wildcard` routes `*.<config.tenancy.baseDomain>` and the apex, which needs wildcard DNS and a wildcard TLS certificate. `single` routes `ingress.host` only. |
| `ingress.host` | hostname | `kairos.example.com` | Used in `single` mode. In `wildcard` mode the host rules derive from `config.tenancy.baseDomain`. |
| `ingress.path` | string | `/` | HTTP path prefix. Kairos serves everything from `/`. |
| `ingress.pathType` | string | `Prefix` | Ingress path type. |
| `ingress.tls.enabled` | bool | `false` | Emit a TLS block on the Ingress. |
| `ingress.tls.secretName` | string | `""` | Name of the TLS Secret. Empty lets the ingress controller's default certificate apply. |

### Scaling and metrics

| Value | Type | Default | Description |
|---|---|---|---|
| `autoscaling.enabled` | bool | `false` | When true, `replicaCount` is ignored. |
| `autoscaling.minReplicas` | integer | `2` | HorizontalPodAutoscaler floor. |
| `autoscaling.maxReplicas` | integer | `6` | HorizontalPodAutoscaler ceiling. |
| `autoscaling.targetCPUUtilizationPercentage` | integer | `70` | Percentage of the CPU request. |
| `autoscaling.targetMemoryUtilizationPercentage` | integer or null | `null` | `null` disables the metric. |
| `metrics.serviceMonitor.enabled` | bool | `false` | Creates a Prometheus Operator ServiceMonitor for `/metrics`. Requires the `monitoring.coreos.com` CRDs. |
| `metrics.serviceMonitor.interval` | duration | `30s` | Scrape interval. |
| `metrics.serviceMonitor.scrapeTimeout` | duration | `10s` | Scrape timeout. |
| `metrics.serviceMonitor.labels` | map | `{}` | Extra labels, e.g. to match a Prometheus `serviceMonitorSelector`. |

### Probes

| Value | Type | Default | Description |
|---|---|---|---|
| `livenessProbe` | Probe | `httpGet` `/healthz` on port `http`, `initialDelaySeconds: 5`, `periodSeconds: 10`, `timeoutSeconds: 3`, `failureThreshold: 3` | Passed through verbatim. `/healthz` is process-only. |
| `readinessProbe` | Probe | `httpGet` `/readyz` on port `http`, `initialDelaySeconds: 5`, `periodSeconds: 10`, `timeoutSeconds: 3`, `failureThreshold: 3` | Passed through verbatim. `/readyz` checks the database and pending migrations. |
| `startupProbe.enabled` | bool | `true` | Emit a startupProbe, which covers slow first-boot migrations before liveness starts counting. |
| `startupProbe` | Probe | `httpGet` `/readyz` on port `http`, `periodSeconds: 5`, `failureThreshold: 30` | Passed through verbatim minus `enabled`. Allows up to `failureThreshold × periodSeconds` for the first successful `/readyz`. |

### Application configuration

| Value | Type | Default | Description |
|---|---|---|---|
| `config.oidc.issuerUrl` | string | `""` | Sets `OIDC_ISSUER_URL`. Required: rendering fails without it. |
| `config.oidc.audience` | string or list | `""` | Sets `OIDC_AUDIENCE`. Required. A list is joined with commas. |
| `config.webClientId` | string | `kairos-web` | Sets `KAIROS_WEB_CLIENT_ID`. |
| `config.apiBearer` | string | `access_token` | Sets `KAIROS_API_BEARER`. |
| `config.webClientSecret` | string | `""` | Sets `KAIROS_WEB_CLIENT_SECRET` through a chart-rendered Secret. Never enters the ConfigMap. |
| `config.webClientSecretExistingSecret` | string | `""` | Name of a pre-existing Secret holding the value. When set, the chart renders none of its own. |
| `config.webClientSecretExistingSecretKey` | string | `KAIROS_WEB_CLIENT_SECRET` | Key within that Secret. |
| `config.tenancy.baseDomain` | string | `""` | Sets `KAIROS_BASE_DOMAIN`. Emitted only when non-empty. |
| `config.tenancy.singleTenant` | string | `""` | Sets `KAIROS_SINGLE_TENANT`. Emitted only when non-empty. |
| `config.deploymentAdmins` | string | `""` | Sets `KAIROS_DEPLOYMENT_ADMINS`. |
| `config.log.level` | string | `info` | Sets `KAIROS_LOG_LEVEL`. |
| `config.log.format` | string | `json` | Sets `KAIROS_LOG_FORMAT`. |
| `config.devUi` | bool | `false` | Sets `KAIROS_DEV_UI`. |
| `config.retention.historyHotDays` | integer or null | `null` | Sets `KAIROS_HISTORY_HOT_DAYS`. Emitted only when non-empty. |
| `config.retention.historyKeepLatest` | integer or null | `null` | Sets `KAIROS_HISTORY_KEEP_LATEST`. Emitted only when non-empty. |
| `config.retention.activityRetentionDays` | integer or null | `null` | Sets `KAIROS_ACTIVITY_RETENTION_DAYS`. Emitted only when non-empty. |
| `config.retention.archiveTarget` | string | `""` | Sets `KAIROS_ARCHIVE_TARGET`. Emitted only when non-empty. |
| `config.retention.mode` | string | `""` | Sets `KAIROS_RETENTION_MODE`. Emitted only when non-empty. |
| `database.url` | string | `""` | Sets `DATABASE_URL` through a chart-rendered Secret. |
| `database.existingSecret` | string | `""` | Name of a pre-existing Secret holding it. When set, the chart renders none of its own and this value wins over `database.url`. |
| `database.existingSecretKey` | string | `DATABASE_URL` | Key within that Secret. |

The chart's two rendering guards:

- **Tenancy.** Exactly one of `config.tenancy.baseDomain` and
  `config.tenancy.singleTenant`. Rendering fails when both are set and when
  neither is.
- **Database.** At least one of `database.url` and `database.existingSecret`.
  Rendering fails only when neither is set; with both set,
  `database.existingSecret` is used and no Secret is rendered.

Retention values left `null` or `""` are absent from the ConfigMap, which is
moot in 0.1.0 because the server reads none of them.

## Reference Compose deployment

`deploy/docker-compose.yaml` with `deploy/.env`, copied from
`deploy/.env.example`. The topology is Caddy for TLS and subdomain routing,
one Kairos container, and PostgreSQL 16 as the sole state — as
`pgvector/pgvector:pg16`, because Kairos requires the `pgvector` extension and
stock `postgres:16` does not carry it. There is no bundled identity provider.

| `.env` variable | Default in the example | Consumed by |
|---|---|---|
| `KAIROS_VERSION` | `0.1.0` | The Kairos image tag. Required — Compose fails if it is unset. |
| `KAIROS_SITE_ADDRESS` | `kairos.example.com` | Caddy's site address. A hostname enables automatic HTTPS; `:80` is a local no-TLS trial. Required. |
| `POSTGRES_PASSWORD` | `change-me` | The Postgres password, and the password inside the `DATABASE_URL` the Compose file composes. Required. |
| `OIDC_ISSUER_URL` | `https://idp.example.com/` | The server, verbatim. Required. |
| `OIDC_AUDIENCE` | `kairos` | The server, verbatim. Required. |
| `KAIROS_WEB_CLIENT_ID` | `kairos-web` | The server. Falls back to `kairos-web`. |
| `KAIROS_BASE_DOMAIN` | `kairos.example` | The server. Defaults to empty. |
| `KAIROS_SINGLE_TENANT` | commented out | The server. Defaults to empty. |
| `KAIROS_DEPLOYMENT_ADMINS` | empty | The server. Defaults to empty. |
| `KAIROS_LOG_LEVEL` | `info` | The server. Falls back to `info`. |
| `KAIROS_API_BEARER` | `access_token` | Nothing. Present in `.env.example` but **not forwarded** to the container by `deploy/docker-compose.yaml`. |
| `KAIROS_WEB_CLIENT_SECRET` | empty | Nothing. Present in `.env.example` but **not forwarded** to the container by `deploy/docker-compose.yaml`. |

The Compose file also fixes two values that have no `.env` entry:
`KAIROS_BIND_ADDR` is `0.0.0.0:8080` and `KAIROS_LOG_FORMAT` is `json`. The
image applies pending migrations on boot, before binding, so no separate
migration step exists in this deployment.

## Related reading

- [CLI](cli.md) — the commands that read the CLI configuration above
- [Archiving](../explanation/archiving.md)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Glossary](glossary.md)

## Related guides

- [Install with Helm](../how-to/install-with-helm.md)
- [Configure an OIDC issuer](../how-to/configure-an-oidc-issuer.md)
- [Provision a tenant](../how-to/provision-a-tenant.md)
- [Connect a git forge](../how-to/connect-a-git-forge.md)
- [Back up and restore](../how-to/back-up-and-restore.md)
