# CLI

`kairos` is the command-line client. It talks to the same HTTP API as the GUI
and the MCP server.

This page describes `kairos` 0.8.2. The command tree below mirrors
`kairos --help`.

## Invocation

```
kairos <COMMAND> [SUBCOMMAND] [ARGUMENTS] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `-h`, `--help` | flag | — | Print help. `-h` prints the summary; `--help` prints the long form. |
| `-V`, `--version` | flag | — | Print the version and exit. |

`kairos help [COMMAND]...` prints the same help as `--help` on that command.

Command groups, each detailed below:

| Group | Commands |
|---|---|
| Authentication | `login`, `logout`, `whoami` |
| Work items | `strategies`, `initiatives`, `tasks`, `documents`, `adrs` |
| Finding work | `search`, `boards` |
| Organization | `orgs`, `members`, `teams`, `streams`, `repos` |
| Machine access | `service-accounts`, `keys` |
| Deployment administration | `admin` |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | API or validation error: not found, forbidden, conflict, invalid transition, bad input, transport failure. Also the case of several cached deployments with no `--url`. |
| 2 | Authentication error: no cached credentials, expired or rejected credentials, a failed token refresh, an expired local session, a rejected email and password, HTTP 401, a corrupted credential cache. |

Structured API rejections are rendered with their actionable detail: a 409
`CONFLICT` prints the server-current version and title, a 422
`INVALID_TRANSITION` prints the allowed target columns with their ids, a 403
prints the required board capability when the server names one, and a 422
`LAST_ADMIN` prints the keep-one-admin constraint.

## Common options

Every command that reaches the API accepts these three. They are omitted from
the per-command tables below.

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | the only cached deployment | Deployment base URL. Required when more than one deployment is cached. |
| `--tenant <TENANT>` | string | the tenant cached at login | Tenant slug, sent as the `X-Tenant` header. Used by deployments that resolve tenants by header rather than by host subdomain. |
| `--json` | flag | off | Print the raw JSON DTO instead of the human-readable table. |

`--url` values are normalized by trimming whitespace and trailing slashes, so
`https://kairos.example/` and `https://kairos.example` address the same cached
entry.

## Authentication

A deployment lets people log in through an OIDC issuer, with local accounts, or
with both. `kairos login` has one mode for each. Both modes write the same
credential cache, so every other command works the same after either.

### `kairos login`

Logs in to a deployment and caches the credential.

```
kairos login --url <URL> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | required | Deployment base URL, e.g. `https://kairos.example.com`. |
| `--email <EMAIL>` | string | none | Email of a local account. Selects the password login. Conflicts with `--issuer`, `--client-id` and `--bearer`. |
| `--issuer <ISSUER>` | string | discovered | OIDC issuer override. Skips RFC 9728 discovery against the deployment. |
| `--tenant <TENANT>` | string | none | Tenant slug, cached and sent as `X-Tenant` on subsequent API calls. |
| `--client-id <CLIENT_ID>` | string | `kairos-cli` | OAuth client id registered for the CLI at the issuer. |
| `--bearer <BEARER>` | `access_token` \| `id_token` | `access_token` | Which token is cached and sent as the API bearer. `id_token` suits issuers whose access token is opaque, such as Google Workspace; `access_token` suits Dex and Keycloak. |

`login` takes neither `--json` nor the cached-deployment form of `--url`.

| Mode | Selected by | Exchange | Cached credential |
|---|---|---|---|
| Issuer | no `--email` | OAuth Device Authorization Grant at the issuer | Access token or ID token, with a refresh token when the issuer gives one. |
| Local account | `--email <EMAIL>` | `POST /api/login` with the email and the password | Session bearer, with the expiry that the server gives. No refresh token. |

The password of a local account has two sources:

| Standard input | Source |
|---|---|
| a terminal | A prompt, `Password for <EMAIL>: `. The terminal does not show the characters. |
| a pipe or a file | The first line of standard input. The line ending is removed. Spaces are kept. |

```
printf '%s' "$PASSWORD" | kairos login --url <URL> --email <EMAIL>
```

There is no `--password` option, and no environment variable holds the
password. An argument is visible in the process list and stays in the shell
history.

Failures of `login`:

| Condition | Exit code | Message |
|---|---|---|
| No `--email`, and the deployment has no issuer | 1 | `This deployment has no OIDC issuer. It uses local accounts.` The message gives the command with `--email`. |
| `--email`, and the deployment has local accounts off | 1 | `Local accounts are off on this deployment.` The message gives the command without `--email`. |
| `--email` with `--issuer`, `--client-id` or `--bearer` | 2 | A usage error from the argument parser. |
| `--password` | 2 | A usage error from the argument parser. |
| Wrong password, or no account with that email | 2 | `The deployment did not accept the email and the password.` The two conditions give the same message. |
| Too many failed attempts (HTTP 429) | 1 | `The number of incorrect logins is too large.` The message gives the wait in seconds. |
| No password given | 1 | `The command got no password.` |

### `kairos logout`

Removes one deployment's entry from the credential cache. For a local session,
`logout` also ends the session on the server with `POST /api/logout`. After
that, the bearer does not work.

```
kairos logout [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | the only cached deployment | Deployment whose credentials are forgotten. |

`logout` takes neither `--tenant` nor `--json`.

| Entry | Server call | Result when the call fails |
|---|---|---|
| Issuer tokens | none | — |
| Local session | `POST /api/logout` | Exit code 1. The entry is removed from the cache. The session stays valid until it expires. |

### `kairos whoami`

```
kairos whoami [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--json` | flag | off | Print the raw `/api/whoami` JSON. |

Also accepts `--url` and `--tenant`.

The default rendering prints three lines: `user:` with display name and email,
`org:` with the organization slug and the caller's role, and `teams:` with the
team names, or `(none)`.

The `/api/whoami` response carries more than the default rendering prints. The
board capabilities the principal holds, the capabilities every member holds
implicitly, and the caller's teams' repositories are available under `--json`
only.

`whoami` works the same with issuer tokens and with a local session. The CLI
does not refresh a local session. After a local session expires, each command
that reaches the API sends no request and exits with code 2. The message is
`The session for <URL> expired.`, and it gives the `kairos login` command
with the cached `--email` and `--tenant`.

## Work items

Five nouns — `strategies`, `initiatives`, `tasks`, `documents`, `adrs` — are
one generated command family over the five entity types. They share verb names,
argument shapes and output format; only the set of verbs and the `create` flags
differ.

### Verbs per noun

| Verb | `strategies` | `initiatives` | `tasks` | `documents` | `adrs` |
|---|---|---|---|---|---|
| `list` | yes | yes | yes | yes | yes |
| `get` | yes | yes | yes | yes | yes |
| `create` | yes | yes | yes | yes | yes |
| `edit` | yes | yes | yes | yes | yes |
| `transition` | yes | yes | yes | no | yes |
| `move` | no | no | yes | yes | no |
| `delete` | yes | yes | yes | yes | yes |
| `restore` | yes | yes | yes | yes | yes |

`documents` has no `transition` verb: documents have no board placement, and
carry an editorial lifecycle instead of a column. `move` exists on `tasks`
and on `documents`. `tasks move` moves a task to a different delivery board.
`documents move` gives a document to a different owner board. See
[Flight levels](../explanation/flight-levels.md) and
[Teams and boards](../explanation/teams-and-boards.md).

### `<noun> list`

```
kairos <noun> list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--include-deleted` | flag | off | Also list archived (put-away) items, marked `[archived]` in the `CODE` column. Without the flag, only live items are returned. |

`documents list` and `adrs list` have one more option:

| Option | Type | Default | Description |
|---|---|---|---|
| `--repo <REPOSITORY>` | slug or UUID | none | Only the items that impact this repository. The repository must be live. |

Among the `list` verbs, `--include-deleted` exists on these five only: the
organization nouns (`boards`, `teams`, `members`, `streams`, `admin tenants`)
have no archived mode. `kairos search` carries the same flag. See
[Archiving](../explanation/archiving.md).

### `<noun> get`

```
kairos <noun> get <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code, e.g. `ACME-T-0001`. |

Prints the item including its markdown content.

A retired short code finds the item. Then the command writes a notice on
stderr. The notice names the current code, for example `The code
COLLIERY-T-0100 is retired. The current code of this item is SKADI-T-0001.`
The output on stdout is the item, and `--json` stays clean.

`documents get` prints the line `owner board`. It has the id of the owner
board of the document.

`documents get` and `adrs get` print the line `impacts`. It has the slugs of
the repositories that the item impacts, or `-`. The mark `[archived]` shows a
repository that is not live.

### `<noun> create`

```
kairos <noun> create --title <TITLE> [OPTIONS]
```

Options common to every noun:

| Option | Type | Default | Description |
|---|---|---|---|
| `--title <TITLE>` | string | required | The item's title. |
| `--content <CONTENT>` | string | `""`; on `documents`, no default | Markdown content. On `documents`, content omitted together with `--template` stamps the template's content. |

Board placement per noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--board <BOARD_ID>` | UUID | required | Board to create the strategy on. |
| `strategies` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `initiatives` | `--board <BOARD_ID>` | UUID | required | Board to create the initiative on. |
| `initiatives` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `tasks` | `--board <BOARD>` | slug or UUID | required unless `--team` is given | Delivery board to create the task on. The board decides the team of the task. With `--team` and no `--board`, the task goes to the delivery board of that team. |
| `tasks` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `documents` | `--board <BOARD>` | slug or UUID | required | The owner board of the document. It gives the right to edit the document, and the code of the document gets the prefix of this board. The document is not a card of the board. |
| `documents` | `--parent <SHORT_CODE>` | string | none | The workflow item the document supports. It does not give the document an owner board. |
| `adrs` | `--board <BOARD_ID>` | UUID | none | ADR board. Omitting it creates an off-board ADR, which is an org-admin operation. |
| `adrs` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |

Options specific to one noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--hypothesis <HYPOTHESIS>` | string | none | The strategy's hypothesis. |
| `initiatives` | `--complexity <COMPLEXITY>` | `xs` \| `s` \| `m` \| `l` \| `xl` | none | T-shirt sizing. |
| `initiatives` | `--bucket-type <KIND>` | `tech_debt` \| `bug` \| `ad_hoc` | none | Marks the initiative as a bucket of this kind. |
| `tasks` | `--type <TASK_TYPE>` | `task` \| `bug` \| `tech_debt` \| `support` | `task` | Task type. |
| `tasks` | `--work-class <WORK_CLASS>` | `planned` \| `support` | `support` for support-type tasks, `planned` otherwise; always `support` on a board that the caller does not manage | Planned/Support lane. On a board that the caller does not manage, `planned` is refused with 403. |
| `tasks` | `--team <TEAM_ID>` | UUID | none | Team. Without `--board`, the task goes to the delivery board of this team. With `--board`, it must be the team of that board. |
| `tasks` | `--repo <REPOSITORY>` | slug or UUID | none | Repository the task links to. It can be any repository, of any team. It does not choose the board. |

`tasks create` needs `--board` or `--team`. `--repo` does not replace them.

`documents create` needs `--board`: each document has an owner board. With
`--parent` too, the document also supports that item. The command needs
`manage_documents` on the owner board. To say that the document impacts a
repository, use [`kairos repos link`](#kairos-repos-link).

A caller without `manage_tasks` on the board creates a request. The request
goes to the entry column. For that caller, a `--column` that names a different
column gets a 403. See
[Send a request to a different team](../how-to/move-work-between-boards.md#send-a-request-to-a-different-team).
| `documents` | `--template <TEMPLATE_ID>` | UUID | none | Template to stamp content and metadata defaults from. |
| `adrs` | `--decision-maker <DECISION_MAKER>` | string | none | Decision maker. |
| `adrs` | `--decision-date <DATE>` | `YYYY-MM-DD` | none | Decision date. |

### `<noun> edit`

```
kairos <noun> edit <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--title <TITLE>` | string | unchanged | New title. |
| `--content <CONTENT>` | string | unchanged | New markdown content; a full replacement, not a patch. Conflicts with `--content-file`. |
| `--content-file <FILE>` | path | — | Read the new content from a file. `-` reads standard input. Conflicts with `--content`. |
| `--version <VERSION>` | integer | the fetched current version | Base the edit on this version. |

The edit is version-checked. The CLI fetches the item and bases the PATCH on
its current version; a concurrent edit is rejected with 409 `CONFLICT`. A
`--version` value that is already stale is rejected the same way.

### `<noun> transition`

```
kairos <noun> transition <SHORT_CODE> --to <COLUMN_ID> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--to <COLUMN_ID>` | UUID | required | Target column. |

A target outside the board's transition graph is rejected with 422
`INVALID_TRANSITION`, and the rejection lists the allowed target columns by
name and id. Not available on `documents`.

### `tasks move`

```
kairos tasks move <SHORT_CODE> --to-board <BOARD> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. |
| `--to-board <BOARD>` | slug or UUID | required | Target delivery board. |
| `--rename` | flag | off | Give the task the next code of the target board. Kairos retires the old code and changes the references to it one time. |

The task lands in the target board's entry column and follows that board's
team. The command needs `manage_tasks` on both boards. The task keeps its
repository.
See [Move work between boards](../how-to/move-work-between-boards.md).

### `documents move`

```
kairos documents move <SHORT_CODE> --to-board <BOARD> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The short code of the document. |
| `--to-board <BOARD>` | slug or UUID | required | The new owner board. It can be a board of each level. |
| `--rename` | flag | off | Give the document the next code of the new owner board. |

The command changes the owner board of the document. The document gets no
column. The command needs `manage_documents` on the board that owns the
document now and on the new board. The creator of the document gets no right
to move it.

Each document has an owner board, so the command cannot remove it. The
command refuses `--no-board` as an unknown argument.

The command prints one of these lines:

```text
Kairos moved the document ACME-D-0004 to the owner board <board-id>.
Kairos did not change the document ACME-D-0004. Its owner board is <board-id> already.
```

### `<noun> delete`

```
kairos <noun> delete <SHORT_CODE> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--confirm` | flag | off | Required for the deletion to happen. Without it, nothing is deleted. |

The delete is a soft delete and cascades to the item's children. Deleted items
are hidden from `list` unless `--include-deleted` is passed, and are recoverable
with `restore`.

The cascade takes the descendants that you can edit
([the edit rule](capabilities.md#the-edit-rule)). It stops at a descendant that
you cannot edit, and takes nothing below it. The command names each descendant
that stays, and the reason. With `--json`, they are in `not_reached`.

### `<noun> restore`

```
kairos <noun> restore <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |

Puts an archived item back on its board. The item's own archived children stay
archived: restore is per item, not a cascade.

## Finding work

### `kairos search`

One command, no subcommands. Covers full-text search and relationship traversal
over all five entity types.

```
kairos search [OPTIONS]
```

Filters:

| Option | Type | Default | Description |
|---|---|---|---|
| `-q`, `--query <QUERY>` | string | none | Full-text query. Websearch semantics: quoted phrases, `OR`, `-negation`. |
| `--type <ENTITY_TYPE>` | `strategy` \| `initiative` \| `task` \| `document` \| `adr` | all | Entity type filter. Repeatable. |
| `--board <BOARD_ID>` | UUID | none | Restrict to items on this board. A document is not a card, so this filter gives no document. |
| `--column <COLUMN_ID>` | UUID | none | Restrict to items in this column. |
| `--team <TEAM_ID>` | UUID | none | Restrict to tasks assigned to this team. |
| `--repo <REPOSITORY>` | slug or UUID | none | Restrict to the items of this repository: the tasks that link to it, and the documents and the ADRs that impact it. |
| `--task-type <TASK_TYPE>` | `task` \| `bug` \| `tech_debt` \| `support` | all | Task type filter. Repeatable. |
| `--work-class <WORK_CLASS>` | `planned` \| `support` | all | Lane filter. Repeatable. |
| `--is-bucket <BOOL>` | `true` \| `false` | both | Restrict to bucket or non-bucket initiatives. |
| `--metadata <KEY=VALUE>` | `slug=value` | none | Metadata condition. Repeatable. Values support trailing-`*` globs, e.g. `component=auth*`. |
| `--after <RFC3339>` | RFC 3339 instant | none | Only items created strictly after this instant. |
| `--before <RFC3339>` | RFC 3339 instant | none | Only items created strictly before this instant. |
| `--include-deleted` | flag | off | Include archived items. Composes with `--query` and `--from`; archived hits are marked in the output. |

Traversal:

| Option | Type | Default | Description |
|---|---|---|---|
| `--from <SHORT_CODE>` | string | none | Starting entity, by short code. |
| `--from-id <UUID>` | UUID | none | Starting entity, by id. |
| `--relationships <REL>` | `parent` \| `supports` \| `informs` \| `supersedes` \| `blocks` | all | Relationship types to follow. Repeatable. |
| `--direction <DIRECTION>` | `outbound` \| `inbound` \| `both` | `outbound` | Edge direction. |
| `--depth <N>` | integer 1–10 | none | Maximum traversal depth. |

Ordering and paging:

| Option | Type | Default | Description |
|---|---|---|---|
| `--sort <FIELD>` | `created_at` \| `updated_at` \| `title` \| `relevance` | `relevance` with `--q`, else `created_at` | Sort field. `relevance` needs `--q` — there is nothing to be relevant to otherwise, and the request is refused rather than quietly reordered. |
| `--order <ORDER>` | `asc` \| `desc` | `desc` | Sort order. |
| `--limit <LIMIT>` | integer | server default 25, maximum 100 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Offset into the combined result set. |

Raw body:

| Option | Type | Default | Description |
|---|---|---|---|
| `--query-json <JSON>` | JSON, `@FILE`, or `-` | none | The full search request body as raw JSON. `@FILE` reads a file; `-` reads standard input. Cannot be combined with any other search flag. |

### `kairos boards list`

```
kairos boards list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

### `kairos boards show`

```
kairos boards show <BOARD> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | Board to show. |
| `--limit <LIMIT>` | integer | server default 200, maximum 1000 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Items to skip. |
| `--team <TEAM>` | slug or UUID | all | Show only the strategies and the initiatives of this team, from tasks or set by hand. Tasks and ADRs do not change. Do not use with `--no-team`. |
| `--no-team` | flag | off | Show only the strategies and the initiatives that have no team. |

Shows one page of the board's live items grouped by column. A strategy or an
initiative that has teams shows them after its title, for example
`[teams: skadi, weir]`. Archived items are
not shown and there is no flag to include them. The last lines give the total,
and they tell you when the page is a part of the board:

```
total: 340 (limit 200, offset 0)
The board has 340 items. This result shows 200 (limit 200, offset 0). To read the next part, use --offset 200.
```

Board capability grants are not part of the CLI surface. See
[Capabilities and access](../explanation/capabilities-and-access.md).

## Organization

### `kairos orgs show`

```
kairos orgs show [OPTIONS]
```

No arguments beyond the common options. Shows the organization the credentials
resolve to: id, slug, and the caller's role.

### `kairos members list`

```
kairos members list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

### `kairos members add`

```
kairos members add --email <EMAIL> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--email <EMAIL>` | string | required | The user's email. The person must have logged in at least once so that their account exists. |
| `--role <ROLE>` | `admin` \| `member` | `member` | Organization role. |

### `kairos members set-role`

```
kairos members set-role <USER_ID> --role <ROLE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<USER_ID>` | UUID | required | User id, from `kairos members list`. |
| `--role <ROLE>` | `admin` \| `member` | required | New role. |

Demoting the last admin is rejected with `LAST_ADMIN`.

### `kairos members remove`

```
kairos members remove <USER_ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<USER_ID>` | UUID | required | User id. |
| `--confirm` | flag | off | Required for the removal to happen. |

Removing the last admin is rejected with `LAST_ADMIN`.

`members` commands are org-admin operations.

### `kairos teams list`

```
kairos teams list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

### `kairos teams get`

```
kairos teams get <TEAM_ID> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<TEAM_ID>` | UUID | required | Team to show. |

### `kairos teams create`

```
kairos teams create --name <NAME> --slug <SLUG> --code-prefix <PREFIX> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Team name. |
| `--slug <SLUG>` | string | required | Team slug. The team's delivery board becomes `{slug}-delivery`. |
| `--code-prefix <PREFIX>` | string | required | The short-code prefix of the delivery board, for example `SKADI`. A capital letter, then 1 to 9 capital letters or digits. A task on the board gets the code `SKADI-T-0001`. The prefix does not change later. |
| `--type <TEAM_TYPE>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | `stream_aligned` | Team Topologies type. |

Creating a team also creates its delivery board.

### `kairos teams update`

```
kairos teams update <TEAM_ID> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM_ID>` | UUID | required | Team to update. |
| `--name <NAME>` | string | unchanged | New name. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--type <TEAM_TYPE>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | unchanged | New type. |

### `kairos teams delete`

```
kairos teams delete <TEAM_ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM_ID>` | UUID | required | Team to delete. |
| `--confirm` | flag | off | Required for the deletion to happen. |

A soft delete.

### `kairos teams members`

```
kairos teams members list   <TEAM_ID> [OPTIONS]
kairos teams members add    <TEAM_ID> --user <USER_ID> [OPTIONS]
kairos teams members remove <TEAM_ID> --user <USER_ID> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM_ID>` | UUID | required | The team. |
| `--user <USER_ID>` | UUID | required on `add` and `remove` | User id, from `kairos members list`. |

### `kairos teams of`

```
kairos teams of <SHORT_CODE> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The initiative or the strategy. |

Shows the teams of an initiative or a strategy, with the source of each team:
`from tasks`, `set by hand`, or both. An initiative gets the team of the board
of each live task below it. A strategy gets the teams of its initiatives, two
levels down. An item with no team prints one line that says so. See
[the teams of an initiative or a strategy](../explanation/teams-and-boards.md#the-teams-of-an-initiative-or-a-strategy).

### `kairos teams set`

```
kairos teams set <SHORT_CODE> <TEAM> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The initiative or the strategy. |
| `<TEAM>` | slug or UUID | required | A live team. |

Sets a team on an initiative or a strategy by hand, for example before it has
tasks. The team stays when tasks come. The caller must be able to edit the
item. The command prints this line:

```text
Kairos set the team weir on ACME-I-0003 by hand.
```

The server refuses a task, a document and an ADR with 422
`RELATIONSHIP_RULE`. It refuses a team that is set already with 422
`ALREADY_LINKED`.

### `kairos teams clear`

```
kairos teams clear <SHORT_CODE> <TEAM> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The initiative or the strategy. |
| `<TEAM>` | slug or UUID | required | A team that is set on the item by hand. It can be archived. |

Clears a team that is set on the item by hand. A team that the item gets from
its tasks stays. The server refuses a team that is not set by hand with 404.
When the item gets that team from its tasks, the message says so.

### `kairos streams list`

```
kairos streams list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

### `kairos streams get`

```
kairos streams get <STREAM_ID> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<STREAM_ID>` | UUID | required | Stream to show. |

### `kairos streams create`

```
kairos streams create --name <NAME> --slug <SLUG> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Stream name. |
| `--slug <SLUG>` | string | required | Stream slug. |
| `--description <DESCRIPTION>` | string | none | Description. |

### `kairos streams update`

```
kairos streams update <STREAM_ID> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM_ID>` | UUID | required | Stream to update. |
| `--name <NAME>` | string | unchanged | New name. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--description <DESCRIPTION>` | string | unchanged | New description. |

### `kairos streams delete`

```
kairos streams delete <STREAM_ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM_ID>` | UUID | required | Stream to delete. |
| `--confirm` | flag | off | Required for the deletion to happen. |

A soft delete.

### `kairos streams teams`

```
kairos streams teams list   <STREAM_ID> [OPTIONS]
kairos streams teams add    <STREAM_ID> --team <TEAM_ID> [OPTIONS]
kairos streams teams remove <STREAM_ID> --team <TEAM_ID> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM_ID>` | UUID | required | The stream. |
| `--team <TEAM_ID>` | UUID | required on `add` and `remove` | The team. |

### `kairos repos list`

```
kairos repos list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--team <TEAM>` | slug or UUID | all teams | Only this team's repositories. |

### `kairos repos get`

```
kairos repos get <REPOSITORY> [OPTIONS]
```

`kairos repos show` is the same command.

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--include-deleted` | flag | off | Also list the archived documents and ADRs that impact the repository, marked `[archived]`. |

Prints one table row with the columns `SLUG`, `FORGE`, `NAME`, `TEAM`,
`OWNER_BOARD`, `OPEN` and `WEBHOOK`. `kairos repos list` prints the same
columns. `OWNER_BOARD` is the delivery board of the owning team. `OPEN` is the
count of open tasks that link to the repository, on all boards.

After the row, the command prints the URL, the default branch and the webhook
connection. It also prints the status of the read token (`read token:`). The
status is
`not set`, or who set the token, when, and the result of the last check. The
command never prints the token. The line `index builder:` gives the setting
`code_index_build` (`on` or `off`). Then it prints three sections:

- the how-to-work-here description
- the documents and the ADRs that impact the repository
- the in-flight branches and pull requests

Each document or ADR is one line. The line has the short code, the lifecycle
or the column, the title, and the kind of the item:

```text
Documents and ADRs that impact this repository:
  ACME-D-0004 [published] The vision of fidius — document (vision)
  ACME-A-0002 [Decided] Plugins are dynamic libraries — adr
```

The section shows `(none)` when no item impacts the repository.

### `kairos repos create`

```
kairos repos create --forge <FORGE> --name <FULL_NAME> --repo-url <URL> --team <TEAM> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--forge <FORGE>` | `github` \| `gitlab` \| `other` | required | The forge. |
| `--name <FULL_NAME>` | `owner/repo` | required | The name on the forge, as the forge sends it in webhooks. |
| `--repo-url <URL>` | string | required | Browser URL of the repository: `http` or `https`. |
| `--team <TEAM>` | slug or UUID | required | Owning team. |
| `--slug <SLUG>` | string | derived from `--name` | Slug. |
| `--default-branch <BRANCH>` | string | `main` | Default branch: a branch name that git accepts. |
| `--description <DESCRIPTION>` | string | none | Short "how to work here" blurb for agents. |

Permitted to an organization admin or a member of the owning team.

The server refuses a `--name`, a `--repo-url` or a `--default-branch` that does
not have the form of its field. The glossary gives the rules: see
[the form of the fields](glossary.md#the-form-of-the-fields-of-a-repository).

### `kairos repos update`

```
kairos repos update <REPOSITORY> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--repo-url <URL>` | string | unchanged | New browser URL. |
| `--default-branch <BRANCH>` | string | unchanged | New default branch. |
| `--team <TEAM>` | slug or UUID | unchanged | New owning team. Re-homes the repository. |
| `--description <DESCRIPTION>` | string | unchanged | New description. |
| `--code-index-build <ON\|OFF>` | `on` or `off` | unchanged | With `off`, the code index builder makes no index of the repository. Another value is refused. See [The base code index](configuration.md#stop-the-builder-for-one-repository). |
| `--code-index-summaries <EMBEDDED\|HOSTED>` | `embedded` or `hosted` | unchanged | With `hosted`, the summaries of the repository come from the provider of the organization, and the code of each changed symbol leaves the host. Kairos refuses `hosted` when the organization has no hosted provider (`CODE_INDEX_NO_HOSTED_PROVIDER`). See [the providers](configuration.md#the-providers-of-the-summaries-and-the-vectors). |

Same permission gate as `create`. The rules of `--repo-url` and
`--default-branch` are those of `create`. They apply to a value that is
different from the value of the repository.

When each value is the value that the repository has, the server writes
nothing. The command prints this line:

```text
Kairos did not change the repository payments-api. It has these values already.
```

### `kairos repos delete`

```
kairos repos delete <REPOSITORY> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--confirm` | flag | off | Required for the removal to happen. |

Organization admin only. Refused while any task or webhook connection still references
the repository.

### `kairos repos bind`

```
kairos repos bind <SHORT_CODE> <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task. |
| `<REPOSITORY>` | slug or UUID | required | The repository. It can be any repository, of any team. |

The command sets the repository that the task links to. The board and the team
of the task do not change.

### `kairos repos unbind`

```
kairos repos unbind <SHORT_CODE> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task whose repository binding is cleared. |

The board and the team of the task do not change.

### `kairos repos link`

```
kairos repos link <SHORT_CODE> <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The document or the ADR. |
| `<REPOSITORY>` | slug or UUID | required | The repository. It can be any live repository, of any team. |

The command says that a document or an ADR impacts a repository. The link says
what the item is about. It gives no right on the item, and no right on the
repository.

The caller must be able to edit the document or the ADR. The caller needs no
right on the repository. The command prints this line:

```text
Kairos made the link: ACME-D-0004 impacts the repository fidius.
```

The server refuses a task, a strategy and an initiative with 422
`RELATIONSHIP_RULE`. To link a task to a repository, use `kairos repos bind`.
The server refuses a link that is there with 422 `ALREADY_LINKED`.

### `kairos repos unlink`

```
kairos repos unlink <SHORT_CODE> <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The document or the ADR. |
| `<REPOSITORY>` | slug or UUID | required | The repository. It can be archived. |

The command removes an `impacts` link. The rule is that of `kairos repos
link`. The command prints this line:

```text
Kairos removed the link: ACME-D-0004 does not impact the repository fidius.
```

The server gives 404 `NOT_FOUND` for a link that is not there.

### `kairos repos reindex`

```text
kairos repos reindex <REPOSITORY> [OPTIONS]
```

Ask Kairos to build the code index of a repository again. The builder of
the server makes a full index of the head of the default branch. The new
index replaces the index of that commit. The command shows the run. An
organization admin or a member of the owner team uses it.

Kairos refuses the command in these cases:

- `CODE_INDEX_BUILD_RUNNING`: a run of the repository is active.
- `CODE_INDEX_BUILD_OFF`: the repository has the builder off.
- `CODE_INDEX_BUILDER_OFF`: the deployment has no builder.

### `kairos repos builds`

```text
kairos repos builds <REPOSITORY> [--limit <N>] [OPTIONS]
```

Show the runs of the code index builder for a repository, newest first. A
run is a build after a push, a first build, a build on request, or an
upload. Each run shows its result. `--limit` gives the most runs to show:
20 when not given, 100 at most. Each member can read the runs.

| Column | Meaning |
|---|---|
| `STARTED` | When the run started. |
| `TRIGGER` | `push`, `first`, `request` or `upload`. |
| `OUTCOME` | `running`, `ok` or `failed`. |
| `COMMIT` | The first 12 characters of the commit, when the run has one. |
| `SYMBOLS` | The symbols of the index that the run wrote. |
| `DETAIL` | The text of a failure, or when the run ended. |

### `kairos repos credential set`

```
kairos repos credential set <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |

Sets or replaces the read token of the repository. The builder of the code
index gives the token to git to fetch a private repository. You must be an
organization admin or a member of the owner team.

The command reads the token from standard input. When standard input is a
terminal, the command asks for the token and does not show it. The command
has no argument for the token, so the token is not in the shell history:

```sh
printf '%s' "$TOKEN" | kairos repos credential set skadi
```

Use a GitHub fine-grained personal access token with only the permission
"Contents: read" on the one repository. The server refuses the token with 501
`SECRETS_NOT_CONFIGURED` when the deployment has no `KAIROS_SECRETS_KEY`.

### `kairos repos credential remove`

```
kairos repos credential remove <REPOSITORY> [OPTIONS]
```

Removes the read token. The next fetch has no credential. The server gives 404
`NOT_FOUND` when the repository has no token.

### `kairos repos credential check`

```
kairos repos credential check <REPOSITORY> [OPTIONS]
```

The server runs `git ls-remote` on the URL of the repository with the token,
and keeps the result. The command prints the status. It fails when git cannot
read the repository with the token. The error of git does not contain the
token.

Repositories are the codebases that tasks link to. See
[Repositories as execution scope](../explanation/repositories-as-execution-scope.md).

## Machine access

Service accounts are machine principals authenticated by API keys.

### `kairos service-accounts create`

```
kairos service-accounts create --name <NAME> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Operator label, e.g. `ci-deploy`. |

### `kairos service-accounts list`

```
kairos service-accounts list [OPTIONS]
```

No arguments beyond the common options.

### `kairos service-accounts delete`

```
kairos service-accounts delete <ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<ID>` | UUID | required | Service account id, from `kairos service-accounts list`. |
| `--confirm` | flag | off | Required for the deletion to happen. |

Deletes the service account and all of its keys.

### `kairos keys create`

```
kairos keys create --service-account <SERVICE_ACCOUNT> --name <NAME> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--service-account <SERVICE_ACCOUNT>` | UUID | required | The service account. |
| `--name <NAME>` | string | required | Operator label for the key, e.g. `gha-main`. |
| `--expires-at <EXPIRES_AT>` | RFC 3339 instant | no expiry | Expiry, e.g. `2027-01-01T00:00:00Z`. |

The raw key is printed once and is not retrievable afterwards.

### `kairos keys list`

```
kairos keys list --service-account <SERVICE_ACCOUNT> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--service-account <SERVICE_ACCOUNT>` | UUID | required | The service account. |

Key prefixes only; the secret is never returned.

### `kairos keys revoke`

```
kairos keys revoke <KEY_ID> --service-account <SERVICE_ACCOUNT> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<KEY_ID>` | UUID | required | Key id, from `kairos keys list`. |
| `--service-account <SERVICE_ACCOUNT>` | UUID | required | The service account the key belongs to. |
| `--confirm` | flag | off | Required for the revocation to happen. |

## Deployment administration

`admin tenants` is cross-tenant tenant provisioning, restricted to deployment
admins: the caller's OIDC subject must be listed in the server's
`KAIROS_DEPLOYMENT_ADMINS`. Otherwise these routes return 403.

### `kairos admin tenants list`

```
kairos admin tenants list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

### `kairos admin tenants create`

```
kairos admin tenants create --slug <SLUG> --name <NAME> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--slug <SLUG>` | string matching `^[a-z][a-z0-9_-]{1,62}$` | required | Organization slug. |
| `--name <NAME>` | string | required | Organization display name. |
| `--initial-admin <OIDC_SUB>` | string | the caller | OIDC `sub` of the initial organization admin. That user must have logged in at least once. |

Provisions the organization row, the schema and the default boards.

### `kairos admin tenants delete`

```
kairos admin tenants delete <SLUG> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SLUG>` | string | required | The tenant's slug. |
| `--confirm` | flag | off | Required for the drop to happen. |

Drops the tenant's organization and schema. Destructive and unrecoverable.

## Operator subcommands (`kairos-server`)

These are not `kairos` commands. They are subcommands of the **server binary**, run
against the database with no login and no running server, which is both what makes
them useful and why they are dangerous. In Kubernetes, `kubectl exec deploy/kairos --
kairos-server <subcommand>`; in Compose, `docker compose run --rm kairos <subcommand>`.

Everything here needs `DATABASE_URL` and applies pending public migrations first, with
two deliberate exceptions noted below.

| Subcommand | What it does |
|---|---|
| `serve` | Run the server. What the container's entrypoint invokes. Before it serves, it applies the pending public migrations, then the pending tenant migrations of each organization, under a database lock. An organization whose migration fails answers 503 `TENANT_NOT_READY` until its schema is current. |
| `migrate` | Apply pending public migrations and exit. |
| `create-tenant --slug <slug> [--name <name>]` | Provision an organization, its schema and its default boards. |
| `drop-tenant --slug <slug> --confirm` | Destroy a tenant: schema CASCADE plus the organization row. Refuses without `--confirm`. **Unrecoverable.** |
| `migrate-tenants` | Apply pending tenant migrations in every tenant schema, under the same lock as `serve`. It goes on after an organization that fails, and exits non-zero naming each one. |
| `list-tenants` | List provisioned tenants. |
| `check-delivery-boards` | List each team with 2 or more live delivery boards. **Only reads.** See below. |
| `set-password --email <email> [--password <pw>]` | Set a local account's password. See below. |
| `hash-password [--password <pw>]` | Print a PHC hash of a password and nothing else. **Needs no database.** |
| `seed-demo [--force]` | Seed the `demo` fixture tenant. |
| `embed-backfill`, `embed-index` | Retrieval maintenance; see [Configure retrieval](../how-to/configure-retrieval.md). |

### `set-password` — the break-glass path

```
kairos-server set-password --email <email> [--password <password>]
```

For the case the GUI cannot help with: the sole admin of a local-auth deployment has
forgotten their password, and there is no reset email. It therefore **cannot require a
login**, which is why it lives here rather than in `kairos`.

Omit `--password` and it asks for the password. Prefer that: an argument is visible in `ps`,
in your shell history, and in a container's command line. On a terminal the command
does not show the password that you type. When stdin is a pipe, the command reads one
line from it.

It **will not create an account**. Creating one would make this a way to mint an admin
on any deployment whose database you can reach; the empty-deployment case is
`KAIROS_BOOTSTRAP_ADMIN`, which is single-use. It also refuses a service account, which
authenticates with API keys, and a password under 12 characters.

Setting a password **revokes every session that person holds**, and the command says how
many. That is the point of running it after a suspected compromise.

### `check-delivery-boards` — a report on old data

```
kairos-server check-delivery-boards
```

A team has one delivery board. An earlier version of the API permitted a second
board. Old data can thus have a team with 2 or more live delivery boards. The
command looks in each tenant, and it prints each such team and the boards of
the team:

```text
acme: team platform (5b0c…) has 2 live delivery boards:
acme:   platform-delivery (91e2…) created 2026-03-02T10:15:00+00:00
acme:   platform-extra (c47a…) created 2026-06-11T08:30:00+00:00
1 team(s) with 2 or more live delivery boards in 1 tenant(s)
```

A deleted team that has live boards is in the report, with the word `deleted`.

The command only reads. It does not apply migrations, and its transaction is
read-only. The exit code is 0 when the report is complete, with or without teams
in it.

To correct a team, move the cards to the board that stays, then delete the other
board with `DELETE /api/boards/{id}`. The delete of the team is a second
procedure. It removes the team and each delivery board of the team together.

### `hash-password` — before the deployment exists

```
kairos-server hash-password [--password <password>]
```

Prints a PHC string on stdout and nothing else, so `kairos-server hash-password >
secret` contains exactly the hash. It is the only subcommand that does **not** connect to the
database, because its whole purpose is to produce a value for
`KAIROS_BOOTSTRAP_PASSWORD_HASH` before there is a deployment to talk to — so the
plaintext password never has to be written into a manifest.

## Files

| Path | Mode | Contents |
|---|---|---|
| config directory | `0700` | Resolved from `KAIROS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/kairos`, else `$HOME/.config/kairos`. |
| `credentials.json` inside it | `0600` | The credential cache, keyed by normalized deployment URL. Holds issuer tokens and local sessions. |

[Configuration](configuration.md#cli-configuration) is the canonical entry for
the resolution order, the file's full shape, and the refresh behaviour.

## Related reading

- [Flight levels](../explanation/flight-levels.md)
- [Teams and boards](../explanation/teams-and-boards.md)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Archiving](../explanation/archiving.md)
- [Repositories as execution scope](../explanation/repositories-as-execution-scope.md)
- [Configuration](configuration.md)
- [Glossary](glossary.md)

## Related guides

- [Give an agent machine access](../how-to/give-an-agent-machine-access.md)
- [Provision a tenant](../how-to/provision-a-tenant.md)
- [Connect a git forge](../how-to/connect-a-git-forge.md)
- [Move work between boards](../how-to/move-work-between-boards.md)
- [Find archived work](../how-to/find-archived-work.md)
