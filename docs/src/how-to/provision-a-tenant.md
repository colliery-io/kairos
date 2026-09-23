# Provision a tenant

Create a new organization on a running deployment and hand it to its first
admin. ("Tenant" and "organization" are the same thing seen from two sides —
[Glossary](../reference/glossary.md).)

**Before you start:**

- Your OIDC `sub` is listed in `KAIROS_DEPLOYMENT_ADMINS` (Helm:
  `config.deploymentAdmins`). With that variable empty, the admin routes return
  403 to everyone, including you.
- **The intended initial org admin has logged in at least once.** Users are
  provisioned just-in-time at first login, so a `sub` that has never
  authenticated does not exist yet and provisioning is refused.
- You are logged in: `kairos login --url https://<host>`.

## 1. Confirm you hold the deployment-admin routes

```sh
kairos admin tenants list
```

A list — even an empty one — means the routes are open to you. A 403 means your
`sub` is not in `KAIROS_DEPLOYMENT_ADMINS`; fix that first, since nothing below
will work.

## 2. Create the tenant

```sh
kairos admin tenants create --slug acme --name "Acme Inc"
```

Or, to name a different initial admin than yourself:

```sh
kairos admin tenants create --slug acme --name "Acme Inc" \
  --initial-admin <their-oidc-sub>
```

The slug must match `^[a-z][a-z0-9_-]{1,62}$` and is permanent — it is the
schema name and, under subdomain tenancy, the hostname. The response reports
what was built: the `org_acme` schema, the tenant migrations applied in it, the
default boards, and the templates and metadata definitions copied in. It all
happens in one transaction, so a failed create leaves nothing behind and is safe
to retry.

Three refusals, and what to do about each
([Errors](../reference/errors.md) has the codes):

| Response | Do |
|---|---|
| `409` | The slug is taken, and slugs are not reused. Choose another. |
| `422`, naming the slug | Make the slug match the pattern above. |
| `422`, naming an `external_id` | Have the initial admin authenticate once, then retry. |

The same operation is available as
[`POST /api/admin/tenants`](../reference/rest/the-deployment-itself.md#post-apiadmintenants),
and —
where you have shell access to the deployment rather than a token — as a server
subcommand:

```sh
kubectl exec deploy/kairos -- kairos-server create-tenant --slug acme --name "Acme Inc"
```

The subcommand provisions the schema but seeds **no** org-admin membership. Use
it for a local or dev stack (`angreal db create-tenant --slug acme` wraps it),
not to hand a tenant to someone.

## 3. Reach the tenant

How a request resolves to `acme` depends on how the deployment was configured —
see [Configuration → Tenant resolution](../reference/configuration.md#tenant-resolution):

- **`KAIROS_BASE_DOMAIN` set:** browse `https://acme.<base-domain>`. Confirm
  wildcard DNS resolves and the certificate covers the new host before handing
  the URL over.
- **`KAIROS_SINGLE_TENANT` set:** that one tenant is pinned and subdomain and
  header resolution are skipped entirely. A second tenant provisioned on such a
  deployment is unreachable until the variable changes.
- **Neither set:** only the `X-Tenant: acme` header resolves a tenant. The CLI
  sends it for you when you pass `--tenant acme` to `kairos login`.

Verify as the new org admin:

```sh
kairos login --url https://<host> --tenant acme
kairos whoami        # org: acme (admin)
kairos boards list   # the default boards
```

## 4. Add its members

Any of three, and they compose:

- **SCIM push from the IdP** — the option that keeps joiners and leavers correct
  without anyone remembering. See [SCIM](../reference/scim.md).
- **`kairos members add --email …`** — explicit, by the org admin. The person
  must have logged in at least once, same as the initial admin in step 2.
- **Just-in-time at first login** — anyone your issuer authenticates becomes a
  member on arrival. Adequate for a small organization, and the reason the
  initial admin had to log in before step 2.

From here the organization shapes its own boards and teams:
[Set up a board](set-up-a-board.md).

## Removing a tenant

```sh
kairos admin tenants delete acme --confirm
```

This drops the `org_acme` schema `CASCADE` and the organization row. It is
unrecoverable, and `--confirm` (`?confirm=true` on the API) is required —
without it you get a 422 `CONFIRMATION_REQUIRED` rather than a deletion. Take a
dump first: [Back up and restore](back-up-and-restore.md).

## Related

- [CLI → Deployment administration](../reference/cli.md#deployment-administration)
- [Configuration](../reference/configuration.md)
- [Install with Helm](install-with-helm.md)
- [Teams and boards](../explanation/teams-and-boards.md) — what the default
  boards are for
