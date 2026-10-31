---
title: "Give an agent your key"
---

Let an agent, for example Claude Code, work in Kairos as you. The agent uses an
**agent key**. The key acts as you. The history and the activity show each
change of the agent with your name and a mark. For example, they show
"Alice (agent)".

The key has a **scope**: a list of capabilities and, if you want, a list of
boards. The agent can do an action only when you can do it and the scope has
it.

**Before you start:** you are a member of the organization. You do not need to
be an admin.

Use an agent key for an agent that works for you. Use a service account for
machine work with no person, for example a CI pipeline. See
[Give an agent machine access](../give-an-agent-machine-access/).

## 1. Log in as yourself

```sh
kairos login --url https://kairos.example.com
kairos whoami
```

`whoami` shows your name. Log in with your own login, not with an agent key. A
request with an agent key cannot make an agent key.

Sometimes `kairos login` does not work, for example with Google as the issuer.
Then sign in to the web app, and do step 2 on the **Agent keys** page.

## 2. Make the key

Give each agent its own key. Then you can revoke one key and keep the others.

```sh
kairos keys create --name laptop-claude-code
#
#     kairos_sk_acme_9f8e7d6c5b4a…            ← copy it now
#
# Keep the key in a safe place now. Kairos does NOT show it again.
```

You can also make the key on the **Agent keys** page of the GUI.

The key gets the default scope: each capability that you have, but `purge` and
`manage_structure`. Thus the agent cannot delete a task for good, and it cannot
change teams, boards or accounts. To choose a different scope, see
[Choose the scope of the key](#choose-the-scope-of-the-key).

A new key expires after 90 days, unless the operator changed
`KAIROS_API_KEY_TTL_DAYS`. The option `--expires-at 2027-01-01T00:00:00Z` sets
a different expiry. The flag `--no-expiry` makes a key with no expiry. Kairos keeps a
hash of the key only. It cannot show the key again. If you lose the key, make a
new key and revoke the old key.

## 3. Put the key and the URL in the settings of the agent

For Claude Code, put the key and the deployment URL in the `env` section
of your user settings, `~/.claude/settings.json`:

```json
{
  "env": {
    "KAIROS_URL": "https://kairos.example.com",
    "KAIROS_MCP_KEY": "kairos_sk_acme_9f8e7d6c5b4a…"
  }
}
```

`KAIROS_URL` has no `/mcp` and no slash at the end. Do not put the key in a
repository. Do not put the key in a chat.

The `kairos` plugin has its own `kairos` MCP server, `kairos mcp-proxy`. It
reads the two variables and connects in each repository of yours. You write
no `.mcp.json` and you run no `claude mcp add`. The key holds its tenant, so
the connection needs no `X-Tenant` header.

In a repository, `/kairos:bootstrap` writes only a project entry in
`~/.config/kairos/config.toml`. The entry has the deployment, your team board and
the repository. It does not write `.claude/kairos.local.md`, and no
command reads that file. If a repository has one, `kairos config migrate`
moves its values to the project entry. Then remove the file.

When one repository must use a different deployment, give its project entry
its own `deployment`, and leave `KAIROS_URL` unset. `KAIROS_URL` comes before
the project entry. The server then connects each session to the deployment of
its repository:

```toml
[[project]]
remote     = "github.com/acme/other-app"
deployment = "https://other.example.com"
```

Some deployments use OpenID Connect and no agent key. There, run
`kairos login`: the plugin server then uses the token of the login. See
[Connect over MCP](../connect-over-mcp/).

## 4. Make sure that it works

Start a new session of the agent, so that it reads the variable. Then call the
`whoami` tool. It shows your name and the scope of the key.

## Choose the scope of the key

Give the agent only the capabilities that its work needs. A scope cannot give
the agent a capability that you do not have.

```sh
# Tasks and moves only, on one board:
kairos keys create --name ci-bot --scope manage_tasks,transition_items --scope-board web-delivery

# A key that can only read:
kairos keys create --name reader --read-only

# Each capability, purge and manage_structure too:
kairos keys create --name admin-bot --scope manage_strategies,manage_initiatives,manage_tasks,manage_documents,manage_adrs,transition_items,configure_boards,administer_members,member,approve_proposals,pages,purge,manage_structure,file_backlog
```

The names come from [Capabilities](../../reference/capabilities/#the-names-of-a-scope).
Kairos refuses an unknown name and an unknown board, and it names them. On the
**Agent keys** page, write the capabilities and the boards with commas between
them. **Read-only** makes a key with no capabilities.

The work of an organization admin needs `manage_structure` in the scope. That
work includes accounts, keys, templates and metadata definitions.

When the scope does not have an action, Kairos refuses it with `KEY_SCOPE`
(403). The refusal names the capability that the scope needs. Make a new key
with a wider scope, or do the action with your login.

The scope of a key does not change. A rotation keeps the scope. To change the
scope, make a new key and revoke the old key.

### Keys from before the scope

A key that you made before Kairos had scopes has no scope. That key keeps each
of your capabilities, so your agents continue to work. `kairos whoami` with
the key shows `key scope: none`. Replace such a key with a new key to limit the
agent.

## 5. Revoke a key

```sh
kairos keys list
kairos keys revoke <KEY_ID> --confirm
```

After the revocation, the key gets `401`. To replace a key, make the new key
first. Put the new key in the settings of the agent. Then revoke the old key.

## Who can revoke your key

A key with no expiry works until a person revokes it. These persons can revoke
your key:

- You, with `kairos keys revoke` or on the **Agent keys** page.
- An organization admin, with `kairos admin agent-keys revoke <KEY_ID> --confirm`
  or in the section **Agent keys of the people** of the page **Admin >
  Members**. Kairos then gives you a notification that names the key.

When an admin removes you from the organization, Kairos revokes each of your
agent keys. A SCIM deprovision does the same.

## Use the key with the CLI

The CLI can use the key in place of `kairos login`. This is the route when the
issuer gives the CLI no device grant, for example Google. Keep the key in a
file that only you can read, and not in the history of your shell:

```sh
install -m 600 /dev/null ~/.kairos-agent-key   # then paste the key into the file
export KAIROS_URL=https://kairos.example.com
export KAIROS_KEY="$(cat ~/.kairos-agent-key)"
kairos whoami
```

The CLI also reads `KAIROS_MCP_KEY`, so a shell that has the variable of
Claude Code needs only `KAIROS_URL`. When a key is set, it wins over a login.
Unset it to use the login again. The key cannot make or revoke agent keys:
use your login or the **Agent keys** page for that.

When the key hides a login, each command writes a warning on stderr.
`KAIROS_QUIET=1` stops the warning. The last line of `kairos whoami` names the
credential that the CLI sends.

## What you cannot do

| You try to | Kairos answers |
|---|---|
| make an agent key with an agent key | `403` |
| revoke the key of a different person | `404` |
| make an agent key as a service account | `403` |
| do an action that the scope of the key does not have | `403 KEY_SCOPE` |
| name an unknown capability or board in the scope | `422 VALIDATION` |
| give a scope to the key of a service account | `422 VALIDATION` |
