# Read a private repository

Give the builder of the code index a read token, so that it can fetch a
private repository.

The builder fetches each indexed repository from its `repo_url`. A public
repository needs no credential. A private repository needs a read token.
Kairos keeps the token encrypted, and no command, page or tool shows it again.

**Before you start:**

- The repository is in Kairos. If it is not, add it with `kairos repos create`.
- You are an organization admin, or a member of the owner team of the
  repository.
- The deployment has the setting `KAIROS_SECRETS_KEY`. See step 1.

## 1. Set the key of the deployment

An operator does this step one time for each deployment.

Make a key of 32 random bytes:

```sh
openssl rand -base64 32
```

Put the value in the setting `KAIROS_SECRETS_KEY` of the server, and start the
server again. Keep the key outside the database, with the database password.
The key encrypts each read token.

When the setting is not there, the server refuses each token with 501
`SECRETS_NOT_CONFIGURED`. The text of the refusal names `KAIROS_SECRETS_KEY`.

When you change the key, the stored tokens do not decrypt. Set each token
again after a change of the key.

## 2. Make a token on GitHub

Use a GitHub fine-grained personal access token:

1. On GitHub, open **Settings**, then **Developer settings**, then
   **Fine-grained tokens**.
2. Select **Generate new token**.
3. For **Repository access**, select **Only select repositories**. Select the
   one repository.
4. For **Permissions**, give **Contents** the value **Read-only**. Give no
   other permission.
5. Copy the token.

The builder sends the token to GitHub over HTTPS, with the user name
`x-access-token`.

## 3. Set the token in Kairos

With the CLI, send the token on standard input:

```sh
printf '%s' "$TOKEN" | kairos repos credential set skadi
```

Or type it at the prompt. The terminal does not show it:

```sh
kairos repos credential set skadi
```

The command has no argument for the token. Thus the token is not in the shell
history or in the process list.

In the GUI, open **Admin**, then **Repositories**. Select **Edit** on the row of
the repository. Type the token in the field **Read token**, and select
**Set token**.

A second `set` replaces the token. Each set and each replacement writes an
activity row with your name. The row does not hold the token.

## 4. Check the token

```sh
kairos repos credential check skadi
```

The server runs `git ls-remote` on the URL of the repository, with the token.
The command prints the result and keeps it:

```text
read token: set by Ada at 2026-10-03T16:26:48+00:00; the last check at 2026-10-03T16:27:02+00:00 passed
```

When git cannot read the repository, the command fails and prints the error of
git. The error does not hold the token. In the GUI, select **Check token**.

## 5. See the status

`kairos repos get skadi` prints the line `read token:`. The MCP tool
`get_repository` and the GUI show the same status. The status gives who set the
token, when, and the result of the last check. It never gives the token.

## Remove the token

```sh
kairos repos credential remove skadi
```

The next fetch has no credential. A delete of the repository also removes its
token.

## When a fetch fails

| The error | What to do |
|---|---|
| `Authentication failed` | The token is wrong, or it cannot read the repository. Make a new token (step 2), and set it again. |
| `The token was set with a different KAIROS_SECRETS_KEY. Set the token again.` | The key of the deployment changed. Set the token again. |
| `The repository has a read token, but this deployment has no KAIROS_SECRETS_KEY.` | Set `KAIROS_SECRETS_KEY` to the key that encrypted the token, or set the token again. |
| `The stored token does not decrypt. Set the token again.` | The row of the token changed in the database. Set the token again. |

## Related

- [Configuration](../reference/configuration.md): `KAIROS_SECRETS_KEY` and the
  settings of the code index
- [CLI](../reference/cli.md): `kairos repos credential`
- [Back up and restore](back-up-and-restore.md): keep the key with the backups
