---
title: "Turn on forge write-back"
---

This page describes Kairos 0.18.0.

Show the state of a Kairos item on its GitHub pull requests. With a write
token on the repository, Kairos writes 2 things on each pull request that
links to an item:

- One comment for each item. The comment has the code, the title, the column,
  the claim and a link to the item. Kairos edits the same comment when the
  item changes.
- The commit status `kairos` on the head commit of the pull request:
  - `success`: each item is done.
  - `failure`: a person cancelled an item.
  - `pending`: all other cases.

A pull request links to an item when its title, body or branch has the code
of the item. See [Connect a git forge](../connect-a-git-forge/).

The same token also writes to the issues that became tasks. A GitLab
repository takes a write token for its issues only. See
[Write back to issues](#write-back-to-issues).

**Before you start:**

- The repository is a GitHub or GitLab repository in Kairos, with a forge
  connection.
- You are an organization admin, or a member of the owner team of the
  repository.
- The deployment has the setting `KAIROS_SECRETS_KEY`. See
  [Read a private repository](../read-a-private-repository/#1-set-the-key-of-the-deployment).

## 1. Make a write token

Use a GitHub fine-grained personal access token:

1. On GitHub, open **Settings**, then **Developer settings**, then
   **Fine-grained tokens**.
2. Select **Generate new token**.
3. For **Repository access**, select **Only select repositories**. Select the
   one repository.
4. For **Permissions**, give **Pull requests** the value **Read and write**.
   Give **Commit statuses** the value **Read and write**. Give **Issues** the
   value **Read and write** for the write-back to issues. Give no other
   permission.
5. Copy the token.

On GitLab, make a project access token on the one project. A personal
access token with the scope **api** works too.

1. On GitLab, open the project. Then open **Settings**, then **Access
   tokens**.
2. Select **Add new token**. Give the role **Reporter** and the scope
   **api**.
3. Copy the token.

The comments and the statuses show the name of the owner of the token. Use
the token of a different GitHub account if you do not want your name on them.

A token of a GitHub App is a later option. This version takes a personal
token only.

## 2. Set the token in Kairos

With the CLI, send the token on standard input:

```sh
printf '%s' "$TOKEN" | kairos repos write-token set site
```

Or use the GUI: open **Admin**, then **Repositories**. Select **Edit** on the
repository, put the token in **Write token**, and select **Set write token**.

Or use REST: `PUT /api/repositories/{slug}/write-token` with
`{"token": "..."}`.

Kairos keeps the token encrypted. No command, page or tool shows it again.
Kairos then writes each open pull request that links to an item.

## 3. Look at the result

```sh
kairos repos write-token status site
# → write-back: on (token set by Robin at 2026-10-12T09:15:00+00:00; the last write at 2026-10-12T09:15:04+00:00 worked)
```

The repository page of the GUI and the MCP tool `get_repository` show the
same line. When a write fails, the line has the error, and Kairos tries the
write again with longer waits. A change of an item does not wait for GitHub.

The forge can refuse the token: `401` (a bad token) or `403` (a missing
permission). Then Kairos does not try that write again. The error names the
permission. Make a new token, and set it again.

## 4. Turn it off

```sh
kairos repos write-token remove site --confirm
```

Kairos writes nothing more. The comments, the statuses and the closed
issues stay on the forge.

## Write back to issues

Issue intake makes a task from an opened issue. See
[Connect a git forge](../connect-a-git-forge/#5-take-issues). With a write
token, the issue then follows its task:

| The task | What Kairos writes on the issue |
|---|---|
| Moves to a done column | A comment, for example `Done in PAY-T-0012; PR #37.` Then Kairos closes the issue as completed. |
| Is cancelled | A comment with the reason, for example `Cancelled in PAY-T-0012: Duplicate of #9.` Then Kairos closes the issue as not planned. |
| Is reopened | No comment. Kairos opens the issue again. |

The comment names the task by its code. The comment names the pull requests of
the task too: `PR #37` on GitHub, `MR !37` on GitLab. The comment links to
the item only when the deployment sets `KAIROS_PUBLIC_URL` to a public host.
An issue can be public, so Kairos puts no private host in the comment. These
hosts are private:

- an address of a private network;
- a name with no dot;
- a name under `ts.net`, `local`, `localhost`, `internal`, `lan`, `home.arpa`
  or `corp`.

GitLab has no reason for a close. Kairos closes a cancelled task's issue, and
the comment gives the reason.

Kairos writes each change one time. A change of the task that does not change
its state writes nothing. When a close fails, Kairos tries the close again.
Kairos does not write the comment a second time.

When you set a token, Kairos writes each issue whose task changed before.
For example, Kairos then closes the issue of a task that is done already.

Kairos does not read the issue. A close or a comment on the forge does not
change the task.

## Related reading

- [Connect a git forge](../connect-a-git-forge/): the links between pull
  requests and items.
- [Configuration](../../reference/configuration/#github-write-back): the
  settings.
- [CLI](../../reference/cli/#kairos-repos-write-token): the commands.
