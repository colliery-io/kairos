# Back up a deployment, and restore it

Take a backup you can actually restore from, and restore from it.

**Before you start:**

- `psql` / `pg_dump` access to the deployment's PostgreSQL, at a version at least
  as new as the server's.
- Back up exactly two things, and nothing else: **the database** — `public` plus
  every `org_<slug>` schema, in one dump — and **the deployment secrets that live
  outside it**: `DATABASE_URL`, `KAIROS_WEBHOOK_SIGNING_KEY`,
  `KAIROS_WEB_CLIENT_SECRET`, plus database roles if you manage them in the
  cluster. There is nothing on the pods to capture: the image is stateless and
  the chart mounts no volume.
- Keep `KAIROS_WEBHOOK_SIGNING_KEY` with the database password. Losing it is not
  data loss but it is an outage — every forge connection has to be rotated and
  re-pasted in the forge.

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
On boot the server applies any pending **public** migrations before it binds —
that is how a dump from an older release comes forward, and it is why there is
no migration Job to run.

### Run the tenant migrations

```sh
kubectl exec deploy/kairos -- kairos-server migrate-tenants
```

Boot migrates the public schema only, and `/readyz` checks only the public
schema. A restored database whose `org_*` schemas are behind the binary will
report **ready** and then fail on tenant queries. Run this every time, not only
when you think it is needed.

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

## Size for unbounded growth

**Nothing prunes anything in 0.1.0.** The five retention variables are inert —
setting `KAIROS_RETENTION_MODE`, `KAIROS_ARCHIVE_TARGET` or any of the windows
has no effect, and configuring an archive target will not reclaim a byte
([Configuration → Retention: recognised but
inert](../reference/configuration.md#retention--recognised-but-inert)).
Archiving work does not reclaim anything either — it is a soft delete
([Archiving](../explanation/archiving.md)).

So plan for it: `item_history` grows by a row per edit and `activity_log` by a
row per write, forever, and your dumps grow with them. Size the volume and the
backup window against the deployment's lifetime to date, and alarm on the growth
rate rather than on a threshold.

## Related

- [Configuration](../reference/configuration.md)
- [Install with Helm](install-with-helm.md)
- [Provision a tenant](provision-a-tenant.md) — dropping a tenant is
  unrecoverable; dump first
- [Archiving](../explanation/archiving.md)
