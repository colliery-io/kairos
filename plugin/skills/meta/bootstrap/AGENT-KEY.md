# The agent-key path

Use this path when `/api/config` gives `issuer: null` and `local_auth: true`. Use it also when the user chooses it for an agent that must run with no browser.

OAuth is not available on a deployment with no issuer. Such a deployment has no authorization server, so the browser flow of the MCP client cannot start. The agent connects with an **agent key** of the user (KAIROS-A-0024). The key acts as the user, with the capabilities of the user. The history of each item that the agent changes names the user.

Do not make a service account for Claude Code. A service account is for machine work with no person, for example a CI pipeline.

The book is the source of each command here: <https://colliery-io.github.io/kairos/how-to/give-an-agent-your-key.html>.

## The secrets stay with the user

This path has two secrets: the password of the local account, and the agent key. The user types each one in their own terminal or their own settings file. The conversation and the repository hold only the name of the variable, `KAIROS_MCP_KEY`.

The user runs the two commands that touch a secret: `kairos login` and `kairos keys create`. If a secret appears in the conversation, do not repeat it and do not write it. Tell the user to revoke the key and make a new one.

## Steps

On a re-run, do only the steps that are not complete. `kairos keys list` shows the agent keys of the user.

1. **The user logs in.** Each member of the organization can make their own agent keys. Give the user this command for their own terminal:

   ```
   kairos login --url <URL> --email <EMAIL>
   ```

   The CLI asks for the password at its own prompt. On a deployment that resolves the tenant by header, the user adds `--tenant <slug>`. Then run `kairos whoami`. Continue when it names the user.

2. **Check the team.** `kairos whoami` shows the teams of the user. The team of the user decides the team board in step 3 of [SKILL.md](SKILL.md). If the user is on no team, an organization admin adds the user to a team. The agent gets no capability that the user does not have.

3. **The user makes the agent key.** Agree a name for the key with the user, for example `laptop-claude-code`. Give the user this command for their own terminal:

   ```
   kairos keys create --name <KEY_NAME>
   ```

   The option `--expires-at <RFC 3339 instant>` sets an expiry. The CLI shows the raw key (`kairos_sk_…`) one time only. The user can also make the key on the **Agent keys** page of the GUI.

4. **The user stores the agent key.** The user puts the key in the `env` section of their Claude Code user settings (`~/.claude/settings.json`):

   ```json
   { "env": { "KAIROS_MCP_KEY": "<the key>" } }
   ```

   The user makes this edit. Continue when the user says that the variable is set.

5. **Write the MCP config.** Create or update the `kairos` entry in the project `.mcp.json`. Preserve other servers. Write `${KAIROS_MCP_KEY}` as literal text:

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

   An agent key carries its own tenant, so the entry needs no `X-Tenant` header.

6. **Offer to ignore `.mcp.json`.** The file names the deployment. Ask the user: commit the file, or add `.mcp.json` to `.gitignore`. The user decides.

7. **Connect.** Tell the user to start a new Claude Code session, so that the session reads the variable. Then the user runs `/mcp` to connect, and runs `/kairos:bootstrap` again. The re-run continues at step 3 of [SKILL.md](SKILL.md). `whoami` names the user when the connection is correct.
