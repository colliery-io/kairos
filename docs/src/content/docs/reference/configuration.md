---
title: "Configuration"
---

Every value that changes how Kairos behaves, in one place. Kairos 0.18.1.

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

The retention variables are not among them. The server reads them at
startup too, but their parsing rules are different. That section gives them.

### Required

| Variable | Type | Description |
|---|---|---|
| `DATABASE_URL` | Postgres connection URL | The external PostgreSQL, which is the deployment's sole state. Example: `postgres://kairos:kairos@localhost:41432/kairos`. Also read directly by every server subcommand, including `migrate`, before the rest of the configuration is resolved. `sslmode` can be `disable`, `prefer` (the default) or `require`. With `prefer` or `require`, every connection uses TLS when the server offers it, and Kairos does not verify the certificate of the server. `verify-ca`, `verify-full` and `sslrootcert` are not accepted. |
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
password form and no SSO button it cannot honour. `kairos login` without `--email`
says that the deployment has no issuer and uses local accounts. For CLI access on such
a deployment, use [`kairos login --email`](../cli/#kairos-login).

### Network and logging

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_BIND_ADDR` | socket address | `127.0.0.1:8080` | Where the HTTP server listens. Rejected at startup if it does not parse as a socket address. Both packaged deployments override it so the container is reachable from outside: the Helm chart to `0.0.0.0:<containerPort>`, the Compose file to `0.0.0.0:8080`. |
| `KAIROS_LOG_LEVEL` | tracing filter directive | `info` | Passed to the tracing filter; accepts per-target directives, e.g. `info,kairos_server=debug`. |
| `KAIROS_LOG_FORMAT` | `json` \| `pretty` | `json` | Log encoding. Any other value fails startup. |
| `KAIROS_PUBLIC_URL` | URL | unset | The deployment's externally reachable base URL. Required to render the webhook delivery URL an operator pastes into a forge. A trailing slash is stripped. Not inferred from the request `Host` header. |

### Database pools

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_POOL_SIZE` | integer | `16` | The connections of each of the two pools of a server: the async pool of the middleware and the blocking pool of the handlers. A server holds up to twice this many connections, plus the job pool, one for `LISTEN` and one for the event emitter. Size it against `max_connections` of PostgreSQL and the number of replicas. |
| `KAIROS_POOL_MIN_IDLE` | integer | `2` | The connections the blocking pool opens at start and keeps idle, so the first requests after a start do not pay the connect. Capped at `KAIROS_POOL_SIZE`. |
| `KAIROS_JOB_POOL_SIZE` | integer | `6` | The connections of the pool of the job worker and the jobs, at least 1. A burst of jobs fills this pool and leaves the handlers their pool. Raise it when you raise the job limits. |

No pool pings a connection on checkout: every checkout sets `search_path` first, and a dead connection fails there. A checkout waits 5 seconds for a free connection, then fails the request.

Each pool sets three timeouts on each new connection: `statement_timeout` 30 seconds, `lock_timeout` 5 seconds and `idle_in_transaction_session_timeout` 60 seconds. A statement that passes its timeout fails with the error of PostgreSQL, and the request reports it. The migration connection gets the `lock_timeout` alone, because a migration can run long. A replica that waits for the migration lock waits with no timeout.

### Request timeout

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_REQUEST_TIMEOUT_SECS` | integer | `30` | The most time, in seconds, that a request under `/api` or `/mcp` gets. After it, the answer is `503` with the code `REQUEST_TIMEOUT`, and the server stops the work of the request. `0` turns the timeout off. |

The timeout ends when the head of the answer is ready. So the timeout does not cut a download that streams its body. These routes are not in the timeout:

- `PUT` and `GET /api/repositories/{slug}/code-indexes/{commit}`: the upload and the download of a code index.
- `PUT /api/repositories/{slug}/site`: the publish of a site.

The paths outside `/api` and `/mcp` are not in the timeout: the WebSocket `/ws/events`, the probes, the GUI and `/pages`. A write that the timeout stops can be complete or not complete. Read the data again before you send the write again.

Each call of the server to the issuer has a timeout of 5 seconds. These calls get the discovery document and the JWKS, and relay the tokens of the GUI. The server gets the JWKS at most once a minute. A token whose key is not in the JWKS gets a `401` until the server can get the JWKS again.

The server accepts HTTP/1.1, and HTTP/2 from a client that sends it with prior knowledge (h2c).

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

- **An organization admin** resets it: `PUT /api/local-accounts/{user_id}/password`.
- **An operator** resets it with no login at all:
  `kairos-server set-password --email <email>`, which reads the password from stdin when
  `--password` is absent. It lives beside `drop-tenant` among the
  [operator subcommands](../cli/#operator-subcommands-kairos-server), because the case it
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

Accounts are created by an organization admin. There is no self-service sign-up and no
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
| `KAIROS_TRUSTED_PROXY` | bool | `false` | Whether `X-Forwarded-For` is trusted for the client address. See below. It also lets the server read `X-Forwarded-Proto` for HSTS (see "Security headers"). |

Two things are counted separately: the **identity** being attempted, and the
**source** it came from. Neither alone is enough. Counting only identities lets an
attacker try one password each against a thousand accounts; counting only sources
is meaningless behind a proxy, where every request shares one address.

#### API keys

The server counts a failed API key two times (KAIROS-T-0539):

| Count | Limit in the window | The 429 |
|---|---|---|
| The key | `KAIROS_AUTH_MAX_FAILURES` (5) | `KEY_LOCKED_OUT` |
| The address of the key | 20 times `KAIROS_AUTH_MAX_FAILURES` (100) | `ADDRESS_LOCKED_OUT` |

- A stale key in a loop locks only itself. The other keys from the same
  address continue to work.
- The server does not count an attempt that a lockout refuses. Thus one
  stale key adds at most 5 failures to its address in each lockout.
- A flood of different keys from one address locks the address. Then the
  server refuses each key from that address, also a good key.
- A good key does not remove the count of its address.
- These counts are apart from the counts of the password login.

See [Errors](../errors/#failed-authentication) for the codes.

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

### The auth cache

The server keeps each accepted API key, session, OIDC user, organization and
role for a short time (KAIROS-T-0609). A request with a known credential then
needs no database read for its authentication.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_AUTH_CACHE_SECS` | integer | `60` | How long the auth cache keeps an entry, in seconds. `0` turns the cache off. |

A write that makes an entry wrong removes it on the server that made the write.
These writes are a key revoke or rotation, a role change, a member removal and
a logout. A revoke of sessions also removes them. The next request on that
server sees the change. Other servers of the same deployment keep the entry
until it expires. To make that delay shorter, set a lower value or `0`.

A cached API key skips the failed-authentication throttle. So an address in a
lockout can still use a key that the server accepted recently. The throttle
stops guesses, and a cached key is not a guess.

### API keys and agent keys

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_API_KEY_TTL_DAYS` | integer | `90` | The default life of a new API key or agent key, in days. A request that names `expires_at` sets its own expiry. A request with `no_expiry: true` (the CLI flag `--no-expiry`, the switch "No expiry" of the Agent keys page) makes a key with no expiry. `0` turns the default off: a new key has no expiry unless the request names one. A key with no expiry works until a person revokes it: the person of the key, or an organization admin (`kairos admin agent-keys revoke`). When a person leaves the organization, Kairos revokes each agent key of the person. |
| `KAIROS_API_KEY_ROTATION_GRACE_SECS` | integer | `86400` (1 day) | After a rotation of a key, the old secret works for this time. `0` ends the old secret at the rotation. |

The keys that exist keep their expiry: the setting applies to new keys. See
[Rotate keys](../../how-to/rotate-keys/).

### Stale claims

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_STALE_CLAIM_DAYS` | integer | `14` | A claim on a task with no write and no state change for this many days is stale. Each hour, a periodic pass tells the holder and the owner of the initiative of the task (the notification `claim_stale`). It tells them once for each quiet period. The claim stays. `0` turns the pass off. The value applies to each tenant of the deployment. |

A server with `KAIROS_ROLE=worker` does not run the pass. See
[Teams and boards](../../explanation/teams-and-boards/#who-works-on-a-task-the-claim).

### Security headers

The server sends these headers on each response:

| Header | Value |
|---|---|
| `X-Content-Type-Options` | `nosniff` |
| `X-Frame-Options` | `DENY` |
| `Referrer-Policy` | `same-origin` |
| `Content-Security-Policy` | On the page of the GUI: `default-src 'self'`, scripts from the server with the hash of each inline script of the page and `'wasm-unsafe-eval'`, inline styles, images from the server, `data:`, `blob:` and HTTPS sites, fonts from the server and `fonts.gstatic.com`, `object-src 'none'`, `frame-ancestors 'none'`. On each other response: `frame-ancestors 'none'`. |
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains`, only when `KAIROS_TRUSTED_PROXY` is on and the proxy sends `X-Forwarded-Proto: https`. |

The server speaks HTTP; a proxy in front of it ends TLS. So the server sends HSTS
only when a trusted proxy says that the request came over HTTPS. The reference
compose stack (Caddy) does. No setting turns the headers off.

#### Where the state lives

In the database, in the table `public.auth_failures` (KAIROS-T-0377). All
replicas share the counts. Thus a lockout holds on each replica, and a
restart keeps it.

- Each attempt reads the counts of its subjects one time, before the server
  checks the credential.
- A failure adds one write. A success adds one write only when the subject
  has a count.
- A row keeps a hash of the email, the key or the address, not the value.
- The retention pass removes the rows whose window and lockout ended.
- If the server cannot read the table, the attempt continues.

A lockout increments
`kairos_auth_lockouts_total{subject="identity"|"source"|"key"|"key_address"}` on
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
| `KAIROS_SINGLE_TENANT_NAME` | string | the slug | The name of the pinned tenant. When that tenant does not exist, the first request from a principal in `KAIROS_DEPLOYMENT_ADMINS` provisions it with this name and makes that principal its first admin. A request from a different principal gets 404 `TENANT_NOT_FOUND`. |
| `KAIROS_AUTO_JOIN_DOMAINS` | comma-separated email domains | empty | A person whose issuer verifies an email at one of these domains (`email_verified: true`) becomes a `member` of the pinned tenant on the first request, with no admin step. The match is exact, so `example.com` does not match `eu.example.com`. A person that an admin or SCIM removed from the organization does not join again, and an admin can add them back. Requires `KAIROS_SINGLE_TENANT`: the server does not start when it is set without that. An entry with an `@` or with no dot also stops the start. |

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

The import interval of a repository (KAIROS-T-0670) needs neither variable.
It reads the open issues with the read token of the repository, so it works
with no webhook. See [Connect a git forge](../../how-to/connect-a-git-forge/#6-take-issues-on-an-interval-with-no-webhook).

### Server role

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_ROLE` | `all` \| `api` \| `worker` | `all` | The work of this server. `all`: the API and each job, the code index builds too. `api`: the API and each job, but no code index build and no new-commits sweep. `worker`: only the code index builds and the new-commits sweep; the server listens on `KAIROS_BIND_ADDR` for `/healthz`, `/readyz`, `/version` and `/metrics` only. Any other value stops the start, and the message names `KAIROS_ROLE`. |

Use `all` for a small install. For a larger install, run servers with `api`
and servers with `worker` on the same database. A build of the code index can
use much memory.

With a worker, a build that runs out of memory stops the worker only. The API
continues to serve. The platform starts the worker
again. The breaker of the repository stops more tries of that build until a
person asks for a build.

The memory limit of the worker is the setting to increase for big
repositories. The packaged defaults are a limit of 8 GiB and 4 CPUs. The
requests are 2 GiB and 1 CPU. These values come from GitHub issue #34. There,
a full build of about 1,200 files used about 6.3 GiB at its peak, and 4
cores. The build used the embedded summary model.

A worker applies the migrations at its start, as an API server does. The
migrations run under a lock in the database. Thus a worker and an API server
can start at the same time.

A deployment with only `api` servers makes no index. Then the list of the runs
of a repository has the field `builder` with a note. The list is
`GET /api/repositories/{slug}/code-indexes/builds`, the code index panel of
the repository page, and `kairos repos builds`. The note shows when no worker
ended a sweep in the last 2 intervals of the sweep and 5 minutes.

A request for a build on an `api` server puts the build in the queue, and a
worker takes it. A request is `POST /api/repositories/{slug}/code-indexes/rebuild`,
the MCP tool `rebuild_code_index` or `kairos repos reindex`. An `api` server
does not need `KAIROS_CODE_INDEX_DIR` for this. It needs the folder to find
the nearest indexed commit (`kairos index update`). For that, give the api
pods the folder and a volume that they can write. Or let the CLI do a full
index.

A worker does not start without `KAIROS_CODE_INDEX_DIR`. The Helm chart gives
the worker pods `worker.codeIndexDir`; the Compose file gives the service
`worker` its volume at `/var/lib/kairos/code-index`.

### The base code index

Kairos keeps the code index of each indexed commit of a repository
(`/api/repositories/{slug}/code-indexes`). The indexes are in Postgres. These
variables control the clones of the repositories and the builder. The builder
makes the first index of each repository that has no index. Then it updates
the index after each push to the default branch.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_CODE_INDEX_DIR` | directory path | unset | The folder for a bare clone of each repository, the tools of the builder and the work folders of the builder. Put it on a volume. When it is not set, the server cannot find the nearest indexed commit, does not build and downloads nothing. Uploads and downloads of indexes still work. A server with `KAIROS_ROLE=worker` does not start without it. A server with `KAIROS_ROLE=api` takes a rebuild request without it: the build goes to a worker. |
| `KAIROS_CODE_INDEX_POLL_SECS` | whole number | `300` | How often the sweep fetches each indexed repository and adds the build jobs. The sweep is a periodic job: one server runs it at each interval. `0` turns the builder off on that server. |
| `KAIROS_CODE_INDEX_THREADS` | whole number, 1 or more | `4` | The number of CPU threads that the summary model of the builder uses. More threads make the summaries faster, but they take CPU from the other services on the host. `0` or a value that is not a number stops the start of the server. |
| `KAIROS_CODE_INDEX_MIN_REBUILD_SECS` | whole number | `0` | The least time in seconds between 2 updates of one repository. `0` turns the limit off. On a slow host, a repository with many pushes then waits, and the other stale repositories get their updates. A value that is not a number stops the start of the server. |
| `KAIROS_CODE_INDEX_DEPS_CAP_MB` | whole number | `10240` | The largest total size in MB of the dependency caches of the SCIP runners of Go, TypeScript and Python. After a run, the builder removes the caches of other repositories, oldest first, until the total is below this value. See [The tools of Go, TypeScript and Python](#the-tools-of-go-typescript-and-python). |
| `KAIROS_CODE_INDEX_MIN_FREE_MB` | whole number | `5120` | The least free disk space in MB before a dependency install. When the free space is less, the builder removes the dependency caches first, oldest first. If the space is still less, the language gets its edges by name, and the run records a warning. |
| `KAIROS_CODE_INDEX_DEPS_ENV` | comma-separated names | unset | More names of environment variables that a dependency install gets. An install gets only `PATH`, its cache folders and the variables in the list below, so no other secret of the server gets to a package install. |
| `KAIROS_CODE_INDEX_TOOL_DOWNLOADS` | `true` or `1` | unset (off) | Lets the builder fetch the Python and the uv that a Python project asks for. By default, a project with `uv.lock` is synced with the Python and the uv of the runner only, and a project that they do not fit gets its edges by name. When on: uv uses the Python of the runner if it fits the `.python-version` and `requires-python` of the project, else it downloads a matching CPython; and when the `required-version` of the project excludes the uv of the runner, `pip` fetches a uv that fits it. Both go into the dependency cache of the repository. These downloads have no pinned checksum. Turn it on only when the server can reach the internet and the Python package index. |

#### The providers of the summaries and the vectors

Each organization chooses where the summaries of its code index are made.
It also chooses where the vectors of those summaries are made. The setting
is for the organization, not for the deployment. An organization admin sets it on the
page Admin, Code index, or with `kairos admin code-index-settings set`. The
REST route is `PUT /api/org/code-index-settings`. Each member can read it.

| Setting | Values | Default | Description |
|---|---|---|---|
| `summary.provider` | `embedded`, `ollama-cloud`, `bedrock` | `embedded` | `embedded` is the model in the server. `ollama-cloud` is an OpenAI-compatible chat endpoint: it also serves a local Ollama, vLLM and OpenRouter. `bedrock` is AWS Bedrock. |
| `summary.base_url` | URL | none | The base URL of the endpoint, for `ollama-cloud`. For example `https://ollama.com/v1`. |
| `summary.model` | text | none | The model name (`ollama-cloud`) or the model id (`bedrock`). |
| `summary.region` | text | none | The AWS region, for `bedrock`. |
| `summary.secret` | text | none | The API key (`ollama-cloud`), or the AWS credentials as `<access key id>:<secret access key>` with an optional `:<session token>` (`bedrock`). |
| `vectors.provider` | `embedded`, `remote` | `embedded` | `remote` is an OpenAI-compatible embeddings endpoint. |
| `vectors.base_url` | URL | none | The base URL of the embeddings endpoint, for `remote`. |
| `vectors.model` | text | none | The model name of the embeddings, for `remote`. |
| `vectors.secret` | text | none | The API key of the embeddings endpoint. A local Ollama needs none. |
| `concurrency` | 1 to 32 | `4` | The most requests that a server sends at a time to the summary endpoint (the URL and the model), for all its builds together. |
| `default_summaries` | `embedded`, `hosted` | `embedded` | The summarizer of each repository that follows the organization. `hosted` needs a hosted `summary.provider`. A write that does not give it keeps the stored value. |

Kairos keeps each secret encrypted with `KAIROS_SECRETS_KEY`, as it keeps
the read tokens of the repositories. No read gives a secret back: a read
says that it is set, by whom and when. A write that does not name a secret
keeps the stored secret. An empty secret removes it. A hosted provider
needs its secret, its URL or region, and its model. Kairos refuses a write
with one of them missing, and the refusal names the field.

`code_search` of the code tools compares the vector of its query with the
vectors of the index. One model must make the two vectors. A CLI with no
local model of the index asks Kairos for the vector: `POST
/api/code-index/query-vector`. Kairos uses its embedded model, or the remote
provider of the organization with its secret.

Each member can ask. The text has 1 to 2,000 characters. When the
organization has no provider for the model, Kairos refuses the request
(`NO_QUERY_PROVIDER`). The search then uses the text of the summaries.

The setting `code_index_summaries` of a repository chooses its summarizer:

- `organization` (the default of a new repository): the `default_summaries`
  of the organization decides.
- `embedded`: the embedded model, whatever the organization sets. The code
  stays on the host.
- `hosted`: the hosted provider of the organization.

A person sets it on the page Admin, Repositories. The command is `kairos
repos update <slug> --code-index-summaries <value>`. Kairos refuses `hosted`
when the organization has no hosted provider. A repository from before
0.9.0 keeps the value that it had.

When the summarizer is the embedded model, the code of the repository stays
on the host.

For a repository on a hosted provider, a server sends `concurrency`
requests at a time to the endpoint. The limit is for all the builds of
the server together, not for each build. A hosted build does not hold the
CPU of the host for its summaries. So it does not wait for the builds of
the embedded model.

When the provider answers 429 or 503, the builder waits and tries the
request again, 6 tries in all:

- When the answer has a `Retry-After` header, the builder waits that time.
  The longest wait is 5 minutes.
- Else the first wait is 2 seconds. Each next wait is 2 times longer, to 2
  minutes at most.

After another 5xx, or when no answer comes, the builder tries the request
again, 3 tries in all. While a request waits, the build reports progress,
so the watchdog does not stop the job as stalled.

A summary that the builder cannot make does not stop the build. The build
stores the index with the structure and the summaries that it made. The
run shows the number of missing summaries. The next build of the
repository makes them. When the build makes no summary and no answer was a
429 or a 503, the build fails. For example, the key is wrong.

Each run records the model that wrote its summaries. See it with `kairos
repos builds <slug>`, or in the panel of the repository. The image
`ghcr.io/colliery-io/kairos:<version>-hosted` has no embedded model and
downloads none. It builds only the repositories on a hosted provider. The
other repositories get a failed run that says so. See [Use a hosted model
for the code index summaries](../../how-to/use-a-hosted-model-for-the-code-index/).

The clone fetches from the `repo_url` of the repository. For a private
repository, set a read token on the repository (see
[Read a private repository](../../how-to/read-a-private-repository/)). With no
token, the clone has no credential, and a fetch that needs one fails at once.

#### The work of the builder

Each build is a job of the [job queue](#job-queue). Thus 2 or more servers
share the builds, and each build runs one time. There are 3 types of build:

- **A requested build.** A person asks for it (`kairos repos reindex`, or
  the button "Build again" of the panel of the repository). It is a full index of the head
  of the default branch.
- **An update.** At each interval of `KAIROS_CODE_INDEX_POLL_SECS`, the
  sweep fetches each repository that has an index. If the head of the
  default branch has no index, the sweep adds an update job. The update
  changes the nearest index to that head. The sweep is a periodic job (see
  [Job queue](#job-queue)): at each interval, one server runs it.
- **A first build.** At the same interval, the sweep adds a job for each
  repository that has no index. The job makes a full index of the head of
  the default branch. The index has the structure, the SCIP edges and the
  summaries. The test code gets summaries too, so the search finds a test
  by what it checks. The test code adds to the number of summaries of a
  build.
  A first build of a large repository can take some hours on a CPU.

A repository has one build job at a time. A server does not add a second
job for a repository that has a job. The note of the pass names that job.

#### The branch that the builder indexes

The builder indexes the default branch of the repository. At each fetch, the
builder reads the default branch of the forge. This is the branch that `HEAD`
of the remote names (`git ls-remote --symref <remote> HEAD`). The builder uses
the token of the fetch for this. That is the read token of the organization
for the host, or else the token of the repository. The builder selects the branch
in this sequence:

1. The branch that a person set by hand. It wins over the forge.
2. The default branch of the forge, from the fetch.
3. The stored default branch, when the forge gives no default branch.

Thus, when the forge gets a new default branch, the next fetch finds it.

The repository shows the branch that the builder indexes and where it comes
from:

- REST: `default_branch`, and `default_branch_source` (`forge` or `hand`).
  `forge_branch` is the default branch of the forge at the last fetch.
- `get_repository` and `kairos repos get`: the line `default branch`.
- The panel of the repository: the line `Indexed branch:`.

To set the branch by hand, send a `default_branch`: `PATCH
/api/repositories/{slug}`, `update_repository`, `kairos repos update
--default-branch`, or "Default branch from: hand" in Admin, Repositories. To
follow the forge again, send `default_branch_source: forge`: `kairos repos
update --follow-forge`, or "Default branch from: forge". A repository that
you add with no `default_branch` follows the forge. A repository that you add
with a `default_branch` has it by hand.

A repository from before this rule follows the forge after the upgrade, also
when a person set its branch. To keep a different branch, set it again.

The jobs run in this order: a requested build, then an update, then a first
build. Inside one type, the repository that has been stale longest goes
first. The order of the list and the time of the pushes do not change it. A
repository that got an update goes to the end of the next pass. A repository
with many pushes does not get 2 updates in a row while another repository is
stale.

With `KAIROS_CODE_INDEX_MIN_REBUILD_SECS`, a repository whose newest index
is younger than that time gets no update job in the pass. A later pass adds
the job.

A build with the embedded model is a job of the type `embedded_build`. One
server runs one of them at a time (`KAIROS_JOB_LIMIT_EMBEDDED_BUILD`). Thus
the builds use the `KAIROS_CODE_INDEX_THREADS` threads one time, not 2
times. A build with a hosted provider is a job of the type `hosted_build`.
One server runs 4 of them at a time (`KAIROS_JOB_LIMIT_HOSTED_BUILD`).

A first build gives its place to an update or a requested build of the same
type. The first build stops after its next summary, and goes back to the
queue. It keeps its summaries and its run. Thus, during a long first build,
the updates continue.

A build tells the job queue about its progress at each summary and at each
batch of vectors. When a build makes no progress for the lease time, its
server stops it. Then a server starts it again. When a server stops during a
build, another server takes the job when the lease runs out.

Each summary goes into the summary pool of the repository in the database
when the model writes it. The next build of the repository, on each server,
uses the summaries of the pool first. Thus a stop or a restart during a
build of some hours does not lose the work of the model.

The servers do not have to share `KAIROS_CODE_INDEX_DIR`. For example, on
Kubernetes each pod has its own disk. A build that another server takes
makes the structure again (some minutes). Then it makes only the summaries
that are not in the pool.

A first build also keeps its work in `KAIROS_CODE_INDEX_DIR/first-builds/`,
and an update in `KAIROS_CODE_INDEX_DIR/updates/`. These files are a local
cache. After the builder stores the index, it removes the file.

An index keeps only the summaries that its code needs. A summary that no
index uses stays in the pool for `KAIROS_CODE_INDEX_POOL_DAYS` (see
[Retention](#retention)). The retention pass keeps the newest
`KAIROS_CODE_INDEX_KEEP` indexes of each repository and ref. The run shows the number of summaries that it made
and the number that it reused.

If a first build of a commit fails, the server does not try that commit
again. It tries again when the head moves or when the server starts again.

Each run records the server that runs it. When a server takes the job of a
run again, the run continues on that server. When a server starts, its
builder ends each run that no open job names. Such a run gets the text "The
server stopped during the run". When a job fails after its last try, its run
fails with the reason.

The log has these lines for each build:

- `code index full build started`, with the repository, the commit and the
  type.
- `code index built`, with the time in seconds and the number of summaries.
- `code index not built`, with the error.
- `code index build not done`, with the reason. For example, the head has an
  index, or the repository has `code_index_build` off. The builder logs a
  reason one time, and again only when the reason changes.

You can also make the first index on a computer and upload it
(`kairos index build`, then the upload). Then the builder does not make a
first build of the repository.

#### Stop the builder for one repository

Each repository has the setting `code_index_build`:

- `on` (the default): the builder makes the first index and the updates.
- `off`: the builder does nothing for the repository. Use it for a template
  or a static site. An upload of an index still works.

Kairos refuses a value that is not `on` or `off`, and the error names the
value. To change the setting, use one of these:

- `kairos repos update <slug> --code-index-build off`
- `PATCH /api/repositories/{slug}` with `{"code_index_build": "off"}`
- the **Edit** form of the repository on the admin **Repositories** page

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_SECRETS_KEY` | 32 bytes in base64 | unset | The key that encrypts the read token of each repository (AES-256-GCM). Make it with `openssl rand -base64 32`. When it is not set, the server refuses a token with 501 `SECRETS_NOT_CONFIGURED`, and the builder reads public repositories only. A value that is not 32 bytes of base64 stops the start of the server. Keep the key outside the database, with the database password. |

| `KAIROS_SECRETS_KEY_OLD` | base64 keys, separated by commas | unset | The old keys, during a rotation of `KAIROS_SECRETS_KEY`. They decrypt the rows that they encrypted; they never encrypt. Set it only with `KAIROS_SECRETS_KEY`. |

To change `KAIROS_SECRETS_KEY`, put the new key in `KAIROS_SECRETS_KEY` and the
old key in `KAIROS_SECRETS_KEY_OLD`, then run `kairos-server rotate-secrets`. It
encrypts each stored secret again with the new key: the read token of each
repository, and the secrets of the hosted models of the code index. See
[Rotate keys](../../how-to/rotate-keys/). A stored secret that no key of the two
settings decrypts fails with the error "Set the token again". Set that secret
again.

#### The tools of the builder

The image does not contain the tools of the builder. The builder needs these
conditions:

- `KAIROS_CODE_INDEX_DIR` is set.
- `KAIROS_CODE_INDEX_POLL_SECS` is not `0`.
- Embeddings are on.
- The server has the `llama` feature.

Then, at each start, the server checks the tools in
`KAIROS_CODE_INDEX_DIR/tools/`:

- It downloads each file that is not there (about 2.7 GB on the first
  start). The download runs in the background. The server serves during the
  download.
- It checks each file against its pinned sha256. It does not download a file
  again when the file has the correct checksum.
- After a check, it records the size and the modification time of each file
  in `tools/.kairos-index-checked`. At the next start, it reads a file again
  only if its size, its modification time or its pin changed. Then it reads
  the full file and checks it again. Thus a start
  after the first start does not read the 2.7 GB of files. To check all the
  files again, remove `tools/.kairos-index-checked`.
- If a download or a check fails, the builder stays off and the server logs
  one line, `the code index builder did not start`, with the file or the URL
  and the expected checksum. The server runs as usual. The next start tries
  again. The server does not replace a file with an incorrect checksum.
  Remove the file to download it again.

| File in `tools/` | Download from | sha256 |
|---|---|---|
| `Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | `https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF/resolve/ae44f08e1392f39c0e474af10c3ff8355c8b6688/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | `2fde00ce69dd4899c70d020845e2638353015bba0fdf161b3eb965f2bca4464e` |
| `rust-analyzer-<target>.gz` | `https://github.com/rust-lang/rust-analyzer/releases/download/2026-09-28/rust-analyzer-<target>.gz` | in `crates/kairos-index/src/rust_analyzer.rs` |
| `rust-src-1.99.0.tar.gz` | `https://static.rust-lang.org/dist/2026-10-01/rust-src-1.99.0.tar.gz` | `82b978093b33c71bcbe69005b92181bfb76d3d0e434ef303e637812e21badcd7` |
| `rustc-1.93.0-<target>.tar.gz`, `cargo-1.93.0-<target>.tar.gz`, `rust-std-1.93.0-<target>.tar.gz` | `https://static.rust-lang.org/dist/<file>` | in `crates/kairos-index/src/tools.rs` |

`<target>` is `aarch64-unknown-linux-gnu` or `x86_64-unknown-linux-gnu` for
the image. The server unpacks rust-analyzer to `tools/rust-analyzer` and the
3 Rust archives to `tools/rust-1.93.0/`.

**With no network:** download the files on a computer with network access.
Keep their names. Put them in `KAIROS_CODE_INDEX_DIR/tools/` before the
start. The user of the server (uid 10001 in the image) must be able to write
the folder. Then the server downloads nothing.

#### The tools of Go, TypeScript and Python

The builder runs a SCIP indexer for Go, TypeScript (with TSX and
JavaScript) and Python. Then the edges and the uses of these languages are
precise, as for Rust. The builder downloads the tools of a language into
`KAIROS_CODE_INDEX_DIR/tools/` when a repository first needs that language.
A repository with no file of a language never causes the download of its
tools.

| Language | Tools | Licences |
|---|---|---|
| Go | Go 1.27.2 (`https://go.dev/dl/`), scip-go 0.2.7 (`https://github.com/sourcegraph/scip-go/releases`) | BSD-3-Clause, Apache-2.0 |
| TypeScript, TSX, JavaScript | Node.js 24.22.0 (`https://nodejs.org/dist/`), `@sourcegraph/scip-typescript` 0.4.0 (npm) | MIT, Apache-2.0 |
| Python | Node.js 24.22.0, `@sourcegraph/scip-python` 0.6.6 (npm), CPython 3.13.16 of python-build-standalone 20261013 (`https://github.com/astral-sh/python-build-standalone/releases`) | MIT, MIT, PSF-2.0 (build scripts MPL-2.0) |
| Python with `uv.lock` | uv 0.13.1 (`https://github.com/astral-sh/uv/releases`), only for a project with `uv.lock` | MIT or Apache-2.0 |

- Each archive has a pinned sha256 for each platform (amd64 and arm64,
  Linux and macOS). The pins are in `crates/kairos-index/src/languages.rs`.
  scip-go has no build for macOS amd64.
- npm installs scip-typescript and scip-python with `npm ci` from the lock
  files in `crates/kairos-index/npm/`. npm checks the integrity of each
  package.
- The builder keeps the archives in `tools/` and unpacks each tool into
  `tools/<tool>-<version>/`.
- Sometimes a download fails, a file has an incorrect sha256, or the
  platform has no pin. Then the language gets its edges by name. The run
  records a warning with the reason. The build continues. The server does not
  replace a file with an incorrect checksum. Remove the file to download
  it again.
- **With no network:** put the archives in `tools/` with the names of their
  URLs. The npm packages need access to the npm registry, or a registry
  that `NPM_CONFIG_REGISTRY` gives.

**The dependency cache.** The indexers give precise results only with the
dependencies of the repository. Before a run, the builder installs them:

- Go: `go mod download`.
- TypeScript: `npm ci` or `npm install`, with `--ignore-scripts`.
- Python: a virtual environment with `pip install`.

The builder keeps them in `KAIROS_CODE_INDEX_DIR/deps/<tenant>/<repository id>/<language>/`:
the Go module cache, the npm cache and the virtual environment.

- A Python project with `uv.lock` and `pyproject.toml` at the root installs
  with `uv sync --frozen`. The builder downloads uv on first need. uv
  installs the locked versions with the CPython of the builder. It
  downloads no Python.
- A Python project with no `uv.lock` installs with `pip install`: `-r`
  for each `requirements*.txt` file, else `-e .`.

- `KAIROS_CODE_INDEX_DEPS_CAP_MB` is the largest total size of the caches.
- When the free disk space is less than `KAIROS_CODE_INDEX_MIN_FREE_MB`,
  the builder removes the caches before an install, oldest first.
- If the install or the indexer fails, the language gets its edges by
  name. The run records a warning.
- A repository with no dependency file gets no install. The indexer reads
  the sources only. A dependency file is `go.mod`, `package.json` or a
  Python requirements file.

**Private dependencies.** The builder keeps no new secrets. An install gets
`PATH` and its cache folders. It also gets these variables, if the server
has them:

- Network: `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` (and lower case),
  `SSL_CERT_FILE`, `SSL_CERT_DIR`.
- Locale: `LANG`, `LC_ALL`, `TZ`.
- Git: `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_*`, `GIT_CONFIG_VALUE_*`,
  `GIT_ASKPASS`, `NETRC`.
- Go: `GOPRIVATE`, `GONOPROXY`, `GONOSUMDB`, `GONOSUMCHECK`, `GOPROXY`,
  `GOSUMDB`, `GOINSECURE`, `GOAUTH`.
- npm: `NODE_AUTH_TOKEN`, `NPM_TOKEN`, `NPM_CONFIG_*`, `npm_config_*`.
- Python: `PIP_INDEX_URL`, `PIP_EXTRA_INDEX_URL`, `PIP_TRUSTED_HOST`,
  `PIP_CERT`, `UV_INDEX_*`, `UV_DEFAULT_INDEX`, `UV_EXTRA_INDEX_URL`.
- GitHub: `GITHUB_TOKEN`, `GH_TOKEN`.

Add more names with `KAIROS_CODE_INDEX_DEPS_ENV`.

**A local run.** `kairos index` on a computer downloads no tools and
installs no dependencies. It uses the dependencies of the checkout. It
uses these tools on `PATH`: `go` and `scip-go`, `node` and
`scip-typescript`, `scip-python` and `python3`. If a tool is not there, the language gets its
edges by name, and the CLI shows a warning.

### Development

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_DEV_UI` | `true` \| `1` \| `false` \| `0` | `false` | Mounts the Swagger UI at `/api/docs`. At the default the route does not exist. Any other value fails startup. |
| `KAIROS_WEB_DIST` | directory path | unset | Serve the GUI from this directory instead of the assets embedded in the binary. When both are present, the directory wins. |

### Retention

The retention pass runs each 24 hours. The interval is fixed. The pass is a
periodic job (see [Job queue](#job-queue)): one server runs each pass. The
first start of a deployment runs the pass at once. One pass:

- In each tenant, removes the `item_history` rows, the `team_page_history`
  rows and the `activity_log` rows that the variables below make prune
  candidates. The mode sets what happens to them first. `team_page_history`
  has the rules of `item_history`: a team page is an item.
- Deletes the password sessions (`local_sessions`) that expired before the
  pass. This occurs in each mode, also `off`, because an expired session
  cannot log in.
- In each tenant, deletes the read notifications that were read more than
  `KAIROS_NOTIFICATIONS_READ_DAYS` days before the pass. This occurs in each
  mode, also `off`, and the pass does not archive them.
- In each tenant, deletes the unread notifications that are older than
  `KAIROS_NOTIFICATIONS_UNREAD_DAYS` days. This occurs in each mode, also
  `off`, and the pass does not archive them.
- In each tenant, deletes the code indexes that are not among the newest
  `KAIROS_CODE_INDEX_KEEP` of their repository and ref. "Newest" is the time
  of the last write of the index. The indexes with no ref are one group of
  the repository. This occurs in each mode, also `off`. A build or an upload
  can make the index again.
- In each tenant, deletes the summaries of the code-index pools that no
  stored index uses, after `KAIROS_CODE_INDEX_POOL_DAYS` days. The pass also
  deletes a summary whose last use is older than each stored index of its
  repository. This occurs in each mode, also `off`. A build can make the
  summary again.
- In each tenant, deletes the webhook deliveries that finished more than 30
  days before the pass, for each target. A finished delivery is
  `delivered`, `failed` or `skipped`. A `pending` delivery stays. The tries
  of a delivery go with it. This occurs in each mode, also `off`. The
  interval is fixed.
- Writes one `retention_sweep` row to the `activity_log` of each tenant that
  it sweeps, with the counts of the rows it archived and removed.
- Logs the counts for each tenant and the number of deleted sessions.
- Writes its record in the field `result` of the payload of its job.
  `GET /api/org/retention` shows the record of the last pass.

The pass reads `item_history` one page of items at a time. It reads only the
version and the time of each row. It reads the content only of the rows that
it archives. It reads `team_page_history` one page of team pages at a time,
in the same way.

It reads `activity_log` one page of rows at a time. It reads
the code indexes one page of repositories at a time, with no structure. Each
delete removes one batch of rows at most.

The pass keeps each `activity_log` row that moves an item: the actions
`transition`, `cancel` and `board_move`. It keeps them at each age, also past
`KAIROS_ACTIVITY_RETENTION_DAYS`. The flow metrics of a board read these rows.
Without them, a task in progress for more than the window has no start and no
age.

In the mode `archive` with no `KAIROS_ARCHIVE_TARGET`, the pass reads no row
of `item_history`, `team_page_history` and `activity_log`, and removes none. The `retention_sweep`
row and the record of the job then say `skipped: no target`. `/readyz` adds a
line that starts with `warning:`, and the status stays 200. The Admin overview
of the GUI shows an alert.

#### The S3 archive target

With `KAIROS_ARCHIVE_TARGET=s3://bucket/prefix`, the pass writes the rows of
each table to a file in the temporary folder of the server. It fsyncs the file
and uploads it to the bucket in parts. Then it removes the file. It removes the
rows from the database after the upload succeeds.

If the upload fails, the pass fails and the rows stay. The next pass tries
again. The temporary folder needs room for the largest archive file of one
table. The Helm chart gives the `tmp` volume to it.

The credentials come from the default AWS chain. These sources are used:

- The variables `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.
- A web identity token (IAM roles for service accounts).
- The container credentials of EKS Pod Identity and ECS.
- The role of the instance.

The server needs `s3:PutObject` on the prefix. The pass never reads the archive
back.

When there are 2 or more replicas, only one replica runs a pass. A failed
pass is logged and tried again. The next pass runs on time. A value that the
server cannot parse stops the start.

Their parsing differs from the sixteen variables above: empty is not treated as
unset. An empty value of one of these variables is a parse error, not a
fallback to the default:

- `KAIROS_HISTORY_HOT_DAYS`
- `KAIROS_HISTORY_KEEP_LATEST`
- `KAIROS_ACTIVITY_RETENTION_DAYS`
- `KAIROS_NOTIFICATIONS_READ_DAYS`
- `KAIROS_NOTIFICATIONS_UNREAD_DAYS`
- `KAIROS_CODE_INDEX_POOL_DAYS`
- `KAIROS_CODE_INDEX_KEEP`
- `KAIROS_RETENTION_MODE`

`KAIROS_ARCHIVE_TARGET` is the one exception: empty means no target.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_HISTORY_HOT_DAYS` | unsigned integer (days) | `90` | Hot window. `item_history` snapshots edited within this window are never touched. |
| `KAIROS_HISTORY_KEEP_LATEST` | unsigned integer | `5` | Latest-N guard. The newest N versions of an item are never prune candidates regardless of age. |
| `KAIROS_ACTIVITY_RETENTION_DAYS` | unsigned integer (days) | `365` | `activity_log` window. Rows older than this are archive-then-delete candidates, with no compaction tiers. The rows that move an item (`transition`, `cancel`, `board_move`) stay at each age. |
| `KAIROS_ARCHIVE_TARGET` | filesystem path or `s3://bucket/prefix` URL | unset | Where prune candidates are offloaded as NDJSON. A `file://` prefix is stripped. An `s3://` value makes the pass upload each archive file to that bucket, as `{prefix}/{tenant}/{table}/{timestamp}.ndjson`, before it removes the rows. An `s3://` URL with no bucket stops the start of the server. Empty means no target. See [The S3 archive target](#the-s3-archive-target). |
| `KAIROS_ARCHIVE_S3_ENDPOINT` | URL | unset | The endpoint of an S3-compatible store, for an `s3://` target. Unset: AWS. An `http://` value is allowed, and the store gets path-style requests. |
| `KAIROS_ARCHIVE_S3_REGION` | region name | unset | The region of the bucket, for an `s3://` target. Unset: the AWS environment decides. |
| `KAIROS_NOTIFICATIONS_READ_DAYS` | unsigned integer (days) | `30` | The days that a read notification stays. `0` keeps read notifications. |
| `KAIROS_NOTIFICATIONS_UNREAD_DAYS` | unsigned integer (days) | `180` | The days that an unread notification stays after the server made it. `0` keeps unread notifications. |
| `KAIROS_CODE_INDEX_KEEP` | unsigned integer | `5` | The code indexes that stay for each repository and ref: the newest by the time of the last write. `0` keeps each index. |
| `KAIROS_CODE_INDEX_POOL_DAYS` | unsigned integer (days) | `30` | The days that a summary of a code-index pool stays when no stored index uses it. `0` keeps such summaries. A summary that a stored index uses stays. |
| `KAIROS_RETENTION_MODE` | `archive` \| `discard` \| `off` | `archive` | `archive` offloads before pruning and, with no target configured, reads and prunes nothing and records `skipped: no target`. `discard` prunes without archiving. `off` disables the sweeper entirely. Case-insensitive; any other value fails to parse. |

Past the hot window, compaction keeps the first and last snapshot per item per
UTC calendar month and treats the intermediate versions as prune candidates.
See [Archiving](../../explanation/archiving/).

A row of `item_history` can be a diff from an older full row of the same item.
At most 10 versions separate a row from its full row. The pass keeps the full
row of each row that it keeps, so each kept version keeps its text. Each row in
the archive has the full text in `content`.

### Job queue

Each server runs a worker for the job queue (KAIROS-T-0373). The jobs are in
the table `public.jobs`. All replicas share the table. A worker takes the next
job with the highest priority. In one priority, the oldest job goes first. Two
workers never take the same job.

A job has a lease. The job tells the worker when it makes progress. At each
heartbeat, the worker writes the progress and extends the lease. When a job
makes no progress for the lease time, its worker stops it. Then the job goes
back to the queue.

When a server stops, the lease of its jobs runs out. Then another worker takes
those jobs. After the last try, the job fails, and the
row keeps the reason.

The embedding refresh uses the queue too. At each `KAIROS_EMBED_REFRESH_SECS`,
each server adds the next embedding batch of each organization. An
organization has one open batch at a time, and the next batch starts one
interval after the last batch ended. Thus 2 servers do not embed an item two
times. When a batch fails after its tries, its items stay stale, and the next
batch starts at the same item.

A periodic pass (retention, and the sweep of the code index) is a job that
adds its next run when it ends. A pass has one open run at a time. The next
run is due one interval after the last run was due. When a server starts, it
adds a run of each pass that has no open run. When a server stops during a
pass, another server runs the pass again after the lease. A server that
cannot run a pass (for example, its code-index tools are not ready) gives
the pass back to the queue, and no try is counted.

A worker removes a job 7 days after it ends. A value that is not a number, or
that is out of range, stops the start of the server. For the table, the order,
the limits and what to watch, see [The job queue](../../explanation/job-queue/).

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_JOB_HEARTBEAT_SECS` | whole number, 1 or more | `30` | How often the worker writes the progress of a job. |
| `KAIROS_JOB_LEASE_SECS` | whole number, 2 heartbeats or more | `300` | The time that a job can run with no progress. The worker stops the job one heartbeat before this time. |
| `KAIROS_JOB_MAX_TRIES` | whole number, 1 or more | `3` | The number of tries of a job. After the last try, the job fails. |
| `KAIROS_JOB_LIMIT_EMBEDDED_BUILD` | whole number | `1` | The code-index builds with the embedded model that one server runs at the same time. `0`: the server does not run them. |
| `KAIROS_JOB_LIMIT_HOSTED_BUILD` | whole number | `4` | The code-index builds with a hosted provider that one server runs at the same time. |
| `KAIROS_JOB_LIMIT_EMBEDDING_BATCH` | whole number | `2` | The embedding batches that one server runs at the same time. |
| `KAIROS_JOB_LIMIT_PERIODIC` | whole number | `1` | The periodic passes that one server runs at the same time. |
| `KAIROS_JOB_LIMIT_WEBHOOK_DELIVERY` | whole number | `4` | The webhook deliveries that one server sends at the same time. |
| `KAIROS_JOB_LIMIT_GITHUB_WRITEBACK` | whole number | `2` | The GitHub write-backs that one server runs at the same time. |
| `KAIROS_JOB_LIMIT_ISSUE_IMPORT` | whole number | `2` | The imports of open issues that one server runs at the same time. |
| `KAIROS_JOB_LIMIT_ISSUE_WRITEBACK` | whole number | `2` | The issue write-backs that one server runs at the same time. |

### Webhook deliveries

Kairos sends signed webhook deliveries to the targets of a board
(KAIROS-T-0384). Each delivery is a job of the job queue, with a priority
between the embedding batches and the periodic passes. Thus 2 servers send a
delivery one time. For the request and its signature, see
[Webhook deliveries](../webhooks/).

A failed try is tried again. The first wait is `KAIROS_WEBHOOK_RETRY_BASE_SECS`.
Each wait is 2 times the wait before it, to a maximum of 12 hours. With the
defaults, the 12 tries of a delivery last about 17 hours.

Each failed try adds one to the failures in a row of the target. A good try
sets the count to 0. At `KAIROS_WEBHOOK_DISABLE_AFTER`, Kairos turns off the
target. Then the owners of the target get a notification of the kind
`delivery_failing`.

Before each try, Kairos resolves the host of the URL. It refuses a loopback,
private, link-local, shared (`100.64.0.0/10`), multicast or unspecified
address. The request goes to the address that Kairos checked.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_WEBHOOK_MAX_TRIES` | whole number, 1 or more | `12` | The number of tries of a delivery. After the last try, the delivery is `failed`. |
| `KAIROS_WEBHOOK_RETRY_BASE_SECS` | whole number | `30` | The wait after the first failed try. |
| `KAIROS_WEBHOOK_TIMEOUT_SECS` | whole number, 1 or more | `10` | The time that Kairos waits for the answer of a target. |
| `KAIROS_WEBHOOK_DISABLE_AFTER` | whole number | `24` | The failed tries in a row that turn off a target. `0`: Kairos does not turn off a target. |
| `KAIROS_WEBHOOK_ALLOW_PRIVATE` | `true` or `false` | `false` | Send deliveries to loopback, private, link-local and shared addresses too. Use it only for a receiver on your own network. |

The secret of each target is sealed with `KAIROS_SECRETS_KEY`. Without that
key, a board manager cannot make a target. `kairos-server rotate-secrets`
seals the secrets of the targets again.

### GitHub write-back

A GitHub repository can have a write token (KAIROS-T-0386). Then Kairos writes
a comment and the status `kairos` on each linked pull request. Each write is a job of the job queue. A failed write is
tried again. See [Turn on forge write-back](../../how-to/turn-on-github-write-back/).

A GitHub or GitLab repository with a write token also writes to its issues
(KAIROS-T-0575). Kairos comments on an issue that became a task, and closes
the issue when the task ends. The comment links to the item only when
`KAIROS_PUBLIC_URL` has a public host. A private address or a tailnet name
gives no link.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_GITHUB_API_URL` | URL | `https://api.github.com` | The REST API of GitHub, for the write-back and for the import of open issues. For GitHub Enterprise Server, use `https://<host>/api/v3`. The import of a GitLab repository uses the host of the URL of the repository. |

Kairos seals the write token with `KAIROS_SECRETS_KEY`, as it seals the read
token.
`kairos-server rotate-secrets` seals the write tokens again.

### The CLI downloads

`GET /version` (open) gives the version of the server. For each platform, it
also gives the tarball of the CLI of that version and its `.sha256`. `kairos self-update`
installs from these links.

| Variable | Type | Default | Description |
|---|---|---|---|
| `KAIROS_CLI_DOWNLOAD_URL` | URL | `https://github.com/colliery-io/kairos/releases` | Where the tarballs of the CLI are. A mirror has the layout of the GitHub releases: `<url>/download/v<version>/kairos-<version>-<target>.tar.gz`, with the `.sha256` beside each. |

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

The CLI reads no server variables. Its configuration surface is where it keeps
cached credentials, the daily version check, and the settings of
`kairos index update` below. `KAIROS_URL` and the keys are in the
[CLI reference](../cli/#authentication).

| Variable | Description |
|---|---|
| `KAIROS_CONFIG_DIR` | The config directory, taken verbatim. Highest precedence. |
| `XDG_CONFIG_HOME` | Used as `$XDG_CONFIG_HOME/kairos` when `KAIROS_CONFIG_DIR` is unset or empty. |
| `HOME` | Used as `$HOME/.config/kairos` when neither of the above is set. |
| `KAIROS_INDEX_DIR` | The folder of the code indexes, taken verbatim. See [The place of the code index](#the-place-of-the-code-index). |
| `XDG_CACHE_HOME` | Used as `$XDG_CACHE_HOME/kairos/index` for the code indexes when `KAIROS_INDEX_DIR` is unset or empty. Else the CLI uses `$HOME/.cache/kairos/index`. |
| `KAIROS_UPDATE_CHECK` | `0` turns off the daily version check of the CLI. |
| `KAIROS_AUTO_UPDATE` | `1` makes the daily check install the newer version of the server. It does not install from an `http://` deployment. |
| `KAIROS_RELEASES_URL` | The base URL of the published releases, for a mirror. `kairos self-update` takes each checksum from it, and the tarballs of `--version` and `--latest`. The default is the GitHub releases. |

With none of the three set, any command that reaches the API fails with exit 1
and a message naming all three.

The directory is created mode `0700`, and `credentials.json` inside it is
written mode `0600` — both at creation and on every rewrite, including the
token-refresh path. Writes are atomic: a temporary file is written and then
renamed. The name of the temporary file has the process id:
`credentials.json.<PID>.tmp`.

A token refresh holds an advisory lock of the operating system on
`credentials.json.lock`. `login` and `logout` hold it while they change the
cache. A second process that must refresh waits for the lock. Then it reads
the cache again and uses the new token. Thus two processes do not send the
same refresh token, which an issuer that rotates refresh tokens accepts once.

### `config.toml`

`config.toml` in the config directory binds a repository to a deployment and
to the boards of a team. The repository then needs no Kairos file. You edit the
file. `kairos config project` adds an entry and keeps the rest of the file.

```toml
[default]
deployment = "https://kairos.example.com"
tenant     = "example"
team_board = "team-delivery"

[[project]]
remote     = "github.com/example-org/example-repo"
team_board = "other-delivery"
# deployment and tenant come from [default]
```

| Key | Description |
|---|---|
| `remote` | `[[project]]` only. The `origin` remote of the repository. Each form that git takes is correct. The CLI compares it as `host/path` in lower case, so each clone and each worktree finds the entry. |
| `path` | `[[project]]` only, in place of `remote`. A folder with no remote: a checkout with no remote, or a board folder that holds clones. The entry applies to the folder and to each folder below it. `~/` is the home folder. `kairos config project` writes it in a folder with no `origin` remote. |
| `deployment` | The deployment URL. |
| `tenant` | The tenant of a login. A key carries its tenant, so the CLI does not send this value with a key. |
| `repository` | The slug of the repository in Kairos. |
| `team_board`, `initiative_board`, `adr_board` | The slugs of the boards. |
| `delivery_stream` | The delivery stream of the team. |

The CLI, the plugin hooks and the plugin MCP server (`kairos mcp-proxy`) use
one order for each value:

1. A flag, for example `--url` or `--tenant`.
2. The environment: `KAIROS_URL`.
3. The `[[project]]` of the checkout.
4. `.claude/kairos.local.md` of the checkout, or of its clone in a worktree.
   This release reads this file and gives a warning. The next release does
   not read it. `kairos config migrate` moves its values
   to a `[[project]]`.
5. `[default]`.
6. The deployment and the tenant of `kairos login`.

A subfolder of a checkout has the binding of the top of the checkout: its
remote and its `.claude/kairos.local.md`.

`[default]` alone does not wire a repository. The hooks of the plugin give
the Kairos context only when a `[[project]]` (or the retired
`.claude/kairos.local.md`) binds the folder. To wire a board folder, use a
`[[project]]` with `path`, not `[default]`. `kairos config show` gives each
value of a folder and where it comes from.

The CLI refuses a `config.toml` that does not parse. It also refuses an
unknown key, and a `[[project]]` with no `remote` and no `path`. The message
gives the file and the line. These commands read the binding, so they refuse
the file:

- each command that talks to the API (exit 1);
- `kairos config show`;
- `kairos adrs check` with no `--snapshot`;
- the index commands that ask Kairos for a base index;
- `kairos mcp-proxy`: it serves no tools, and gives the reason.

The CLI never ignores the file. It does not use `[default]` or a login in its
place. A typo must not send work to the wrong deployment.

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
| `deployments.*.kind` | `oauth` \| `local_session` | How the entry was made. Omitted for `oauth`. `local_session` is an entry from `kairos login --email`. |
| `deployments.*.email` | string, optional | The email of a local session. Omitted for `oauth`. |

An entry for a local session holds the session bearer in `access_token`.
`expires_at` holds the expiry that the server gave. The entry has no
`refresh_token`. `issuer` and `client_id` are empty strings.

```json
{
  "access_token": "kairos_ss_…",
  "expires_at": 1793491200,
  "issuer": "",
  "client_id": "",
  "tenant": "acme",
  "api_bearer": "access_token",
  "kind": "local_session",
  "email": "you@example.com"
}
```

A token is refreshed when its `expires_at` is within 30 seconds of now, so a
token never expires mid-request. The CLI does not refresh a local session. The
CLI uses it until its `expires_at`. After that, each command fails with exit 2
and gives the `kairos login` command.

A file that does not parse is an authentication error (exit 2). The message
names the file and the fault. The JSON is not valid, it stops early, or it does
not have the fields of a credential cache.

### The place of the code index

The local code index of a checkout is in the user cache, not in the checkout
(KAIROS-A-0072). The file is `<home>/<key>/index.db`. Its lock file
`index.db.lock` is next to it.

- `<home>` is `KAIROS_INDEX_DIR`. When it is unset or empty, `<home>` is
  `$XDG_CACHE_HOME/kairos/index`, else `$HOME/.cache/kairos/index`. macOS
  uses the same folders.
- `<key>` is the folder name of the checkout, `-`, and the first 16 hex
  digits of the SHA-256 of the canonical path of the checkout. For example:
  `app-4a68e4c25d4d38c3`. The checkout is its top folder: a subfolder, and
  a `--root` in a subfolder, have the key of the top.

Each worktree has its own key, so it has its own index. When you move or
rename a checkout, its key changes. Then the CLI builds the index again, or
it downloads the base index from Kairos.

An index of an older CLI is in the checkout, at `.kairos/index.db`. On the
first open, the CLI moves it to the user cache with a rename. When the
rename fails, the CLI leaves the old file, and it builds the index again. The
CLI does not change the line of `.kairos/index.db` in `.git/info/exclude`.
You can remove that line. The CLI writes no line there now.

The plugin hooks find the index with the same rule.

### `kairos index update` and the base index

`kairos index update` asks Kairos for the nearest base index of the checkout.
It downloads that index if the checkout has no index. It also downloads it if
the base is nearer to the tree than the local index. Then it indexes the
changed files.

When the local index is as near to the tree as the base, the CLI keeps the
local index. The base can have more summaries than the local index links.
Then the CLI downloads the base and takes its summaries into the local
index. The output says so. The tool `fetch_index` of the code tools does the same.

| Setting | Description |
|---|---|
| `--url`, then `KAIROS_URL`, then the binding of the checkout (the `[[project]]` of [`config.toml`](#configtoml), the retired `.claude/kairos.local.md`, `[default]`) | The Kairos deployment. With none of them, the CLI uses the deployment of `kairos login`. |
| `KAIROS_KEY`, then `KAIROS_MCP_KEY` | An agent key or a service-account key, sent as the bearer, as for each other command ([CLI → Authentication](../cli/#authentication)). With neither, the CLI uses the credentials of `kairos login`. |
| `--tenant`, then `tenant` of the binding of the checkout (`kairos config show`) | The tenant, for the credentials of `kairos login`. With neither, the CLI uses the tenant of the login. A key carries its tenant. |
| `--repository`, then `repository` of the binding of the checkout (`kairos config show`) | The repository in Kairos. With neither, the CLI uses the repository whose `repo_url` is the `origin` remote of the checkout. |
| `--max-changed` | The most files that can change since the base index. When the update runs a model, the default is 200. With `--link-only` or on a hosted repository, there is no default limit. |
| `--link-only` | Do not run the summary model. The update links the summaries that are in the pool of the index, and it does not make new summaries. The background update of the Claude Code plugin uses this option. |

A worktree has the `[[project]]` of its clone, because the entry matches the
`origin` remote. Git excludes the retired `.claude/kairos.local.md`, so a
worktree has no such file. In a worktree, the CLI reads the file of the main
clone. The main clone is the folder above `git rev-parse --git-common-dir`.
The plugin hooks use the same rule.

A repository can opt in to the hosted provider of its organization
(`code_index_summaries = hosted`). On such a repository, the update makes
no summary, whatever `KAIROS_INDEX_SUMMARIZE` says. It links the summaries
of the pool, and its last line says so. The structure of the checkout is
still built for the changed files. So search and the call graph are right
on a branch before its push.

The next push makes the summaries on Kairos. The CLI never has the key of
the organization. `kairos index build` and `kairos index --full` do not ask
Kairos. They run the model of the machine when it has one.

The base is the nearest indexed commit at or below the merge base of `HEAD`
and the default branch. The CLI also asks for the nearest indexed commit at
or below `origin/<default branch>`. It takes the base with fewer changed
files. A clone that is only behind the default branch then starts from the
index of its tip.

If more files than the limit changed since the base, the CLI builds
nothing. An update that runs no model has no default limit. It parses the changed files, about 0.1 s for each file, and links the
summaries of the pool.

The tool `fetch_index` runs no model, so it has no limit either. Give it
`max_changed` to set a limit. Above the limit, the tool does not change the
index.

The refusal gives the count of changed files. It names the index that it
counted from: the base, or the local index when it is nearer. Kairos can
have no index of the commit that the CLI asked for. Then the refusal also
gives the distance in commits from the base to that commit. Then it tells
you what to do:

- The checkout is only behind the default branch (`HEAD` is an ancestor of
  `origin/<default branch>`): run `git pull --ff-only`, or open a worktree
  at `origin/<default branch>`.
- Else: rebase on the default branch, or run `kairos index --full`. The
  text gives the time of a full build.
- In each case: give `--max-changed <N>` (the tool: `max_changed`) with the
  count, to update from this base.

After a refusal of `fetch_index`, the tools still answer for the previous
checkout, and the error names it. With no previous checkout, the error says
that the other tools refuse until `fetch_index` succeeds.

If the CLI cannot reach Kairos, it updates the local index and says so.

### The code tools: choose one symbol

The tools `symbol`, `callers`, `callees`, `references` and `path` of `kairos index mcp`
find a symbol by its name: `name`, `Type::name` or `Type.name`. When more
than one symbol has the name, the tool uses these steps:

1. `file` keeps the symbols of that file. The value is a path from the root
   of the checkout, or the end of the path.
2. `line` keeps the symbol that starts on that line. If no symbol starts on
   it, `line` keeps the smallest symbol that holds the line.
3. A Rust `impl` block has the name of its type. When the name matches a
   type and its `impl` blocks, the tool uses the type. To get an `impl`
   block, give its `line`.
4. When only one of the symbols is a function or a type, the tool uses it.

If more than one symbol is left, the tool gives the error `AMBIGUOUS`. The
error lists the candidates as `name (file:line)`. Give a candidate as the
symbol to choose it. The error also tells you which argument to give. It
is `file` when the candidates are in different files, and `line` when they
are in one file. `path` has the arguments `from_file`, `from_line`, `to_file` and
`to_line`.

### The code tools: the source of a symbol

Give `source: true` to the tool `symbol` to get the source text of the
symbol. The tool reads the text from the file in the checkout. It uses the
bytes of the symbol that the index has.

- The text is 16384 bytes or less. If the symbol is longer, the tool cuts
  the text at the end of a line. The result gives the length of the symbol
  and the line where you can read the rest of the file.
- If the file changed after the last index update, the result has a
  warning: the source can be different from the symbol. Run
  `kairos index update` to update the index.
- If the file is not in the checkout, the result has no source.

### The code tools: the uses of a symbol

The tool `references` gives each use of a symbol, not only the calls.
`kairos index references <symbol>` gives the same text. The tool chooses the
symbol as `symbol` does.

- For Rust, the uses come from the SCIP data of rust-analyzer. Each use has
  its file, its line, its kind and the symbol that holds it. The kinds are
  `call`, `type`, `field`, `import` and `other` (a constant, a module or an
  enum variant). A use of a field of a struct is a use of the struct, with
  the kind `field`. The definition and `Self` are not uses.
- For Go, TypeScript, TSX, JavaScript and Python, the uses come from the
  SCIP data of their indexers, if the build ran them (see [The tools of Go, TypeScript and Python](#the-tools-of-go-typescript-and-python)).
  The kinds are the same. A use of a method of a Go interface is a `call`
  of the interface.
- When the build did not run the indexer of a language (the tool is not
  there, or the install or the indexer failed), the index has the calls
  only for that language. This applies to Rust too. A missing
  rust-analyzer or std source gives Rust its calls by name. A file with an
  incorrect sha256 or a failed run does the same. The result says so. The
  build shows a warning with the reason.
- `kairos index update` with no `--rust-edges` (or `--scip`) keeps the uses
  of each file that did not change. A changed file has its calls only until
  the next SCIP run. The result gives the count of these files. Run
  `kairos index update --scip` to get all the uses again.
- The result shows the possible calls only when `possible` is true. A
  possible call is a call by a name that more than one function has.
- `limit` is the most uses to show, from 1 to 1000. The default is 100.

The uses are part of the index since schema version 7. Kairos refuses an
index of an earlier version.

### The code tools: the places that cite an ADR

The index records each ADR short code in a comment, a doc string or the
name of a test. Give an ADR short code to the tool `references` as `symbol`
(for example `KAIROS-A-0065`). The result gives each place that cites the
ADR, by file, with its line, its context and its symbol.
`kairos index references KAIROS-A-0065` gives the same text.

- An ADR short code is a board prefix, `-A-` and a number of 4 digits or
  more. A code that is part of a longer word is not a citation.
- The context `doc` is a doc comment or a Python doc string. In Rust, a doc
  comment is `///`, `//!`, `/** */` or `/*! */`. In TypeScript and
  JavaScript, it is a `/**` comment. In Go, it is a comment on the line
  above a declaration.
- The context `comment` is each other comment. In SQL, a comment is `--`
  or `/* */`. In Terraform, it is `#`, `//` or `/* */`.
- The context `test` is the name of a test in snake case:
  `retry_is_safe_kairos_a_0065` cites `KAIROS-A-0065`.
- A code in a string that is not a doc string is not a citation.
- The symbol of a citation is the smallest symbol that holds it. A doc
  comment above an item has the item as its symbol. A citation outside
  each symbol is at the top of the file.
- With an ADR short code, the tool takes no `file`, `line` or `possible`.
  It refuses them.

`get_repository` lists the ADRs that the code cites, in the section "ADRs
that the code cites". The list comes from the newest code index of the
default branch. Each line has the ADR, its column, and the count of its
places and of its files. For a code that no ADR of the organization has,
the line says so.

The citations are part of the index since schema version 8. A base index
of an earlier version has no citations: the next build of the repository
reads them. Run `kairos index --full` to build a local index again. Run `kairos index --full`, or take a new base
index from Kairos. A CLI refuses an index of a later version: the index comes
from a newer server or CLI. Run `kairos self-update`, or run `kairos index
--full` with this CLI.

### The code index: Terraform

The index reads `.tf` and `.tfvars` files as the language `terraform`. Each
top-level block gives a symbol. The name and the container of the symbol
make the address that Terraform code uses:

| Block | Kind | Name in the tools |
|---|---|---|
| `resource "aws_s3_bucket" "b"` | `resource` | `aws_s3_bucket.b` |
| `data "aws_iam_policy" "p"` | `data` | `data.aws_iam_policy.p` |
| `module "net"` | `module` | `module.net` |
| `variable "region"` | `variable` | `var.region` |
| each entry of a `locals` block | `local` | `local.name` |
| `output "id"` | `output` | `id` |
| `provider "aws"` | `provider` | `aws` |

- A `terraform`, `moved`, `import` or `check` block gives no symbol.
- A `.tfvars` file has no symbols.
- The index does not read `.tf.json` files. They are JSON, and the HCL
  grammar does not read JSON.
- A resource and a data source get a summary. A file of variables, outputs
  and module calls gets a summary from its first comment and its blocks.
- The index does not read the `.terraform/` folder. `terraform init` puts
  the downloaded modules there.

The tools `callers`, `callees` and `path` use the references of the
Terraform code. A reference is a call from the block that holds it to the
block that it names:

- `var.x`, `local.x`, `module.z`, `data.t.n` and `t.n` go to the block in
  the same folder. Terraform reads one folder as one module. Thus a block
  of the same name in a different folder is not a match.
- `module.z.o` goes to the output `o` of the module that `module "z"`
  calls. Each argument `x` of `module "z"` goes to the variable `x` of that
  module. The `source` must be a local path (`./` or `../`). A module from a
  registry, from git or from a URL is outside the checkout: its references
  are external.
- `count.index`, `each.key`, `self`, `path` and `terraform` are not
  references to a block.

The tool `module_map` shows the Terraform blocks of each folder with
Terraform code, before its files:

```text
## infra/modules/bucket
resources: aws_s3_bucket (this [count]), aws_sns_topic (events [for_each])
variables: name, owner
outputs: arn
called by: infra (module.store)
```

- `resources:` and `data:` give the managed resources and the data
  resources, by type.
- `module calls:` gives each `module` block with its `source` and its
  `version`.
- `[count]` and `[for_each]` mark a block that has `count` or `for_each`.
  The tool does not calculate the count of the instances.
- `called by:` gives the module calls of the checkout that call the folder.
- A root folder has no `called by:` line. It has a `calls:` line with each
  chain of module calls from it, for example
  `module.store > infra/modules/bucket`. A module from a registry, from git
  or from a URL has `(remote)`.

A folder with no Terraform code shows its summary and its files only.

## Helm chart values

`deploy/helm/kairos/values.yaml`. Most values under `config` become entries in a
ConfigMap that the Deployment loads with `envFrom`. Two exceptions: the
`config.webClientSecret*` values never reach the ConfigMap — the secret is
Secret-sourced, like `DATABASE_URL` — and the `config.retention.*` values
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
| `replicaCount` | integer | `2` | Server replicas. Ignored when `autoscaling.enabled` is true. The replicas share the background work through the [job queue](#job-queue), and the login throttle counts in the database. The `KAIROS_JOB_LIMIT_*` limits are for each replica. |
| `image.repository` | string | `ghcr.io/colliery-io/kairos` | Image repository. |
| `image.tag` | string | `""` | Empty tracks the chart's `appVersion`. An explicit value pins a published release, e.g. `"0.18.1"`. The chart never pins `latest`. |
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
| `tmp.enabled` | bool | `true` | Mounts an emptyDir at `/tmp`. The root filesystem is read-only, and the code index builder, index uploads and the git helper make temporary files. Turn it off only when `extraVolumes` and `extraVolumeMounts` give `/tmp`. |
| `tmp.sizeLimit` | quantity | `8Gi` | The size limit of that emptyDir. Empty means no limit. |
| `extraVolumes` | list | `[]` | Extra pod volumes. |
| `extraVolumeMounts` | list | `[]` | Extra container volume mounts. |
| `worker.enabled` | bool | `false` | Runs the code index builds in a Deployment of their own, with `KAIROS_ROLE=worker`, and sets `KAIROS_ROLE=api` on the server pods. See [Server role](#server-role). |
| `worker.replicaCount` | integer | `1` | Worker pods. |
| `worker.resources` | resources | requests `1` CPU, `2Gi`; limits `4` CPU, `8Gi` | CPU and memory of a worker pod. Increase the memory limit for big repositories. The limits of the server pods do not change. |
| `worker.codeIndexDir` | path | `/var/lib/kairos/code-index` | `KAIROS_CODE_INDEX_DIR` of the worker pods, an emptyDir of the pod. A pod that starts again downloads the tools again. For a folder that stays, mount a volume at this path with `worker.extraVolumes` and `worker.extraVolumeMounts`. |
| `worker.codeIndexSizeLimit` | quantity | `null` | Size limit of that emptyDir. |
| `worker.extraEnv` | list of EnvVar | `[]` | Added after `extraEnv`, on the worker pods only, e.g. `KAIROS_CODE_INDEX_THREADS`. |
| `worker.extraVolumes` / `worker.extraVolumeMounts` | list | `[]` | Volumes of the worker pods, e.g. a volume for `worker.codeIndexDir`. |
| `worker.podAnnotations` / `worker.nodeSelector` / `worker.tolerations` / `worker.affinity` | map / list | `{}` / `[]` | Annotations and scheduling of the worker pods. |

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
| `containerSecurityContext.readOnlyRootFilesystem` | bool | `true` | Container-level security context. Temporary files go to the `tmp` volume. |
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
| `autoscaling.enabled` | bool | `false` | When true, `replicaCount` is ignored. The HPA can add and remove pods. When it removes a pod during a job, another pod takes the job after its lease. |
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
| `config.tenancy.singleTenantName` | string | `""` | Sets `KAIROS_SINGLE_TENANT_NAME`. Emitted only when non-empty. |
| `config.tenancy.autoJoinDomains` | list or string | `[]` | Sets `KAIROS_AUTO_JOIN_DOMAINS`; a list is joined with commas. Emitted only when non-empty. Rendering fails when it is set without `singleTenant`. |
| `config.deploymentAdmins` | string | `""` | Sets `KAIROS_DEPLOYMENT_ADMINS`. |
| `config.log.level` | string | `info` | Sets `KAIROS_LOG_LEVEL`. |
| `config.log.format` | string | `json` | Sets `KAIROS_LOG_FORMAT`. |
| `config.devUi` | bool | `false` | Sets `KAIROS_DEV_UI`. |
| `config.otel.endpoint` | string | `""` | Sets `KAIROS_OTEL_ENDPOINT`. Emitted only when non-empty; empty disables tracing. |
| `config.otel.sampleRatio` | string | `""` | Sets `KAIROS_OTEL_SAMPLE_RATIO`. Emitted only when non-empty. |
| `config.auth.maxFailures` | integer or `""` | `""` | Sets `KAIROS_AUTH_MAX_FAILURES`. Emitted when set, **including `0`**, which turns throttling off. |
| `config.auth.failureWindowSecs` | integer or `""` | `""` | Sets `KAIROS_AUTH_FAILURE_WINDOW_SECS`. Emitted only when set. |
| `config.auth.lockoutSecs` | integer or `""` | `""` | Sets `KAIROS_AUTH_LOCKOUT_SECS`. Emitted only when set. |
| `config.auth.cacheSecs` | integer or `""` | `""` | Sets `KAIROS_AUTH_CACHE_SECS`. Emitted when set, **including `0`**, which turns the cache off. |
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
| `config.retention.notificationsReadDays` | integer or null | `null` | Sets `KAIROS_NOTIFICATIONS_READ_DAYS`. Emitted only when not null, so `0` is emitted. |
| `config.retention.notificationsUnreadDays` | integer or null | `null` | Sets `KAIROS_NOTIFICATIONS_UNREAD_DAYS`. Emitted only when not null, so `0` is emitted. |
| `config.retention.codeIndexPoolDays` | integer or null | `null` | Sets `KAIROS_CODE_INDEX_POOL_DAYS`. Emitted only when not null, so `0` is emitted. |
| `config.retention.codeIndexKeep` | integer or null | `null` | Sets `KAIROS_CODE_INDEX_KEEP`. Emitted only when not null, so `0` is emitted. |
| `config.retention.archiveTarget` | string | `""` | Sets `KAIROS_ARCHIVE_TARGET`: a path or an `s3://bucket/prefix` URL. Emitted only when non-empty. |
| `config.retention.archiveS3Endpoint` | string | `""` | Sets `KAIROS_ARCHIVE_S3_ENDPOINT`. Emitted only when non-empty. |
| `config.retention.archiveS3Region` | string | `""` | Sets `KAIROS_ARCHIVE_S3_REGION`. Emitted only when non-empty. |
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

Retention values left `null` or `""` are absent from the ConfigMap. Then the
server uses its defaults.

## Reference Compose deployment

`deploy/docker-compose.yaml` with `deploy/.env`, copied from
`deploy/.env.example`. The topology is Caddy for TLS and subdomain routing,
one Kairos container, and PostgreSQL 16 as the sole state — as
`pgvector/pgvector:pg16`, because Kairos requires the `pgvector` extension and
stock `postgres:16` does not carry it. There is no bundled identity provider.

| `.env` variable | Default in the example | Consumed by |
|---|---|---|
| `KAIROS_VERSION` | `0.18.1` | The Kairos image tag. Required — Compose fails if it is unset. |
| `KAIROS_SITE_ADDRESS` | `kairos.example.com` | Caddy's site address. A hostname enables automatic HTTPS; `:80` is a local no-TLS trial. Required. |
| `POSTGRES_PASSWORD` | `change-me` | The Postgres password, and the password inside the `DATABASE_URL` the Compose file composes. Required. |
| `OIDC_ISSUER_URL` | `https://idp.example.com/` | The server, verbatim. Required. |
| `OIDC_AUDIENCE` | `kairos` | The server, verbatim. Required. |
| `KAIROS_WEB_CLIENT_ID` | `kairos-web` | The server. Falls back to `kairos-web`. |
| `KAIROS_BASE_DOMAIN` | `kairos.example` | The server. Defaults to empty. |
| `KAIROS_SINGLE_TENANT` | commented out | The server. Defaults to empty. |
| `KAIROS_SINGLE_TENANT_NAME` | commented out | The server. Defaults to empty. |
| `KAIROS_AUTO_JOIN_DOMAINS` | commented out | The server. Defaults to empty. |
| `KAIROS_DEPLOYMENT_ADMINS` | empty | The server. Defaults to empty. |
| `KAIROS_LOG_LEVEL` | `info` | The server. Falls back to `info`. |
| `KAIROS_API_BEARER` | `access_token` | The server. Defaults to empty, so the server default `access_token` holds. |
| `KAIROS_WEB_CLIENT_SECRET` | empty | The server. Defaults to empty. |

The Compose file also passes each other setting under
[Server environment variables](#server-environment-variables). The default is
empty, so the server default holds. There are two exceptions. The retention
settings get their documented defaults: the server reads an empty retention
value as an error. The two settings under [Development](#development) are not
in the Compose file. `scripts/check_compose_settings.py` (in `angreal test unit` and CI) fails
when the Compose file, `.env.example` and this page do not agree.

The profile `worker` adds the service `worker`. It is the same image with
`KAIROS_ROLE=worker` and its own volume for the clones and the tools. The
volume is at `/var/lib/kairos/code-index`, its `KAIROS_CODE_INDEX_DIR`, unless
`.env` names another folder.

Its limits are `mem_limit` 8g, `mem_reservation` 2g and `cpus` 4. To use it, add `worker` to
`COMPOSE_PROFILES` and set `KAIROS_ROLE=api` in `.env`. Increase `mem_limit`
for big repositories. See [Server role](#server-role).

The Compose file also fixes two values that have no `.env` entry:
`KAIROS_BIND_ADDR` is `0.0.0.0:8080` and `KAIROS_LOG_FORMAT` is `json`. The
image applies pending migrations on boot, before binding, so no separate
migration step exists in this deployment.

## Related reading

- [CLI](../cli/) — the commands that read the CLI configuration above
- [Archiving](../../explanation/archiving/)
- [Capabilities and access](../../explanation/capabilities-and-access/)
- [Glossary](../glossary/)

## Related guides

- [Install with Helm](../../how-to/install-with-helm/)
- [Configure an OIDC issuer](../../how-to/configure-an-oidc-issuer/)
- [Provision a tenant](../../how-to/provision-a-tenant/)
- [Connect a git forge](../../how-to/connect-a-git-forge/)
- [Back up and restore](../../how-to/back-up-and-restore/)
