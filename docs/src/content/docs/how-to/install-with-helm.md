---
title: "Install Kairos on Kubernetes with Helm"
---

Get a Kairos 0.18.1 deployment serving on a cluster you already run.

**Before you start**, have all four:

- A cluster and Helm 3.8 or newer (the chart is distributed as an OCI artifact).
- **A PostgreSQL the cluster can reach, with `pgvector` available.** The
  extension is a requirement, not an option: the first migration installs it, and
  a database that cannot is refused at startup with a message saying so. Managed
  Postgres offers pgvector on RDS, Cloud SQL and Azure; `pgvector/pgvector:pg16`
  is the upstream image with it added.

  If you are only trying Kairos out, you can skip this: the chart will stand a
  Postgres up for you. See [Trying it out without a
  database](#trying-it-out-without-a-database) — and do not use it for anything
  real.
- **An OIDC issuer with Kairos registered as an app.** The chart bundles no
  identity provider. If you have not registered the clients yet, do
  [Configure an OIDC issuer](../configure-an-oidc-issuer/) first — you need the
  issuer URL and audience before the chart will render.
- Your own OIDC `sub`, which becomes the deployment admin that provisions the
  first tenant.

Every value named below is specified in
[Configuration → Helm chart values](../../reference/configuration/#helm-chart-values).

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
  [Connect a git forge](../connect-a-git-forge/)): neither has a dedicated chart
  value. Set them through `extraEnv`.
- **You want a different number of replicas:** set `replicaCount` (default
  2). The pods share the background work through the job queue in the
  database, and each job runs on one pod. The `KAIROS_JOB_LIMIT_*` limits are
  for each pod. To let the cluster set the number of pods, set
  `autoscaling.enabled: true`. See [The job queue](../../explanation/job-queue/).
- **You scrape with the Prometheus Operator:** set
  `metrics.serviceMonitor.enabled: true`, which scrapes `/metrics`. Liveness
  probes go at `/healthz`, readiness at `/readyz`; the chart wires both already.

Leave `image.tag` empty so it tracks the chart's `appVersion`. Do not pin
`latest`; the chart never resolves it. Leave `config.devUi` at `false` — it
mounts the Swagger UI at `/api/docs`.

## 3. Install

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos \
  --version 0.18.1 -f my-values.yaml
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
([Probes](../../reference/configuration/#probes)).

## 5. Confirm the release

```sh
helm test kairos          # runs a Pod that curls /healthz
```

## 6. Provision the first tenant

A fresh install has no organizations. Log in through your issuer, then follow
[Provision a tenant](../provision-a-tenant/) — the `config.deploymentAdmins`
value from step 2 is what grants you the cross-tenant admin routes.

## Upgrading and rolling back

```sh
helm upgrade kairos oci://ghcr.io/colliery-io/charts/kairos \
  --version <new> -f my-values.yaml
helm rollback kairos
```

`helm rollback` rolls the process back, not the database: migrations are
forward-only. Take a dump before every upgrade, and roll the database back with
it — see [Back up and restore](../back-up-and-restore/).

Before it serves, the server applies the new public migrations and the new
tenant migrations of each organization. An upgrade thus needs no migration
step. The replicas take turns: a database lock lets one migrate at a
time. A failed migration of one organization does not stop
the server. That organization answers 503 `TENANT_NOT_READY`, and the server
log has the error. Repair the
schema and run `kubectl exec deploy/kairos -- kairos-server migrate-tenants`;
the organization serves again within 30 seconds.

One query finds the organizations with a pending tenant migration. The
server migrates only those organizations. A restart with no new migration
thus waits for one query, not one for each organization.

A migration waits for a table lock for 5 seconds at most. Then it fails,
and the log names the migration. Run `migrate-tenants` again when the
database is less busy. A migration with `CONCURRENTLY` can fail after it
starts the index. Postgres then keeps an INVALID index, and the next run
stops at that index. Drop it with `DROP INDEX CONCURRENTLY org_<slug>.<index>`,
then run `migrate-tenants`.

### The upgrade notes of a release

Each section below has the same form. Copy this form for a new release:

```md
### From 0.X to 0.Y

These changes of 0.Y show after the upgrade. They apply to Compose too.

- N migrations run: `name` (what it does), `name` (what it does).
- Locks: each migration that locks a large table, with the table, the lock
  and the time. Write "No migration locks a large table" when none does.
- Scans: each VALIDATE CONSTRAINT and each CONCURRENTLY index, with its
  table. Writes to the table continue during the scan.
- Each other change that an operator sees after the upgrade.
```

### From 0.12 to 0.13

Three changes of 0.13 show after the upgrade. They apply to Compose too.

- The migration `team_scaffold_removed` deletes the seed folders of a team
  that nobody edited. It has no down.
- The code index has a new schema. Every repository gets a full build after
  the upgrade, one at a time. Until the build of a repository ends,
  `kairos index update` on that repository fails with a schema-version error:
  run `kairos index --full`, or wait.
- Every repository follows the default branch of its forge now. The next
  fetch replaces a branch that a person set by hand in 0.12. Set it again
  with `kairos repos update <slug> --default-branch <branch>`.

### From 0.13 to 0.14

These changes of 0.14 show after the upgrade. They apply to Compose too.

- Six migrations run. One gives each tenant a "Forge" system user. The
  others add forge filters, forge refs, issue intake and two run columns.
- The forge webhook records every branch and pull request now, also with no
  short code. Set a filter on a repository to record fewer.
- To take issues, add the Issues event to the GitHub webhook. On GitLab, add
  Issue events. The import of open issues needs Issues: read on the read
  token. On GitLab it needs `read_api`.
- The code index builder downloads the SCIP tools of Go, TypeScript and
  Python when a repository first needs them. Each language takes a few
  hundred MB in `KAIROS_CODE_INDEX_DIR/tools/`.
- The dependency caches grow to `KAIROS_CODE_INDEX_DEPS_CAP_MB` (10 GiB by
  default). On a small disk, set a lower cap before the upgrade.
- `concurrency` of the summary model is now one limit for each server, not
  for each build. A rate limit makes a build wait, and a missing summary
  does not stop the build.
- After login, the browser opens the "For me" page at `/`. The boards are at
  `/boards`.

### From 0.14 to 0.15

These changes of 0.15 show after the upgrade. They apply to Compose too.

- Two migrations run: `item_external_refs` (a link from an item to a URI)
  and `task_state` (the live state of a task).
- Each note gets an id in its heading, like `n-3`. An old note gets one by
  its position when the server reads it. `append_note` returns the id.
- The MCP server has three more operations: `promote_note`, `set_state`,
  and `relationship: external` on `link_items`. The tool count is 70.
- The plugin has three new hooks: a question before a push, merge, rebase,
  tag or release; the Kairos rules once per session; and a reminder on a
  write to local memory. Update the plugin with `/plugin` to get them.

### From 0.15.0 to 0.15.1

A point release with fixes. No migration runs.

- The Compose file passes every documented setting to the containers. A
  local override for a setting of 0.14 can go.
- Drag-and-drop in the tree of the team pages works again in Chromium. It
  did not work from 0.13.0 to 0.15.0.
- The plugin tells you when the `kairos` CLI is older than the plugin or
  the server. Run `kairos self-update`.

### From 0.15.1 to 0.16.0

These changes of 0.16 show after the upgrade. They apply to Compose too.

Before the upgrade:

- Stop every old server before a new one serves. Do not run 0.15 and 0.16
  at the same time. A 0.15 server drops the new event batches of the outbox.
  Its write-back can also fail on the new unique indexes.
- Take a dump. The migrations below change data, and they have no down for
  the data.
- Look at the repositories that have a write token. After the upgrade,
  Kairos comments on the issue of each done or cancelled task, and closes
  it. This includes old tasks.

25 migrations run, 5 public and 20 for each tenant:

- Public:
  - `local_sessions_expires_at`: an index.
  - `event_outbox`: a new table for the events.
  - `writeback_jobs_dedupe`: it ends the duplicate queued write-back jobs.
    The oldest job stays.
  - `writeback_jobs_pr_key`, `writeback_jobs_issue_key`: a unique index on
    the queued jobs.
- Tenant, data: `item_notes` moves each note out of the content into a row
  of its own. `one_parent` keeps the oldest `parent` edge of an item. It
  removes each other one, and writes an `activity_log` row for each.
  `retention_more` fills in the item of the old relationship rows of
  `activity_log`. `embedding_dirty` marks each item, so the embedding sweep
  reads each item one time.
- Tenant, schema only:
  - `blocked_claims`, `board_parent_gate`, `board_owner_gate`,
    `board_blockers_gate`, `notification_reads`, `indexes`,
    `issue_writeback`, `api_key_scope`, `history_diffs`.
  - `item_search`: a stored tsvector for each item. Triggers write it.
  - A new kind of notification: `blocked_again_notification`,
    `blocker_cancelled_notification`, `claim_dropped_and_stale`,
    `key_expiring`, `proposal_decided`, `proposal_decided_validate`.

Locks and scans:

- `writeback_jobs_pr_key` and `writeback_jobs_issue_key` build their index
  with `CONCURRENTLY` on `public.jobs`. Writes to `jobs` continue. If one of
  them stops at the lock timeout, drop the INVALID index and migrate again
  (see above).
- `proposal_decided_validate` reads each row of `notifications` to validate
  its CHECK. Writes continue.
- `history_diffs` adds its CHECK `NOT VALID`. It reads no row.
- `indexes`, `retention_more` and `item_search` build plain indexes on
  `activity_log`, `notifications`, `webhook_deliveries` and the item
  tables. Writes to a table wait while its index builds. Reads continue.

Changes that people and agents see:

- A task starts in the entry column of its board. A create into another
  column gets 422. Move the task after the create.
- A note is a row of its own. `get_item` shows the notes after the content,
  and `append_note` writes no version of the content. History keeps a diff,
  with a full row every 10 versions.
- An edit or a move on a strategy board or an initiative board needs the
  `member` grant. Give `member` to each person who needs it.
- Only a person with the `approve_proposals` grant on the board of one end,
  or an organization admin, confirms or rejects a proposal. At first, only
  organization admins have it. The error `PERSON_DECIDES` is gone.
- A `blocks` edge needs the right to edit the blocked item. With rights on
  the source only, the call makes a proposal.
- An agent key can have a scope. A key from before the upgrade keeps all the
  rights of its person. A new agent key gets each capability except `purge`
  and `manage_structure`, so it does no administration of the organization. A refusal has
  the code `KEY_SCOPE`.
- The details of an error name the input with the key `field`. The key
  `argument` is gone.
- The REST lists of team pages have no content. Page history and
  announcements answer a list with `limit` and `offset` (default 50, at most
  200). CLI `--json` lists answer one envelope with `next_offset`.
- Retention deletes unread notifications after 180 days, and keeps 5 code
  indexes for each repository and ref. It keeps the `transition`, `cancel`
  and `board_move` rows of `activity_log` at every age.
- Each server holds one more database connection, for the event emitter.
- The MCP server has 78 tools. The CLI has new verbs, and each verb takes a
  name or a slug. Run `kairos self-update` and update the plugin with
  `/plugin`.

### From 0.16.0 to 0.16.1

These changes of 0.16.1 show after the upgrade. They apply to Compose too.

Before the upgrade, stop every 0.16.0 server before a new one serves, as
for 0.16. No test runs 0.16.0 servers and a 0.16.1 server together.

1 migration runs, for each tenant: `issue_sync`. It adds the table
`forge_issue_sync`. It adds one nullable column to `repositories` and one to
`item_links`. It lets `item_links.connection_id` be empty. It rewrites no
row, and every repository keeps the new import interval off.

- Locks: `repositories` and `item_links` are small tables. The CHECK and the
  FOREIGN KEY take a short lock. No migration locks a large table.
- Scans: no migration validates a constraint or builds an index with
  `CONCURRENTLY`.

Changes that people and agents see:

- A repository can import its open issues on an interval, with no webhook.
  An organization admin sets it: `kairos repos update --import-interval 15m`,
  the MCP tool `update_repository`, or the Admin page Repositories. The
  default is `off`. The shortest interval is 1 minute and the longest is 30
  days. The import and the interval need only the read token of the
  repository. They need no `KAIROS_PUBLIC_URL` and no webhook connection.
  `kairos repos import-status` shows the last run.
- The retention archive can go to a bucket. Set `KAIROS_ARCHIVE_TARGET` to
  `s3://bucket/prefix`. Credentials come from the AWS chain. Two optional
  settings, `KAIROS_ARCHIVE_S3_ENDPOINT` and `KAIROS_ARCHIVE_S3_REGION`, are
  for an S3-compatible store. The Helm values are
  `config.retention.archiveS3Endpoint` and `config.retention.archiveS3Region`.
  An `s3://` value with no bucket stops the server at start. Before this
  release, an `s3://` target made each retention pass fail.
- The code index of a Go project no longer fails with "The file go.mod
  changed after the build of the structure". The builder restores the Go
  files after each Go step. When another file changes, the builder builds the
  structure again once.
- A Python project can ask for another Python or another uv than the pinned
  ones. Set `KAIROS_CODE_INDEX_TOOL_DOWNLOADS=true`, and the builder fetches
  them from the internet. The default is off, and the builder fetches nothing. A failed
  `uv sync` now says what the project asks for.
- A repository shows each language whose edges are by name: `kairos repos
  show`, the MCP tool `get_repository`, and the field
  `code_index_scip_warnings` of `GET /api/repositories/{slug}`.
- JavaScript with `.mjs` or `.cjs` files, or with a `tsconfig.json` that has
  no `allowJs`, gets a SCIP index.
- The image build of a release no longer fails on a race in its smoke test.

### From 0.16.1 to 0.17.0

These changes of 0.17.0 show after the upgrade. They apply to Compose too.

Before the upgrade, stop every 0.16.1 server before a new one serves, as
for 0.16. No test runs 0.16.1 servers and a 0.17.0 server together.

2 migrations run, for each tenant:

- `initiative_cancel_mark` removes the foreign key from
  `task_cancellations.task_id` to `tasks`. The table now holds the cancel
  mark of a task or of an initiative. It changes no row.
- `request_keys` adds the empty table `request_keys` and 2 indexes on it.
  It reads and changes no row of another table.

- Locks: `task_cancellations` is a small table. The drop of the constraint
  takes a short lock. No migration locks a large table.
- Scans: no migration validates a constraint or builds an index with
  `CONCURRENTLY`.

Changes that people and agents see:

- `cancel_item` cancels an initiative, as it cancels a task. The initiative
  goes to the done column of its board with the mark "cancelled", and the
  card shows the mark. The tool refuses a strategy, an ADR or a document. The
  refusal names the route to retire it (KAIROS-A-0067).
- `transition_item` takes `claim: true`. For an initiative with no owner,
  the call makes you the owner and moves it in one step (KAIROS-A-0066).
- A refused move names the full path to the target column, for example
  "Discovery → Design → Ready → Decompose → Active". From a done column the
  refusal names `reopen_item`.
- 14 MCP write tools take an optional `request_key`. A second call with the
  same key in 24 hours writes nothing and returns the first answer. For the
  same key with other arguments, the tool refuses the call with
  `REQUEST_KEY_REUSED` (KAIROS-A-0065). The retention pass deletes the keys after 24 hours.
- The MCP tools accept a fixed list of aliases for some arguments, for
  example `query` for `q` and `id` for `short_code`. The schema of each tool
  lists them. Each other unknown argument is still refused.
- A wrong type of argument names the field and the type, for example
  `filter.entity_type must be a list of strings`.
- The `link_items` description has the table of the allowed source and
  target types. When the rules let the edge go in the other direction, the
  refusal says to swap source and target.
- When `add_team_member` or a grant cannot find a person, the refusal names
  how a person becomes a member of the organization on this deployment.
- `get_events` takes `unread: true`. A failed `edit_item` search that differs
  only in whitespace quotes the exact text to send. An unknown team lists
  the team slugs.
- `kairos mcp-proxy` takes `--profile admin` or `--profile all`. To get the
  admin tools in Claude Code, add a second server:
  `claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin`.
- The `kairos` MCP server of the plugin is now `kairos mcp-proxy`, so it
  needs the `kairos` CLI on the PATH. The proxy has timeouts (connect 5 s,
  request 60 s, `KAIROS_TIMEOUT`). It retries a read 2 times. It does not
  retry a write. A 502, 503 or 504, a timeout or a reset gives a short
  message with no HTML. The message says whether the server can have the write.
- `config.toml` in the config folder binds a repository to a deployment and
  its boards. `kairos config show`, `kairos config project` and
  `kairos config path` read and write it.
- `set_complexity` changes the complexity of an initiative after it is
  created.
- A code index session that starts in a folder above several checkouts lists
  them and keeps several open. A `checkout` argument picks one for each
  call.

### From 0.17.0 to 0.18.0

These changes of 0.18.0 show after the upgrade. They apply to Compose too.

Before the upgrade, stop every 0.17.0 server before a new one serves, as
for 0.16. No test runs 0.17.0 servers and a 0.18.0 server together.

4 migrations run, for each tenant:

- `code_index_citations` adds the empty table `code_index_citations`. It
  holds the ADR codes that the code of an index cites.
- `amends_relationship` replaces the CHECK constraint of
  `item_relationships.relationship` so that it accepts `amends`.
- `tool_refusals` adds the empty table `tool_refusals` and 1 index. It
  holds one row for each refused MCP call, with no argument values.
- `adr_revisit_metadata` adds 3 system metadata definitions for ADRs:
  `revisit_when`, `revisit_on` and `accepted_commit`.

- Locks: the new CHECK on `item_relationships` reads the table under a
  lock. The table is small (about 5000 rows on a large tenant), so the lock
  is short. No migration locks a large table.
- Scans: no migration builds an index with `CONCURRENTLY`.

The code index has a new schema (version 8). Every repository gets a full
build after the upgrade, one at a time. Until its build ends, the code
tools of a repository say that it has no index.

Changes that people and agents see:

- The local code index is no longer in the checkout. It is in
  `~/.cache/kairos/index/` (or `$XDG_CACHE_HOME/kairos/index/`, or
  `KAIROS_INDEX_DIR`). The CLI moves an old `.kairos/index.db` there on the
  first open, and writes no git exclude line. You can delete an empty
  `.kairos/` folder after the move.
- `.claude/kairos.local.md` gives a warning. Run `kairos config migrate` to
  write its values to a `[[project]]` of `config.toml`. The next release
  does not read the file. A board folder, or a checkout with no remote,
  gets a `[[project]]` with `path`. A session in a subfolder of a checkout
  uses the binding of the checkout.
- When the `kairos` server of the plugin has no login, no URL for its key,
  two logins, or a `config.toml` that does not parse, it starts with no
  tools. Its instructions give the reason and the command to run.
- `link_items` takes `amends`, from an ADR to an ADR: a new ADR that
  changes part of an old one. `get_item` of an ADR shows "Amended by" and
  "Amends" (KAIROS-A-0069).
- `kairos adrs check` reads the ADR codes that a tree cites. It fails on a
  missing or archived ADR, and it warns on a superseded one. Before an edit,
  the plugin shows the ADRs that the file cites. ADRs take `revisit_when`,
  `revisit_on` and `accepted_commit` (KAIROS-A-0070).
- The code index finds ADR citations. The `references` code tool takes an
  ADR code, and `get_repository` lists the ADRs that the code cites.
- The REST write routes take an `Idempotency-Key` header. A replay gives
  the first answer with `Idempotent-Replayed: true`.
- `mcp-proxy` retries a write that carries `request_key`, and the
  idempotent writes.
- The admin MCP tool `refusal_counts` gives the refused MCP calls by tool
  and code. The retention pass deletes the rows after 90 days.
- More refusals name their field: `details.field`, `parameter` or `header`.
  No refusal shows the text of a library.
- Some MCP arguments have new names, and the old names stay as aliases:
  `set_transition` takes `from_column` and `to_column`, `my_boards` takes
  `board_level`, and the metadata definition is `definition`. A reference
  of a person accepts `me` in more tools.

### From 0.18.0 to 0.18.1

0.18.1 repairs the release of 0.18.0. It has no migration and no new
setting. A 0.18.0 server and a 0.18.1 server can serve together.

- The CLI of 0.18.0 did not build for Intel macOS, so the release has no
  CLI tarballs and `kairos self-update` cannot install 0.18.0. 0.18.1 has
  all four tarballs again. The image of 0.18.1 is the same code as 0.18.0.

## Trying it out without a database

`helm install` with nothing but the two OIDC values works, because the chart
stands a PostgreSQL up inside the release:

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos \
  -n kairos --create-namespace \
  --set config.oidc.issuerUrl=https://idp.example.com/ \
  --set config.oidc.audience=kairos \
  --set config.tenancy.singleTenant=demo
```

That exists because Kairos requires `pgvector`, and asking someone to find a
Postgres with a particular extension before they can see the product is a poor
first afternoon.

**Do not run it in production.** One replica, one PVC, no backups, no failover,
and a password that sits in your values and your release history. When you are
past evaluating, turn it off and point at a managed instance:

```yaml
postgresql:
  enabled: false
database:
  url: "postgres://kairos:PASSWORD@my-postgres:5432/kairos"
```

You do not have to remember to do both. `postgresql.enabled` is **unset** by
default, which means *on unless you name a database* — so adding `database.url`
is enough, and every release that already exists keeps the database it has
through an upgrade without any edit. Setting `postgresql.enabled: true` *and* a
`database.url` is refused when the chart renders, because that is asking for two
databases and picking one for you would either abandon yours or quietly stand a
second up beside it.

## Evaluating without an identity provider

Kairos requires an OIDC issuer, which for an evaluator means running Dex by hand or
standing up a cloud OAuth client before seeing the product at all. So the chart can
stand a Dex up inside the release, the same way it can stand up a PostgreSQL.

You need three things: an ingress, an email, and a bcrypt hash.

```sh
# The chart ships NO default password. Generate one — nothing to install:
HASH=$(docker run --rm httpd:2.4 htpasswd -bnBC 10 "" 'your-password' | tr -d ':\n')

helm install kairos oci://ghcr.io/colliery-io/charts/kairos \
  -n kairos --create-namespace \
  --set ingress.enabled=true \
  --set ingress.tenancyMode=single \
  --set ingress.host=kairos.example.com \
  --set config.tenancy.singleTenant=demo \
  --set dex.adminEmail=you@example.com \
  --set-string dex.adminPasswordHash="$HASH"
```

No `config.oidc.issuerUrl` and no `config.oidc.audience`: both are derived from the
bundled Dex. You get a PostgreSQL too, since naming neither gets you both.

**Do not run it in production, and mean it more than you did for the database.** One
replica, **in-memory storage — so every restart rotates the signing keys and
invalidates every token in flight** — one static user, and a password hash in your
values and your release history. There is no user lifecycle, no password reset, no
MFA, and no audit trail. Point `config.oidc.issuerUrl` at a real issuer before
anybody who is not you logs in:

```yaml
config:
  oidc:
    issuerUrl: https://idp.example.com/
    audience: kairos
```

As with the database you do not have to remember to turn the bundle off.
`dex.enabled` is **unset** by default, meaning *on unless you name an issuer* — so
adding `issuerUrl` is enough, and every release that already exists keeps its issuer
through an upgrade without any edit. Setting `dex.enabled: true` *and* an
`issuerUrl` is refused at render time, because that is asking for two issuers.

### Why this one needs an ingress

The bundled PostgreSQL does not, and the difference is worth understanding rather
than working around.

An OIDC issuer URL is not just an address to fetch from — it is an **identity**. Dex
stamps it into the `iss` claim of every token it mints, and Kairos rejects a token
whose `iss` is not the issuer it was configured with. So the URL has to be the same
string in two places that see the cluster differently: the browser being redirected
to log in, and the server validating the token afterwards.

An in-cluster Service name like `http://kairos-dex:5556/dex` cannot be that string,
because a browser cannot resolve it. So the chart routes Dex at `/dex` on your
ingress and uses that public URL for both sides. The server then reaches its own
issuer by hairpinning out through the ingress and back — which works, and is another
reason this is for evaluation rather than production.

If TLS is not enabled on the ingress the issuer is `http://`, which browsers
increasingly dislike for a login form. Enable `ingress.tls` for anything beyond a
first look.

## Using a hosted embedding model

Retrieval works out of the box: the model ships inside the image and needs no
configuration. To use an OpenAI-compatible endpoint instead:

```yaml
embeddings:
  url: "https://api.openai.com/v1"
  model: "text-embedding-3-small"
  existingSecret: "kairos-embed"      # holding KAIROS_EMBED_API_KEY
```

The key follows the same pattern as `database.url` — inline as `embeddings.apiKey`
or from a Secret you already keep. A local Ollama needs no key at all. Setting
`embeddings.provider: none` turns embeddings off entirely; search still works,
from text alone.

## Related

- [Configuration](../../reference/configuration/) — every value and variable
- [Configure an OIDC issuer](../configure-an-oidc-issuer/)
- [Provision a tenant](../provision-a-tenant/)
- [Back up and restore](../back-up-and-restore/)
