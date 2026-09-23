# Configure an OIDC issuer

Point Kairos at the identity provider you already run, so the GUI and the
`kairos` CLI log people in against it.

**Before you start:** you can register OAuth clients at the issuer, and you know
the deployment's externally reachable base URL — call it `https://<host>` below.
Kairos ships no identity provider and validates tokens with the issuer's own
JWKS, so nothing here is optional.

Kairos needs **two** OAuth clients registered, because the browser and the CLI
use different grants.

## 1. Register the GUI client

| Setting | Value |
|---|---|
| Client id | `kairos-web`, or any id you then set as `KAIROS_WEB_CLIENT_ID` |
| Client type | Public, Authorization Code with PKCE |
| Redirect URI | `https://<host>/callback` |
| Scopes | `openid email profile` |

Register the redirect URI **exactly**. Most issuers, Dex included, exact-match
it — a trailing slash or a missing port is a failed login, not a warning.

If you also run the server locally on several ports, enumerate every one of
them (`http://localhost:41080/callback`, `…41081/callback`, …). Do not work
around this by registering no redirect URI at all: in Dex 2.43.1 a public
client with none registered accepts any host at `authorize`.

## 2. Register the CLI client

| Setting | Value |
|---|---|
| Client id | `kairos-cli` (the CLI's default; override per-invocation with `--client-id`) |
| Client type | Public |
| Grant | Device Authorization Grant |

## 3. Set the server variables

```sh
OIDC_ISSUER_URL=https://idp.example.com/    # trailing slash is stripped
OIDC_AUDIENCE=kairos                        # the `aud` tokens must carry
KAIROS_DEPLOYMENT_ADMINS=<your-oidc-sub>    # who may provision tenants
```

Add `KAIROS_WEB_CLIENT_ID` only if the GUI client id is not `kairos-web`.

Under Helm these are `config.oidc.issuerUrl`, `config.oidc.audience`,
`config.webClientId` and `config.deploymentAdmins`; the chart will not render
without the first two. Full types and defaults are in
[Configuration → Identity and the browser client](../reference/configuration.md#identity-and-the-browser-client).

Then apply whichever of these your issuer forces:

- **Its access token is opaque** — Google and Google Workspace issue a `ya29.…`
  string, not a JWT, so local JWKS cannot validate it. Set
  `KAIROS_API_BEARER=id_token`, and tell CLI users to
  `kairos login --url https://<host> --bearer id_token`.
- **It has no public client type for hosted web apps** — Google's "Web
  application" client rejects the code exchange without a secret even under
  PKCE. Set `KAIROS_WEB_CLIENT_SECRET` (or the chart's
  `config.webClientSecretExistingSecret`).
- **It mints a distinct `aud` per OAuth client** — again Google Workspace. Make
  `OIDC_AUDIENCE` a comma-separated allow-list of the client ids, or a YAML
  list under `config.oidc.audience`. A token matching any listed audience
  validates. It is a strict allow-list; there is no "any audience" mode.

  ```sh
  OIDC_AUDIENCE=<gui-client-id>,<cli-client-id>
  ```

- **You want the issuer to manage user and group lifecycle** — configure a SCIM
  app against `/scim/v2`. See [SCIM](../reference/scim.md). Without it, users
  are provisioned just-in-time at first login, which is adequate for small
  organizations.
- **Google Workspace specifically** — use a Workspace account, not consumer
  Gmail, set the OAuth consent screen to **Internal**, and pass
  `hd=<yourcompany.com>` if you expose login broadly. `OIDC_AUDIENCE` is the
  client id.

## 4. Restart and verify

Restart the server (`helm upgrade`, or recreate the container) and check the
resolved client configuration the GUI will use:

```sh
curl https://<host>/api/config
```

([`GET /api/config`](../reference/rest/the-deployment-itself.md#get-apiconfig)
is unauthenticated and returns the issuer, client id and bearer mode the
browser will use.)

Then complete a real login in the browser, and one from the CLI:

```sh
kairos login --url https://<host>
kairos whoami
```

`whoami` printing your email and organization means the token validated, the
audience matched, and the user was provisioned. If login fails at the issuer,
the redirect URI is the first thing to re-check; if it fails at Kairos with a
401, the mismatch is `OIDC_AUDIENCE`.

## Two things that will not work

**Do not plan machine access around a `client_credentials` grant.** Kairos
issues its own API keys instead, so an issuer that lacks that grant — Dex does —
costs you nothing. See
[Give an agent machine access](give-an-agent-machine-access.md).

**An issuer with no password login form cannot be used with Kairos's acceptance
suite.** `angreal test uat` logs its personas in by driving a password form, so
it needs Dex, or Keycloak with direct grants enabled. Google Workspace has none
and is **not supported by that pass** — the deployment itself works; only the
acceptance journeys cannot be run against it.

Dex is the reference issuer, and `.angreal/dex/config.yaml` in the repository
configures both clients above (plus a confidential one) — copy from it.

## Related

- [Configuration](../reference/configuration.md)
- [Install with Helm](install-with-helm.md)
- [Provision a tenant](provision-a-tenant.md)
- [SCIM](../reference/scim.md)
