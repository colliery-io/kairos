---
name: bootstrap
description: Wire this repo and engineer to a Kairos deployment — MCP endpoint config, auth check, repository detection from the git remote, board discovery, and .claude/kairos.local.md.
argument-hint: "[deployment URL]"
disable-model-invocation: true
---

# Bootstrap

Connect the current repo to a Kairos deployment so every other kairos skill (and the SessionStart hook) knows which **repository** this checkout is (KAIROS-A-0019) and which **board** is the queue: the delivery board of the team of the principal that connects (COLLIERY-A-0023). Idempotent: re-running updates the existing configuration in place.

## 1. Gather the deployment

On a re-run, recover what is already known before asking: the `kairos` entry in the repo's `.mcp.json` (URL, `X-Tenant` or `Authorization` header) and the frontmatter of `.claude/kairos.local.md`. Ask the user only for what is missing or being changed (or take it from the arguments):

- **Deployment URL** — e.g. `https://acme.kairos.example` or `http://localhost:41080`. Strip any trailing slash. The MCP endpoint is `<deployment-url>/mcp`.
- **Tenant slug (optional, dev setups only)** — production deployments resolve the tenant from the URL's subdomain, so most users skip this. When the URL has no tenant subdomain (localhost, IP, plain host), ask for the tenant slug; it is sent as an `X-Tenant` header on every request.

Check the deployment before you write anything. Fetch `<deployment-url>/healthz` and expect `ok`. Then fetch `<deployment-url>/api/config`, which needs no authentication. If a fetch fails, show the user what failed, confirm the URL with them, and stop.

### Choose the authentication path

Read `issuer` and `local_auth` in the `/api/config` response. They decide the path:

| `issuer` | `local_auth` | Path |
|---|---|---|
| a URL | `false` or absent | **OAuth.** Continue with step 2. |
| `null` | `true` | **Service account.** OAuth is not available. Tell the user, then do [SERVICE-ACCOUNT.md](SERVICE-ACCOUNT.md) in place of step 2. |
| a URL | `true` | **OAuth** is the default. The service-account path is the alternative for an agent that must run with no browser. |

On the OAuth path, fetch `<deployment-url>/.well-known/oauth-protected-resource/mcp` also. It is the RFC 9728 metadata that names the authorization server.

On a re-run, a `kairos` entry with an `Authorization` header shows the service-account path. If the `kairos` tools answer, continue with step 3.

## 2. Write the MCP config

This step is for the OAuth path. On the service-account path, [SERVICE-ACCOUNT.md](SERVICE-ACCOUNT.md) writes the config.

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

On the OAuth path, authentication is client-driven: the MCP client opens the browser flow the first time it connects — the skill never handles tokens. If the `kairos` MCP tools are not available in this session yet (config just written), tell the user to run `/mcp` to connect and authenticate, then re-run `/kairos:bootstrap` to finish; the re-run picks up here.

On the service-account path, the API key of a service account authenticates the client (KAIROS-A-0017). [SERVICE-ACCOUNT.md](SERVICE-ACCOUNT.md) is the only source for that path. The skill never handles the key.

Once connected:

1. `whoami` — confirms auth; names the principal (a person or a service account) and gives its org, teams, the repositories their teams own, and the boards where they hold capabilities.
2. **Detect this repository** (below).
3. `my_boards` — boards grouped by level; the user's delivery boards are marked `[mine]` and include column names and per-column item counts.
4. **Find the team board** (below).

### Detect this repository

Run `git remote get-url origin`. Normalize the remote to `(forge, full_name)`:

| Remote | forge | full_name |
|---|---|---|
| `git@github.com:acme/payments-api.git` | `github` | `acme/payments-api` |
| `https://github.com/acme/payments-api` | `github` | `acme/payments-api` |
| `git@gitlab.com:acme/portal/web.git` | `gitlab` | `acme/portal/web` |
| `https://gitlab.example.com/acme/web.git` | `gitlab` (any host containing `gitlab`) | `acme/web` |
| anything else | `other` | path after the host, `.git` stripped |

Strip a trailing `.git` and any leading `/`. Then match on `forge` + `full_name` against `list_repositories` (the MCP directory carries both fields; `whoami`'s repository list does not show the forge), or over HTTP with `GET /api/repositories?forge=<forge>&name=<full_name>` (one row or empty).

- **Found** → its `slug` is the repository. The repository is a link: it does not choose the team board. Confirm with the user only if the remote matched more than one entry (it cannot: the pair is unique).
- **Found, with no description** → `get_repository` prints `(no description yet)`. Offer to write one: draft it as in step 3 of "Add this repository", show it to the user, and on a yes call `update_repository` (`repository`, `description`).
- **Not found** → add it with `add_repository`. See "Add this repository" below.
- **No remote, or the user declines** → leave `repository:` empty and say what unblocks it. Everything else still works; the session is just board-scoped instead of repo-scoped.

### Add this repository

Do this when the directory does not have the repository of the checkout. `add_repository` is for a member of the owner team or an organization admin.

1. **Owner team.** One team in `whoami` → that team. More than one → ask the user which team owns this repository. No team → do not call the tool. Say that an organization admin must add the principal to a team, or add the repository. Leave `repository:` empty.
2. **Values from git.**

   | Argument | Source |
   |---|---|
   | `forge`, `repo_full_name` | The normalized remote, from the table above. |
   | `repo_url` | The browser URL of the remote: `https://<host>/<full_name>`, with no `.git` and no credentials. |
   | `default_branch` | `git symbolic-ref --short refs/remotes/origin/HEAD`, without the `origin/` prefix. If the command fails, ask the user. |
   | `team` | The slug of the owner team. |
   | `slug` | Omit it. Kairos makes the slug from the full name (`acme/payments-api` gives `acme-payments-api`). Send a slug only if the user wants a different one. |

3. **Draft the description.** It is the "how to work here" text that each agent reads with `get_repository`. Read the README and the task runner of the checkout (for example `.angreal/`, `Makefile`, `justfile`, the `scripts` of `package.json`). Write a short text: what the repository is, the commands to build, test and lint, and the rules for a branch and a pull request. Write only what the files say.
4. **Show, then call.** Show the user each argument and the full description. Call `add_repository` only after the user agrees. Use the text that the user agreed to.
5. **Read the result.** The result gives the slug: `Added repository <slug>: …`. That slug is the `repository:` of step 4.

If the tool refuses, tell the user the message and do not try again with different values of your own:

| Refusal | What to do |
|---|---|
| `FORBIDDEN` | The principal is not a member of the owner team. The message names the team. Leave `repository:` empty. |
| `CONFLICT` | A repository has the slug already. Ask the user for a different `slug`. |
| `VALIDATION` | The message names the value that is not correct. Correct it with the user. |

`update_repository` changes the description, the default branch and the URL later. No tool changes the slug or the owner team, and no tool deletes a repository. A person does these on the GUI page Admin → Repositories, with `kairos repos update` and `kairos repos delete`, or with the REST API.

### Find the team board

The team board is the delivery board of the team of the principal that connects. The owner of the repository does not decide it.

- **One team in `whoami`** → that team is the **delivery stream**. Its delivery board is the **team board**: the `[mine]` delivery board in `my_boards` (the default slug is `<team>-delivery`).
- **More than one team** → ask the user which team works in this checkout. Do the same when more than one delivery board is `[mine]`.
- **No team** → leave `delivery_stream:` and `team_board:` empty. Say that an organization admin must add the principal to a team.

From the results pick, confirming with the user whenever there is more than one candidate:

- **repository** — the slug detected above
- **delivery stream / team board** — the team found above and its delivery board
- **initiative board** — the default initiative-level board for new initiatives

If auth is declined or fails, or there are no boards yet (fresh tenant, no memberships): keep the step 2 config, proceed to step 4 regardless, and record in the prose section what is missing and what unblocks it (authenticate via `/mcp`; on the service-account path, set `KAIROS_MCP_KEY`; ask an org admin for team/board membership). Do not fail the bootstrap.

## 4. Write `.claude/kairos.local.md`

Per-repo wiring read by the SessionStart hook and skills (KAIROS-A-0014). Create or update `.claude/kairos.local.md` — substitute the real discovered values everywhere in this example:

```markdown
---
deployment_url: https://acme.kairos.example
tenant: acme
repository: payments-api
delivery_stream: platform
team_board: platform-delivery
initiative_board: initiatives
---

# Kairos wiring for this repo

Connected as alice@acme.example (org: Acme Inc, team: platform).
Repository payments-api (github acme/payments-api, owned by platform) matched
from the origin remote; boards from the team of alice 2026-09-22. Re-run
/kairos:bootstrap after remote, team or board changes. <Note anything
missing and what unblocks it.>
```

Frontmatter keys are exactly: `deployment_url`, `tenant`, `repository`, `delivery_stream`, `team_board`, `initiative_board`. `repository` is the slug (KAIROS-A-0019); `team_board` and `initiative_board` are board **slugs** (what `board_items` resolves — never display names); `delivery_stream` and `team_board` come from the team of the principal, also when a different team owns the repository. Leave a value empty (`key:`) when undiscovered rather than omitting the key. The prose section is short: who connected, what was discovered when, anything missing.

When the principal is a service account, the first line of the prose names it. It also names the person who set it up, if known:

```markdown
Connected as the service account claude-code (org: Acme Inc, team: platform),
set up by alice@acme.example.
```

Then ensure it is gitignored — it is org-specific wiring, not for the repo's history (tokens live with the MCP client and the API key lives in the user's settings, never in this file). If `.gitignore` does not already cover it, append:

```
.claude/kairos.local.md
```

## 5. Report

Tell the user: config path(s) written, deployment/tenant, who they are connected as, the repository matched (or why not), the boards chosen, and — if anything was skipped (offline, unauthenticated, a repository that is not in the directory, boardless) — exactly what to do to finish. Mention that from the next session on, the SessionStart hook injects this wiring and pulls this repository's queue automatically.

On the service-account path, tell the user these three facts also:

- The deployment shows the API key one time only.
- The key is in the `env` section of the user's Claude Code settings, as `KAIROS_MCP_KEY`. It is in no file of the repository.
- To rotate the key, follow "Rotate without downtime": <https://colliery-io.github.io/kairos/how-to/give-an-agent-machine-access.html>.
