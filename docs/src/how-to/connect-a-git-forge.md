# Connect a git forge

Connect a GitHub or GitLab repository so that a branch or pull request naming a
short code shows up on the work item, on the repository, and in the owning
team's **In flight** panel.

**Before you start:**

- **Kairos is reachable from the forge.** Webhooks are inbound. For a deployed
  ingress that is ordinary; for local development you need a tunnel
  (`cloudflared`, `ngrok`), because `localhost` is not routable from
  github.com.
- The repository is **registered** in Kairos, with a `github` or `gitlab` forge.
  If it is not:

  ```sh
  kairos repos create --forge github --name acme/payments-api \
    --repo-url https://github.com/acme/payments-api --team platform \
    --slug payments-api
  ```
- You are an **org admin**. Connecting and rotating are admin-only.
- Two configuration values are set:

  ```sh
  KAIROS_PUBLIC_URL=https://kairos.acme.example      # externally reachable base URL
  KAIROS_WEBHOOK_SIGNING_KEY=<a long random secret>  # every webhook secret derives from this
  ```

  Without them the connection endpoints answer `501` and the integration is
  simply off, not degraded. Neither has a dedicated Helm value; set them through
  the chart's `extraEnv`. Treat the signing key like any other deployment
  secret, and do not rotate it casually: **changing it invalidates every
  existing connection**, because per-connection secrets are derived from it
  rather than stored.

## 1. Connect the repository

```sh
curl -X POST https://<host>/api/forge-connections \
  -H "Authorization: Bearer <your-admin-token>" \
  -H 'Content-Type: application/json' \
  -d '{"repository":"payments-api"}'
```

The response carries the `webhook_url` and the `webhook_secret`. **The secret is
shown exactly once.** Paste it into the forge in step 2 before you lose it; if
you do, rotate (below) rather than reconnecting.

*Admin → Repositories → Connect webhook* in the GUI does the same thing and
shows the secret under the same one-time rule.

The repository's forge determines the webhook dialect. A repository registered
with forge `other` can own tasks but cannot be connected.

## 2. Add the webhook in the forge

**GitHub** — *Settings → Webhooks → Add webhook*:

| Field | Value |
|---|---|
| Payload URL | the returned `webhook_url` |
| Content type | `application/json` |
| Secret | the returned `webhook_secret` |
| Events | **Pull requests** and **Branch or tag creation** |

**GitLab** — *Settings → Webhooks → Add new webhook*:

| Field | Value |
|---|---|
| URL | the returned `webhook_url` |
| Secret token | the returned `webhook_secret` |
| Triggers | **Merge request events** and **Push events** |

## 3. Use it

Name the short code in the branch — the convention worth adopting team-wide —
or in the PR title or description. All three of these link:

```text
dylan/DEMO-T-0002-fix-auth
Fix login (DEMO-T-0002)
…a description mentioning DEMO-T-0002
```

The link appears when the branch is pushed or the PR is opened, and merging
updates it live. Activity on a repository rolls up to its owning team's In
flight panel even for work items that carry no team.

## Rotate the secret

Rotation changes **both** the secret and the delivery URL, because the secret
derives from the connection id. Update both fields in the forge, not just the
secret:

```sh
curl -X POST https://<host>/api/forge-connections/<id>/rotate \
  -H "Authorization: Bearer <admin>"
```

## Disconnect

```sh
curl -X DELETE https://<host>/api/forge-connections/<id> \
  -H "Authorization: Bearer <admin>"
```

The repository and its tasks stay; only the webhook connection goes. Delete the
webhook in the forge too, or it will keep delivering to a dead endpoint.

## Limits to expect

- **A missing link is not evidence of a broken webhook.** Deliveries naming
  short codes that do not exist here are accepted and ignored. Check the forge's
  delivery log before you go looking at Kairos.
- **Nothing is written back to the forge**, and **PR state never moves cards** —
  so do not wire a column to a merge. Move cards from Kairos. (Why:
  [Flight levels](../explanation/flight-levels.md#why-the-boards-are-configurable).)
- **Editing a short code out of a PR removes that link.** Re-add the code to get
  it back.
- **Replays are safe.** A redelivered or out-of-order event cannot regress
  state; a replayed "opened" will not un-merge a merged PR. Re-send deliveries
  freely when debugging.

## Related

- [Forge connections](../reference/rest/execution-scope.md) — the endpoints and
  their responses
- [Events](../reference/events.md)
- [Repositories as execution scope](../explanation/repositories-as-execution-scope.md)
- [Configuration](../reference/configuration.md#forge-webhooks)
