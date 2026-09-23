# Give an agent machine access

Get a non-interactive client — an agent, a CI pipeline, a script — talking to
Kairos without a person or a browser in the loop, and able to rotate its
credential without downtime.

**Before you start:** you are an **org admin**. Creating service accounts,
minting keys and revoking them are all org-admin only. Do not plan around your
IdP's `client_credentials` grant; Kairos issues its own API keys instead, and
Dex — the reference issuer — does not support that grant at all.

A service account's id is an ordinary `user_id`, so team membership and
capability grants work on it exactly as they do for a person.

## 1. Create the service account

```sh
kairos service-accounts create --name ci-deploy
# → Created service account ci-deploy (a1b2c3…).
```

Or `POST /api/service-accounts` with `{"name":"ci-deploy"}`.

Keep the id. Every key command needs it.

## 2. Give it somewhere to work, and only what it needs

Put it on the team whose board it works:

```sh
kairos teams members add <team-id> --user a1b2c3…
```

Then grant it the narrowest capability that does its job — `transition_items` on
one delivery board for something that only moves cards, never a `manage_*`.
Grants are per board, through
[`POST /api/boards/{id}/members`](../reference/rest/boards-and-teams.md) or the
GUI's board members panel; **the CLI does not grant capabilities.** The
vocabulary is in [Capabilities](../reference/capabilities.md).

A service account **can never be an org admin or a deployment admin.** There is
no escalation path here, which is also why you cannot shortcut the grants.

## 3. Mint a key

```sh
kairos keys create --service-account a1b2c3… --name gha-main \
  --expires-at 2027-01-01T00:00:00Z
#
#     kairos_sk_acme_9f8e7d6c5b4a…            ← copy it now
#
# Store it now — it will NOT be shown again.
```

**The raw key is shown exactly once.** Only a SHA-256 hash is stored, so there
is no "show me that key again" — `kairos keys list` returns prefixes and
nothing else. Put it straight into the secret store the client reads from. If
you lose it, mint another and revoke the old one; that is step 5 anyway.

`--expires-at` is optional and worth setting. Expiry takes effect immediately on
the next request, with no sweeper involved.

## 4. Use it

The key goes in an ordinary `Authorization: Bearer` header, and **carries its own
tenant** — no `X-Tenant`, no login, no token refresh:

```sh
curl https://<host>/api/whoami \
  -H "Authorization: Bearer kairos_sk_acme_9f8e7d6c5b4a…"
```

It works identically on `/api`, `/mcp` and `/ws`, subject to the same access
control as a member. For an agent, point the MCP client's bearer at it and the
whole tool surface is available with no browser OAuth dance — see
[Connect over MCP](connect-over-mcp.md).

`whoami` naming the service account is the check that it is wired up. Writes it
makes are attributed to it in the activity feed as a principal in its own right,
which is what makes "who deployed that?" answerable.

## 5. Rotate without downtime

**Two keys can be live at once**, and that is the whole mechanism:

```sh
kairos keys create --service-account a1b2c3… --name rotation   # both now work
# …switch the client over to the new key, and confirm it is using it…
kairos keys revoke <old-key-id> --service-account a1b2c3… --confirm
```

Do it in that order. Revocation lands immediately on the next request, so
revoking first is an outage; minting first is not. The revocation is surgical —
the account's other keys keep working, so responding to a leak does not take the
pipeline down.

## 6. Retire the account

```sh
kairos service-accounts delete a1b2c3… --confirm
```

**This kills every key it holds**, immediately. Deleting the account is
therefore the blunt instrument for a compromise you cannot attribute to one key;
for a single leaked key, revoke that key instead.

Never commit a key to source control — add the `kairos_sk_` prefix to your
secret scanner.

## Related

- [CLI → Machine access](../reference/cli.md#machine-access)
- [Machine access endpoints](../reference/rest/machine-access.md)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Connect over MCP](connect-over-mcp.md)
- [Configure an OIDC issuer](configure-an-oidc-issuer.md) — for the humans
