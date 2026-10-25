---
title: "Run an admin agent"
---

Let an agent set up a team, its boards and its members with no person at the
GUI. The agent uses the key of a service account. The service account holds
the capability [`manage_structure`](../../reference/capabilities/#manage_structure).

**Before you start:** you are an **organization admin**. Only an organization
admin can make a service account, make its key, and grant `manage_structure`.

## Know the risk

Read this before you make the key.

- A key with `manage_structure` on the organization can change the structure
  of the full organization. It can make and delete teams and boards. It can
  change the columns of each board. It can add each member of the
  organization to each team. It can grant each board capability on each
  board, also to itself.
- The key is a line in a settings file of the agent, for example
  `~/.claude/settings.json`. A person or a program that can read that file
  can use the key. The key works until you revoke it or until it expires.
- The activity log records each change with the name of the service account.
  It does not record which person ran the agent.

The key cannot do these. They stay with the organization admin:

- make or delete an account, an API key or a SCIM token,
- grant `manage_structure`,
- change the organization admins, the deployment admins or the tenant.

Use the smallest scope that does the work:

| The agent must | Grant |
|---|---|
| Make teams and boards | `manage_structure` on the organization |
| Change the columns, transitions and members of one board | `manage_structure` on that board |
| Work the items of a board | A board capability, or team membership. Not `manage_structure`. |

An agent that works for one person does not need a service account. Give it
the agent key of that person. The agent then has the capabilities of that
person. See [Give an agent your key](../give-an-agent-your-key/).

## 1. Make the service account and its key

```sh
kairos service-accounts create --name admin-agent
# → Kairos made the service account admin-agent (a1b2c3…).
kairos keys create --service-account a1b2c3… --name admin-agent-key \
  --expires-at 2027-01-01T00:00:00Z
#
#     kairos_sk_acme_9f8e7d6c5b4a…            ← copy it now
```

Set an expiry. Kairos shows the key one time only.

## 2. Grant manage_structure

On the organization:

```sh
kairos grants add --user admin-agent
# → Granted manage_structure on the organization to admin-agent <…> (grant d4e5f6…).
```

On one board only:

```sh
kairos grants add --user admin-agent --board home-ops-delivery
```

`--user` takes the id, the email or the user name of a member, or the name of
a service account. The REST route is `POST /api/structure-grants` with
`{"user": "admin-agent"}` or `{"user": "admin-agent", "board": "home-ops-delivery"}`.

## 3. Check the grant

With the key of the agent:

```sh
curl https://<host>/api/whoami \
  -H "Authorization: Bearer kairos_sk_acme_9f8e7d6c5b4a…"
```

The field `structure` shows the grant:
`[{"scope": "organization", "board_id": null, "board_slug": null}]`. The MCP
tool `whoami` shows it in the section `Structure (manage_structure)`.

## 4. Put the key in the settings of the agent

For Claude Code, put the key in the `env` section of the user settings,
`~/.claude/settings.json`. Do not put the key in a repository or in a chat.
See [Give an agent your key](../give-an-agent-your-key/) for the MCP
configuration.

The four tools below are in the admin profile of the MCP server. The URL of
the server is `<deployment>/mcp?profile=admin`, not `/mcp`
([Profiles](../../reference/mcp-tools/#profiles)). The plugin's own server
lists the work profile, which has none of them. To add the admin profile to
Claude Code with the plugin, run
`claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin`.

## 5. Let the agent work

The agent uses four MCP tools:

| Tool | What it does |
|---|---|
| `create_team` | Makes a team and its delivery board. With `adr_board` `true`, it also makes the ADR board of the team. |
| `create_board` | Makes one board. |
| `add_team_member` | Adds a person or a service account to a team. |
| `grant_capability` | Grants a board capability on one board. |

For example, the agent sends `create_team` with
`{"name": "Home operations", "slug": "home-ops", "code_prefix": "OPS"}`. Then it
sends `add_team_member` for each person of the team. The new boards have no
members until then. See [MCP tools](../../reference/mcp-tools/#the-structure-of-the-organization).

## 6. Revoke the grant when the work is done

```sh
kairos grants list
kairos grants remove d4e5f6… --confirm
```

Or revoke the key: `kairos keys revoke <key-id> --service-account a1b2c3…
--confirm`. A revoke takes effect at the next request.

## Related reading

- [Capabilities](../../reference/capabilities/)
- [Give an agent machine access](../give-an-agent-machine-access/)
- [Connect over MCP](../connect-over-mcp/)
