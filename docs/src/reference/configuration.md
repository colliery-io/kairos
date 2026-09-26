# Configuration

Every value that changes how Kairos behaves, in one place. Kairos 0.4.0.

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
| `OIDC_ISSUER_URL` | URL | The OIDC issuer. Discovery and JWKS endpoints are derived from it. A trailing slash is stripped. **Not required when `KAIROS_LOCAL_AUTH` is on** — see below. |
| `OIDC_AUDIENCE` | string, or comma-separated list | The `aud` claim bearer tokens must carry. A comma-separated allow-list is accepted for issuers that mint a distinct `aud` per OAuth client, such as Google Workspace; a token matching any listed audience validates. Enforced non-empty at startup. Required exactly when `OIDC_ISSUER_URL` is set. |

#### Running with no identity provider

The two OIDC variables are required **unless `KAIROS_LOCAL_AUTH` is on**, in which
case a deployment can run with no identity provider at all: people log in with a
password and nothing else is needed. That is the point of the exception, and it is what
makes Kairos runnable by a small team or on a laptop without first standing up Dex or a
cloud OAuth client.

The condition is *local auth is on*, not *the variable is empty*. A deployment that
means to use OIDC and mistyped its issuer still fails at startup with the message it
always gave — coming up with no way for anyone to log in would be a far worse failure
than refusing to start. For the same reason, setting one of the pair without the other
is refused: the symptom of a missing audience is every token being rejected, which
points at the token rather than at the configuration.

With no issuer, a JWT bearer gets a 401 that **says** there is no issuer configured.
That is the one failure on the auth path that deliberately names its cause: everywhere
else a uniform message protects a secret, and here there is none — no amount of
guessing turns "this server has no issuer" into access, and a caller told only
"invalid token" would search their own token for a fault that is not there.

`GET /api/config` reports `issuer: null` and `local_auth: true`, so the GUI offers a
password form and no SSO button it cannot honour. `kairos login` says plainly that
there is nothing to authenticate against; for CLI access on such a deployment, use a
service-account API key.

### Network and logging

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_BIND_ADDR` | socket address | `127.0.0.1:8080` | Where the HTTP server listens. Rejected at startup if it does not parse as a socket address. Both packaged deployments override it so the container is reachable from outside: the Helm chart to `0.0.0.0:<containerPort>`, the Compose file to `0.0.0.0:8080`. |
| `KAIROS_LOG_LEVEL` | tracing filter directive | `info` | Passed to the tracing filter; accepts per-target directives, e.g. `info,kairos_server=debug`. |
| `KAIROS_LOG_FORMAT` | `json` \| `pretty` | `json` | Log encoding. Any other value fails startup. |
| `KAIROS_PUBLIC_URL` | URL | unset | The deployment's externally reachable base URL. Required to render the webhook delivery URL an operator pastes into a forge. A trailing slash is stripped. Not inferred from the request `Host` header. |

### Local password accounts

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_LOCAL_AUTH` | bool | `false` | Accept local password accounts in addition to the OIDC issuer. Off means `POST /api/login` is **not routed** — a 404 from an absent route, not a 401 from a handler that declines. |
| `KAIROS_SESSION_TTL_SECS` | integer | `1209600` (14 days) | How long a session bearer lasts. |
| `KAIROS_BOOTSTRAP_ADMIN` | email | unset | A first-boot admin, created **only on a boot that finds no users at all**. See below. |
| `KAIROS_BOOTSTRAP_PASSWORD` | string | unset | Its password. Prefer the hash form. |
| `KAIROS_BOOTSTRAP_PASSWORD_HASH` | PHC string | unset | Its password, already hashed — produce one with `kairos-server hash-password`. Wins over the plaintext when both are set. |

#### The first boot

A fresh deployment with local auth has nobody who can log in and no way to create
anybody, so `KAIROS_BOOTSTRAP_ADMIN` makes the first account.

The obvious shape has a trap worth designing around rather than discovering: **an
environment variable in a values file is a permanent credential**, in the deployment
manifest and in the release history. So the mechanism is **single-use**. It is consumed
on a boot that finds `public.users` empty, and inert on every boot after — including a
boot with the same email, which will *not* reset the password of the account it made.
An idempotent bootstrap would silently restore a known password on every restart, which
is a backdoor with a documented name.

Every outcome is logged, and the two that are not the happy path are logged at WARN,
because each is something to act on. When it is consumed, the log says to remove the
variables. When it is inert, the log says the same and adds why — silence there is how a
bootstrap password survives in a values file for a year.

The email also becomes a **deployment admin** (as `local:<email>`, the synthetic
`external_id` a local account gets). That is not a convenience: a fresh deployment has
no organization either, and creating one is a deployment-admin action, so without it the
bootstrap admin can log in and do nothing at all — which reads as a broken install
rather than a missing variable.

Prefer `KAIROS_BOOTSTRAP_PASSWORD_HASH`. `kairos-server hash-password` prints a PHC
string and nothing else, needs no database, and can be run before the deployment exists,
so the plaintext never has to be written into a file. In the Helm chart, set it in a
values file rather than with `--set`: a PHC string contains commas, and `--set` would
silently truncate it to something that parses as a hash and then never matches the
password.

#### Recovering an account

There is no password-reset email. Two paths exist instead:

- **An org admin** resets it: `PUT /api/local-accounts/{user_id}/password`.
- **An operator** resets it with no login at all:
  `kairos-server set-password --email <email>`, which reads the password from stdin when
  `--password` is absent. It lives beside `drop-tenant` among the
  [operator subcommands](cli.md#operator-subcommands-kairos-server), because the case it
  exists for is that nobody can log in — so requiring a login is precisely what it cannot
  do. It refuses to *create* an account, since that would be a way to mint an admin on
  any deployment whose database you can reach.

Either way, **setting a password revokes every session that person held**. A password
change after a suspected compromise that left the attacker logged in would defeat the
only thing the person was trying to do.

Passwords must be at least **12 characters**, with no composition rules. A length floor
reliably buys entropy; "one upper, one digit, one symbol" mostly buys `Password1!` and
makes people write the password down.

Local accounts are **additive**, not an alternative: a deployment may have both an
issuer and local accounts, and neither path knows about the other. A person with one
email address is one `users` row either way (KAIROS-T-0197).

Accounts are created by an org admin. There is no self-service sign-up and no
password-reset email — the two intended uses are a small team with no identity
provider, and a break-glass admin for when an issuer is unreachable.

A successful login returns an opaque bearer, `kairos_ss_<64-hex>`, which the client
presents as `Authorization: Bearer <token>`. Only its SHA-256 is stored, so a
database dump yields nothing usable. Unlike a service-account API key it carries no
tenant: a session stands in for an OIDC token, and an OIDC token is deployment-wide,
so one login covers every organization the person belongs to and membership is
enforced per request as usual.

Two weeks is the default lifetime because it is a decision and not an accident.
Forever would be a permanent credential handed out by a login form; an hour logs
people out in the middle of the work they came to do.

`POST /api/logout` revokes the presented session. **Changing a password revokes
every other session that person holds** — enforced in the storage layer, in the same
transaction as the password write, because a password change after a suspected
compromise that left the attacker logged in would defeat the only thing the person
was trying to do.

Every way a login can fail — unknown email, wrong password, an account that has no
password because it authenticates through the issuer, a revoked or expired session —
returns one 401 with one message, and an unknown email deliberately costs the same
argon2 work as a real attempt. Otherwise the response time would answer the question
the message refuses to. Failed logins are throttled per account and per source; see
[Failed-authentication throttling](#failed-authentication-throttling).

### Failed-authentication throttling

Repeated failed authentications are throttled (KAIROS-T-0202). This matters most
for the local password login (KAIROS-I-0018): an OIDC token and an API key are
both high-entropy random values that nobody guesses, but a password is guessable
by definition.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_AUTH_MAX_FAILURES` | integer | `5` | Failures inside the window before a lockout. **`0` turns throttling off** — the escape hatch for a throttle that is misfiring, so that the fix does not need a new build. |
| `KAIROS_AUTH_FAILURE_WINDOW_SECS` | integer | `300` | How long failures accumulate. A failure older than this is forgotten, so someone who mistypes once a week never accumulates a lockout. |
| `KAIROS_AUTH_LOCKOUT_SECS` | integer | `60` | How long a lockout lasts. Short on purpose — see below. |
| `KAIROS_TRUSTED_PROXY` | bool | `false` | Whether `X-Forwarded-For` is trusted for the client address. See below. |

Two things are counted separately: the **identity** being attempted, and the
**source** it came from. Neither alone is enough. Counting only identities lets an
attacker try one password each against a thousand accounts; counting only sources
is meaningless behind a proxy, where every request shares one address.

The lockout is deliberately short. A long one is a denial-of-service an attacker
can aim at an account they know the name of, and it turns one person's typo into a
support request. Five more failures locks the subject out again, so a script gains
nothing from the brevity.

#### What is trusted for the client address

`X-Forwarded-For` is read **only** when `KAIROS_TRUSTED_PROXY` is on, and it is
off by default. The header is caller-supplied: anyone can send one. On a directly
exposed server, trusting it gives an attacker a new source per request, which does
not weaken a source-based throttle so much as remove it.

Turn it on only when **every** request reaches Kairos through a proxy you control
that appends the peer it saw. The reference compose stack qualifies and sets it
itself — the Kairos container publishes no port, so Caddy is the only way in. A
server you expose directly does not qualify. The chart follows `ingress.enabled`
unless you say otherwise, because behind an Ingress the socket peer is the
ingress controller and the header is the only real client address there is.

When the header is trusted, the **last** value in the list is used, not the first.
A proxy appends, so the list reads `<whatever the client sent>, <what the proxy
saw>`; the first element is the one under the caller's control.

#### Where the state lives

In process, in memory. The consequences are worth knowing:

- Lockouts are **per replica**. A Deployment scaled to three pods, or behind the
  chart's HPA, allows roughly three times the configured failures overall.
- A restart forgets every lockout.

This is the right trade for the single-binary deployments Kairos targets
(KAIROS-A-0013): the alternative puts a database write on the failure path of an
endpoint that is under attack, which is the moment you least want extra writes.
If you run many replicas and need a shared limit, rate-limit at the ingress.

A lockout increments `kairos_auth_lockouts_total{subject="identity"|"source"}` on
`/metrics` and logs at WARN. The subject *kind* is recorded; the email address or
IP is not, because a log line outlives the incident it was gathered for.

### Trace export

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_OTEL_ENDPOINT` | URL | unset | The OTLP/**HTTP** traces endpoint, e.g. `http://collector:4318/v1/traces`. **Unset disables tracing entirely** — no exporter is built and no span leaves the process. A URL that cannot be reached does not stop Kairos serving; it logs `otel: tracing is DISABLED` and carries on. |
| `KAIROS_OTEL_SAMPLE_RATIO` | 0.0–1.0 | `1.0` | Head sampling. Refused at startup if it does not parse or falls outside the range, rather than clamped — a clamped typo produces a collector that is mysteriously empty. An empty value means unset. |

Kairos exports over **HTTP/protobuf, not gRPC**, so the port is `4318` and the
path is part of the endpoint. A collector's gRPC port (`4317`) will accept the
connection and then reject the payload, which is a slow way to discover this.

Sampling is head-only: the decision is made when a trace starts, and it respects
a decision an upstream caller already made rather than cutting a trace in half.
Keeping only the slow and failed traces is *tail* sampling, which belongs in your
collector — it can see a whole trace, and this process cannot.

One span per HTTP request is produced, named `<METHOD> <matched route>` — the
route **pattern**, never the concrete path, so a collector groups requests by
operation instead of showing one operation per id. It carries the method, route,
status code and tenant, and is marked as an error only for 5xx: a 404 or a 403 is
the server working correctly.

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

**In 0.4.0 the server reads none of the five variables below, and they have no
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
plus the two `config.otel.*` values are emitted only when non-empty. The chart
provides no identity provider, and
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
| `image.tag` | string | `""` | Empty tracks the chart's `appVersion`. An explicit value pins a published release, e.g. `"0.4.0"`. The chart never pins `latest`. |
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
| `config.oidc.issuerUrl` | string | `""` | Sets `OIDC_ISSUER_URL`. Required **unless** the chart is bundling a Dex (`dex.enabled`) or `config.localAuth.enabled` is on; rendering fails when none of the three gives anyone a way to log in. |
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
| `config.otel.endpoint` | string | `""` | Sets `KAIROS_OTEL_ENDPOINT`. Emitted only when non-empty; empty disables tracing. |
| `config.otel.sampleRatio` | string | `""` | Sets `KAIROS_OTEL_SAMPLE_RATIO`. Emitted only when non-empty. |
| `config.auth.maxFailures` | integer or `""` | `""` | Sets `KAIROS_AUTH_MAX_FAILURES`. Emitted when set, **including `0`**, which turns throttling off. |
| `config.auth.failureWindowSecs` | integer or `""` | `""` | Sets `KAIROS_AUTH_FAILURE_WINDOW_SECS`. Emitted only when set. |
| `config.auth.lockoutSecs` | integer or `""` | `""` | Sets `KAIROS_AUTH_LOCKOUT_SECS`. Emitted only when set. |
| `config.localAuth.enabled` | bool | `false` | Sets `KAIROS_LOCAL_AUTH`. |
| `config.localAuth.sessionTtlSecs` | integer or `""` | `""` | Sets `KAIROS_SESSION_TTL_SECS`. Emitted only when set. |
| `config.localAuth.bootstrapAdmin` | string | `""` | Sets `KAIROS_BOOTSTRAP_ADMIN`. Rendering fails if it is set without a password, or a password without it. |
| `config.localAuth.bootstrapPasswordHash` | string | `""` | The first-boot password as a PHC hash, rendered into a Secret (never the ConfigMap). Set it in a values file, not with `--set` — a PHC string contains commas. |
| `config.localAuth.bootstrapPasswordExistingSecret` | string | `""` | Name of a Secret holding it instead. Wins over the above. |
| `config.localAuth.bootstrapPasswordExistingSecretKey` | string | `KAIROS_BOOTSTRAP_PASSWORD_HASH` | Key within that Secret. |
| `config.auth.trustedProxy` | bool or `""` | `""` | Sets `KAIROS_TRUSTED_PROXY`. Empty **follows `ingress.enabled`**: behind an Ingress the socket peer is the controller, so the header is the only real client address; with no Ingress the header is caller-supplied. Set it explicitly for a gateway of your own. |
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
moot in 0.4.0 because the server reads none of them.

## Reference Compose deployment

`deploy/docker-compose.yaml` with `deploy/.env`, copied from
`deploy/.env.example`. The topology is Caddy for TLS and subdomain routing,
one Kairos container, and PostgreSQL 16 as the sole state — as
`pgvector/pgvector:pg16`, because Kairos requires the `pgvector` extension and
stock `postgres:16` does not carry it. There is no bundled identity provider.

| `.env` variable | Default in the example | Consumed by |
|---|---|---|
| `KAIROS_VERSION` | `0.4.0` | The Kairos image tag. Required — Compose fails if it is unset. |
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
