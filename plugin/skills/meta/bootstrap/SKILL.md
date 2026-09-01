---
name: bootstrap
description: Wire this repo and engineer to a Kairos deployment — MCP endpoint config, auth check, board discovery, and .claude/kairos.local.md.
argument-hint: "[deployment URL]"
disable-model-invocation: true
---

# Bootstrap

Connect the current repo to a Kairos deployment so every other kairos skill (and the SessionStart hook) has a board to work against. Idempotent: re-running updates the existing configuration in place.

## 1. Gather the deployment

On a re-run, recover what is already known before asking: the `kairos` entry in the repo's `.mcp.json` (URL, `X-Tenant` header) and the frontmatter of `.claude/kairos.local.md`. Ask the user only for what is missing or being changed (or take it from the arguments):

- **Deployment URL** — e.g. `https://acme.kairos.example` or `http://localhost:41080`. Strip any trailing slash. The MCP endpoint is `<deployment-url>/mcp`.
- **Tenant slug (optional, dev setups only)** — production deployments resolve the tenant from the URL's subdomain, so most users skip this. When the URL has no tenant subdomain (localhost, IP, plain host), ask for the tenant slug; it is sent as an `X-Tenant` header on every request.

Sanity-check reachability before writing anything: fetch `<deployment-url>/healthz` (expect `ok`) and `<deployment-url>/.well-known/oauth-protected-resource/mcp` (RFC 9728 metadata naming the deployment's authorization server). If unreachable, show the user what failed, confirm the URL with them, and stop — do not write config for a URL that doesn't answer.

## 2. Write the MCP config

Two equivalent paths — propose the project `.mcp.json` (shareable and reviewable); use `claude mcp add` instead if the user prefers per-user config:

**Project `.mcp.json`** (create or update the `kairos` server entry in the repo root; preserve other servers):

```json
{
  "mcpServers": {
    "kairos": {
      "type": "http",
      "url": "https://acme.kairos.example/mcp"
    }
  }
}
```

With a dev tenant slug, add: `"headers": { "X-Tenant": "<slug>" }`.

**Or `claude mcp add`** (per-user, no file in the repo):

```
claude mcp add --transport http kairos https://acme.kairos.example/mcp
```

With a dev tenant slug, append: `--header "X-Tenant: <slug>"`.

On re-run, update the existing `kairos` entry's URL/headers rather than adding a new server.

## 3. Connect and discover

Authentication is client-driven OAuth: the MCP client opens the browser flow the first time it connects — the skill never handles tokens. If the `kairos` MCP tools are not available in this session yet (config just written), tell the user to run `/mcp` to connect and authenticate, then re-run `/kairos:bootstrap` to finish; the re-run picks up here.

For **non-interactive** contexts (CI, headless agents) where no browser is available, an org admin can mint a **service-account API key** (`kairos_sk_…`, KAIROS-A-0017) and set it as the MCP client's bearer instead of the OAuth flow — the same tool surface, no browser. See the README's "Service accounts & API keys".

Once connected:

1. `whoami` — confirms auth; gives the user's name/email, org, teams, and the boards where they hold capabilities.
2. `my_boards` — boards grouped by level; the user's delivery boards include column names and per-column item counts.

From the results pick, confirming with the user whenever there is more than one candidate:

- **delivery stream / team board** — the delivery-level board the user works from
- **initiative board** — the default initiative-level board new initiatives land on

If auth is declined or fails, or there are no boards yet (fresh tenant, no memberships): keep the step 2 config, proceed to step 4 regardless, and record in the prose section what is missing and what unblocks it (authenticate via `/mcp`; ask an org admin for team/board membership). Do not fail the bootstrap.

## 4. Write `.claude/kairos.local.md`

Per-repo wiring read by the SessionStart hook and skills (KAIROS-A-0014). Create or update `.claude/kairos.local.md` — substitute the real discovered values everywhere in this example:

```markdown
---
deployment_url: https://acme.kairos.example
tenant: acme
delivery_stream: platform
team_board: Platform Delivery
initiative_board: Platform Initiatives
---

# Kairos wiring for this repo

Connected as alice@acme.example (org: Acme Inc, team: platform).
Boards discovered 2026-07-10 via whoami/my_boards. Re-run /kairos:bootstrap
after team or board changes. <Note anything missing and what unblocks it.>
```

Frontmatter keys are exactly: `deployment_url`, `tenant`, `delivery_stream`, `team_board`, `initiative_board`. Leave a value empty (`key:`) when undiscovered rather than omitting the key. The prose section is short: who connected, what was discovered when, anything missing.

Then ensure it is gitignored — it is org-specific wiring, not for the repo's history (tokens live with the MCP client, never in this file). If `.gitignore` does not already cover it, append:

```
.claude/kairos.local.md
```

## 5. Report

Tell the user: config path(s) written, deployment/tenant, who they are connected as, the boards chosen, and — if anything was skipped (offline, unauthenticated, boardless) — exactly what to do to finish. Mention that from the next session on, the SessionStart hook injects this wiring and pulls live board state automatically.
