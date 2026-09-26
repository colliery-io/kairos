# Serve Kairos on a tailnet

This guide shows how to run the compose deployment so that only your
[Tailscale](https://tailscale.com) tailnet can reach it. The result is
`https://kairos.<tailnet>.ts.net/`, with a valid certificate, and no port open on
the host or the local network.

This is the right shape for a small team, a home lab, or one person. Nothing is
public. Every device that reaches Kairos is a device you admitted to the tailnet.

## Before you start

You need:

- A tailnet, and admin access to its console.
- Docker on the host, with Docker Compose.
- A clone of the repository, or a copy of the `deploy/` directory.

Identity: a tailnet deployment can use your OIDC issuer, or local accounts, or both.
This guide uses local accounts, because a tailnet has no wildcard DNS and often no
identity provider. See [Use local accounts](use-local-accounts.md) for the first
admin.

## How it works

The stack adds two containers when you enable the `tailscale` profile:

- `tailscale`: a Tailscale node that shares the Kairos container's network
  namespace. It terminates HTTPS with a certificate for the node's name and proxies
  to Kairos on the loopback interface.
- `tailscale-http-redirect`: a small nginx that answers plain HTTP with a
  redirect to HTTPS. `tailscale serve` does not tell Kairos which port a request used. So port 80
  must land somewhere that is not Kairos.

The Kairos port is never published. Caddy does not start. `tailscale serve` is a
tailnet-only proxy; the stack never enables Funnel, which would make the
deployment public.

## Turn on HTTPS certificates for the tailnet

**Step 1.** Open the admin console. Go to **DNS**.

**Step 2.** Make sure **MagicDNS** is on.

**Step 3.** Under **HTTPS Certificates**, click **Enable HTTPS**.

Without this step the serve configuration matches no hostname, and the name
answers nothing. The node's name becomes `kairos.<tailnet>.ts.net`, where
`<tailnet>` is the name the console shows you.

## Make an auth key

**Step 1.** In the admin console, go to **Settings**, then **Keys**.

**Step 2.** Click **Generate auth key**. One use is enough. The node keeps its
identity in a Docker volume, so it does not use the key again.

**Step 3.** Copy the key. It starts with `tskey-auth-`.

## Configure the deployment

**Step 1.** Copy the example environment file:

```sh
cp deploy/.env.example deploy/.env
```

**Step 2.** In `deploy/.env`, select the tailnet front door and give it the key:

```
COMPOSE_PROFILES=tailscale
TAILSCALE_AUTHKEY=tskey-auth-...
TAILSCALE_HOSTNAME=kairos
```

`TAILSCALE_HOSTNAME` is the first label of the address you will browse to. Leave
`KAIROS_SITE_ADDRESS` empty. The `caddy` profile is off, so nothing reads it.

**Step 3.** Set the tenant. A tailnet has no wildcard DNS, so pin one tenant
instead of a base domain:

```
KAIROS_BASE_DOMAIN=
KAIROS_SINGLE_TENANT=acme
```

**Step 4.** Turn on local accounts and the first-boot admin, as
[Use local accounts](use-local-accounts.md) describes:

```
KAIROS_LOCAL_AUTH=true
KAIROS_BOOTSTRAP_ADMIN=you@example.com
KAIROS_BOOTSTRAP_PASSWORD_HASH=<hash>
OIDC_ISSUER_URL=
OIDC_AUDIENCE=
```

To make the hash without a running deployment:

```sh
docker run --rm ghcr.io/colliery-io/kairos:0.4.0 hash-password
```

**Step 5.** Set `POSTGRES_PASSWORD` to a long random value.

## Start the deployment

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env up -d
```

**Step 1.** Watch the node join:

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env logs -f tailscale
```

Wait for a line that contains `Success`. The first HTTPS request can take some
seconds while the node gets its certificate.

**Step 2.** Check the serve configuration:

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env exec tailscale tailscale serve status
```

Both listeners must say `(tailnet only)`:

```
https://kairos.<tailnet>.ts.net (tailnet only)
|-- / proxy http://127.0.0.1:8080

http://kairos.<tailnet>.ts.net (tailnet only)
|-- / proxy http://127.0.0.1:8081
```

If a line says `(Funnel on)`, stop. Something outside this stack enabled Funnel,
and the deployment is public. Run `tailscale funnel off` in the container.

**Step 3.** In the admin console, open the new node and **Disable key expiry**.
Without this, the key expires, the node logs out, and the address stops
answering.

**Step 4.** From a device on the tailnet, open `https://kairos.<tailnet>.ts.net/`.
Sign in as the first admin, then remove the two bootstrap variables from
`deploy/.env`.

## Provision the tenant

The tenant that `KAIROS_SINGLE_TENANT` names does not exist yet. Create it:

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env exec kairos kairos-server create-tenant --slug acme --name "Acme Inc"
```

Then make the first admin a member of it, as
[Provision a tenant](provision-a-tenant.md) describes.

## After you recreate the Kairos container

The two sidecars share the Kairos container's network namespace. When you
recreate that container, for example to change `KAIROS_VERSION`, the sidecars
keep the old namespace. The address stops answering. Recreate them
too:

```sh
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env up -d --force-recreate --no-deps tailscale tailscale-http-redirect
```

`docker compose up -d` after a change to `deploy/.env` does this for you, because
compose sees that the dependency changed.

## What this deployment does not do

- It does not serve subdomain tenants. MagicDNS has no wildcard, so
  `acme.kairos.<tailnet>.ts.net` resolves to nothing. Use `KAIROS_SINGLE_TENANT`.
- It does not reach the public internet. A person who is not on the tailnet gets
  no answer at all, not a login page.
- It does not replace backups. The Postgres volume is the data; the
  `tailscale_state` volume is the node's identity. Back up both. See
  [Back up and restore](back-up-and-restore.md).
