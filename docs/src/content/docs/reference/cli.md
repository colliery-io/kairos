---
title: "CLI"
---

`kairos` is the command-line client. It talks to the same HTTP API as the GUI
and the MCP server.

This page describes `kairos` 0.15.1. The command tree below mirrors
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
| Organization | `orgs`, `members`, `teams`, `streams`, `repos`, `grants`, `boards create`, `boards columns`, `boards transitions`, `metadata`, `templates` |
| Machine access | `service-accounts`, `keys` |
| Deployment administration | `admin` |
| The CLI itself | `self-update` |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | API or validation error: not found, forbidden, conflict, invalid transition, bad input. A transport failure: the deployment or the OIDC issuer gave no answer, also during a token refresh. Also the case of several cached deployments with no `--url`. |
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

## Common options

Every command that reaches the API accepts these three. They are omitted from
the per-command tables below.

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | `KAIROS_URL`, then the only cached deployment | Deployment base URL. Required when more than one deployment is cached and `KAIROS_URL` is not set. |
| `--tenant <TENANT>` | string | the tenant cached at login (none with a key) | Tenant slug, sent as the `X-Tenant` header. Used by deployments that resolve tenants by header rather than by host subdomain. |
| `--json` | flag | off | Print the raw JSON DTO instead of the human-readable table. |

`--url` values are normalized by trimming whitespace and trailing slashes, so
`https://kairos.example/` and `https://kairos.example` address the same cached
entry.

## Authentication

A deployment lets people log in through an OIDC issuer, with local accounts, or
with both. `kairos login` has one mode for each. Both modes write the same
credential cache, so every other command works the same after either.

A command can also use a key in place of the login.

| Variable | Description |
|---|---|
| `KAIROS_KEY`, then `KAIROS_MCP_KEY` | An agent key (it acts as you) or a service-account key. When one is set, each command that reaches the API sends it as the bearer, and does not read the credential cache. The key wins over a login. |
| `KAIROS_URL` | The deployment, when `--url` is not given. With a key and no URL, the CLI uses the only cached deployment. Otherwise it exits with code 2. |
| `KAIROS_TIMEOUT` | The time in seconds that a command waits for an answer. The default is 60. A connection gets 5 seconds. A value that is not a positive integer stops the command with exit code 1. |
| `KAIROS_UPDATE_CHECK` | `0` turns off the daily version check (see [`kairos self-update`](#kairos-self-update)). |
| `KAIROS_AUTO_UPDATE` | `1` makes the daily check install the newer version of the server instead of printing a line. |
| `KAIROS_RELEASES_URL` | The base URL of the release tarballs for `self-update --version` and `--latest`, for a mirror. The default is the GitHub releases. |

A key carries its tenant, so it needs no `--tenant`. A 401 for a key names the
variable that holds it. Use a key when the issuer gives the CLI no device
grant, for example Google. See
[Give an agent your key](../../how-to/give-an-agent-your-key/#use-the-key-with-the-cli).

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
team names, or `(none)`. A caller with a grant of `manage_structure` gets a
fourth line, `manage_structure:`. It gives the scope of each grant:
`organization` or `board <slug>`.

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
| `get` | yes | yes | yes | yes | yes |
| `create` | yes | yes | yes | yes | yes |
| `edit` | yes | yes | yes | yes | yes |
| `transition` | yes | yes | yes | no | yes |
| `move` | no | no | yes | yes | no |
| `claim` | no | no | yes | no | no |
| `hand-off` | no | no | yes | no | no |
| `release` | no | no | yes | no | no |
| `cancel` | no | no | yes | no | no |
| `archive` (alias `delete`) | yes | yes | yes | yes | yes |
| `purge` | no | no | yes | no | no |
| `restore` | yes | yes | yes | yes | yes |

`documents` has no `transition` verb: documents have no board placement, and
carry an editorial lifecycle instead of a column. `move` exists on `tasks`
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
`Next:` line. With `--json`, the command prints the envelope of the server
as it is: `items`, `total`, `limit` and `offset`.

These verbs page so: `<noun> list`, `boards list`,
`members list`, `teams list`, `streams list`, `repos list`, `metadata list`,
`templates list`, `admin agent-keys list`, `admin tenants list` and
`kairos search`.

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
| `strategies` | `--board <BOARD>` | slug or UUID | required | Board to create the strategy on. |
| `strategies` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `initiatives` | `--board <BOARD>` | slug or UUID | required | Board to create the initiative on. |
| `initiatives` | `--column <COLUMN_ID>` | UUID | the board's first column | Only the entry column (Discovery). An initiative starts there, and a different column is refused with `VALIDATION` (KAIROS-T-0643). |
| `tasks` | `--board <BOARD>` | slug or UUID | required unless `--team` is given | Delivery board to create the task on. The board decides the team of the task. With `--team` and no `--board`, the task goes to the delivery board of that team. |
| `tasks` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `documents` | `--board <BOARD>` | slug or UUID | required | The owner board of the document. It gives the right to edit the document, and the code of the document gets the prefix of this board. The document is not a card of the board. |
| `documents` | `--parent <SHORT_CODE>` | string | none | The workflow item the document supports. It does not give the document an owner board. |
| `adrs` | `--board <BOARD>` | slug or UUID | none | ADR board. Omitting it creates an off-board ADR, which is an org-admin operation. |
| `adrs` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |

Options specific to one noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--hypothesis <HYPOTHESIS>` | string | none | The strategy's hypothesis. |
| `initiatives` | `--complexity <COMPLEXITY>` | `xs` \| `s` \| `m` \| `l` \| `xl` | none | T-shirt sizing. |
| `initiatives` | `--bucket-type <KIND>` | `tech_debt` \| `bug` \| `ad_hoc` | none | Marks the initiative as a bucket of this kind. |
| `tasks` | `--type <TASK_TYPE>` | `task` \| `bug` \| `tech_debt` \| `support` | `task` | Task type. |
| `tasks` | `--work-class <WORK_CLASS>` | `planned` \| `support` | `support` for support-type tasks, `planned` otherwise; always `support` on a board that the caller does not manage | Planned/Support lane. On a board that the caller does not manage, `planned` is refused with 403. |
| `tasks` | `--from-team <TEAM>` | slug or UUID | none | The team that sends the task. When it is not the team of the board, the task is a request: entry column, support lane, also for a manager of the board. With it, `planned` and a column other than the entry column are refused with 422. Kairos does not keep it on the task: name the sending team in the body. |
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
[Send a request to a different team](../../how-to/move-work-between-boards/#send-a-request-to-a-different-team).
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

### `<noun> note`

```
kairos <noun> note <SHORT_CODE> --text <TEXT> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--text <TEXT>` | string | required | The markdown text of the note. Must not be empty. |

Appends a note at the end of the item's content. The note gets a title with
its id, the date and your name: `## Note n-3 (2026-10-11, Robin)`. Send no
version: the server locks the item and writes one new version. Two notes at
the same time both stay. The edit rule applies, as for `edit`.

The output names the id: `Kairos appended note n-3 to ACME-T-0012 (version
4).` Cite the note by its id: "note n-3 on ACME-T-0012". With `--json`, the
response has `note_id`.

### `<noun> promote-note`

```
kairos <noun> promote-note <SHORT_CODE> <NOTE_ID> --kind <KIND> --board <BOARD> [--title <TITLE>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `<NOTE_ID>` | string | required | The id of the note, as `get` lists it: `n-3`. |
| `--kind <KIND>` | `adr` \| `document` | required | What the note becomes. |
| `--board <BOARD>` | slug or UUID | required | The board that gets the new item: an ADR board for an ADR, the owner board for a document. |
| `--title <TITLE>` | string | the first line of the note | The title of the new item. |

Turns a note into a draft ADR or document. The new item has the text of the
note. A line names the note, its author and its date. The new item
supports the item when the item is a strategy, an initiative or a task. The
note gets a trailing line that names the new item.

You need the edit rule on the item, and `manage_adrs` or `manage_documents`
on the board. The command prints one line:

```text
Kairos promoted note n-3 on ACME-T-0012 to the draft adr ACME-A-0029. ACME-A-0029 supports ACME-T-0012.
```

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

A task that a person moves to Active gets the [claim](../glossary/#claim) of the
person. `tasks get` shows it in the field `claim`. A task with a live state
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
repository.
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
`tasks get` shows the mark in the line `cancelled`. A move out of the done
column removes the mark. You need `transition_items` on the board of the task.

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

### `state`

```
kairos state <SHORT_CODE> [--branch <BRANCH>] [--commit <COMMIT>] [--pushed | --not-pushed] [--pr <URL>] [--next <TEXT>] [--clear <FIELD>]... [--expected-updated-at <INSTANT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's short code. |
| `--branch <BRANCH>` | string | none | The branch of the work. |
| `--commit <COMMIT>` | string | none | The last commit (a hash or a short hash). |
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
holder of its claim (KAIROS-T-0532), and `manage_tasks` on its board. `tasks get` shows the state in the field
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

### `<noun> archive`

```
kairos <noun> archive <SHORT_CODE> --confirm [OPTIONS]
```

`delete` is an alias of `archive`: `kairos tasks delete ACME-T-0001 --confirm`
archives the task.

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |
| `--confirm` | flag | off | Required for the archive to happen. Without it, nothing is archived. |

The archive is a soft delete and cascades to the item's children. Archived
items are hidden from `list` unless `--include-deleted` is passed, and are
recoverable with `restore`.

The cascade takes the descendants that you can edit
([the edit rule](../capabilities/#the-edit-rule)). It stops at a descendant that
you cannot edit, and takes nothing below it. The command names each descendant
that stays, and the reason. With `--json`, they are in `not_reached`.

### `tasks purge`

```
kairos tasks purge <SHORT_CODE> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task's current short code. |
| `--confirm` | flag | off | Required for the purge to happen. Without it, nothing is deleted. |

Deletes a task for good (a [purge](../glossary/#purge)). **You cannot undo
this.** No `restore` brings the task back. The task can be live or archived.
You need `manage_tasks` on the board of the task. To keep a copy that you can
restore, use `tasks archive`.

### `<noun> restore`

```
kairos <noun> restore <SHORT_CODE> [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item's short code. |

Puts an archived item back on its board. The item's own archived children stay
archived: restore is per item, not a cascade.

### `kairos link`

```
kairos link <SHORT_CODE> external <URI> [--label <TEXT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item, of each type. |
| `external` | relationship | required | The one relationship of this command. |
| `<URI>` | string | required | The URI. The scheme is `http`, `https` or `mailto`. |
| `--label` | string | none | The label of the reference, at most 200 characters. |

The command links an item to a document outside Kairos: an
[external reference](../glossary/#external-reference). You must be able to
edit the item. The command prints this line:

```text
Kairos made the link: ACME-T-0012 -[external]-> RFC: zen as the policy engine <https://example.com/wiki/rfc-42>.
```

The server refuses each other scheme with 422 `VALIDATION`, and the refusal
names the scheme. The server refuses a URI that the item has with 422
`ALREADY_LINKED`. To say that a document impacts a repository, use `kairos
repos link`.

### `kairos unlink`

```
kairos unlink <SHORT_CODE> external <URI> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The item. |
| `external` | relationship | required | The one relationship of this command. |
| `<URI>` | string | required | The URI of the reference. |

The command removes an external reference. The rule is that of `kairos link`.
The command prints this line:

```text
Kairos removed the link: ACME-T-0012 -[external]-> RFC: zen as the policy engine <https://example.com/wiki/rfc-42>.
```

The command fails with exit code 1 when the item has no reference with that
URI.

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
| `--task-type <TASK_TYPE>` | `task` \| `bug` \| `tech_debt` \| `support` | all | Task type filter. Repeatable. |
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

Board capability grants are not part of the CLI surface. See
[Capabilities and access](../../explanation/capabilities-and-access/).

A column with a WIP limit shows its live count against the limit after its
name, for example `[WIP 2/3]`. When it has more items than the limit, it shows
`[WIP 4/3, over the limit]`.

### `kairos boards flow`

```
kairos boards flow <BOARD> [--from <YYYY-MM-DD>] [--to <YYYY-MM-DD>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--from <DATE>` | `YYYY-MM-DD` | 89 days before `--to` | The first day of the range (UTC). |
| `--to <DATE>` | `YYYY-MM-DD` | today | The last day of the range (UTC). |

Shows the flow metrics of the board for the range:

- the cycle time: the count, the mean, the median and the 85th percentile;
- the throughput per week;
- the work in progress, with its age;
- the cumulative flow of the last 14 days. `--json` gives each day.

A cancelled task counts apart from the done items. A range has 366 days at
most. The definitions are in [`board_flow`](../mcp-tools/#board_flow).

### `kairos boards policy`

```
kairos boards policy <BOARD> [--parent-gate on|off] [--owner-gate on|off] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<BOARD>` | slug or UUID | required | The board. |
| `--parent-gate` | `on` or `off` | unchanged | The policy `parent_gate` of a delivery board. |
| `--owner-gate` | `on` or `off` | unchanged | The policy `owner_gate` of an initiative board. |

Shows the policies of a board, and sets them. To set a policy, you need
`configure_boards` on the board. With `parent_gate` on, a task moves to a
claims column only while its parent initiative is active. Kairos refuses a
different move with `PARENT_NOT_ACTIVE`.

With `owner_gate` on, an initiative
moves to Active only with an owner. Kairos refuses a different move with
`OWNER_REQUIRED`. See
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
kairos boards columns update <BOARD> <COLUMN> [--name <NAME>] [--position <N>] [--done <BOOL>] [--claims <BOOL>] [OPTIONS]   (one option at least)
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
kairos boards dispatch test-target <BOARD> <TARGET> [OPTIONS]
kairos boards dispatch add-rule <BOARD> [--trigger entered_column|unblocked] [--column <COLUMN>] --target <TARGET> [--task-type <TYPE>] [--metadata <SLUG=VALUE>] [--repository <REPO>] [OPTIONS]
kairos boards dispatch remove-rule <BOARD> <RULE> --confirm [OPTIONS]
kairos boards dispatch agent-triage <BOARD> [--on | --off] [OPTIONS]
kairos boards dispatch log <BOARD> [--target <TARGET>] [--rule <RULE>] [--limit <N>] [OPTIONS]
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
| `enable-target` | `configure_boards` | Turns on a target that Kairos turned off. |
| `test-target` | `configure_boards` | Sends the event `ping` to a target. |
| `add-rule` | `configure_boards` | Makes a rule. |
| `remove-rule` | `configure_boards` | Removes a rule. Its deliveries stay in the log. It needs `--confirm`. |
| `agent-triage` | a place on the board; `configure_boards` with `--on` or `--off` | Shows or sets who decides the triage of intake. `--on`: an agent decides. `--off`: an agent proposes with a note, and a person decides. |
| `log` | a place on the board | Shows the newest deliveries, with the status, the tries and the response code. |

A place on the board is a grant on the board, a place in its team, or the
admin role. Without `--confirm`, `remove-target` and `remove-rule` remove
nothing and exit 1.

| Option of `add-rule` | Type | Default | Description |
|---|---|---|---|
| `--trigger <TRIGGER>` | string | `entered_column` | `entered_column`: a task enters the column. `unblocked`: the last open blocker of a task closes (the event `task.unblocked`). |
| `--column <COLUMN>` | name or UUID | required for `entered_column` | The column. For `unblocked`, the column the task must be in; with none, each column that is not done. |
| `--target <TARGET>` | UUID | required | A target of the board. |
| `--task-type <TYPE>` | string | none | Only tasks of this type: `task`, `bug`, `tech_debt` or `support`. |
| `--metadata <SLUG=VALUE>` | string | none | Only tasks with this metadata value, for example `priority=high`. |
| `--repository <REPO>` | slug or UUID | none | Only tasks of this repository. |

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
kairos teams get <TEAM> [OPTIONS]
```

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
| `--type <TEAM_TYPE>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | `stream_aligned` | Team Topologies type. |

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
| `--type <TEAM_TYPE>` | `stream_aligned` \| `platform` \| `enabling` \| `complicated_subsystem` | unchanged | New type. |

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
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

After the table, the command prints the count of the repositories and the
page: `total: 12 (limit 50, offset 0)`. See
[the pages of a list](#the-pages-of-a-list).

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
- `Not linked to an item:` the in-flight branches and pull requests with no
  item that links to them

The line `forge filters:` gives the forge filters of the repository. It is
`default (admit everything)`, or each field with a pattern that is not the
default.

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

The repository must have a webhook connection and a read token with
**Issues: read** (GitHub) or **read_api** (GitLab).

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

Sets, removes or shows the write token of the GitHub write-back
(KAIROS-T-0386). With the token, Kairos writes a comment and the status
`kairos` on each linked pull request. You must be an organization admin or a
member of the owner team.

`set` reads the token from standard input, as `kairos repos credential set`
does. Then Kairos writes each open pull request that links to an item. Use a
GitHub fine-grained token with "Pull requests: write" and "Commit statuses:
write" on the one repository. Only a GitHub repository takes a write token.

`remove` needs `--confirm`. Without it, the command removes nothing and
exits 1.

`remove` removes the token, and Kairos writes nothing more. `status` prints
`off (no write token)`, or `on` with the last write. `kairos repos get` shows
the same line. No command shows the token. See
[Turn on GitHub write-back](../../how-to/turn-on-github-write-back/).

Repositories are the codebases that tasks link to. See
[Repositories as execution scope](../../explanation/repositories-as-execution-scope/).

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
| `--name <NAME>` | string | required on `create` | The name. |
| `--slug <SLUG>` | string | required on `create` | The slug. |
| `--type <TYPE>` | string | required on `create` | `string`, `enum` or `date`. It does not change later. |
| `--option <VALUE>` | string | none | A value of an enum. Repeat it for each value, in the order of the scale. On `update`, the values replace the list. |
| `--entity-type <TYPE>` | string | each type | An entity type of the definition: `strategy`, `initiative`, `task`, `document` or `adr`. Repeat it. On `update`, the values replace the list. |
| `--confirm` | flag | off | Required on `delete` for the deletion to happen. Without it, the command exits 1. |

`list` shows one page of the metadata definitions. See
[the pages of a list](#the-pages-of-a-list). `update` and `delete` find the
slug on each page. Only an organization admin can create, update or delete a
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
| `--name <NAME>` | string | required on `create` | The name. |
| `--slug <SLUG>` | string | required on `create` | The slug. |
| `--content <TEXT>` | string | empty | The markdown of a new item of the template. |
| `--field <FIELD>` | string | none | A metadata field: `SLUG`, `SLUG=DEFAULT`, `SLUG!` or `SLUG=DEFAULT!`. The `!` makes the field required. Repeat it. On `update`, the values replace the list. |
| `--confirm` | flag | off | Required on `delete` for the deletion to happen. Without it, the command exits 1. |

`list` shows one page of the templates. See
[the pages of a list](#the-pages-of-a-list). `update` and `delete` find the
slug on each page. Only an organization admin can create, update or delete a
template. Items from a deleted template keep their content and their
metadata.

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
kairos service-accounts delete <ID> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<ID>` | UUID | required | Service account id, from `kairos service-accounts list`. |
| `--confirm` | flag | off | Required for the deletion to happen. |

Deletes the service account and all of its keys.

### `kairos keys create`

```
kairos keys create --name <NAME> [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Label for the key, for example `laptop-claude-code` or `gha-main`. |
| `--service-account <SERVICE_ACCOUNT>` | UUID | none | The service account. Without it, the command makes an agent key for you. |
| `--expires-at <EXPIRES_AT>` | RFC 3339 instant | the default life | Expiry, for example `2027-01-01T00:00:00Z`. Without it, the key expires after the default life of the server: 90 days, unless the operator changed `KAIROS_API_KEY_TTL_DAYS`. |
| `--no-expiry` | flag | off | The key never expires. Do not use it with `--expires-at`. |

Without `--service-account`, the command makes an **agent key**. An agent key acts
as you, with your capabilities. Each member can make agent keys. Log in as
yourself to make one: a request with an agent key cannot make an agent key. Put
the key in the settings of the agent. For Claude Code, that is the variable
`KAIROS_MCP_KEY`. Do not put the key in a repository. See
[Give an agent your key](../../how-to/give-an-agent-your-key/).

With `--service-account`, the command makes a key for that service account. Only
an organization admin can do this.

The command shows the raw key one time only. You cannot get it again.

### `kairos keys list`

```
kairos keys list [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--service-account <SERVICE_ACCOUNT>` | UUID | none | The service account. Without it, the command lists your agent keys. |

The list shows the start of each key only. It never shows the secret.

### `kairos keys rotate`

```
kairos keys rotate <KEY_ID> [--service-account <SERVICE_ACCOUNT>] [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<KEY_ID>` | UUID | required | Key id, from `kairos keys list`. |
| `--service-account <SERVICE_ACCOUNT>` | UUID | none | The service account of the key. Without it, the command rotates your agent key. |

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
| `--service-account <SERVICE_ACCOUNT>` | UUID | none | The service account of the key. Without it, the command revokes your agent key. |
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
| `--user <USER>` | UUID or email | none | Only the keys of this person. A value that names no person gets `422`. |
| `--limit <LIMIT>` | integer | server default 50, maximum 200 | Page size. |
| `--offset <OFFSET>` | integer | 0 | Rows to skip. |

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

## The CLI itself

### `kairos self-update`

```
kairos self-update [--url <URL>] [--version <VERSION> | --latest | --check]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | `KAIROS_URL`, then the only cached deployment | The deployment whose version to install. |
| `--version <VERSION>` | string | the version of the server | Install this version (e.g. `0.12.0`). |
| `--latest` | flag | off | Install the latest release on GitHub. |
| `--check` | flag | off | Print the version of the CLI and of the server, and change nothing. The exit code is 3 when the server is newer, else 0. |

Installs the version of the server in place of the running binary. The server
tells its version at `GET /version` (open). It also gives the links of the
tarball of the CLI and its `.sha256` for each platform. `KAIROS_CLI_DOWNLOAD_URL`
on the server sets these links; the default is the GitHub releases.

The CLI downloads the tarball of its platform,
`kairos-<version>-<target>.tar.gz`, and checks it against the `.sha256`. It
writes the new binary to a new file next to the old one. It keeps the old
binary as `kairos.bak` and renames the new file over the old path. A failed
download or a bad checksum changes nothing. With `--version` or `--latest`,
the CLI takes the tarball from the GitHub releases; `KAIROS_RELEASES_URL`
replaces that URL, for a mirror.

`--check` is for a script: `kairos self-update --check || kairos self-update`
updates the CLI only when the server is newer.

A server older than this command does not tell its version: name one with
`--version`, or use `--latest`. The CLI refuses a server whose links
(`KAIROS_CLI_DOWNLOAD_URL`) have no tarball for this platform. The message
names the platforms that the server lists.

The command writes the binary that runs, wherever it is: a `cargo install`
binary in `~/.cargo/bin` too. A folder the CLI cannot write refuses the
update, and nothing changes.

The CLI, the server and the Claude Code plugin have one version. The code
tools of the plugin run `kairos index mcp`, and an older CLI reads an older
index schema. An update of the plugin does not update the CLI. When the CLI
is older than the plugin, the plugin says so at the start of the session. Its
code tools are off until you run `kairos self-update`.

#### The daily check

After a command that succeeded, the CLI asks the default deployment for its
version. That is `KAIROS_URL`, then the only cached deployment, not the
`--url` of the command. It does this once a day, with a 2 second limit. The
commands `login`, `logout`, `index` and `self-update` make no check.
`KAIROS_UPDATE_CHECK=0` turns the check off, for a script or CI.

When the server is newer, the CLI prints one line on stderr that names
`kairos self-update`. With `KAIROS_AUTO_UPDATE=1`, the CLI updates itself
instead of printing the line. It says so on stderr, and the output of the
command stays what it is. The check keeps its state in `version-check.json`
in the config directory. A server that does not answer counts as a check.

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
| `version-check.json` inside it | default | The last daily version check: the deployment, the time and the version of the server. |

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
