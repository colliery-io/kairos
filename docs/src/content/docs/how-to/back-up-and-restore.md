---
title: "Back up a deployment, and restore it"
---

Take a backup you can actually restore from, and restore from it.

**Before you start:**

- `psql` / `pg_dump` access to the deployment's PostgreSQL, at a version at least
  as new as the server's.
- Back up exactly two things, and nothing else: **the database** — `public` plus
  every `org_<slug>` schema, in one dump — and **the deployment secrets that live
  outside it**: `DATABASE_URL`, `KAIROS_WEBHOOK_SIGNING_KEY`,
  `KAIROS_SECRETS_KEY`, `KAIROS_WEB_CLIENT_SECRET`, plus database roles if you
  manage them in the cluster. There is nothing on the pods to capture: the image is stateless and
  the chart mounts no volume.
- Keep `KAIROS_WEBHOOK_SIGNING_KEY` with the database password. Losing it is not
  data loss but it is an outage — every forge connection has to be rotated and
  re-pasted in the forge.
- Keep `KAIROS_SECRETS_KEY` with the database password too. The database holds
  the read tokens of the repositories, encrypted with this key. Without the
  key, you must set each read token again.

## 1. Dump the database

A single database-wide dump captures every tenant, because tenants are schemas
inside it:

```sh
pg_dump --format=custom --compress=9 \
  --dbname="$DATABASE_URL" \
  --file="kairos-$(date -u +%Y%m%dT%H%M%SZ).dump"
```

Do not dump schema-by-schema. `public` holds the organization rows, users,
service accounts and API-key hashes that every `org_*` schema refers to; a
per-tenant dump restores to a tenant nobody can log in to.

If cluster roles are yours to manage:

```sh
pg_dumpall --roles-only > kairos-roles.sql
```

## 2. Check the dump is a dump

```sh
pg_restore --list kairos-….dump | grep -c 'SCHEMA - org_'
```

The count should equal your tenant count — compare with
`kairos admin tenants list`. A dump that lists no `org_*` schemas was taken
against the wrong database.

## 3. Schedule it

Kairos imposes nothing here. Whatever your organization already does for
Postgres — a `CronJob` running the command above, your provider's automated
snapshots, WAL archiving with point-in-time recovery — applies unchanged.
Kairos's own guidance is only the two things above: the whole database, and the
secrets alongside it.

### The compose deployment has a schedule

The compose deployment has a service with the name `postgres-backup`. It makes
the dump of step 1, and it does the check of step 2.

The service makes a dump when it starts. Then it makes a dump at each interval.
It keeps the newest dumps and removes the others.

| Setting | Default | Meaning |
|---|---|---|
| `KAIROS_BACKUP_DIR` | `./backups` | The directory on the host that gets the dumps. |
| `KAIROS_BACKUP_INTERVAL` | `21600` | The seconds between two dumps. The default is 6 hours. |
| `KAIROS_BACKUP_KEEP` | `4` | The number of dumps that the service keeps. |

Set `KAIROS_BACKUP_DIR` to a directory on a different disk from the Docker
volumes. A disk failure then does not remove the database and its backups
together.

To see the result of the last dump, read the log of the service:

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env logs --tail 5 postgres-backup
```

The line `backup ok` gives the file and its size. A dump that fails leaves no
file, and it removes no old dump.

The service does not copy the secrets of the deployment. Keep a copy of
`deploy/.env` in a different location.

## Restore

Four steps, in this order.

### Stop the servers

```sh
kubectl scale deploy/kairos --replicas=0
```

Restoring under a live server means a schema changing beneath open connections.

### Restore into an empty database

```sh
createdb kairos_restored
pg_restore --dbname=kairos_restored --no-owner --clean --if-exists kairos-….dump
```

Restore into a fresh database rather than over the existing one, so a failed
restore leaves you somewhere to go back to.

### Point Kairos at it and bring it up

Update `DATABASE_URL` (the Secret, then `helm upgrade`), then scale back up.
On boot, before it binds, the server applies the new **public** migrations.
Then it applies the new **tenant** migrations of each organization. That is how a dump
from an older release comes forward, and it is why there is no migration Job to
run. Replicas that start together take turns: a database lock lets one migrate
at a time.

### Check the organizations

The server log has one line for each organization. A failed migration of one
organization does not stop the server. The server starts, and that organization
answers each request with 503 `TENANT_NOT_READY`. The log line has
the error. Repair the schema, then run:

```sh
kubectl exec deploy/kairos -- kairos-server migrate-tenants
```

The organization serves again within 30 seconds, with no restart.

### Verify

```sh
curl -fsS https://<host>/readyz          # ready
kairos admin tenants list                 # every tenant, with schema_exists true
kairos boards list --tenant <slug>        # one tenant's boards actually answer
```

Restore **forward or level, never backward**: the dump's schema must be the
binary's version or older. Migrations are forward-only — there is no `revert`
subcommand — so an older image against a database a newer release has already
migrated is not a supported configuration, and `/readyz` will not catch it.
When you roll an image back, roll the database back to a dump taken before the
upgrade.

## Size for the growth

The server runs a retention pass at startup and then each 24 hours
([Configuration → Retention](../../reference/configuration/#retention)). With
the defaults (`KAIROS_RETENTION_MODE=archive` and no `KAIROS_ARCHIVE_TARGET`),
the pass removes no history and no activity. The server does not remove a row
that it has not copied to an archive. So the pass skips the sweep of each
tenant and reads no row of `item_history`, `team_page_history` or
`activity_log`. It deletes only the rows in the list below.

In each mode, the pass deletes these rows and does not copy them to an
archive:

- The code indexes that are not among the newest 5 of their repository and
  ref (`KAIROS_CODE_INDEX_KEEP`). A build or an upload can make an index
  again.
- The summaries of the code-index pools that no stored index uses, after 30
  days (`KAIROS_CODE_INDEX_POOL_DAYS`).
- The unread notifications after 180 days
  (`KAIROS_NOTIFICATIONS_UNREAD_DAYS`), and the read ones 30 days after they
  were read (`KAIROS_NOTIFICATIONS_READ_DAYS`).
- The webhook deliveries that finished more than 30 days ago, with their
  tries.
- The rows of the count of the MCP refusals after 90 days (`refusal_counts`).
- The expired password sessions and the other short-lived rows.

If you must keep these rows, take a dump before the pass deletes them. Set a
value to `0` to keep each row of that kind.

The server shows this setting in five places:

- `/readyz` gives `ready` and a second line that starts with `warning:`. The
  status stays 200.
- The Admin overview of the GUI shows the alert "Retention removes nothing".
- The job of the pass records `skipped: no target`. `GET /api/org/retention`
  shows the record of the last pass to an organization admin.
- The `retention_sweep` row of each tenant starts with
  `mode:archive skipped: no target`.
- The server logs a warning at startup and at each pass.

To make the database smaller, set one of these:

- `KAIROS_ARCHIVE_TARGET` to a directory. The pass writes the old rows to NDJSON
  files in that directory, and then removes them from the database. There is
  one file for each table: `item_history`, `team_page_history` and
  `activity_log`. Back up that directory too: the rows are not in the dumps
  after the pass.
- `KAIROS_ARCHIVE_TARGET` to an `s3://bucket/prefix` URL. The pass uploads the
  same files to the bucket, and then removes the rows. The credentials come
  from the AWS chain. A Kubernetes deployment can use a role for the service
  account, and needs no volume. See
  [The S3 archive target](../../reference/configuration/#the-s3-archive-target).
- `KAIROS_RETENTION_MODE=discard`. The pass removes the old rows and keeps no
  copy.

After you set one of these, `/readyz` gives only `ready`.

The pass reads `item_history` one page of items at a time. It reads only the
version and the time of each row. It reads the content only of the rows that
it copies to the archive. It reads `team_page_history` in the same way, one
page of team pages at a time. It removes the rows of each table in batches.
So a large history does not fill the memory of the server.

The pass does not remove items. Archiving work does not make the database
smaller either, because it is a soft delete
([Archiving](../../explanation/archiving/)). So `item_history` grows by a row
for each edit in the hot window (90 days by default), and items grow with the
work. Most rows hold only a diff. The server writes a full copy of the text at
most each 10 versions. Size the volume and the backup window for that growth, and alarm on the
growth rate, not on a threshold.

## Related

- [Configuration](../../reference/configuration/)
- [Install with Helm](../install-with-helm/)
- [Provision a tenant](../provision-a-tenant/) — dropping a tenant is
  unrecoverable; dump first
- [Archiving](../../explanation/archiving/)
