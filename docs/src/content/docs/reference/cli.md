---
title: "CLI"
---

`kairos` is the command-line client. It talks to the same HTTP API as the GUI
and the MCP server.

This page describes `kairos` 0.18.1. The command tree below mirrors
`kairos --help`. A unit test of `kairos-cli` checks that each command has a
section or a synopsis line here.

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
| Work items | `strategies`, `initiatives`, `tasks`, `documents`, `adrs`, `state`, `link`, `unlink` |
| Finding work | `me`, `search`, `boards` |
| Organization | `orgs`, `members`, `teams`, `streams`, `repos`, `grants`, `metadata`, `templates` |
| Published sites | `pages` |
| Machine access | `service-accounts`, `keys`, `admin agent-keys` |
| Code index settings | `admin code-index-settings` |
| The job queue | `admin jobs` |
| Deployment administration | `admin tenants` |
| The code index of a checkout | `index` |
| The CLI itself | `self-update` |

`boards` also has the verbs that change a board: `create`, `columns`,
`transitions`, `wip-limit`, `dispatch` and `archive-completed`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | API or validation error: not found, forbidden, conflict, invalid transition, bad input. A transport failure: the deployment or the OIDC issuer gave no answer, also during a token refresh. Also the case of several cached deployments with no `--url`. `adrs check`: a citation fails (the report is on stdout). |
| 2 | Authentication error: no cached credentials, expired or rejected credentials, a token refresh that the issuer refused, an expired local session, a rejected email and password, HTTP 401, a corrupted credential cache. |
| 3 | `self-update --check`: the server is newer than the CLI. |
| 64 | Usage error: an argument or a command that this CLI does not know, a missing argument, two options that exclude each other. The message is the one of the argument parser. An older CLI exits with this code for a new option. |

`--help` and `--version` exit with 0.

Structured API rejections are rendered with their actionable detail: a 409
`CONFLICT` prints the server-current version and title, a 422
`INVALID_TRANSITION` prints the allowed target columns with their ids, a 403
prints the required board capability when the server names one, and a 422
`LAST_ADMIN` prints the keep-one-admin constraint. A 401 gives the `kairos
login` command with the URL of the deployment. A 403 `MEMBERSHIP_REQUIRED`
says that you are not a member of the organization. A body that the CLI
cannot read names `kairos self-update --check`.

### Errors with `--json`

With `--json`, an error goes to stderr as one JSON object, the error envelope
of the API:

```json
{"error": {"code": "VALIDATION", "message": "422 VALIDATION: team_id is not a UUID", "details": {"field": "team_id"}}}
```

`code` and `details` are the ones of the deployment when the deployment
refused the request. `message` is the text that the CLI prints without
`--json`. A failure of the CLI itself has one of these codes and `null`
details:

| Code | Meaning | Exit code |
|---|---|---|
| `LOGIN_REQUIRED` | No credentials, expired credentials, or a refresh that the issuer refused. | 2 |
| `TRANSPORT` | The deployment or the issuer gave no answer. | 1 |
| `BAD_RESPONSE` | The CLI cannot read the answer of the deployment. | 1 |
| `CLI_ERROR` | Any other failure of the CLI: a bad value, a file, a missing `--confirm`. | 1 |

A usage error (exit code 64) is not JSON: the argument parser prints it
before the CLI reads `--json`. Stdout stays empty on an error.

A refusal of `kairos index` has a code of its own, with details. See
[The JSON of `kairos index`](#the-json-of-kairos-index).

## Common options

Every command that reaches the API accepts these three. They are omitted from
the per-command tables below.

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | `KAIROS_URL`, then the binding of the current folder (see [`config.toml`](../configuration/#configtoml)), then the only cached deployment | Deployment base URL. Required when more than one deployment is cached and nothing else names one. |
| `--tenant <TENANT>` | string | the binding of the current folder, then the tenant cached at login (none with a key) | Tenant slug, sent as the `X-Tenant` header. Used by deployments that resolve tenants by header rather than by host subdomain. |
| `--json` | flag | off | Print JSON instead of the human-readable table. A list prints one envelope: see [the JSON of a list](#the-json-of-a-list). |

`--url` values are normalized by trimming whitespace and trailing slashes, so
`https://kairos.example/` and `https://kairos.example` address the same cached
entry.

## Names, slugs and flag names

Each command that names a board, a column, a team, a user, a template, a
service account or a stream takes a name or a slug. It also takes the id
(UUID). A value with the form of a UUID is an id (KAIROS-T-0589).

| Reference | You can give | How the CLI finds it |
|---|---|---|
| board | slug or id | The server takes the slug. |
| column | name or id | One read of the board. The case of the name does not count. An unknown name is refused, and the error names each column of the board. |
| team | slug or id | One read of the team. |
| user | id, email or user name | The server takes each form. |
| template | slug or id | One read of the template. |
| service account | name or id | One read of the service accounts. Two accounts with the same name are refused: send the id. |
| stream | slug, name or id | One read of the streams. |

One concept has one flag name in each command:

| Concept | Flag | Old name (it still works) |
|---|---|---|
| a repository | `--repo` | `--repository` on `boards dispatch add-rule` and `index` |
| the kind of a task | `--kind` | `--type` on `tasks create`, `--task-type` on `search` and `boards dispatch add-rule` |
| the kind of a team | `--kind` | `--type` on `teams create` and `teams update` |
| a column or a person as the target | `--to` | — |
| the last day of a range | `--until` | `--to` on `boards flow` |
| a file with the markdown text | `--content-file`, `--text-file` | — |

The verb `show` shows one thing. `get` is its old name, and it still works.
The verb `archive` puts a work item away; `delete` is its old name on items. The
verb `delete` removes a structure: a team, a repository, a stream or a service
account.

`transition` prints the name of the column after the move, not its id.
`boards list` prints the slug of the team. `teams delete`, `repos delete`,
`streams delete`, `service-accounts delete` and `members remove` print the
reference that you gave.

## Authentication

A deployment lets people log in through an OIDC issuer, with local accounts, or
with both. `kairos login` has one mode for each. Both modes write the same
credential cache, so every other command works the same after either.

A command can also use a key in place of the login.

| Variable | Description |
|---|---|
| `KAIROS_KEY`, then `KAIROS_MCP_KEY` | An agent key (it acts as you) or a service-account key. When one is set, each command that reaches the API sends it as the bearer. The key wins over a login. |
| `KAIROS_URL` | The deployment, when `--url` is not given. `login` and `logout` also read it. With a key and no URL, the CLI uses the only cached deployment. Otherwise it exits with code 2. |
| `KAIROS_QUIET` | `1` stops the warning of a key that hides a login. |
| `KAIROS_TIMEOUT` | The time in seconds that a command waits for an answer. The default is 60. A connection gets 5 seconds. A value that is not a positive integer stops the command with exit code 1. |
| `KAIROS_UPDATE_CHECK` | `0` turns off the daily version check (see [`kairos self-update`](#kairos-self-update)). |
| `KAIROS_AUTO_UPDATE` | `1` makes the daily check install the newer version of the server instead of printing a line. It does not install from an `http://` deployment. |
| `KAIROS_RELEASES_URL` | The base URL of the published releases, for a mirror. `self-update` takes each checksum from it, and the tarballs of `--version` and `--latest`. The default is the GitHub releases. |

A key carries its tenant, so it needs no `--tenant`. A 401 for a key names the
variable that holds it.

A key can hide a login. When the cache has a login for the deployment of the
key, each command writes one line on stderr:

```
warning: KAIROS_MCP_KEY is set, so the CLI sends that key and not the login for <URL>. Set KAIROS_QUIET=1 to hide this warning.
```

The command still sends the key. To use the login, unset the variable.
`kairos whoami` shows which credential a command sends. Use a key when the issuer gives the CLI no device
grant, for example Google. See
[Give an agent your key](../../how-to/give-an-agent-your-key/#use-the-key-with-the-cli).

### `kairos login`

Logs in to a deployment and caches the credential.

```
kairos login [--url <URL>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | `KAIROS_URL` | Deployment base URL, e.g. `https://kairos.example.com`. With neither, the command exits with code 1. |
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
| `--email` with `--issuer`, `--client-id` or `--bearer` | 64 | A usage error from the argument parser. |
| `--password` | 64 | A usage error from the argument parser. |
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
| `--url <URL>` | string | `KAIROS_URL`, then the only cached deployment | Deployment whose credentials are forgotten. |

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

The default rendering prints these lines: `user:` with display name and email,
`org:` with the organization slug and the caller's role, and `teams:` with the
team names, or `(none)`. A caller with a grant of `manage_structure` gets the
line `manage_structure:`. It gives the scope of each grant: `organization` or
`board <slug>`. The last line, `credential:`, tells the source of the
credential:

| Source | Line |
|---|---|
| A key in the environment | `credential: key from KAIROS_MCP_KEY` (or `KAIROS_KEY`) |
| The credential cache | `credential: login cached for <URL>` |

A request with an agent key gets the line `key scope:` before `credential:`.
It gives the capabilities and the boards of the key, for example
`key scope: manage_tasks, transition_items on each board`. A key with no
scope gets `key scope: none`.

An agent key and a service-account key have the same format, so the line
says `key`. `--json` prints the response of the server only, with no
`credential:` line.

The `/api/whoami` response carries more than the default rendering prints. The
board capabilities the principal holds, the capabilities every member holds
implicitly, and the caller's teams' repositories are available under `--json`
only.

`whoami` works the same with issuer tokens, with a local session and with a
key in `KAIROS_KEY` or `KAIROS_MCP_KEY`. The CLI
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
| `show` (alias `get`) | yes | yes | yes | yes | yes |
| `history` | yes | yes | yes | yes | yes |
| `create` | yes | yes | yes | yes | yes |
| `edit` | yes | yes | yes | yes | yes |
| `note` | yes | yes | yes | yes | yes |
| `promote-note` | yes | yes | yes | yes | yes |
| `diff` | yes | yes | yes | yes | yes |
| `links` | yes | yes | yes | yes | yes |
| `transition` | yes | yes | yes | no | yes |
| `move` | no | no | yes | yes | no |
| `claim` | no | no | yes | no | no |
| `hand-off` | no | no | yes | no | no |
| `release` | no | no | yes | no | no |
| `cancel` | no | no | yes | no | no |
| `reopen` | no | no | yes | no | no |
| `lane` | no | no | yes | no | no |
| `review` | no | no | yes | no | no |
| `set-parent` | no | yes | yes | no | no |
| `complexity` | no | yes | no | no | no |
| `lifecycle` | no | no | no | yes | no |
| `metadata` | yes | yes | yes | yes | yes |
| `archive` (alias `delete`) | yes | yes | yes | yes | yes |
| `purge` | no | no | yes | no | no |
| `restore` | yes | yes | yes | yes | yes |
| `check` | no | no | no | no | yes |

`adrs check` checks the ADR citations of a tree; see
[`adrs check`](#adrs-check). `adr` is an alias of the noun `adrs`.

`documents` has no `transition` verb: documents have no board placement, and
carry an editorial lifecycle instead of a column. `documents lifecycle` sets
it. `move` exists on `tasks`
and on `documents`. `tasks move` moves a task to a different delivery board.
`documents move` gives a document to a different owner board. See
[Flight levels](../../explanation/flight-levels/) and
[Teams and boards](../../explanation/teams-and-boards/).

### `<noun> list`

```
kairos <noun> list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |
| `--include-deleted` | flag | off | Also list archived (put-away) items, marked `[archived]` in the `CODE` column. Without the flag, only live items are returned. |

`documents list` and `adrs list` have one more option:

| Option | Type | Default | Description |
|---|---|---|---|
| `--repo <REPOSITORY>` | slug or UUID | none | Only the items that impact this repository. The repository must be live. |

Among the `list` verbs, `--include-deleted` exists on these five only: the
organization nouns (`boards`, `teams`, `members`, `streams`, `admin tenants`)
have no archived mode. `kairos search` carries the same flag. See
[Archiving](../../explanation/archiving/).

#### The pages of a list

A `list` verb prints one page. After the table, it prints the count and the
page:

```
total: 340 (limit 200, offset 0)
Next: --offset 200
```

The `Next:` line is there only when more rows follow the page. To read the
next page, run the command again with that option. The last page has no
`Next:` line.

`--all` reads each page. The CLI asks for one page after the other, with the
maximum page size of the server. It prints each row, and then `total: N`.
`--all` with `--limit` or `--offset` is a usage error (exit 64).

These verbs page so: `<noun> list`, `boards list`,
`members list`, `teams list`, `streams list`, `repos list`, `metadata list`,
`templates list`, `admin agent-keys list`, `admin tenants list` and
`kairos search`.

#### The JSON of a list

With `--json`, each `list` verb and `kairos search` print one envelope
(KAIROS-A-0057):

```json
{
  "items": [ ... ],
  "total": 340,
  "next_offset": 200
}
```

| Key | Description |
|---|---|
| `items` | The rows of the page. With `--all`, each row. |
| `total` | The count of the rows that match, before the page. |
| `next_offset` | The `--offset` of the next page. It is `null` on the last page and with `--all`. |

A list that the server does not page has the same envelope. Its `total` is
the count of the rows, and `next_offset` is `null`. These lists are
`grants list`, `keys list`, `service-accounts list`,
`boards columns list`, `boards transitions list`, `teams members list`,
`teams pages list` and `streams teams list`.

`kairos search` gives the hits in one `items` list, in the order of the
groups: strategies, initiatives, tasks, documents, ADRs. Each hit has an
`entity_type` key: `strategy`, `initiative`, `task`, `document` or `adr`.

`dispatch list` gives two lists, and keeps its shape:
`{"rules": [...], "targets": [...]}`.

A script must accept a key that it does not know: a new key can come later.

This envelope changes the `--json` shape of the lists (KAIROS-T-0644).
Before, a paged list printed the envelope of the server (`items`, `total`,
`limit`, `offset`). `search` printed the groups in `results`. Some lists
printed a bare array. A script that reads `results.tasks`, a bare array,
`limit` or `offset` must change. Read `items`, and use `next_offset` for the
next page.

### `<noun> show`

```
kairos <noun> show <SHORT_CODE> [OPTIONS]
```

`get` is the old name of `show`, and it still works.

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code, e.g. `ACME-T-0001`. |

Prints the item including its markdown content. The notes of the item
follow the content, each one under its heading: `## Note n-3 (2026-10-11,
Robin)`. With `--json`, the item has the array `notes`, and `content` has no
note.

A retired short code finds the item. Then the command writes a notice on
stderr. The notice names the current code, for example `The code
COLLIERY-T-0100 is retired. The current code of this item is SKADI-T-0001.`
The output on stdout is the item, and `--json` stays clean.

In the text output, the command prints three more lists before the content
(KAIROS-T-0591):

| Line | Description |
|---|---|
| `metadata` | Each metadata value of the item, as `slug: value`. |
| `relationships` | Each relationship of the item, from the side of the item. |
| `external` | Each external reference of the item, as `label <uri>`. |

An empty list prints `-`. The labels of `relationships` are `parent`,
`child`, `blocked by`, `blocks`, `supported by`, `supports`, `informed by`,
`informs`, `superseded by`, `supersedes`, `amended by` and `amends`. A
neighbour has the marks
`[archived]`, `[cancelled]` or `[done]`. For example:

```text
  metadata:
    priority: high
  relationships:
    parent: ACME-I-0002 — Checkout redesign
    blocked by: ACME-T-0003 — Payment API [done]
    supported by: ACME-D-0004 — Rollout note
  external: -
```

The command reads the three lists with three more requests. With `--json`,
the command prints the item of the server only, and makes no more requests.

`tasks show` prints the line `repository`. It has the slug of the repository
of the task, or `-`.

`documents show` prints the line `owner board`. It has the id of the owner
board of the document.

`documents show` and `adrs show` print the line `impacts`. It has the slugs of
the repositories that the item impacts, or `-`. The mark `[archived]` shows a
repository that is not live.

### `<noun> history`

```
kairos <noun> history <SHORT_CODE> [--limit <LIMIT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. A retired code finds the item. |
| `--limit <LIMIT>` | integer | 20, maximum 200 | Maximum rows of each list: the versions and the activity. |

Prints the history of the item, newest first. The output has the same lines
as the MCP tool [`get_history`](../mcp-tools/#get_history). First come the
content versions, with the time and the editor. Then the section
`## Activity` comes. Each line tells what changed, the time, and who did it.
The activity has moves, claims, hand-offs, releases, the lane, metadata
values, state writes, links and more:

```text
# History of ACME-T-0001
- v2 — 2026-10-17T09:00:00Z by Robin

## Activity
- claim handed off: Robin -> Sam — 2026-10-17T10:05:00Z by Robin
- claim taken: Robin — 2026-10-17T10:00:00Z by Robin
- moved from Todo to Active — 2026-10-17T10:00:00Z by Robin
- 3 older rows are not shown. Use a larger --limit to see them.
```

With `--json`, the command prints one object with two keys: `versions` and
`activity`. Each key has a list in the shape of the `list` verbs: `items`,
`total` and `next_offset`.

### `<noun> create`

```
kairos <noun> create --title <TITLE> [OPTIONS]
```

Options common to every noun:

| Option | Type | Default | Description |
|---|---|---|---|
| `--title <TITLE>` | string | required | The item's title. |
| `--content <CONTENT>` | string | `""`; on `documents`, no default | Markdown content. On `documents`, content omitted together with `--template` stamps the template's content. Conflicts with `--content-file`. |
| `--content-file <FILE>` | path | — | Read the markdown content from a file. `-` reads standard input. Conflicts with `--content`. |

Board placement per noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--board <BOARD>` | slug or UUID | required | Board to create the strategy on. |
| `strategies` | `--column <COLUMN>` | name or UUID | the board's first column | Column to place it in. |
| `initiatives` | `--board <BOARD>` | slug or UUID | required | Board to create the initiative on. |
| `initiatives` | `--column <COLUMN>` | name or UUID | the board's first column | Only the entry column (Discovery). An initiative starts there, and a different column is refused with `VALIDATION` (KAIROS-T-0643). |
| `tasks` | `--board <BOARD>` | slug or UUID | required unless `--team` is given | Delivery board to create the task on. The board decides the team of the task. With `--team` and no `--board`, the task goes to the delivery board of that team. |
| `tasks` | `--column <COLUMN>` | name or UUID | the board's first column | Only the entry column. A name is a column of `--board`, else of the delivery board of `--team`. A task starts there, and a different column is refused with `VALIDATION`, for each caller (KAIROS-T-0566). Move the task after the create. |
| `documents` | `--board <BOARD>` | slug or UUID | required | The owner board of the document. It gives the right to edit the document, and the code of the document gets the prefix of this board. The document is not a card of the board. |
| `documents` | `--parent <SHORT_CODE>` | string | none | The workflow item the document supports. It does not give the document an owner board. |
| `adrs` | `--board <BOARD>` | slug or UUID | none | ADR board. Omitting it creates an off-board ADR, which is an org-admin operation. |
| `adrs` | `--column <COLUMN>` | name (with `--board`) or UUID | the board's first column | Column to place it in. |

Options specific to one noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--hypothesis <HYPOTHESIS>` | string | none | The strategy's hypothesis. |
| `initiatives` | `--complexity <COMPLEXITY>` | `xs` \| `s` \| `m` \| `l` \| `xl` | none | T-shirt sizing. [`initiatives complexity`](#initiatives-complexity) changes it later. |
| `initiatives` | `--bucket-type <KIND>` | `tech_debt` \| `bug` \| `ad_hoc` | none | Marks the initiative as a bucket of this kind. |
| `initiatives` | `--parent <STRATEGY>` | short code | none | The parent strategy. Kairos writes the initiative and the `parent` edge in one transaction: a refused parent leaves no initiative (KAIROS-T-0572). |
| `tasks` | `--kind <KIND>` | `task` \| `bug` \| `tech_debt` \| `support` | `task` | The kind of the task. The old name `--type` still works. |
| `tasks` | `--work-class <WORK_CLASS>` | `planned` \| `support` | `support` for support-type tasks, `planned` otherwise; always `support` on a board that the caller does not manage | Planned/Support lane. On a board that the caller does not manage, `planned` is refused with 403. |
| `tasks` | `--from-team <TEAM>` | slug or UUID | none | The team that sends the task. When it is not the team of the board, the task is a request: entry column, support lane, also for a manager of the board. With it, `planned` and a column other than the entry column are refused with 422. Kairos does not keep it on the task: name the sending team in the body. |
| `tasks` | `--team <TEAM>` | slug or UUID | none | Team. Without `--board`, the task goes to the delivery board of this team. With `--board`, it must be the team of that board. |
| `tasks` | `--repo <REPOSITORY>` | slug or UUID | none | Repository the task links to. It can be any repository, of any team. It does not choose the board. |
| `tasks` | `--parent <INITIATIVE>` | short code | none | The parent initiative. Kairos writes the task and the `parent` edge in one transaction: a refused parent leaves no task (KAIROS-T-0572). In a claims column, `parent_gate` applies. |
| `documents` | `--template <TEMPLATE>` | slug or UUID | none | Template to stamp content and metadata defaults from. |
| `adrs` | `--decision-maker <DECISION_MAKER>` | string | none | Decision maker. |
| `adrs` | `--decision-date <DATE>` | `YYYY-MM-DD` | none | Decision date. |
| `adrs` | `--parent <SHORT_CODE>` | short code | none | The strategy, initiative or task that the ADR supports. Kairos writes the ADR and the `supports` edge in one transaction: a refused parent leaves no ADR (KAIROS-T-0591). |

`tasks create` needs `--board` or `--team`. `--repo` does not replace them.

`--parent` is on each `create` but `strategies create`. A strategy is the top
of the hierarchy, so it has no parent. To link an item after the create, use
[`kairos link`](#kairos-link).

`documents create` needs `--board`: each document has an owner board. With
`--parent` too, the document also supports that item. The command needs
`manage_documents` on the owner board. To say that the document impacts a
repository, use [`kairos repos link`](#kairos-repos-link).

A caller without `manage_tasks` on the board creates a request. The request
goes to the entry column, as each new task does. See
[Send a request to a different team](../../how-to/move-work-between-boards/#send-a-request-to-a-different-team).

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

### `<noun> note`

```
kairos <noun> note <SHORT_CODE> (--text <TEXT> | --text-file <FILE>) [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--text <TEXT>` | string | required without `--text-file` | The markdown text of the note. Must not be empty. |
| `--text-file <FILE>` | path | — | Read the text of the note from a file. `-` reads standard input. Conflicts with `--text`. |

Appends a note to the item. The note is a row of its own with the next id,
not part of the content. `get` shows it after the content, under a heading
with its id, the date and your name: `## Note n-3 (2026-10-11, Robin)`.

Send no version: a note writes no version of the content. Two notes at the
same time both stay. The text can have headings. The edit rule applies, as
for `edit`.

The output names the id: `Kairos appended note n-3 to ACME-T-0012. The
content stays at version 4.` Cite the note by its id: "note n-3 on
ACME-T-0012". With `--json`, the response has `note_id`.

### `<noun> promote-note`

```
kairos <noun> promote-note <SHORT_CODE> <NOTE_ID> --kind <KIND> --board <BOARD> [--title <TITLE>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `<NOTE_ID>` | string | required | The id of the note, as `show` lists it: `n-3`. |
| `--kind <KIND>` | `adr` \| `document` | required | What the note becomes. |
| `--board <BOARD>` | slug or UUID | required | The board that gets the new item: an ADR board for an ADR, the owner board for a document. |
| `--title <TITLE>` | string | the first line of the note | The title of the new item. |

Turns a note into a draft ADR or document. The new item has the text of the
note. A line names the note, its author and its date. The new item
supports the item when the item is a strategy, an initiative or a task.

The note gets the mark of the new item, and `get` shows it after the note. Kairos
promotes a note one time only. A second promote gets 409 `CONFLICT`, which
names the item.

You need the edit rule on the item, and `manage_adrs` or `manage_documents`
on the board. The command prints one line:

```text
Kairos promoted note n-3 on ACME-T-0012 to the draft adr ACME-A-0029. ACME-A-0029 supports ACME-T-0012.
```

### `<noun> transition`

```
kairos <noun> transition <SHORT_CODE> --to <COLUMN> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--to <COLUMN>` | name or UUID | required | Target column, on the board of the item. The case of the name does not count. |

The output names the column: `Kairos moved the task ACME-T-0012 to the column
Active.` Before KAIROS-T-0589, it gave the id of the column.

A target outside the board's transition graph is rejected with 422
`INVALID_TRANSITION`, and the rejection lists the allowed target columns by
name and id. Not available on `documents`.

A task that a person moves to Active gets the [claim](../glossary/#claim) of the
person. `tasks show` shows it in the field `claim`. A task with a live state
(`kairos state`) shows it in the field `state`.

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
repository. A task in a done column does not move (`TASK_DONE`): run
`tasks reopen` first.
See [Move work between boards](../../how-to/move-work-between-boards/).

### `tasks claim`

```
kairos tasks claim <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. The task must be in Active, with no claim. |

Gives you the [claim](../glossary/#claim) of a free task in Active
(KAIROS-T-0528). You need `transition_items` on the board of the task
(KAIROS-T-0642). Kairos refuses a task that a different person holds
with `CLAIMED` and the name of the holder. A move of a task to Active gives
you the claim too.

### `tasks hand-off`

```
kairos tasks hand-off <SHORT_CODE> --to <PERSON> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. The task must be in Active. |
| `--to <PERSON>` | string | required | The person who gets the claim: a user id, an email or a user name of a person of the organization, or `me`. The person must hold `transition_items` on the board of the task. |

Gives the [claim](../glossary/#claim) of a task in Active to a different person.
The task stays in Active. You can hand off a claim that you have. To hand off
the claim of a different person, you need `transition_items` on the board of
the task. A service account cannot have a claim.

The person who gets the claim must hold `transition_items` on the board of the
task. The team of the board gives it, and so do a grant and the organization
admin role. Else Kairos refuses the hand-off with `VALIDATION`. The text names
the board and the capability. `GET /api/tasks/{short_code}/hand-off-targets`
lists the persons who can get the claim.

### `tasks release`

```
kairos tasks release <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. The task must be in Active. |

Ends the claim of a task in Active. The task stays in Active with no claim.
`tasks claim` takes it, and a move to Active gives it. You can release a
claim that you have. To release the claim of a different person, you need
`transition_items` on the board of the task.

### `tasks cancel`

```
kairos tasks cancel <SHORT_CODE> --reason <TEXT> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. The task must not be in a done column. |
| `--reason <TEXT>` | string | required | Why the task is cancelled. Must not be empty. |

[Cancels](../glossary/#cancel) a task: the team will not do it. The task moves
to the done column of its board with the mark "cancelled" and the reason.
`tasks show` shows the mark in the line `cancelled`. `tasks reopen` brings the
task back and removes the mark. You need `transition_items` on the board of the
task.

### `tasks reopen`

```
kairos tasks reopen <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. The task must be in a done column. |

Reopens a done task: a completed task or a cancelled one. The task moves to the
first column of its board that is not done. On a default delivery board, that
column is Backlog.

The transition rules of the board do not apply. The policy `parent_gate`
applies when that column holds claims. The reopen removes the cancel mark, and
the activity log keeps the reason. You need `transition_items` on the board of
the task.

```text
Kairos reopened ACME-T-0001. It is in the first column of its board that is not done.
```

### `tasks lane`

```
kairos tasks lane <SHORT_CODE> <LANE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. |
| `<LANE>` | string | required | `planned` or `support`. |

Changes the lane of a task. Moving a support request to the planned backlog
is a lane change. The task stays in its column. You need `transition_items`
on the board of the task; creating the task gives no right here. A lane the
task already has changes nothing. The command prints the lane of the task
after the call:

```text
ACME-T-0001 is in the planned lane.
```

### `tasks review`

```
kairos tasks review <SHORT_CODE> <REVIEW> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. |
| `<REVIEW>` | string | required | `not_needed`, `needed` or `done`. |

Sets the review state of a task (KAIROS-T-0718): whether the task needs a
review before it is done. The review state is a reserved field, like the
claim and the live state. It is not a column and not a metadata value. No
value means nobody decided. It is not content: the command needs no version
and makes no new version. A value the task already has changes nothing.

The edit rule applies: you created the task, you hold its claim, or you hold
`manage_tasks` on its board. A change writes one activity row (`review:
needed`) and emits `task_review_changed`. It fires the dispatch rules of the
board with the trigger `review_changed`. `tasks show --json` has the value
under `review`. The command prints:

```text
Review of ACME-T-0001: needed (set by Robin).
```

### `initiatives complexity`

```
kairos initiatives complexity <SHORT_CODE> <COMPLEXITY> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The initiative's short code. |
| `<COMPLEXITY>` | string | required | `xs`, `s`, `m`, `l` or `xl`. `none` clears the complexity. |

Sets the complexity of an initiative, or clears it. The complexity is not
content: the command needs no version, and it does not make a new version.
A complexity the initiative already has changes nothing.

The edit rule applies. You can change the complexity when:

- you created the initiative,
- you are a member of its board,
- you hold `manage_initiatives` on its board, or
- you are an organization admin.

The command prints the complexity after the call:

```text
ACME-I-0001 has the complexity l.
```

```text
ACME-I-0001 has no complexity.
```

### `initiatives set-parent`, `tasks set-parent`

```
kairos initiatives set-parent <SHORT_CODE> (<PARENT> | --remove) [OPTIONS]
kairos tasks set-parent <SHORT_CODE> (<PARENT> | --remove) [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The short code of the initiative or the task. |
| `<PARENT>` | string | required unless `--remove` | The new parent: a strategy for an initiative, an initiative for a task. |
| `--remove` | flag | off | Remove the parent. |

An item has one parent (KAIROS-T-0572). The new parent replaces the old one
in one transaction. The link rule applies to each edge that changes: you can
edit the item, or the parent at that end. A parent that the item has already
changes nothing. A change of parent is not a move, so `parent_gate` does not
apply. The command prints the parent after the call:

```text
The parent of ACME-T-0001 is ACME-I-0002 (was ACME-I-0001).
```

### `state`

```
kairos state <SHORT_CODE> [--branch <BRANCH>] [--commit <COMMIT>] [--pushed | --not-pushed] [--pr <URL>] [--next <TEXT>] [--clear <FIELD>]... [--expected-updated-at <INSTANT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. |
| `--branch <BRANCH>` | string | kept | The branch of the work. |
| `--commit <COMMIT>` | string | kept | The last commit (a hash or a short hash). |
| `--pushed` | flag | kept | The last commit is pushed. |
| `--not-pushed` | flag | kept | The last commit is not pushed. |
| `--pr <URL>` | string | kept | The URL of the PR. |
| `--next <TEXT>` | string | kept | What comes next. |
| `--clear <FIELD>` | `branch`, `commit`, `pr` or `next` | none | Clear a field. Repeat the option for more than one. |
| `--expected-updated-at <INSTANT>` | RFC 3339 | none | The `updated_at` of the state you read. The write is refused with `CONFLICT` when the state changed since. |

Writes the live state of a task: one current line, separate from the notes.
The write is a patch (KAIROS-T-0533). An option that you give sets its field.
A field in `--clear` becomes empty. A field that you do not name keeps its
value. Send at least one.

The state is not content: the write makes no new version of the task. A
hand-off or a release clears the next step.

The edit rule applies, as for `note`. It passes the creator of the task, the
holder of its claim (KAIROS-T-0532), and `manage_tasks` on its board. `tasks show` shows the state in the field
`state`. The board card and the item page of the GUI show it too. The
command prints one line:

```text
State of ACME-T-0001: branch acme-t-0001 at abc1234, pushed, PR https://github.com/acme/app/pull/12. Next: Wait for CI. (updated 2026-10-15 by Robin)
```

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

The command prints the owner board of the document after the call:

```text
The owner board of the document ACME-D-0004 is <board-id>.
```

### `documents lifecycle`

```
kairos documents lifecycle <SHORT_CODE> --to <STATE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The short code of the document. |
| `--to <STATE>` | `draft`, `review`, `published` or `archived` | required | The lifecycle of the document after the call. |

The command sets the editorial lifecycle of a document (KAIROS-A-0018). You
can move a document from each value to each other value. A value that the
document already has changes nothing. The edit rule applies: the creator of
the document, or `manage_documents` on its owner board. The CLI refuses
another value and lists the four values.

The `archived` lifecycle is a label: the document stays live. To put the
document away, use `documents archive`.

The command prints the lifecycle of the document after the call:

```text
The document ACME-D-0004 is published.
```

### `<noun> metadata set`

```
kairos <noun> metadata set <SHORT_CODE> <SLUG=VALUE>... [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `<SLUG=VALUE>...` | one or more pairs | required | Each pair is the slug of a field, `=` and the value. For example: `priority=high`. |

The command sets metadata values of an item (KAIROS-A-0003). The fields are
the metadata definitions of the organization. `kairos metadata list` shows
them.

The server checks each value against its definition. An enum value must
be an option of the field. A date has the form `YYYY-MM-DD`. The server refuses
an unknown slug with 422 `VALIDATION`.

The edit rule applies: the creator of the item, or `manage_<type>` on its
board. A pair without `=`, an empty slug or an empty value is a usage error
(exit 64). A slug that the command gives two times gets exit code 1.

The command prints the metadata of the item after the call:

```text
Kairos changed the metadata of ACME-T-0001.
  metadata:
    priority: high
```

### `<noun> metadata unset`

```
kairos <noun> metadata unset <SHORT_CODE> <SLUG>... [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `<SLUG>...` | one or more strings | required | The slugs of the fields. |

The command removes metadata values of an item. The rule is that of
`metadata set`. A field with no value stays without a value. The command
prints the metadata of the item after the call.

### `<noun> archive`

```
kairos <noun> archive <SHORT_CODE> --confirm [OPTIONS]
kairos <noun> archive <SHORT_CODE> --preview [OPTIONS]
```

`delete` is an alias of `archive`: `kairos tasks delete ACME-T-0001 --confirm`
archives the task.

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--confirm` | flag | off | Required for the archive to happen. Without it, nothing is archived. |
| `--preview` | flag | off | Archive nothing. Show the descendants that the archive takes, and the descendants that it cannot reach. Not with `--confirm`. |

The archive is a soft delete and cascades to the item's children. Archived
items are hidden from `list` unless `--include-deleted` is passed, and are
recoverable with `restore`.

The cascade takes the descendants that you can edit
([the edit rule](../capabilities/#the-edit-rule)). It stops at a descendant that
you cannot edit, and takes nothing below it. The command names each descendant
that stays, and the reason. With `--json`, they are in `not_reached`.

With `--preview`, the command asks the server for the
[cascade preview](../glossary/#cascade) and archives nothing. The answer is
for you: a person with other capabilities can get a different answer. The
command prints the codes:

```text
An archive of ACME-I-0001 also takes 2 descendants: ACME-T-0001, ACME-T-0002. Kairos archived nothing.
To archive, run the command again with --confirm in place of --preview.
```

With `--json`, the command prints the answer of
`GET /api/{family}/{short_code}/cascade-preview`.

### `tasks purge`

```
kairos tasks purge <SHORT_CODE> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The current short code of an archived task. |
| `--confirm` | flag | off | Required for the purge to happen. Without it, nothing is deleted. |

Deletes an archived task for good (a [purge](../glossary/#purge)). **You
cannot undo this.** No `restore` brings the task back. Archive the task first
with `tasks archive`. Kairos refuses a live task with `TASK_LIVE`.

You need the `purge` capability on the board of the task. Team membership does
not give it, and an organization admin has it.

### `adrs check`

```
kairos adrs check [PATH]... [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `[PATH]...` | path | `.` | The files and folders to check. |
| `--prefix <PREFIX>` | string | all | Check only the codes with this prefix, for example `KAIROS`. More than one is allowed. |
| `--snapshot <FILE>` | path | none | Read the states of the ADRs from this file, not from the deployment. |
| `--save-snapshot <FILE>` | path | none | Write the states of the cited ADRs to this file, for a later `--snapshot`. |
| `--stale-lines <N>` | integer | 50 | Flag an ADR when the files that cite it changed by this count of lines (added and removed) since its `accepted_commit`. |
| `--board-prefixes` | flag | off | Read only the codes with the prefix of an ADR board of the deployment. Each other code is an `unknown_prefix` info. Not with `--snapshot`. |

Finds each ADR short code (for example `KAIROS-A-0031`) in the files of the
paths. Gives a finding for each citation. The walk reads the hidden folders
(`.github/`, `.angreal/`). It skips these files:

- the files that the `.gitignore` files of the checkout ignore;
- each `.git` folder or file;
- the file of `--snapshot` or `--save-snapshot`;
- a file that is not UTF-8 or is larger than 2 MiB.

The walk does not read your global gitignore, so the result is the same on
each machine.

A code is an upper-case prefix, `-A-`, and four digits or more. A letter, a
digit, `_` or `-` just before the code makes it part of a longer word. A
letter, a digit or `_` just after the code does the same. The code index and
the plugin hook `adr_context.py` use the same rule. The
check reads each text of a file, also the strings and the prose. It does not
read the snake-case names of tests (`retry_kairos_a_0065`), as the code index
does.

| Finding | Severity | Meaning |
|---|---|---|
| `missing` | fail | No ADR has the code. |
| `archived` | fail | The ADR is archived. |
| `not_in_snapshot` | warn | The snapshot of `--snapshot` has no state for the code. Save the snapshot again. |
| `superseded` | warn | An ADR supersedes the cited ADR. The finding names the successor. |
| `ended_column` | warn | The ADR is in the column Superseded, Rejected or Deprecated, and no ADR supersedes it. |
| `retired_code` | warn | The citation uses a retired code. The finding names the current code. |
| `revisit_due` | warn | The `revisit_on` date of the ADR is today or earlier. The finding gives `revisit_when`. |
| `stale_code` | warn | The files that cite the ADR changed by `--stale-lines` lines or more since its `accepted_commit`. The finding names each file with its count. |
| `amended` | info | ADRs amend the cited ADR. The finding names them. |
| `commit_unknown` | info | The checkout does not have the `accepted_commit` of the ADR, so the check cannot compare. |
| `unknown_prefix` | info | With `--board-prefixes`: no ADR board of the deployment has the prefix of the code. It is not known to this deployment. |

The text output gives one line for each finding, `FILE:LINE: CODE SEVERITY:
MESSAGE`, and then the counts. A citation of a live ADR gives no line.

The states come from the deployment of the checkout of the first path. The
checkout is the nearest folder with `.git` at or above the path. The binding
of that checkout gives the deployment. In a board folder with several
checkouts, the binding of the board folder does not apply. The CLI reads the
item graph of each cited ADR once. It also reads the metadata of each live ADR: `revisit_when`,
`revisit_on` and `accepted_commit` (KAIROS-A-0070).

With `--snapshot`, the states come from a file that `--save-snapshot` wrote.
Then the command needs no deployment and no key. Use this in CI: CI must not
have a key of a deployment.

A citation that the snapshot does not hold is a warn, not a fail. Thus CI
passes for a contributor with no login. A person with a login then saves the
snapshot again. With `--snapshot`, a citation fails only when the snapshot
gives its ADR as missing or archived.

With no deployment and no `--snapshot`, the CLI skips the check: it says so on
stderr and exits with 0. A snapshot with no ADRs is a placeholder: the CLI
skips the check in the same way.

The CI of the kairos repository runs the check on the committed snapshot
`.github/adr-snapshot.json`. The walk does not read the snapshot file, so its
codes are not citations:

```
kairos adrs check --prefix KAIROS --snapshot .github/adr-snapshot.json
```

To update the snapshot, log in to the deployment and run this from the root
of the checkout. Commit the file. Do this when a change cites a new ADR. Do it
also when an ADR gets a successor or an amendment, or goes to the archive:

```
kairos adrs check --prefix KAIROS --save-snapshot .github/adr-snapshot.json
```

For `stale_code`, the CLI runs `git diff --numstat <accepted_commit> -- <file>`
in the folder of each citing file. The diff is local, also with `--snapshot`.

The exit code is 0 when no citation fails, also with warnings or a skipped
check. It is 1 when a citation fails, and 2 for an error of the login. With
`--json`, the report is one JSON object on stdout, also when a citation fails:

```json
{
  "source": "deployment",
  "skipped": false,
  "ok": false,
  "summary": {"citations": 3, "adrs": 2, "fail": 1, "warn": 1, "info": 0},
  "citations": [
    {"file": "src/main.rs", "line": 12, "code": "ACME-A-0002", "severity": "warn",
     "findings": [{"severity": "warn", "reason": "superseded",
                   "message": "The ADR ACME-A-0002 is superseded by ACME-A-0007. ..."}]}
  ],
  "adrs": [
    {"code": "ACME-A-0002", "found": true, "current_code": "ACME-A-0002",
     "title": "...", "status": "Superseded", "superseded_by": [{"code": "ACME-A-0007", "title": "..."}],
     "amended_by": []}
  ]
}
```

`citations` has each citation, also the ones with no finding (`severity`
`ok`). `adrs` has the state of each cited ADR. A skipped check has `source`
`none`, `skipped` true and a `message`.

### `<noun> restore`

```
kairos <noun> restore <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |

Puts an archived item back on its board. The item's own archived children stay
archived: restore is per item, not a cascade.

### `<noun> diff`

```
kairos <noun> diff <SHORT_CODE> <VERSION> [<OTHER>] [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `<VERSION>` | integer | required | A version of the item. The first version is 1. |
| `<OTHER>` | integer | the version before `<VERSION>` | A second version. The command compares `<VERSION>` with `<OTHER>`. |

Compares two versions of the item. With one version, the command compares it
with the version before it. The command compares version 1 with no content. The output
names the two versions and their times. A changed title gets one line. Then a
unified diff of the content comes, with 3 lines of context:

```text
--- ACME-T-0001 v1 (2026-10-17T09:00:00Z)
+++ ACME-T-0001 v2 (2026-10-17T10:00:00Z)
title: "Sweep old history" -> "Sweep history older than 90 days"
@@ -1,2 +1,2 @@
 The sweeper runs each night.
-It removes history older than 30 days.
+It removes history older than 90 days.
```

When the content is the same, the command prints the line `The content did not
change.`. The versions are arguments, not options. `--to` names a column or a
person. With `--json`, the command prints one object. It has `short_code`,
`from` and `to` (the two versions of
`GET /api/{family}/{short_code}/history?version=N`), and `diff` (the text).
`from` is null for version 1.

### `<noun> links`

```
kairos <noun> links <SHORT_CODE> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |

Lists the branches, the pull requests and the issues of the forge that link
to the item. A branch or a pull request links to an item when its name, title
or body has the short code. The columns are `KIND`, `REPOSITORY`, `REF`,
`STATE`, `TITLE` and `URL`. With `--json`, the command prints the list
envelope with the rows of `GET /api/{family}/{short_code}/links`.

### `kairos link`

```
kairos link <SOURCE> <RELATIONSHIP> <TARGET> [--label <TEXT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SOURCE>` | string | required | The short code of the source item. |
| `<RELATIONSHIP>` | relationship | required | `parent`, `blocks`, `informs`, `supports`, `supersedes`, `amends`, `impacts` or `external`. |
| `<TARGET>` | string | required | The short code of the target item. For `impacts`, a repository (slug or UUID). For `external`, a URI. |
| `--label` | string | none | The label of an external reference, at most 200 characters. Only with `external`. |

The command makes a relationship of an item (KAIROS-T-0591). `<SOURCE>` and
`<TARGET>` are the two ends of the edge, as in the MCP tool `link_items`:

| Relationship | Source | Target | Meaning |
|---|---|---|---|
| `parent` | strategy or initiative | initiative or task | The source is the parent of the target. |
| `blocks` | strategy, initiative or task | strategy, initiative or task | The target waits for the source. |
| `informs` | document, ADR or task | strategy, initiative or task | The source informs the target. A task informs a task only. |
| `supports` | strategy, initiative or task | document or ADR | The target supports the source. |
| `supersedes` | ADR | ADR | The source replaces the target. |
| `amends` | ADR | ADR | The source changes a part of the target (KAIROS-A-0069). |
| `impacts` | document or ADR | repository | The source is about the repository. It gives no right. |
| `external` | each type | URI | The item links to a document outside Kairos. |

The server checks the types of the two ends, and refuses a cycle of `parent`,
`blocks` or `amends`. You must be able to edit one of the two items. For `impacts` and
`external`, you must be able to edit the source. An item has one parent. To
change it, use [`set-parent`](#initiatives-set-parent-tasks-set-parent).

The command prints the edge and its meaning:

```text
Kairos made the link: ACME-T-0003 -[blocks]-> ACME-T-0004. ACME-T-0004 waits for ACME-T-0003.
Kairos made the link: ACME-T-0012 -[external]-> RFC: zen as the policy engine <https://example.com/wiki/rfc-42>.
```

An `external` link is an
[external reference](../glossary/#external-reference). The scheme of the URI is
`http`, `https` or `mailto`. The server refuses each other scheme with 422
`VALIDATION`, and the refusal names the scheme. The server refuses a URI that
the item has with 422 `ALREADY_LINKED`.

The CLI refuses `--label` with a relationship other than `external`, and
names the relationship (exit code 1). An unknown relationship is a usage error
(exit 64). With `--json`, the command prints the new edge, impact or
reference.

`kairos link <CODE> impacts <REPOSITORY>` does the same as
[`kairos repos link`](#kairos-repos-link).

### `kairos unlink`

```
kairos unlink <SOURCE> <RELATIONSHIP> <TARGET> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SOURCE>` | string | required | The short code of the source item. |
| `<RELATIONSHIP>` | relationship | required | A relationship of `kairos link`. |
| `<TARGET>` | string | required | The short code of the target item. For `impacts`, a repository. For `external`, the URI of the reference. |

The command removes a relationship that `kairos link` makes. The ends are
those of `kairos link`. The rule is that of `kairos link`. To remove the
`supports` edge of a document, you must be able to edit the document. The
command prints this line:

```text
Kairos removed the link: ACME-T-0003 -[blocks]-> ACME-T-0004.
```

For an edge between two items, the CLI reads the relationships of the source.
Then it removes the edge by its id. The command fails with exit code 1 when the
source has no such edge, or no reference with that URI.

## Finding work

### `kairos me`

Your work has two lanes: the initiatives that you own, and your tasks. Your
tasks are:

- the tasks that you or your agent keys claim;
- the tasks in Todo, Active or Blocked under an initiative that you own;
- the tasks that one of these tasks blocks.

Each lane shows the items in three groups: Todo, Active and Blocked. An item
with an open blocker goes in Blocked. An item in a column named Blocked goes
there too. The command does not show an item in a done column.

```
kairos me [--json]
```

Each row gives the code, the title, the board, the column, the claimant or
the owner, and the open blockers. `--json` prints the answer of
`GET /api/me/work`.

### `kairos activity`

```
kairos activity [--item <CODE>] [--actor <USER>] [--action <ACTION>] [--since <TIME>] [--team <TEAM>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--item <CODE>` | short code | all | Only the activity of this item. |
| `--actor <USER>` | UUID, `me` or email | all | Only the activity of this person. |
| `--action <ACTION>` | string | all | Only this action, for example `transition`, `create`, `delete`, `claim` or `update`. |
| `--since <TIME>` | `YYYY-MM-DD` or RFC 3339 | all | Only the activity at or after this time. A date is the start of the day in UTC. |
| `--team <TEAM>` | slug or UUID | all | Only the activity of the members of this team. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

Shows the activity log of the organization, newest first: `GET
/api/activity`. The filters combine. Each member can read the log. The columns
are `WHEN`, `ACTION`, `ITEM`, `DETAILS` and `BY`. `BY` is the name of the
person when you can read the members, else the id. The mark `(agent)` shows a
change that an agent key made.

The count of the log stops at 1000 rows, so the command has no `--all`. Use
`--offset` for the next page. For the activity of one item with the names of
the people, use `<noun> history`. With `--json`, the command prints the list
envelope.

### `kairos notifications list`

```
kairos notifications list [--before <ID> | --read-before <ID>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--before <ID>` | notification id | none | The next page of the unread notifications: those older than this one. |
| `--read-before <ID>` | notification id | none | The next page of the read notifications: those read before this one. Not with `--before`. |

Lists your notifications: each unread one, newest first, 100 at most. Then the
20 newest read ones come. The first line gives the count of the unread ones.
The last lines give the option for the next page. An agent key has a read
state of its own.

With `--json`, the command prints the list envelope. The object also has
`unread`, the count of the unread notifications. `next_offset` is null:
these pages use `--before` and `--read-before`.

### `kairos notifications read`

```
kairos notifications read <ID>... [OPTIONS]
kairos notifications read --all [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<ID>...` | notification ids | required without `--all` | The notifications to mark read. |
| `--all` | flag | off | Mark each unread notification read, up to the newest one now. Not with `<ID>`. |

Marks notifications read, and prints the count that changed and the count
that stays unread. With `--all`, a notification that comes after the command
stays unread.

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
| `--board <BOARD>` | slug or UUID | none | Restrict to items on this board. A document is not a card, so this filter gives no document. |
| `--column <COLUMN>` | name or UUID | none | Restrict to items in this column. A name needs `--board`; an unknown name is refused with the columns of the board (KAIROS-T-0552). |
| `--team <TEAM>` | slug or UUID | none | Restrict to tasks assigned to this team. |
| `--repo <REPOSITORY>` | slug or UUID | none | Restrict to the items of this repository: the tasks that link to it, and the documents and the ADRs that impact it. |
| `--kind <KIND>` | `task` \| `bug` \| `tech_debt` \| `support` | all | The kind of a task. Repeatable. The old name `--task-type` still works. |
| `--work-class <WORK_CLASS>` | `planned` \| `support` | all | Lane filter. Repeatable. |
| `--is-bucket <BOOL>` | `true` \| `false` | both | Restrict to bucket or non-bucket initiatives. |
| `--metadata <KEY=VALUE>` | `slug=value` | none | Metadata condition. Repeatable. Values support trailing-`*` globs, e.g. `component=auth*`. |
| `--after <RFC3339>` | RFC 3339 instant | none | Only items created strictly after this instant. |
| `--before <RFC3339>` | RFC 3339 instant | none | Only items created strictly before this instant. |
| `--updated-after <RFC3339>` | RFC 3339 instant | none | Only items with a last change strictly after this instant. |
| `--updated-before <RFC3339>` | RFC 3339 instant | none | Only items with a last change strictly before this instant. For the items with no change in 90 days, give the time 90 days ago. |
| `--metadata-missing <SLUG>` | metadata-definition slug | none | Only items with no value for this definition, e.g. `--metadata-missing priority`. Repeatable. An unknown slug is refused. |
| `--include-deleted` | flag | off | Include archived items. Composes with `--query` and `--from`; archived hits are marked in the output. |
| `--claimed-by <PERSON>` | `me`, user id, email or user name | none | Only tasks that this person has the claim on. A claim of an agent key of the person counts. |
| `--owned-by <PERSON>` | `me`, user id, email or user name | none | Only strategies and initiatives that this person owns. |
| `--created-by <PERSON>` | `me`, user id, email or user name | none | Only items that this person created. |

Traversal:

| Option | Type | Default | Description |
|---|---|---|---|
| `--from <SHORT_CODE>` | string | none | Starting entity, by short code. |
| `--from-id <UUID>` | UUID | none | Starting entity, by id. |
| `--relationships <REL>` | `parent` \| `supports` \| `informs` \| `supersedes` \| `amends` \| `blocks` | required with `--from` or `--from-id` | Relationship types to follow. Repeatable. |
| `--direction <DIRECTION>` | `outbound` \| `inbound` \| `both` | `outbound` | Edge direction. |
| `--depth <N>` | integer 1–10 | required with `--from` or `--from-id` | Maximum traversal depth. |

A traversal needs one `--relationships` or more, and `--depth`. Without
them, the command exits 1, and the message names the missing option.
`--relationships`, `--direction` and `--depth` with no `--from` or
`--from-id` also exit 1.

Ordering and paging:

| Option | Type | Default | Description |
|---|---|---|---|
| `--sort <FIELD>` | `created_at` \| `updated_at` \| `title` \| `relevance` | `relevance` with `--query`, else `created_at` | Sort field. `relevance` needs `--query`. Without a query, the server refuses `relevance`. |
| `--order <ORDER>` | `asc` \| `desc` | `desc` | Sort order. |
| `--limit <LIMIT>` | integer | server default 25, maximum 100 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Offset into the combined result set. |
| `--all` | flag | off | Read each page of hits. Not with `--limit`, `--offset` or `--query-json`. |

Raw body:

| Option | Type | Default | Description |
|---|---|---|---|
| `--query-json <JSON>` | JSON, `@FILE`, or `-` | none | The full search request body as raw JSON. `@FILE` reads a file; `-` reads standard input. |

The command sends the `--query-json` body to the server as it is. The server
checks each field and refuses an unknown field by its name. So an older CLI
can send a field that only a newer server knows.

`--query-json` does not accept a search flag beside it. The command then
exits 1, and the message names each flag. Put the values in the JSON, or use
the flags alone. Paging is in the JSON too: `"limit"` and `"offset"`.

The output pages as [the pages of a list](#the-pages-of-a-list) tell. The
`Next:` line counts the hits of all the groups.

### `kairos boards list`

```
kairos boards list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

### `kairos boards show`

```
kairos boards show <BOARD> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | Board to show. |
| `--limit <LIMIT>` | integer | server default 200, maximum 1000 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Items to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and shows each item. Not with `--limit` or `--offset`. |
| `--team <TEAM>` | slug or UUID | all | Show only the strategies and the initiatives of this team, from tasks or set by hand. Tasks and ADRs do not change. Do not use with `--no-team`. |
| `--no-team` | flag | off | Show only the strategies and the initiatives that have no team. |
| `--details` | flag | off | Show more on each item: the date of its last change, the claim of a task, and the metadata values, for example `{updated 2026-10-08; claimed by Alice; priority: high}`. |

Shows one page of the board's live items grouped by column. A strategy or an
initiative that has teams shows them after its title, for example
`[teams: skadi, weir]`. Archived items are
not shown and there is no flag to include them. The last lines give the total,
and they tell you when the page is a part of the board:

```
total: 340 (limit 200, offset 0)
The board has 340 items. This result shows 200 (limit 200, offset 0). To read the next part, use --offset 200.
```

With `--all`, the command shows each item of the board, and the last line is
`total: N`.

To give or revoke a capability on a board, use
[`kairos grants`](#kairos-grants). See
[Capabilities and access](../../explanation/capabilities-and-access/).

A column with a WIP limit shows its live count against the limit after its
name, for example `[WIP 2/3]`. When it has more items than the limit, it shows
`[WIP 4/3, over the limit]`.

### `kairos boards flow`

```
kairos boards flow <BOARD> [--from <YYYY-MM-DD>] [--until <YYYY-MM-DD>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--from <DATE>` | `YYYY-MM-DD` | 89 days before `--until` | The first day of the range (UTC). |
| `--until <DATE>` | `YYYY-MM-DD` | today | The last day of the range (UTC). The old name `--to` still works. |

Shows the flow metrics of the board for the range:

- the cycle time: the count, the mean, the median and the 85th percentile;
- the throughput per week;
- the work in progress, with its age;
- the cumulative flow of the last 14 days. `--json` gives each day.

A cancelled task counts apart from the done items. A range has 366 days at
most. The definitions are in [`board_flow`](../mcp-tools/#board_flow).

### `kairos boards policy`

```
kairos boards policy <BOARD> [--parent-gate on|off] [--owner-gate on|off] [--blockers-gate on|off] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--parent-gate` | `on` or `off` | unchanged | The policy `parent_gate` of a delivery board. |
| `--owner-gate` | `on` or `off` | unchanged | The policy `owner_gate` of an initiative board. |
| `--blockers-gate` | `on` or `off` | unchanged | The policy `blockers_gate` of a delivery board. |

Shows the policies of a board, and sets them. To set a policy, you need
`configure_boards` on the board. With `parent_gate` on, a task moves to a
claims column only while its parent initiative is active. Kairos refuses a
different move with `PARENT_NOT_ACTIVE`.

With `owner_gate` on, an initiative
moves to Active only with an owner. Kairos refuses a different move with
`OWNER_REQUIRED`.

With `blockers_gate` on, a task with open blockers does not move to a claims
column or a done column. Kairos refuses the move with `BLOCKED`. The policy is
off by default: the move succeeds with a warning. See
[`set_board_policy`](../mcp-tools/#set_board_policy).

### `kairos boards wip-limit`

```
kairos boards wip-limit <BOARD> <COLUMN> (--limit <N> | --none) [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `<COLUMN>` | name or UUID | required | The column. |
| `--limit <N>` | integer | none | The WIP limit, a number above 0. |
| `--none` | flag | off | Remove the limit. |

Sets or removes the WIP limit of a column. You need `configure_boards` on the
board. Kairos does not refuse a move into a column over its limit. The REST
answer has the header `Kairos-Warning`, and the MCP answer has a warning line.
The board marks the column.

### `kairos boards columns`

```
kairos boards columns list   <BOARD> [OPTIONS]
kairos boards columns add    <BOARD> <NAME> [--position <N>] [OPTIONS]
kairos boards columns update <BOARD> <COLUMN> [--name <NAME>] [--position <N>] [--done <BOOL>] [--claims <BOOL>] [--keeps-claims <BOOL>] [OPTIONS]   (one option at least)
kairos boards columns remove <BOARD> <COLUMN> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `<NAME>` | string | required on `add` | The name of the new column. |
| `<COLUMN>` | name or UUID | required on `update` and `remove` | The column. |
| `--position <N>` | integer | the last (on `add`) | From 0, the first column. |
| `--name <NAME>` | string | no change | The new name. |
| `--done <BOOL>` | `true` or `false` | no change | A done column. |
| `--claims <BOOL>` | `true` or `false` | no change | A task in the column has a claim. |
| `--keeps-claims <BOOL>` | `true` or `false` | no change | A task with a claim keeps it in the column, and a task with no claim gets none there (Blocked, KAIROS-T-0529). |
| `--confirm` | flag | off | Required on `remove` for the removal to happen. Without it, the command exits 1. |

`list` shows the columns in their order. `add`, `update` and `remove` need
`configure_boards` on the board. A new column has no transition. `remove`
refuses a column that holds a live item. `kairos boards wip-limit` sets the
WIP limit.

### `kairos boards transitions`

```
kairos boards transitions list   <BOARD> [OPTIONS]
kairos boards transitions add    <BOARD> <FROM> <TO> [OPTIONS]
kairos boards transitions remove <BOARD> <FROM> <TO> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `<FROM>` | name or UUID | required on `add` and `remove` | The column where the move starts. |
| `<TO>` | name or UUID | required on `add` and `remove` | The column where the move ends. |
| `--confirm` | flag | off | Required on `remove` for the removal to happen. Without it, the command exits 1. |

`list` shows the moves that the board allows. `add` allows a move, and
`remove` stops it. The two need `configure_boards` on the board.

### `kairos boards dispatch`

```
kairos boards dispatch list <BOARD> [OPTIONS]
kairos boards dispatch add-target <BOARD> --name <NAME> --hook-url <URL> [OPTIONS]
kairos boards dispatch remove-target <BOARD> <TARGET> --confirm [OPTIONS]
kairos boards dispatch enable-target <BOARD> <TARGET> [OPTIONS]
kairos boards dispatch redeliver <BOARD> <TARGET> [--since <TIME>] [OPTIONS]
kairos boards dispatch test-target <BOARD> <TARGET> [OPTIONS]
kairos boards dispatch add-rule <BOARD> [--trigger entered_column|unblocked|review_changed|claim_released] [--column <COLUMN>] --target <TARGET> [--kind <KIND>] [--metadata <SLUG=VALUE>] [--repo <REPOSITORY>] [OPTIONS]
kairos boards dispatch remove-rule <BOARD> <RULE> --confirm [OPTIONS]
kairos boards dispatch agent-triage <BOARD> [--on | --off] [OPTIONS]
kairos boards dispatch log <BOARD> [--target <TARGET>] [--rule <RULE>] [--limit <N>] [--before <DELIVERY>] [OPTIONS]
```

Sends tasks to agent runners (KAIROS-T-0385). A rule names a column, a filter
and a webhook target. When a task enters the column and matches the filter,
Kairos sends one signed delivery to the target. See
[Webhook deliveries](../webhooks/).

| Subcommand | Needs | Does |
|---|---|---|
| `list` | a place on the board | Shows the rules and the targets of the board. |
| `add-target` | `configure_boards` | Makes a target. The command shows its secret one time. |
| `remove-target` | `configure_boards` | Removes a target, with its rules and its log. It needs `--confirm`. |
| `enable-target` | `configure_boards` | Turns on a target that Kairos turned off. Kairos sends the skipped deliveries of the last 30 days again. |
| `redeliver` | `configure_boards` | Sends the skipped deliveries of a target that is on again, oldest first. `--since` sends only the events at or after that time (RFC 3339). |
| `test-target` | `configure_boards` | Sends the event `ping` to a target. |
| `add-rule` | `configure_boards` | Makes a rule. |
| `remove-rule` | `configure_boards` | Removes a rule. Its deliveries stay in the log. It needs `--confirm`. |
| `agent-triage` | a place on the board; `configure_boards` with `--on` or `--off` | Shows or sets who decides the triage of intake. `--on`: an agent decides. `--off`: an agent proposes with a note, and a person decides. |
| `log` | a place on the board | Shows the newest deliveries, with the status, the tries and the response code. `--before <DELIVERY>` shows the deliveries older than that delivery. |

A place on the board is a grant on the board, a place in its team, or the
admin role. Without `--confirm`, `remove-target` and `remove-rule` remove
nothing and exit 1.

| Option of `add-rule` | Type | Default | Description |
|---|---|---|---|
| `--trigger <TRIGGER>` | string | `entered_column` | `entered_column`: a task enters the column; on an initiative or a strategy board, an initiative or a strategy does (the event `initiative.entered_column`, KAIROS-T-0719). `unblocked`: the last open blocker of a task closes (the event `task.unblocked`). `review_changed`: the review state of a task changes (the event `task.review_changed`, KAIROS-T-0718). `claim_released`: the claim of a task is released, or ends on a move out of the claims column (the event `task.claim_released` with the live state of the task, KAIROS-T-0720). |
| `--column <COLUMN>` | name or UUID | required for `entered_column` | The column. For the other triggers, the column the task must be in; with none, each column that is not done. |
| `--target <TARGET>` | UUID | required | A target of the board. |
| `--kind <KIND>` | string | none | Only tasks of this kind: `task`, `bug`, `tech_debt` or `support`. The old name `--task-type` still works. Refused on an initiative or a strategy board. |
| `--metadata <SLUG=VALUE>` | string | none | Only tasks with this metadata value, for example `priority=high`. |
| `--repo <REPOSITORY>` | slug or UUID | none | Only tasks of this repository. The old name `--repository` still works. Refused on an initiative or a strategy board. |

### `kairos boards archive-completed`

```
kairos boards archive-completed <BOARD> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | A delivery board. |
| `--confirm` | flag | off | Required for the archive to happen. Without it, nothing is archived. |

Archives each task in the done columns of the board, cancelled tasks too. Each
task goes through the same archive as `tasks archive`. The call archives each
task, or none. You need `manage_tasks` on the board. The command prints the
count and the codes. To get a task back, use `kairos tasks restore <code>`.

### `kairos boards create`

```
kairos boards create --name <NAME> --slug <SLUG> --level <LEVEL> --code-prefix <PREFIX> [--team <TEAM>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Board name. |
| `--slug <SLUG>` | string | required | Board slug. It must match `^[a-z][a-z0-9_-]{1,62}$`. |
| `--level <LEVEL>` | `strategy` \| `initiative` \| `delivery` \| `adr` | required | The level of the board. |
| `--code-prefix <PREFIX>` | string | required | The short-code prefix of the board. A capital letter, then 1 to 9 capital letters or digits. The ADR board of a team takes the prefix of the team. |
| `--team <TEAM>` | slug or UUID | none | The owner team. A delivery board needs a team. An ADR board with a team is the ADR board of that team. A strategy or initiative board with no team gets its own empty team. |

`POST /api/boards`. The board gets the default columns and transitions of its
level. You need the organization admin role or `manage_structure` on the
organization.

### `kairos boards members`

```
kairos boards members <BOARD> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |

Lists the members of the board that have a board capability. The columns are
`EMAIL`, `NAME` and `CAPABILITIES`. With `--json`, the command prints the
list envelope with the rows of `GET /api/boards/{id}/members`. To change a
capability, use [`kairos grants`](#kairos-grants).

### `kairos boards update`

```
kairos boards update <BOARD> [--name <NAME>] [--slug <SLUG>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--name <NAME>` | string | unchanged | The new name. |
| `--slug <SLUG>` | string | unchanged | The new slug. It must match `^[a-z][a-z0-9_-]{1,62}$`. |

Changes the name or the slug of a board. Give one option at least. You need
`configure_boards` on the board. The short-code prefix and the team of a board
do not change. For the policies, use `kairos boards policy`.

### `kairos boards delete`

```
kairos boards delete <BOARD> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--confirm` | flag | off | Required for the delete to happen. Without it, the command exits 1. |

Deletes an empty board. The server refuses a board that has items. You need
`configure_boards` on the board.

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
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

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
kairos members set-role <USER> --role <ROLE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<USER>` | id, email or user name | required | The member, from `kairos members list`. |
| `--role <ROLE>` | `admin` \| `member` | required | New role. |

Kairos refuses a demotion of the last admin with `LAST_ADMIN`.

### `kairos members remove`

```
kairos members remove <USER> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<USER>` | id, email or user name | required | The member. |
| `--confirm` | flag | off | Required for the removal to happen. |

Kairos refuses the removal of the last admin with `LAST_ADMIN`.

`members` commands are org-admin operations.

### `kairos teams list`

```
kairos teams list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

### `kairos teams show`

```
kairos teams show <TEAM> [OPTIONS]
```

`get` is the old name of `show`, and it still works.

| Argument | Type | Default | Description |
|---|---|---|---|
| `<TEAM>` | slug or UUID | required | Team to show. |

### `kairos teams create`

```
kairos teams create --name <NAME> --slug <SLUG> --code-prefix <PREFIX> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Team name. |
| `--slug <SLUG>` | string | required | Team slug. The team's delivery board becomes `{slug}-delivery`. |
| `--code-prefix <PREFIX>` | string | required | The short-code prefix of the delivery board, for example `SKADI`. A capital letter, then 1 to 9 capital letters or digits. A task on the board gets the code `SKADI-T-0001`. The prefix does not change later. |
| `--kind <KIND>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | `stream_aligned` | The Team Topologies kind of the team. The old name `--type` still works. |

Creating a team also creates its delivery board.

### `kairos teams update`

```
kairos teams update <TEAM> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM>` | slug or UUID | required | Team to update. |
| `--name <NAME>` | string | unchanged | New name. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--kind <KIND>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | unchanged | The new kind. The old name `--type` still works. |

### `kairos teams delete`

```
kairos teams delete <TEAM> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM>` | slug or UUID | required | Team to delete. |
| `--confirm` | flag | off | Required for the deletion to happen. |

A soft delete.

### `kairos teams members`

```
kairos teams members list   <TEAM> [OPTIONS]
kairos teams members add    <TEAM> --user <USER> [OPTIONS]
kairos teams members remove <TEAM> --user <USER> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM>` | slug or UUID | required | The team. |
| `--user <USER>` | string | required on `add` and `remove` | A user id, an email, a user name, or the name of a service account. `kairos members list` gives them. |

### `kairos teams pages`

```
kairos teams pages list   <TEAM> [OPTIONS]
kairos teams pages show   <TEAM> <PAGE> [OPTIONS]
kairos teams pages create <TEAM> --kind <KIND> --title <TITLE> [--slug <SLUG>] [--parent <PAGE>] [--content <TEXT> | --content-file <FILE>] [--position <N>] [OPTIONS]
kairos teams pages edit   <TEAM> <PAGE> [--title <TITLE>] [--content <TEXT> | --content-file <FILE>] [--version <N>] [OPTIONS]
kairos teams pages move   <TEAM> <PAGE> [--parent <PAGE> | --root] [--slug <SLUG>] [--position <N>] [OPTIONS]
kairos teams pages delete <TEAM> <PAGE> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEAM>` | slug or UUID | required | The team. |
| `<PAGE>` | path or UUID | required | A path of slugs from the root, for example `documentation/runbooks`. `charter` is the charter. |
| `--kind <KIND>` | `page` or `folder` | required on `create` | A folder holds folders and pages. A page holds nothing. |
| `--slug <SLUG>` | string | from the title | Unique among the siblings. |
| `--parent <PAGE>` | path or UUID | the root | The folder. |
| `--content-file <FILE>` | path | — | Reads the content from a file. `-` reads standard input. |
| `--version <N>` | integer | the current version | The version that the edit is based on. A stale version gives `409 CONFLICT`. |
| `--root` | flag | off | Moves the node to the root. |
| `--confirm` | flag | off | Required to archive. |

The team page has the charter of the team and the folders and pages that the
team makes. Every member of the organization can read it. To write, you must be
a member of the team or an organization admin.

`list` shows the tree, the charter first. `edit` replaces the whole content.
`delete` archives a page, or a folder with everything in it. You cannot move or archive the
charter.

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
[the teams of an initiative or a strategy](../../explanation/teams-and-boards/#the-teams-of-an-initiative-or-a-strategy).

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
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

### `kairos streams show`

```
kairos streams show <STREAM> [OPTIONS]
```

`get` is the old name of `show`, and it still works.

| Argument | Type | Default | Description |
|---|---|---|---|
| `<STREAM>` | slug, name or UUID | required | Stream to show. |

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
kairos streams update <STREAM> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM>` | slug, name or UUID | required | Stream to update. |
| `--name <NAME>` | string | unchanged | New name. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--description <DESCRIPTION>` | string | unchanged | New description. |

### `kairos streams delete`

```
kairos streams delete <STREAM> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM>` | slug, name or UUID | required | Stream to delete. |
| `--confirm` | flag | off | Required for the deletion to happen. |

A soft delete.

### `kairos streams teams`

```
kairos streams teams list   <STREAM> [OPTIONS]
kairos streams teams add    <STREAM> --team <TEAM> [OPTIONS]
kairos streams teams remove <STREAM> --team <TEAM> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<STREAM>` | slug, name or UUID | required | The stream. |
| `--team <TEAM>` | slug or UUID | required on `add` and `remove` | The team. |

### `kairos repos list`

```
kairos repos list [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--team <TEAM>` | slug or UUID | all teams | Only this team's repositories. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

After the table, the command prints the count of the repositories and the
page: `total: 12 (limit 50, offset 0)`. See
[the pages of a list](#the-pages-of-a-list).

### `kairos repos show`

```
kairos repos show <REPOSITORY> [OPTIONS]
```

`get` is the old name of `show`, and it still works.

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
- `Not linked to an item:` the in-flight branches and pull requests with no
  item that links to them

The line `forge filters:` gives the forge filters of the repository. It is
`default (admit everything)`, or each field with a pattern that is not the
default.

The line `issue import:` gives the import interval. It is `off`, or `every`
and the interval. The line also gives the last complete run and the next
run. It gives the count of issues in the record, and the last error.

Each document or ADR is one line. The line has the short code, the lifecycle
or the column, the title, and the kind of the item:

```text
Documents and ADRs that impact this repository:
  ACME-D-0004 [published] The vision of fidius — document (vision)
  ACME-A-0002 [Decided] Plugins are dynamic libraries — adr
```

The section shows `(none)` when no item impacts the repository.

The line `default branch:` gives the branch that the code index builder
indexes. Then it gives where the branch comes from: `set by hand`, `from the
forge`, or `from the forge; no fetch yet`.

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
| `--default-branch <BRANCH>` | string | the default branch of the forge | Default branch: a branch name that git accepts. With it, the branch is set by hand. Without it, the code index builder follows the default branch of the forge. |
| `--description <DESCRIPTION>` | string | none | Short "how to work here" blurb for agents. |

Permitted to an organization admin or a member of the owning team.

The server refuses a `--name`, a `--repo-url` or a `--default-branch` that does
not have the form of its field. The glossary gives the rules: see
[the form of the fields](../glossary/#the-form-of-the-fields-of-a-repository).

### `kairos repos update`

```
kairos repos update <REPOSITORY> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--slug <SLUG>` | string | unchanged | New slug. |
| `--repo-url <URL>` | string | unchanged | New browser URL. |
| `--default-branch <BRANCH>` | string | unchanged | New default branch. The branch is then set by hand: the code index builder indexes it, whatever the forge says. |
| `--follow-forge` | flag | off | Remove the branch set by hand. The code index builder follows the default branch of the forge again. You cannot use it with `--default-branch`. |
| `--team <TEAM>` | slug or UUID | unchanged | New owning team. Re-homes the repository. |
| `--description <DESCRIPTION>` | string | unchanged | New description. |
| `--code-index-build <ON\|OFF>` | `on` or `off` | unchanged | With `off`, the code index builder makes no index of the repository. Another value is refused. See [The base code index](../configuration/#stop-the-builder-for-one-repository). |
| `--code-index-summaries <EMBEDDED\|HOSTED\|ORGANIZATION>` | `embedded`, `hosted` or `organization` | unchanged | With `organization`, the default summarizer of the organization decides. With `hosted`, the summaries of the repository come from the provider of the organization, and the code of each changed symbol leaves the host. Kairos refuses `hosted` when the organization has no hosted provider (`CODE_INDEX_NO_HOSTED_PROVIDER`). See [the providers](../configuration/#the-providers-of-the-summaries-and-the-vectors). |
| `--include <FIELD=PATTERN>` | repeatable | unchanged | The include pattern of a forge filter field. Organization admin only. The fields are `branch_name`, `pr_head_branch`, `pr_title`, `pr_author`, `issue_title` and `issue_author`. See [Connect a git forge](../../how-to/connect-a-git-forge/#3-set-the-forge-filters). |
| `--exclude <FIELD=PATTERN>` | repeatable | unchanged | The exclude pattern of a forge filter field. Organization admin only. An empty pattern excludes nothing. |
| `--import-interval <OFF\|INTERVAL>` | `off`, or a number with a unit | unchanged | How often the repository takes its open issues, with no webhook. Organization admin only. The units are `s`, `m`, `h` and `d`, for example `90s`, `15m`, `1h` or `1d`. The shortest interval is `1m`. The longest is `30d`. The default is `off`. The repository needs a read token. See [Take issues on an interval](../../how-to/connect-a-git-forge/#6-take-issues-on-an-interval-with-no-webhook). |

Same permission gate as `create`. The rules of `--repo-url` and
`--default-branch` are those of `create`. They apply to a value that is
different from the value of the repository.

When each value is the value that the repository has, the server writes
nothing. The command prints this line:

```text
Kairos did not change the repository payments-api. It has these values already.
```

### `kairos repos import-issues`

```
kairos repos import-issues <REPOSITORY> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--wait` | flag | off | Wait until the import ends, then print its counts. |

Organization admin only. Imports the open issues of the repository one time.
The forge filter decides which issues Kairos takes. Each issue that it
admits becomes a support task, as an opened issue does. The import skips an
issue that Kairos took before.

The repository must have a read token with **Issues: read** (GitHub) or
**read_api** (GitLab). It needs no webhook connection.

The import is a job of the server. The command starts the job and prints
it. When an import of the repository waits or runs, the command prints that
import and starts no second one. With `--wait`, the command prints the
counts and the short codes of the new tasks when the job ends:

```text
Import 0b6f3c1e-7a52-4d8e-9f10-2c4b5a6d7e8f of payments-api: done (1 page(s) read).
Kairos imported 2 issue(s), skipped 1 taken before, and did not take 1 that the forge filter excludes.
  PAY-T-0012
  PAY-T-0013
```

### `kairos repos import-status`

```
kairos repos import-status <REPOSITORY> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |

Organization admin only. Prints the last import of the open issues of the
repository. The state is `queued`, `running`, `waiting`, `done` or
`failed`. The command also prints the pages that the import read, its
counts, and the error of a failed import. The state `waiting` gives the
time when the rate limit of the forge ends.

The command also prints the sync state of the repository:

| Line | Meaning |
|---|---|
| `Interval` | The import interval: `off`, or for example `15m`. |
| `last complete run` | When the last run that read every page ended. |
| `next run` | When the next run is due. It is `none` when the interval is `off`. |
| `issues in the record` | The count of issues that Kairos took for the repository. |
| `last run` | The open issues that the last run listed, the tasks that it made, and whether it was complete. |
| `last error` | Why the last run stopped. The next run tries again. |

A repository with an interval and no run yet prints `no import job has run
yet`, and the sync state.

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

### `kairos repos indexes`

```text
kairos repos indexes <REPOSITORY> [OPTIONS]
```

List the indexed commits of a repository, newest first. The order is the
time of the last write of each index. Use it when `fetch_index` refuses a
checkout. The list shows how far the nearest index is from the checkout.
Each member can read the list.

| Option | Type | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

| Column | Meaning |
|---|---|
| `COMMIT` | The first 12 characters of the commit. |
| `REF` | The branch, the pull request or the tag of the index, or `-`. |
| `SCHEMA` | The schema version of the index file, or `-` when the server cannot read it. |
| `SIZE` | The size of the structure: the index with no summaries. |
| `SUMMARIES` | The summaries that the index uses. |
| `CREATED` | When Kairos first stored an index of the commit, in UTC. |
| `UPDATED` | When Kairos last stored an index of the commit, in UTC. |

After the table, the command prints the count of the indexes and the page.
With `--json`, each row has the fields of
`GET /api/repositories/{slug}/code-indexes/commits`. See
[the pages of a list](#the-pages-of-a-list).

### `kairos repos local`

```text
kairos repos local [<FOLDER>] [OPTIONS]
```

List the clones and the worktrees of a folder, with the repository of each.
Use it in a board folder: a folder that holds the clones of a team. The
command lists each subfolder that has a `.git`. A folder that is a checkout
gives only that checkout. The command skips hidden folders.

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<FOLDER>` | path | the current folder | The board folder, or a checkout. |
| `--url <URL>` | string | `KAIROS_URL`, else the binding of the folder (see [`config.toml`](../configuration/#configtoml)), else the only deployment of `kairos login` | The deployment. |

| Column | Meaning |
|---|---|
| `FOLDER` | The name of the subfolder, or `.` for the folder itself. |
| `REPOSITORY` | The slug of the repository whose `repo_url` is the `origin` remote, or `-`. |
| `KIND` | `clone`, or `worktree of <clone>/`. |
| `BRANCH` | The branch of the checkout, or `(detached)`. |
| `BEHIND` | The commits of the default branch that the checkout does not have, and the branch. |
| `REMOTE` | The `origin` remote as `host/path`, or `-`. |

The command compares the remotes as `kairos index update` does. It removes
the scheme, the user, the token and the `.git` suffix. The output never
shows a user or a token.

The command runs no fetch. It counts from the local refs, as of the last
fetch. The default branch is the branch of the repository in Kairos. When
the checkout has no ref of that branch, the command uses `origin/HEAD`. Then
it uses `origin/main`, then `origin/master`.

With `--json`, the command prints `items`, `total` and `next_offset`. Each
item has the fields `folder`, `path`, `worktree`, `clone`, `branch`,
`remote`, `repository`, `default_branch` and `behind`. A field with no value
is `null`.

The session hook of the plugin runs this command in a board folder. When the
CLI has no such command, the hook lists the clones itself.

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
kairos repos credential remove <REPOSITORY> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--confirm` | flag | off | Required for the removal to happen. Without it, the command exits 1. |

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

### `kairos repos write-token`

```
kairos repos write-token set <REPOSITORY> [OPTIONS]
kairos repos write-token remove <REPOSITORY> --confirm [OPTIONS]
kairos repos write-token status <REPOSITORY> [OPTIONS]
```

Sets, removes or shows the write token of the write-back (KAIROS-T-0386).
With the token, Kairos writes a comment and the status `kairos` on each
linked GitHub pull request. Kairos also comments on each issue that became a
task. It closes or opens the issue with the task (KAIROS-T-0575). You must be
an organization admin or a member of the owner team.

`set` reads the token from standard input, as `kairos repos credential set`
does. Then Kairos writes each open pull request that links to an item. On
GitHub, use a fine-grained token with "Pull requests: write", "Commit
statuses: write" and "Issues: write" on the one repository. On GitLab, use a
project access token with the scope "api". Only a GitHub or a GitLab
repository takes a write token.

`remove` needs `--confirm`. Without it, the command removes nothing and
exits 1.

`remove` removes the token, and Kairos writes nothing more. `status` prints
`off (no write token)`, or `on` with the last write. `kairos repos show` shows
the same line. No command shows the token. See
[Turn on forge write-back](../../how-to/turn-on-github-write-back/).

Repositories are the codebases that tasks link to. See
[Repositories as execution scope](../../explanation/repositories-as-execution-scope/).

### `kairos repos webhook`

```
kairos repos webhook add <REPOSITORY> [OPTIONS]
kairos repos webhook list [<REPOSITORY>] [OPTIONS]
kairos repos webhook rotate <REPOSITORY> [OPTIONS]
kairos repos webhook remove <REPOSITORY> --confirm [OPTIONS]
```

Connects, lists, rotates or removes the webhook of a repository on GitHub or
GitLab. With the webhook, Kairos links the branches and the pull requests to
the items. `add`, `rotate` and `remove` need an organization admin.

`add` prints the payload URL and the secret. Paste them into the webhook
settings of the repository on the forge. **Kairos shows the secret only one
time.** The server makes the secret from the connection and does not keep it.
`list` shows no secret. With `--json`, `add` and `rotate` print the URL and
the secret too.

`rotate` makes a new connection, with a new URL and a new secret. The old URL
and secret stop at once, so change both on the forge. `remove` needs
`--confirm`. After it, Kairos refuses the deliveries of the forge.

The server refuses `add` with 501 when the deployment has no
`KAIROS_WEBHOOK_SIGNING_KEY` or no public URL. It refuses a second `add` with
409. `kairos repos show` gives the id of the connection on the line
`webhook:`.

### `kairos grants`

```
kairos grants list [OPTIONS]
kairos grants add --user <USER> [--capability <CAPABILITY>] [--board <BOARD>] [OPTIONS]
kairos grants remove <GRANT_ID> --confirm [OPTIONS]
kairos grants board-set --board <BOARD> --user <USER> --capability <CAPABILITY>... [OPTIONS]
kairos grants board-revoke --board <BOARD> --user <USER> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `--user <USER>` | string | required on `add`, `board-set` and `board-revoke` | A user id, an email, a user name, or the name of a service account. |
| `--capability <CAPABILITY>` | string | `manage_structure` | `manage_structure`, or a board capability such as `manage_tasks` or `configure_boards`. |
| `--board <BOARD>` | slug or UUID | none on `add`; required on `board-set` and `board-revoke` | For `manage_structure`: the one board of the grant. With no `--board`, the grant is on the organization. A board capability needs it. |
| `<GRANT_ID>` | UUID | required on `remove` | A grant of `manage_structure`, from `kairos grants list`. |
| `--confirm` | flag | off | Required on `remove` and `board-revoke` for the revoke to happen. |

`board-set` replaces the board capabilities of a member with the
`--capability` values. Repeat `--capability` for each capability. The member
must have one or more capabilities on the board. `board-revoke` revokes each
board capability of the member on the board. The two need
`administer_members` on the board, or `manage_structure` on the board or on
the organization.

`list` shows each grant of
[`manage_structure`](../capabilities/#manage_structure). Only an organization
admin can add or remove a grant of `manage_structure`. A board capability
needs `administer_members` on the board, or `manage_structure` on the board or
on the organization. See [Run an admin agent](../../how-to/run-an-admin-agent/).

### `kairos metadata`

```
kairos metadata list [OPTIONS]
kairos metadata create --name <NAME> --slug <SLUG> --type <TYPE> [--option <VALUE>]... [--entity-type <TYPE>]... [OPTIONS]
kairos metadata update <DEFINITION> [--name <NAME>] [--slug <SLUG>] [--option <VALUE>]... [--entity-type <TYPE>]... [OPTIONS]
kairos metadata delete <DEFINITION> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<DEFINITION>` | slug or UUID | required on `update` and `delete` | The metadata definition. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | On `list`: the page size. |
| `--offset <OFFSET>` | integer | 0 | On `list`: the rows to skip. |
| `--all` | flag | off | On `list`: read each page. Not with `--limit` or `--offset`. |
| `--name <NAME>` | string | required on `create` | The name. |
| `--slug <SLUG>` | string | required on `create` | The slug. |
| `--type <TYPE>` | string | required on `create` | `string`, `enum` or `date`. It does not change later. |
| `--option <VALUE>` | string | none | A value of an enum. Repeat it for each value, in the order of the scale. On `update`, the values replace the list. |
| `--entity-type <TYPE>` | string | each type | An entity type of the definition: `strategy`, `initiative`, `task`, `document` or `adr`. Repeat it. On `update`, the values replace the list. |
| `--confirm` | flag | off | Required on `delete` for the deletion to happen. Without it, the command exits 1. |

`list` shows one page of the metadata definitions. See
[the pages of a list](#the-pages-of-a-list). `update` and `delete` send the
slug or the id to the server. The server finds the definition in one call. Only an organization admin can create, update or delete a
definition. `delete` refuses a definition that an item or a
template uses, and the message names them.

### `kairos templates`

```
kairos templates list [OPTIONS]
kairos templates create --name <NAME> --slug <SLUG> [--content <TEXT>] [--field <FIELD>]... [OPTIONS]
kairos templates update <TEMPLATE> [--name <NAME>] [--slug <SLUG>] [--content <TEXT>] [--field <FIELD>]... [OPTIONS]
kairos templates delete <TEMPLATE> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<TEMPLATE>` | slug or UUID | required on `update` and `delete` | The template. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | On `list`: the page size. |
| `--offset <OFFSET>` | integer | 0 | On `list`: the rows to skip. |
| `--all` | flag | off | On `list`: read each page. Not with `--limit` or `--offset`. |
| `--name <NAME>` | string | required on `create` | The name. |
| `--slug <SLUG>` | string | required on `create` | The slug. |
| `--content <TEXT>` | string | empty | The markdown of a new item of the template. |
| `--field <FIELD>` | string | none | A metadata field: `SLUG`, `SLUG=DEFAULT`, `SLUG!` or `SLUG=DEFAULT!`. The `!` makes the field required. Repeat it. On `update`, the values replace the list. |
| `--confirm` | flag | off | Required on `delete` for the deletion to happen. Without it, the command exits 1. |

`list` shows one page of the templates. See
[the pages of a list](#the-pages-of-a-list). `update` and `delete` send the
slug or the id to the server. The server finds the template in one call. Only an organization admin can create, update or delete a
template. Items from a deleted template keep their content and their
metadata.

## Published sites

Kairos serves the built static site of a repository. The CI of the
repository builds the site and sends it. See
[Publish the docs site of a repository](../../how-to/publish-a-docs-site/).

Each `pages` command takes `--repo` and the common options.

| Option | Type | Default | Description |
|---|---|---|---|
| `--repo <REPOSITORY>` | slug or UUID | required | The repository of the site. |

### `kairos pages on`

```
kairos pages on --repo <REPOSITORY> [OPTIONS]
```

Turns the site of the repository on. The site is off until you turn it on.
A person who can change the repository can turn the site on.

### `kairos pages off`

```
kairos pages off --repo <REPOSITORY> [OPTIONS]
```

Turns the site off. Kairos does not serve it, and Kairos refuses a publish.
The files stay.

### `kairos pages base-path`

```
kairos pages base-path --repo <REPOSITORY> [OPTIONS]
```

Prints the base path of the site, for example
`/pages/acme/platform/repos/payments/`. Give it to the build as the path of
the site.

### `kairos pages publish`

```
kairos pages publish <PATH> --repo <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<PATH>` | path | required | The build folder, or a `.tar.gz` of it. The archive has `index.html` at its root. |

Publishes a built site. The site must be on. You need the capability `pages`
on the delivery board of the owner team.

### `kairos pages unpublish`

```
kairos pages unpublish --repo <REPOSITORY> --confirm [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--confirm` | flag | off | Required for the unpublish to happen. Without it, the command exits 1. |

Kairos deletes the files of the site. The list of the publishes stays.

## Machine access

Service accounts are machine principals authenticated by API keys. Agent keys
act as the person who made them.

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
kairos service-accounts delete <SERVICE_ACCOUNT> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SERVICE_ACCOUNT>` | name or UUID | required | The service account, from `kairos service-accounts list`. |
| `--confirm` | flag | off | Required for the deletion to happen. |

Deletes the service account and all of its keys.

### `kairos keys create`

```
kairos keys create --name <NAME> [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Label for the key, for example `laptop-claude-code` or `gha-main`. |
| `--service-account <SERVICE_ACCOUNT>` | name or UUID | none | The service account. Without it, the command makes an agent key for you. |
| `--expires-at <EXPIRES_AT>` | RFC 3339 instant | the default life | Expiry, for example `2027-01-01T00:00:00Z`. Without it, the key expires after the default life of the server: 90 days, unless the operator changed `KAIROS_API_KEY_TTL_DAYS`. |
| `--no-expiry` | flag | off | The key never expires. Do not use it with `--expires-at`. |
| `--scope <CAPABILITIES>` | comma list | the default scope | The capabilities of an agent key, for example `manage_tasks,transition_items`. Without it, the key gets each capability but `purge` and `manage_structure`. Not with `--service-account`. |
| `--read-only` | flag | off | An agent key with no capabilities. The key can only read. Not with `--scope` or `--service-account`. |
| `--scope-board <BOARD>` | slug or UUID | each board | A board where the agent key can act. Repeat the option, or separate the boards with commas. Not with `--service-account`. |

Without `--service-account`, the command makes an **agent key**. An agent key acts
as you, with your capabilities. Each member can make agent keys. Log in as
yourself to make one: a request with an agent key cannot make an agent key. Put
the key in the settings of the agent. For Claude Code, that is the variable
`KAIROS_MCP_KEY`. Do not put the key in a repository. See
[Give an agent your key](../../how-to/give-an-agent-your-key/).

The scope of the key can take capabilities away. It cannot add a capability.
See [The scope of an agent key](../capabilities/#the-scope-of-an-agent-key).

With `--service-account`, the command makes a key for that service account. Only
an organization admin can do this.

The command shows the raw key one time only. You cannot get it again.

### `kairos keys list`

```
kairos keys list [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--service-account <SERVICE_ACCOUNT>` | name or UUID | none | The service account. Without it, the command lists your agent keys. |

The list shows the start of each key only. It never shows the secret. The
column `SCOPE` shows the scope of each agent key. A key with no scope shows `-`.

### `kairos keys rotate`

```
kairos keys rotate <KEY_ID> [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<KEY_ID>` | UUID | required | Key id, from `kairos keys list`. |
| `--service-account <SERVICE_ACCOUNT>` | name or UUID | none | The service account of the key. Without it, the command rotates your agent key. |

The command makes a new key with the same name and a new secret. It shows the new
secret one time only. The old secret works until the end of the grace time: 1
day, unless the operator changed `KAIROS_API_KEY_ROTATION_GRACE_SECS`. The
command shows that time. Put the new key in the settings of the agent before it.

The new key has the life of the old key, from now. A key with no expiry gives a
key with no expiry. The list shows the old key with `REPLACED_BY`.

### `kairos keys revoke`

```
kairos keys revoke <KEY_ID> [--service-account <SERVICE_ACCOUNT>] --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<KEY_ID>` | UUID | required | Key id, from `kairos keys list`. |
| `--service-account <SERVICE_ACCOUNT>` | name or UUID | none | The service account of the key. Without it, the command revokes your agent key. |
| `--confirm` | flag | off | Required for the revocation to happen. |

## Agent keys of each person

An organization admin lists the agent keys of each person of the organization,
and revokes any one. Another person gets `403`.

### `kairos admin agent-keys list`

```
kairos admin agent-keys list [--user <USER>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--user <USER>` | UUID, email or user name | none | Only the keys of this person. A value that names no person gets `422`. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

The newest key is first. Each row shows the id, the owner, the name and the
prefix. It also shows the dates of creation, expiry, last use and revocation.
The list never shows a secret. A revoked key stays in the list. The keys of a service account are not
in the list: see `kairos keys list --service-account`.

### `kairos admin agent-keys revoke`

```
kairos admin agent-keys revoke <KEY_ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<KEY_ID>` | UUID | required | Key id, from `kairos admin agent-keys list`. |
| `--confirm` | flag | off | Required for the revocation to happen. |

The key then gets `401`. Kairos writes an activity row and gives the owner a
notification of the kind `agent_key_revoked` that names the key. An unknown id
gets `404`. A revoked key gets `409`.

## Code index settings

The providers of the summaries and the vectors of the code index are a
setting of the organization. Each member can read the setting. Only an
organization admin can change it. See
[the providers](../configuration/#the-providers-of-the-summaries-and-the-vectors)
and [Use a hosted model for the code index](../../how-to/use-a-hosted-model-for-the-code-index/).

### `kairos admin code-index-settings show`

```
kairos admin code-index-settings show [OPTIONS]
```

Shows the providers. For each secret, it shows if the secret is set, by whom
and when. It never shows the secret.

### `kairos admin code-index-settings set`

```
kairos admin code-index-settings set [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--summary-provider <SUMMARY_PROVIDER>` | `embedded`, `ollama-cloud` or `bedrock` | unchanged | The provider of the summaries. |
| `--summary-url <SUMMARY_URL>` | URL | unchanged | The base URL of the OpenAI-compatible endpoint, for `ollama-cloud`. |
| `--summary-model <SUMMARY_MODEL>` | string | unchanged | The model name (`ollama-cloud`) or the model id (`bedrock`). |
| `--summary-region <SUMMARY_REGION>` | string | unchanged | The AWS region, for `bedrock`. |
| `--summary-secret-stdin` | flag | off | Read the secret of the summaries from standard input: the API key, or `<access key id>:<secret access key>[:<session token>]` for `bedrock`. |
| `--clear-summary-secret` | flag | off | Remove the stored secret of the summaries. |
| `--vector-provider <VECTOR_PROVIDER>` | `embedded` or `remote` | unchanged | The provider of the vectors. |
| `--vector-url <VECTOR_URL>` | URL | unchanged | The base URL of the OpenAI-compatible embeddings endpoint, for `remote`. |
| `--vector-model <VECTOR_MODEL>` | string | unchanged | The model name of the embeddings, for `remote`. |
| `--vector-secret-stdin` | flag | off | Read the API key of the embeddings endpoint from standard input. |
| `--clear-vector-secret` | flag | off | Remove the stored secret of the vectors. |
| `--concurrency <CONCURRENCY>` | integer 1 to 32 | unchanged | The requests that a hosted summarizer sends at a time. |
| `--default-summaries <EMBEDDED\|HOSTED>` | `embedded` or `hosted` | unchanged | The summarizer of each repository that follows the organization. With `hosted`, the code of each changed symbol leaves the host. |

An option that you do not give keeps its value. A secret comes from standard
input, never from an argument:

```sh
printf '%s' "$OLLAMA_KEY" | kairos admin code-index-settings set --summary-provider ollama-cloud --summary-url https://ollama.com/v1 --summary-model gemma4:31b --summary-secret-stdin
```

## Read tokens of the organization

A read token of the organization is for one forge host, for example
`github.com`. The builder of the code index gives it to git. It does this for
each repository on the host with no read token of its own. Only an organization admin can
read and change them. No command shows a token.

### `kairos admin read-tokens list`

```
kairos admin read-tokens list [OPTIONS]
```

Lists the hosts that have a token, who set each token and when. With
`--json`, the command prints the list envelope.

### `kairos admin read-tokens set`

```
kairos admin read-tokens set <HOST> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<HOST>` | string | required | The forge host, with `:port` when the port is not the default. |

Sets or replaces the token of the host. The command reads the token from
standard input. When standard input is a terminal, the command asks for the
token and does not show it:

```sh
printf '%s' "$TOKEN" | kairos admin read-tokens set github.com
```

### `kairos admin read-tokens remove`

```
kairos admin read-tokens remove <HOST> --confirm [OPTIONS]
```

Removes the token of the host. Without `--confirm`, the command exits 1. With
`--json`, the command prints the tokens that stay.

## The job queue

An organization admin reads the job queue. Another person gets `403`. See
[The job queue](../../explanation/job-queue/#what-to-watch).

### `kairos admin jobs`

```
kairos admin jobs [OPTIONS]
```

Shows a table with one row for each kind of job. The columns are the counts of
the jobs in each state: `QUEUED`, `LEASED`, `DONE` and `FAILED`. Below the
table, it shows the last 20 failed jobs, newest first. Each failure has a line
with the time, the kind, the organization, the try and the job id. The error
is on the next line, at most 500 characters. A job of the deployment shows
`(deployment)` in place of the organization.

The command shows the jobs of your organization and the jobs of the
deployment. It does not show the jobs of other organizations. `--json` prints
the response of `GET /api/admin/jobs`.

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
| `--all` | flag | off | Read each page. The CLI asks for one page after the other and prints each row. Not with `--limit` or `--offset`. |

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

## The code index of a checkout

`kairos index` builds the code index of a git checkout, and serves its code
tools to an agent. The index is in the user cache, not in the checkout
(KAIROS-A-0072). Each checkout and each worktree has its own folder. See
[The place of the code index](../configuration/#the-place-of-the-code-index).
The CLI writes no file into the checkout. On the first open, it moves an old
`.kairos/index.db` of the checkout to the user cache.

Each `index` command takes `--root`. Only `index update` takes `--url` and
`--tenant`. Each `index` command except `index mcp` takes `--json`. See
[The JSON of `kairos index`](#the-json-of-kairos-index).

| Option | Type | Default | Description |
|---|---|---|---|
| `--root <ROOT>` | path | the root of the git repository of the current folder | The root of the checkout. |
| `--json` | flag | off | Print one JSON object with the result, in place of the text. |

`index build`, `index update`, `index status` and `index --full` need a
checkout. The CLI refuses a folder that is not in a git checkout, for example
a board folder that holds clones. It does not index the clones as one tree.

The refusal `NOT_A_CHECKOUT` (exit 1) lists the checkouts below the folder.
Run the command in one of them, or give one with `--root`. A `--root` that is
not in a checkout and holds no checkout is a root, as before. `index mcp` in a
board folder opens no checkout and names the checkouts.

### `kairos index`

```
kairos index --full [--root <ROOT>] [--json]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--full` | flag | off | Build the whole index again, with no base index. It is the same as `kairos index build`. |

With no subcommand and no `--full`, the command prints its help.

### `kairos index build`

```
kairos index build [--root <ROOT>] [--json]
```

Builds the index of the checkout: the structure, the call edges and the
summaries. A CLI with no summarizer makes the structure and the edges, and
says why it made no summaries.

### `kairos index update`

```
kairos index update [--root <ROOT>] [--rust-edges] [--link-only] [--max-changed <MAX_CHANGED>] [--repo <REPOSITORY>] [--url <URL>] [--tenant <TENANT>] [--json]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--rust-edges` (alias `--scip`) | flag | off | Run the SCIP indexers again for the precise edges of the changed code: rust-analyzer, scip-go, scip-typescript and scip-python. |
| `--link-only` | flag | off | Run no model. Link the summaries of the pool, and make no new summaries. |
| `--max-changed <MAX_CHANGED>` | integer | 200 when a model runs, else no limit | The most files that can change since the base index. Above it, the CLI builds nothing. With `--link-only` or on a hosted repository, no model runs. |
| `--repo <REPOSITORY>` | slug or UUID | `repository` of the binding of the checkout (`kairos config show`), else the repository of the `origin` remote | The repository in Kairos. The old name `--repository` still works. |
| `--url <URL>` | string | `KAIROS_URL`, else the binding of the checkout (see [`config.toml`](../configuration/#configtoml)), else the only deployment of `kairos login` | The deployment. |
| `--tenant <TENANT>` | string | `tenant` of the binding of the checkout (`kairos config show`), else the tenant of the login | The tenant, for a token from `kairos login`. A key carries its tenant. |

Updates the index for the changes in the checkout, also the changes that are
not committed.

A worktree has the `[[project]]` of its clone (the entry
matches the `origin` remote). Git excludes the retired
`.claude/kairos.local.md`, so a worktree has no such file. In a worktree, the
command reads the file of the main clone. The main clone is the folder above
`git rev-parse --git-common-dir`. With no index, the command starts from the nearest base index
in Kairos. See
[`kairos index update` and the base index](../configuration/#kairos-index-update-and-the-base-index).

### `kairos index status`

```
kairos index status [--root <ROOT>] [--json]
```

Shows the counts of the index: files, symbols, edges and summaries.

### `kairos index duplicates`

```
kairos index duplicates [--root <ROOT>] [--kind <KIND>] [--min-lines <MIN_LINES>] [--tests] [--under <UNDER>] [--limit <LIMIT>] [--json]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--kind <KIND>` | `exact`, `near` or `same-idea` | each kind | Only this kind of repeated code. |
| `--min-lines <MIN_LINES>` | integer | 5 | The smallest function to compare, in lines. |
| `--tests` | flag | off | Also compare test code. |
| `--under <UNDER>` | path | the whole checkout | Only the functions in files under this path, from the root of the checkout. |
| `--limit <LIMIT>` | integer 1 to 200 | 20 | The most groups to show. |

Finds repeated code: exact copies, near copies and the same idea in other
code. The text is that of the MCP tool `duplicates`.

### `kairos index references`

```
kairos index references <SYMBOL> [--root <ROOT>] [--file <FILE>] [--line <LINE>] [--possible] [--limit <LIMIT>] [--json]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SYMBOL>` | string | required | A name, `Type::name` or `Type.name`, or a candidate `name (file:line)`. Or an ADR short code, as `KAIROS-A-0065`. |
| `--file <FILE>` | path | none | The file of the symbol, when more than one symbol has the name. |
| `--line <LINE>` | integer | none | A line of the symbol, when more than one symbol of one file has the name. |
| `--possible` | flag | off | Also show the calls by a name that more than one function has. |
| `--limit <LIMIT>` | integer 1 to 1000 | 100 | The most uses to show. |

Shows each use of a symbol. In Rust, a use is a type, a field, an import or
a call. In the other languages, a use is a call.

With an ADR short code, the command shows each place that cites the ADR.
A place is a comment, a doc string or the name of a test. Each place has
its line and its symbol. The command then takes no `--file`, `--line` or
`--possible`.

### `kairos index upload`

```
kairos index upload [--root <ROOT>] [--url <URL>] [--tenant <TENANT>] [--repo <REPOSITORY>] [--ref <REF>] [--json]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--repo <REPOSITORY>` | slug or UUID | as `kairos index update` | The repository in Kairos. |
| `--ref <REF>` | string | the branch of the checkout | The branch, the pull request or the tag of the index, for example `main`, `pull/12` or `v1.0.0`. |
| `--url`, `--tenant` | | as `kairos index update` | The deployment and the tenant. |

Sends the index of the checkout to Kairos as the index of its HEAD commit:
`PUT /api/repositories/{slug}/code-indexes/{commit}`. Use it when the
deployment has no builder of the code index, for example in a CI job. Build
or update the index first:

```sh
kairos index build
kairos index upload --repo payments-api --ref main
```

The index describes the tree of the checkout. So the command refuses a tree
with changes that have no commit. It also refuses a checkout with no index. You must be an organization admin or a member of the owner team. The
command prints the commit, the ref and the counts of the summaries. With
`--json`, it prints the answer of the server.

### `kairos index mcp`

```
kairos index mcp [--root <ROOT>]
```

Serves the code tools of the index to an agent: MCP over standard input and
output. The Claude Code plugin starts this command.

When the root is not a git checkout, the server opens no index. Its
instructions and the `NO_CHECKOUT` error of each tool name the clones and
worktrees directly below the root. For each one they give the
path, the `origin` remote and whether it has a local index. The agent opens one
of them with `fetch_index`.

The server keeps more than one checkout open. Each call of `fetch_index` adds a
checkout, and the last one opened is the default of the tools. Each tool takes
`checkout`, the folder name or the path of an open checkout, to read another
one. `code_search` with no `checkout` searches each open checkout, and each
result names its checkout.

### The JSON of `kairos index`

With `--json`, an `index` command prints one JSON object on stdout, and no
text. A refusal is the error envelope on stderr (see
[Errors with `--json`](#errors-with---json)). Stdout then stays empty.

`index build`, `index --full` and `index update` print what the run did:

```json
{
  "command": "update",
  "index": "/home/robin/.cache/kairos/index/app-4a68e4c25d4d38c3/index.db",
  "moved_from": null,
  "start": {
    "from": "base",
    "base": { "repository": "app", "branch": "main", "commit": "9f2c…", "changed": 3, … },
    "local_changed": null,
    "no_base": null,
    "summaries_taken": null
  },
  "structure": { "files": 120, "parsed_files": 98, "symbols": 840, "edges": { "total": 2210, … }, … },
  "summaries": { "result": "not_made", "linked": 830, "left": { "symbols": 10, … }, "reason": "…", … },
  "warnings": []
}
```

| Key | Description |
|---|---|
| `command` | `build` or `update`. `index --full` gives `build`. |
| `index` | The path of the index file. |
| `moved_from` | The old index of the checkout (`.kairos/index.db`), when the run moved it to the user cache. Else `null`. |
| `start` | Where the update started. It is `null` for a build. |
| `start.from` | `base`: the update downloaded the base index of Kairos. `local`: the update changed the local index. |
| `start.base` | The base index that Kairos gave: the repository, the branch, the commit, the changed files and the count of its summaries. The update can keep the local index and not use it. |
| `start.local_changed` | The files of the tree that differ from the local index. It is `null` when the checkout has no local index. |
| `start.no_base` | Why Kairos gave no base index. |
| `start.summaries_taken` | What the update did with the summaries of the base index, when the local index is as near to the tree. |
| `structure` | The counts of the files, the symbols, the edges, the SCIP runs and the token vectors, and the time in seconds. |
| `summaries` | `result` is `linked`, `not_made` or `made`. The object also has the counts by level, and why no model made the missing summaries. |
| `warnings` | The warnings of the run. |

`index status` prints the counts of the index: `files`, `symbols`, `edges`
and `summaries`, with the models.

`index duplicates` and `index references` print the list envelope:
`items`, `total` and `next_offset` (see [The JSON of a list](#the-json-of-a-list)). `total` is the count of
the rows that the index found. `items` has the first `--limit` rows. These
commands have no `--offset`, so `next_offset` is `null`. To get more rows,
give a larger `--limit`.

- `index duplicates`: each item is a group, with its `kind`, its `score` and
  its `symbols`. The object also has `compared`, `tests`, `min_lines` and
  `summary_vectors`.
- `index references`: each item is a use, with its `file`, `line`, `kind` and
  the `enclosing` symbol. The object also has the `symbol`, the `coverage` of
  its language and `possible_not_shown`.

A refusal of `index` has one of these codes in the error envelope:

| Code | Description |
|---|---|
| `NO_INDEX` | The checkout has no index. The details give the `path`. |
| `NO_START` | The update has no index to start from. The details give the `path` and the reason. |
| `TOO_FAR` | More files than the limit changed since the base index. The details give `changed`, `limit`, `commit`, `branch` and `behind`. `local` is `true` when the count is that of the local index. |
| `NO_ROOT` | The folder of `--root` is not there. |
| `NOT_A_CHECKOUT` | `index build`, `index update`, `index status` or `index --full` in a folder that is not in a git checkout. The details give the `path` and the `checkouts` below it. |
| `NOT_FOUND`, `AMBIGUOUS` | `index references` found no symbol, or more than one. The details of `AMBIGUOUS` give the `candidates`. |
| `CLI_ERROR` | Any other failure. |

## The CLI itself

### `kairos config`

```
kairos config show [FOLDER] [--json]
kairos config project [FOLDER] [--remote <REMOTE> | --path] [--deployment <URL>] [--tenant <TENANT>]
    [--repository <SLUG>] [--team-board <SLUG>] [--initiative-board <SLUG>]
    [--adr-board <SLUG>] [--delivery-stream <SLUG>] [--json]
kairos config migrate [FOLDER] [--remote <REMOTE>] [--json]
kairos config path
```

These commands read and write the config of the person, `config.toml` in the
config directory (see [`config.toml`](../configuration/#configtoml)).

`config show` gives the binding of the folder: the deployment, the tenant,
the repository and the boards. A subfolder of a checkout has the binding of
the top of the checkout. For each value it gives the source: the
`[[project]]` of the folder, `.claude/kairos.local.md`, or `[default]`. For
the file, see `config migrate`. When `KAIROS_URL` is set, the output says
that commands use it.

The hooks of the plugin read `config show --json`. The JSON has `bound`, which
is `true` when a `[[project]]` or `.claude/kairos.local.md` binds the folder.
It also has `env_url`, the value of `KAIROS_URL`. It has `login_url`: the
deployment of the only login of the CLI, else `null`. When the binding has
no deployment, the hooks use `login_url` and name it as the source.

`config project` adds a `[[project]]` for the `origin` remote of the folder, or
for `--remote`. The command adds the text to the end of the file and does not
change the other lines. When the file has an entry for the remote with the
same values, the command changes nothing. When the entry has other values, the
command refuses. Then you edit the entry in the file.

A folder with no `origin` remote gets a `[[project]]` with `path`. Examples
are a board folder that holds clones, and a checkout with no remote. In a
checkout, the path is the top of the checkout. With `--path`, a folder that
has a remote also gets a `path` entry.

`config migrate` moves the values of `.claude/kairos.local.md` to a
`[[project]]` for the `origin` remote of the folder, or for `--remote`. It
moves only the values that the file gives to the binding: a value of the
`[[project]]` stays. When the file gives no value, the command changes
nothing.

The command does not remove the file: check the result with `config show`,
then remove the file. This release still reads the file. While the file gives
a value, `config show` gives a warning (`wiring_warning` in the JSON). Each
other command that reads the binding gives the warning once on stderr.
`mcp-proxy` also adds it to the instructions of the server.
The next release does not read the file.

`config path` prints the path of `config.toml`.

### `kairos mcp-proxy`

```
kairos mcp-proxy [--url <URL>] [--tenant <TENANT>] [--profile work|admin|all]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--profile <PROFILE>` | `work`, `admin` or `all` | `work` | The tool profile: `work` sends to `/mcp`, `admin` to `/mcp?profile=admin`, `all` to `/mcp?profile=all`. |

Serves the MCP tools of Kairos over standard input and output. The command
sends each message to `/mcp` of the deployment, over HTTP. The Claude Code
plugin starts this command as its `kairos` MCP server.

To get the admin tools in Claude Code, add a second server with the admin
profile:
`claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin`.

The command finds the deployment and the tenant in the order of the
[common options](#common-options). The folder of the binding is
`CLAUDE_PROJECT_DIR`, else the current folder. The credential is
`KAIROS_KEY` or `KAIROS_MCP_KEY`, else the token of `kairos login`. The
command refreshes the token when it expires.

When the deployment refuses a request or does not answer, the client gets a
JSON-RPC error with the reason.

When the command cannot find a deployment or a credential, it does not stop.
It serves MCP with no tools, and the instructions of the server give the
reason and the commands to run. Standard error has the same text. These
faults give the server with no tools:

- There is no login, and no key with a URL.
- `KAIROS_KEY` or `KAIROS_MCP_KEY` is set, but no URL names the deployment.
- The credential cache has logins for more than one deployment, and no flag,
  no `KAIROS_URL` and no binding names one.
- `config.toml` does not parse. The reason gives the file and the line.

After the fix, reconnect the server with `/mcp` in Claude Code, or start the
session again.

### `kairos self-update`

```
kairos self-update [--url <URL>] [--version <VERSION> | --latest | --check] [--allow-downgrade] [--json]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | `KAIROS_URL`, then the only cached deployment | The deployment whose version to install. |
| `--version <VERSION>` | string | the version of the server | Install this version (e.g. `0.12.0`). It can be older than the CLI. |
| `--latest` | flag | off | Install the latest release on GitHub. |
| `--check` | flag | off | Print the version of the CLI and of the server, and change nothing. The exit code is 3 when the server is newer, else 0. |
| `--allow-downgrade` | flag | off | Install the version of the server or of `--latest` also when it is older than the CLI. |
| `--json` | flag | off | Print one JSON object: the versions and the action. |

Installs the version of the server in place of the running binary. The server
tells its version at `GET /version` (open). It also gives the link of the
tarball of the CLI for each platform. `KAIROS_CLI_DOWNLOAD_URL` on the server
sets these links; the default is the GitHub releases.

The CLI downloads the tarball of its platform,
`kairos-<version>-<target>.tar.gz`. It checks the tarball against the
`.sha256` of the published release. The CLI always takes the `.sha256` from
the GitHub releases, also when the server names a mirror for the tarball. The
CLI does not use the `.sha256` of the server. A server cannot give a tarball that is
not a published release. `KAIROS_RELEASES_URL` replaces the GitHub releases,
for a mirror that you trust.

The CLI writes the new binary to a new file next to the old one. It keeps
the old binary as `kairos.bak` and renames the new file over the old path. A
failed download or a bad checksum changes nothing.

A version older than the CLI is a downgrade. The command refuses the version
of the server or of `--latest` when it is older. The refusal names
`--allow-downgrade` and `--version`, and nothing changes. The exit code is 1.
`--version` installs the version that it names, older or newer.

`--check` is for a script: `kairos self-update --check || kairos self-update`
updates the CLI only when the server is newer.

With `--json`, the command prints one object on stdout:

```json
{
  "cli_version": "0.14.0",
  "server_version": "0.13.0",
  "deployment": "https://kairos.example.com",
  "version": "0.13.0",
  "action": "refused",
  "message": "The version 0.13.0 is older than this CLI (0.14.0). Nothing changed. To install it, use --allow-downgrade or --version 0.13.0.",
  "binary": null,
  "backup": null
}
```

`action` is `checked` (`--check`), `unchanged` (the same version),
`updated` (a newer version), `installed` (an older version) or `refused`.
`server_version` and `deployment` are `null` with `--version` and
`--latest`. `binary` and `backup` are the paths after a swap. The exit codes
do not change. An error that is not a refusal is the JSON error envelope on
stderr.

A server older than this command does not tell its version: name one with
`--version`, or use `--latest`. The CLI refuses a server whose links
(`KAIROS_CLI_DOWNLOAD_URL`) have no tarball for this platform. The message
names the platforms that the server lists. The CLI also refuses a version
that is not a release version, such as `1.2.3` or `1.2.3-rc.1`.

The command writes the binary that runs, wherever it is: a `cargo install`
binary in `~/.cargo/bin` too. A folder the CLI cannot write refuses the
update, and nothing changes.

The CLI, the server and the Claude Code plugin have one version. The code
tools of the plugin run `kairos index mcp`, and an older CLI reads an older
index schema. An update of the plugin does not update the CLI. When the CLI
is older than the plugin, the plugin says so at the start of the session. Its
code tools are off until you run `kairos self-update`.

#### The daily check

After a command that succeeded, the CLI asks the deployment of that command
for its version. That is the `--url` of the command, else `KAIROS_URL`, the
checkout settings of `index` or the only cached deployment. A command that
uses no deployment asks the default deployment. The check runs once a day
for each deployment, with a 2 second limit.

The commands `login`, `logout`,
`index mcp` and `self-update` make no check. The other `index` commands make
the check. `KAIROS_UPDATE_CHECK=0` turns the check off, for a script or CI.

When the server is newer, the CLI prints one line on stderr. The line names
`kairos self-update` and `kairos self-update --check`. With
`KAIROS_AUTO_UPDATE=1`, the CLI updates itself instead of printing the line.
It says so on stderr, and the output of the command stays what it is.
`KAIROS_AUTO_UPDATE` does not update from an `http://` deployment: the CLI
prints the line, and it says why.

The check keeps its state in `version-check.json` in the config directory.
The CLI records only an answer. When a server does not answer, the next
command asks it again.

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
| `embed-backfill`, `embed-index` | Retrieval maintenance; see [Configure retrieval](../../how-to/configure-retrieval/). |

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
| `credentials.json.lock` inside it | `0600` | The lock of the cache. A token refresh, `login` and `logout` hold it while they change the cache. The lock is an advisory lock of the operating system. Another process waits for it, and then reads the new token. |
| `credentials.json.<PID>.tmp` inside it | `0600` | The temporary file of a write to the cache. The rename to `credentials.json` removes it. |
| `version-check.json` inside it | default | The last daily version check of each deployment: the time and the version of the server. |

[Configuration](../configuration/#cli-configuration) is the canonical entry for
the resolution order, the file's full shape, and the refresh behaviour.

## Related reading

- [Flight levels](../../explanation/flight-levels/)
- [Teams and boards](../../explanation/teams-and-boards/)
- [Capabilities and access](../../explanation/capabilities-and-access/)
- [Archiving](../../explanation/archiving/)
- [Repositories as execution scope](../../explanation/repositories-as-execution-scope/)
- [Configuration](../configuration/)
- [Glossary](../glossary/)

## Related guides

- [Give an agent machine access](../../how-to/give-an-agent-machine-access/)
- [Provision a tenant](../../how-to/provision-a-tenant/)
- [Connect a git forge](../../how-to/connect-a-git-forge/)
- [Move work between boards](../../how-to/move-work-between-boards/)
- [Find archived work](../../how-to/find-archived-work/)
