# Use local accounts

This guide shows how to let people sign in with an email and a password, without an
identity provider.

Read [Choosing how people log in](../explanation/choosing-how-people-log-in.md) first
if you have not decided yet. Local accounts make Kairos responsible for your users'
passwords. That is a decision, not a setting.

Local accounts are additive. A deployment can have an OIDC issuer and local accounts
at the same time.

## Before you start

You need:

- A Kairos deployment, or the configuration for one you are about to start.
- Access to the deployment's environment variables.
- For the recovery procedure: shell access to the container.

## Turn on local accounts

Set one variable:

```
KAIROS_LOCAL_AUTH=true
```

With this variable off, `POST /api/login` does not exist. The route is absent, not
disabled.

In the Helm chart, set `config.localAuth.enabled`:

```yaml
config:
  localAuth:
    enabled: true
```

## Create the first admin

A new deployment has no users. Nobody can sign in, and nobody can create an account.
The first-boot admin solves this.

**Step 1.** Make a password hash. Run this command anywhere. It needs no database:

```bash
kairos-server hash-password
```

Type the password at the prompt. The command prints one line, which is the hash.

**Step 2.** Give the deployment the email and the hash:

```
KAIROS_BOOTSTRAP_ADMIN=you@example.com
KAIROS_BOOTSTRAP_PASSWORD_HASH=<the line from step 1>
```

In the Helm chart, put them in a values **file**. Do not use `--set`. A hash contains
commas, and `--set` reads a comma as a separator:

```yaml
config:
  localAuth:
    enabled: true
    bootstrapAdmin: you@example.com
    bootstrapPasswordHash: "$argon2id$v=19$m=19456,t=2,p=1$..."
```

**Step 3.** Start the deployment. Read the log. One of these lines appears:

- `created the first-boot admin` — the account exists.
- `this deployment already has users` — the bootstrap did nothing, as designed.

**Step 4.** Sign in at `/login` with the email and the password.

**Step 5.** Remove both variables.

The bootstrap is single-use. It runs only on a boot that finds no users, and it does
nothing after that, even with the same email. The variables are now a credential in
your configuration that does nothing. Remove them.

The bootstrap email is also a deployment admin. A new deployment has no organization,
and only a deployment admin can create one.

## Create an organization

The first admin has no organization to work in. Create one:

```bash
kairos admin tenants create --slug acme --name "Acme Inc"
```

The caller becomes the organization's first admin.

## Create an account for a colleague

An org admin creates local accounts. There is no self-service sign-up.

```bash
curl -X POST https://kairos.example.com/api/local-accounts \
  -H "authorization: Bearer $TOKEN" \
  -H "content-type: application/json" \
  -d '{
        "email": "colleague@example.com",
        "display_name": "A Colleague",
        "password": "a-password-of-at-least-12-characters",
        "role": "member"
      }'
```

The endpoint does two things. It creates the account, and it makes the person a
member of your organization. An account that belongs to no organization can sign in
and then see nothing.

Passwords must be 12 characters or longer. There are no other rules.

If the email already signed in through your issuer, the endpoint adds a password to
that person. It does not make a second person. The response field `created` is
`false` when this happens.

## Reset a password

An org admin resets a password:

```bash
curl -X PUT https://kairos.example.com/api/local-accounts/$USER_ID/password \
  -H "authorization: Bearer $TOKEN" \
  -H "content-type: application/json" \
  -d '{"password": "a-new-password-of-12-or-more"}'
```

This ends every session that person holds. A reset after a suspected compromise must
not leave the attacker signed in.

## End a person's sessions

To sign someone out everywhere without changing their password:

```bash
curl -X DELETE https://kairos.example.com/api/local-accounts/$USER_ID/sessions \
  -H "authorization: Bearer $TOKEN"
```

To see what they hold first:

```bash
curl https://kairos.example.com/api/local-accounts/$USER_ID/sessions \
  -H "authorization: Bearer $TOKEN"
```

Each session shows `active`, `created_at`, `last_used_at` and `expires_at`. It never
shows the token.

## Recover when nobody can sign in

Use this procedure when the only admin has forgotten the password. It needs no login.

**Step 1.** Get a shell where `DATABASE_URL` is set.

On Kubernetes:

```bash
kubectl exec -it deploy/kairos -- sh
```

With Docker Compose, from the `deploy` directory:

```bash
docker compose run --rm kairos sh
```

**Step 2.** Set the password:

```bash
kairos-server set-password --email admin@example.com
```

Type the new password at the prompt. Do not use `--password` on a shared machine. An
argument is visible in the process list.

The command reports how many sessions it ended.

The command does not create accounts. It changes the password of an account that
already exists. For an empty deployment, use the first-boot admin instead.

## Turn local accounts off again

Set `KAIROS_LOCAL_AUTH=false` and restart. The login endpoint disappears. Session
bearers stop working immediately.

The password hashes stay in the database. To remove one person's password, an org
admin has no endpoint for it today. Use the database, or leave the hash in place: it
cannot be used while local auth is off.

Make sure an issuer works before you do this. A deployment with no issuer and no
local accounts refuses to start, which is deliberate — it has no way to let anybody
in.

## Related

- [Choosing how people log in](../explanation/choosing-how-people-log-in.md) — which of the three to use
- [Configure an OIDC issuer](../how-to/configure-an-oidc-issuer.md) — the other path
- [Configuration](../reference/configuration.md) — every variable on this page
- [CLI](../reference/cli.md#operator-subcommands-kairos-server) — `set-password` and `hash-password`
