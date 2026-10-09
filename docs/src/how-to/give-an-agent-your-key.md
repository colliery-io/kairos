# Give an agent your key

Let an agent, for example Claude Code, work in Kairos as you. The agent uses an
**agent key**. The key acts as you. The agent gets your capabilities and no
more. The history of each item that the agent changes shows your name.

**Before you start:** you are a member of the organization. You do not need to
be an admin.

Use an agent key for an agent that works for you. Use a service account for
machine work with no person, for example a CI pipeline. See
[Give an agent machine access](give-an-agent-machine-access.md).

## 1. Log in as yourself

```sh
kairos login --url https://kairos.example.com
kairos whoami
```

`whoami` shows your name. Log in with your own login, not with an agent key. A
request with an agent key cannot make an agent key.

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

The option `--expires-at 2027-01-01T00:00:00Z` sets an expiry. Kairos keeps a
hash of the key only. It cannot show the key again. If you lose the key, make a
new key and revoke the old key.

## 3. Put the key in the settings of the agent

For Claude Code, put the key in the `env` section of your user settings,
`~/.claude/settings.json`:

```json
{ "env": { "KAIROS_MCP_KEY": "kairos_sk_acme_9f8e7d6c5b4a…" } }
```

Do not put the key in a repository. Do not put the key in a chat.

The MCP configuration of the repository holds the name of the variable only:

```json
{
  "mcpServers": {
    "kairos": {
      "type": "http",
      "url": "https://kairos.example.com/mcp",
      "headers": { "Authorization": "Bearer ${KAIROS_MCP_KEY}" }
    }
  }
}
```

The key holds its tenant. The entry needs no `X-Tenant` header. The
`/kairos:bootstrap` skill of the plugin writes this entry.

## 4. Make sure that it works

Start a new session of the agent, so that it reads the variable. Then call the
`whoami` tool. It shows your name.

## 5. Revoke a key

```sh
kairos keys list
kairos keys revoke <KEY_ID> --confirm
```

After the revocation, the key gets `401`. To replace a key, make the new key
first. Put the new key in the settings of the agent. Then revoke the old key.

## What you cannot do

| You try to | Kairos answers |
|---|---|
| make an agent key with an agent key | `403` |
| revoke the key of a different person | `404` |
| make an agent key as a service account | `403` |
