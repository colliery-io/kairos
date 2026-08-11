# Kairos

Kairos is an API-first, multi-tenant work management platform that evolves the
Metis Flight Levels methodology (Strategy → Initiative → Delivery) from a
single-repo, file-based system into a centralized platform for distributed
teams. All clients — CLI, GUI, and MCP servers for AI agents — consume the
same HTTP API, backed by PostgreSQL with schema-per-tenant isolation.

## Development

Development tasks are driven through [angreal](https://pypi.org/project/angreal/)
so that agents, humans, and CI all share one entry point:

```
angreal test unit          # tier 1: pure cargo unit tests, no services
angreal test integration   # tiers 2+3: real Postgres + Dex via docker compose
angreal services up|down   # manage the compose stack directly
```

See `.metis/adrs/KAIROS-A-0012.md` for the full testing and verification
strategy.

## CLI

`kairos` is the command-line client (KAIROS-A-0015). It talks to the same
HTTP API as the GUI and MCP clients.

### Install from release binaries

Every version tag (`v*`) publishes tarballs named
`kairos-<version>-<target>.tar.gz` on the GitHub release
(`.github/workflows/release.yml`). Pick your target and:

```sh
# macOS Apple Silicon
curl -fsSL -o kairos.tar.gz \
  https://github.com/<owner>/kairos/releases/download/v0.1.0/kairos-0.1.0-aarch64-apple-darwin.tar.gz

# macOS Intel
curl -fsSL -o kairos.tar.gz \
  https://github.com/<owner>/kairos/releases/download/v0.1.0/kairos-0.1.0-x86_64-apple-darwin.tar.gz

# Linux x86_64
curl -fsSL -o kairos.tar.gz \
  https://github.com/<owner>/kairos/releases/download/v0.1.0/kairos-0.1.0-x86_64-unknown-linux-gnu.tar.gz

# Linux arm64
curl -fsSL -o kairos.tar.gz \
  https://github.com/<owner>/kairos/releases/download/v0.1.0/kairos-0.1.0-aarch64-unknown-linux-gnu.tar.gz

tar -xzf kairos.tar.gz
install -m 0755 kairos ~/.local/bin/   # or any directory on your PATH
kairos --version
```

A `.sha256` checksum file is published next to each tarball.

### Install from source

```sh
cargo install --path crates/kairos-cli
```

No native Postgres libraries are required — the CLI links only the shared
`kairos-client` crate (reqwest + rustls).

### Quickstart

```sh
kairos login --url https://<tenant>.kairos.example   # device flow: prints a code + URL
kairos whoami         # confirm who you are authenticated as
kairos boards list    # see the boards you can work with
```

Credentials are cached per deployment; after `login`, commands default to
the only cached deployment (or take `--url` to pick one).

From there the subcommand nouns mirror the API: `strategies`, `initiatives`,
`tasks`, `documents`, and `adrs` (each with list/get/create/edit/transition/
delete verbs), plus `orgs`, `search`, `teams`, `streams`, `members`, and
`admin`. Every command supports `--json` for scripting; the default output
is a human-readable table. `kairos --help` lists everything.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | API or validation error (not found, forbidden, conflict, bad input, network failure) |
| 2 | Authentication error (not logged in, expired/rejected credentials) |

## Deployment

Kairos ships as a **single OCI image** — one binary that serves the GUI at
`/`, the API at `/api`, MCP at `/mcp`, SCIM at `/scim/v2`, and `/healthz`, all
from one process (KAIROS-A-0013 "one image, one binary"; the Leptos GUI is
embedded via `--features embed-web`, KAIROS-A-0015). The multi-stage
`Dockerfile` at the repo root builds the WASM bundle (`trunk`) and the release
binary, then ships a minimal `debian:bookworm-slim` runtime carrying only the
binary and `libpq` (the sole native dependency). The container applies pending
migrations on boot (KAIROS-T-0007) before it binds, so it is self-provisioning
against `DATABASE_URL`.

Images are published to `ghcr.io/colliery-io/kairos` with a **versioned tag
per release — never `latest`** for reference deployments (KAIROS-A-0013). The
`.github/workflows/release.yml` `image` job builds and pushes on every `v*`
tag (linux/amd64 for v1).

### Reference deployment (Docker Compose)

`deploy/docker-compose.yaml` is the production-shaped stack: **Caddy + the
Kairos image + Postgres 16**, with Postgres as the sole state (KAIROS-A-0013).

**Identity is external** (KAIROS-A-0016): Kairos bundles no IdP. Point
`OIDC_ISSUER_URL` / `OIDC_AUDIENCE` at the OIDC issuer you already run (Okta,
Entra ID, Auth0, Keycloak, Dex, …) and register Kairos there as an OIDC app
(plus an optional SCIM app for user/group lifecycle). The dev/test stack in
`.angreal/docker-compose.yaml` (Dex as a fixture, `angreal services up`) is a
separate, dev-only profile — *example, not product*.

```sh
cp deploy/.env.example deploy/.env    # then edit: image tag, DB password,
                                      # OIDC issuer/audience, tenant addressing
docker compose -f deploy/docker-compose.yaml --env-file deploy/.env up -d
```

Configuration is 12-factor env vars only (KAIROS-A-0013) — see
`deploy/.env.example` for the full surface. Back up the single Postgres volume
with `pg_dump` (all `org_*` schemas plus `public`); there are no server-local
files to back up.

> **Health/observability:** `/healthz` (liveness — the image's `HEALTHCHECK`
> and the compose readiness gate use it), `/readyz` (DB + pending-migration
> readiness), and `/metrics` (Prometheus) are all live per KAIROS-A-0013
> (KAIROS-T-0049). Point orchestrator liveness probes at `/healthz`, readiness
> probes at `/readyz`, and scrape `/metrics`.

### Google Workspace as your IdP (KAIROS-T-0054)

Google is a supported OIDC issuer. Two Google-specific settings are needed,
because Google differs from Dex/Keycloak in two ways:

- Google's **access token is opaque** (a `ya29.…` string, not a JWT) so it
  cannot be validated by Kairos's local JWKS. Google's **ID token** *is* a
  validatable RS256 JWT, so set `KAIROS_API_BEARER=id_token` and the GUI/CLI
  send the ID token as the API bearer (KAIROS-T-0054).
- Google has **no public-SPA client type**: a hosted web app with an
  `https://…/callback` redirect must be a *confidential* "Web application"
  client, and Google's token endpoint rejects the exchange without the
  **client secret** even under PKCE. Set `KAIROS_WEB_CLIENT_SECRET` and Kairos's
  server-side token relay presents it (the secret never reaches the browser,
  KAIROS-T-0056).

Use a **Google Workspace** (corporate) account, not consumer Gmail: it gives
you domain restriction and SCIM.

1. **Google Cloud console → APIs & Services → Credentials → Create OAuth client
   ID → Web application.** Add redirect URI `https://<your-kairos-host>/callback`.
   On the OAuth consent screen choose **Internal** (your Workspace org only).
   Note the **Client ID** and **Client secret**.
2. **Configure Kairos** (env, Compose `.env`, or Helm `config.*`):
   ```sh
   OIDC_ISSUER_URL=https://accounts.google.com
   OIDC_AUDIENCE=<your-google-client-id>       # Google's aud = the client id
   KAIROS_WEB_CLIENT_ID=<your-google-client-id>
   KAIROS_API_BEARER=id_token                  # ← opaque access token → send id_token
   KAIROS_WEB_CLIENT_SECRET=<your-google-client-secret>   # ← confidential Web client
   ```
3. **Restrict to your domain.** Google returns an `hd` (hosted-domain) claim;
   set the OAuth app to Internal and, if you expose login broadly, pass
   `hd=<yourcompany.com>` so only corporate accounts can complete login.
4. **Scopes:** `openid email profile` (already requested by the GUI/CLI). The
   ID token then carries `email`, which drives Kairos's JIT user provisioning.
5. **CLI:** `kairos login --url https://<host> --bearer id_token`.
6. **(Optional) SCIM:** Google Workspace can auto-provision users/groups into
   Kairos's `/scim/v2` endpoint — set that up in the Workspace admin console.

> **Multi-client note:** Google issues a distinct `aud` (the client id) per
> OAuth client. A single `OIDC_AUDIENCE` covers a GUI-only deployment. To run
> the GUI **and** CLI **and** service accounts against Google together, set
> `OIDC_AUDIENCE` to a **comma-separated allow-list** of the client ids
> (KAIROS-T-0055) — a token matching any listed audience validates:
>
> ```sh
> OIDC_AUDIENCE=<gui-client-id>,<cli-client-id>
> ```
>
> Helm accepts the same as a YAML list under `config.oidc.audience`. This is a
> strict allow-list — there is no "any audience" mode.

## Service accounts & API keys

For **non-interactive** access — CI jobs, scripts, and agents — Kairos issues
native **API keys** instead of bending your IdP's headless-auth flow (KAIROS-A-0017).
Humans authenticate with your OIDC issuer (above); machines authenticate with a key.

A **service account** is a first-class, org-scoped machine principal (it is not a
person's login). It holds ordinary board capabilities — grant it *only* what its
job needs — and authenticates with an opaque key `kairos_sk_<org>_<secret>` that
is **hashed at rest, revocable, and optionally time-boxed**. A key resolves to its
own tenant, so it works on `/api`, `/mcp`, and `/ws` exactly like a member, subject
to the same ABAC. Management is **org-admin only**.

**1. Create a service account** (CLI or API):

```sh
kairos service-accounts create --name ci-deploy
# → Created service account ci-deploy (a1b2c3…).
```
```sh
curl -X POST https://<host>/api/service-accounts \
  -H "Authorization: Bearer <your-admin-token>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"ci-deploy"}'
```

**2. Grant it least-privilege capabilities.** A service account id is an ordinary
`user_id`, so use the normal board-capability grants (e.g. give `ci-deploy` just
`transition_items` on the delivery board — not `manage_*`). See `kairos boards
--help` for the grant commands.

**3. Mint a key — shown once:**

```sh
kairos keys create --service-account a1b2c3… --name gha-main --expires-at 2027-01-01T00:00:00Z
#
#     kairos_sk_acme_9f8e7d6c5b4a…            ← copy it now
#
# Store it now — it will NOT be shown again.
```

**4. Use the key** — as a `Bearer` token, no tenant header needed (the key carries
its own tenant):

```sh
curl https://<host>/api/whoami -H "Authorization: Bearer kairos_sk_acme_9f8e7d6c5b4a…"
```
```sh
kairos login --url https://<host> --bearer id_token   # humans; keys need no login
```
For **MCP** (agents), point the client's bearer at the key — the whole `/mcp`
tool surface is available non-interactively, so a CI agent or the skills plugin
can drive Kairos without a browser OAuth dance.

**Rotate, revoke, expire:**

```sh
kairos keys list   --service-account a1b2c3…                 # prefixes only; never the secret
kairos keys revoke k1d2… --service-account a1b2c3… --confirm # immediate
kairos service-accounts delete a1b2c3… --confirm            # removes the account + all its keys
```

**Security notes:** keys are stored only as a SHA-256 hash (the raw key is shown
exactly once); revocation and expiry take effect immediately on the next request;
service accounts can never be org admins or deployment admins; rotate by minting a
new key and revoking the old one; never commit a key to source control.

## CI

`.github/workflows/ci.yml` runs the KAIROS-A-0012 verification gates on every
push to `main` and every pull request, in one sequential job named `ci`:

1. `cargo fmt --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `angreal test unit`
4. `angreal test integration` (the task brings the compose stack up and down
   itself; `ubuntu-latest` provides docker + compose)

### Activating CI (one-time, once a remote exists)

This repository does not have a GitHub remote yet. The workflow is committed
and validated but dormant until:

1. Create the GitHub repository and add it as a remote:
   `git remote add origin git@github.com:<owner>/kairos.git`
2. Push: `git push -u origin main` — the `ci` workflow runs on that push.
3. Enable branch protection on `main` (Settings → Branches → Add rule):
   require status checks to pass before merging and select the **`ci`** check.
