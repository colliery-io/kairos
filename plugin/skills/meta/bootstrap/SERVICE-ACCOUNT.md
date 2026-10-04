# The service-account path

Use this path when `/api/config` gives `issuer: null` and `local_auth: true`. Use it also when the user chooses it for an agent that must run with no browser.

OAuth is not available on a deployment with no issuer. Such a deployment has no authorization server, so the browser flow of the MCP client cannot start. The agent connects as a **service account**, and an **API key** authenticates it.

The book is the source of each command here: <https://colliery-io.github.io/kairos/how-to/give-an-agent-machine-access.html>.

## The secrets stay with the user

This path has two secrets: the password of the local account, and the API key. The user types each one in their own terminal or their own settings file. The conversation and the repository hold only the name of the variable, `KAIROS_MCP_KEY`.

The user runs the two commands that touch a secret: `kairos login` and `kairos keys create`. If a secret appears in the conversation, do not repeat it and do not write it. Tell the user to rotate the key.

## Steps

On a re-run, do only the steps that are not complete. `kairos service-accounts list` shows a service account that exists.

1. **The user logs in.** Only an organization admin can create a service account and an API key. Give the user this command for their own terminal:

   ```
   kairos login --url <URL> --email <EMAIL>
   ```

   The CLI asks for the password at its own prompt. On a deployment that resolves the tenant by header, the user adds `--tenant <slug>`. Then run `kairos whoami`. Continue when it names the user.

2. **Create the service account.** Agree a name with the user, for example `claude-code`. Then run:

   ```
   kairos service-accounts create --name <NAME>
   ```

   Keep the id from the output. The subsequent steps use it.

3. **Put the service account on the team.** `kairos teams list` gives the id of each team. If there is more than one team, ask the user which team works in this checkout. Then run:

   ```
   kairos teams members add <TEAM_ID> --user <SERVICE_ACCOUNT_ID>
   ```

   This team decides the team board in step 3 of [SKILL.md](SKILL.md). Membership of the team also lets the service account add a repository of that team to the directory (`add_repository`), if the directory does not have the repository of this checkout.

4. **Grant the capabilities.** An agent that implements tasks needs `manage_tasks`, `transition_items` and `manage_documents` on the team board. Membership of the team gives these three on the delivery board of that team, so that board needs no grant. On the ADR board of the team, membership also gives `manage_adrs`, so an agent in the team writes delivery ADRs with no grant.

   A different board needs a grant. The CLI does not grant capabilities. An organization admin makes the grant in the board members panel of the GUI, or with `POST /api/boards/{id}/members`.

   Propose the narrowest set: named capabilities, one board at a time. For example, propose `manage_initiatives` on the initiative board only if the agent creates initiatives. The user decides. A service account can never be an organization admin.

5. **The user makes the API key.** Give the user this command for their own terminal:

   ```
   kairos keys create --service-account <SERVICE_ACCOUNT_ID> --name <KEY_NAME>
   ```

   The option `--expires-at <RFC 3339 instant>` sets an expiry. The CLI shows the raw key (`kairos_sk_…`) one time only.

6. **The user stores the API key.** The user puts the key in the `env` section of their Claude Code user settings (`~/.claude/settings.json`):

   ```json
   { "env": { "KAIROS_MCP_KEY": "<the key>" } }
   ```

   The user makes this edit. Continue when the user says that the variable is set.

7. **Write the MCP config.** Create or update the `kairos` entry in the project `.mcp.json`. Preserve other servers. Write `${KAIROS_MCP_KEY}` as literal text:

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

   An API key carries its own tenant, so the entry needs no `X-Tenant` header.

8. **Offer to ignore `.mcp.json`.** The file names the deployment. Ask the user: commit the file, or add `.mcp.json` to `.gitignore`. The user decides.

9. **Connect.** Tell the user to start a new Claude Code session, so that the session reads the variable. Then the user runs `/mcp` to connect, and runs `/kairos:bootstrap` again. The re-run continues at step 3 of [SKILL.md](SKILL.md). `whoami` names the service account when the connection is correct.
