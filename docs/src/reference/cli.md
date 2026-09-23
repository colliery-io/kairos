# CLI

`kairos` is the command-line client. It talks to the same HTTP API as the GUI
and the MCP server.

This page describes `kairos` 0.1.0. The command tree below mirrors
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
| 2 | Authentication error: no cached credentials, expired or rejected credentials, a failed token refresh, HTTP 401, a corrupted credential cache. |

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

### `kairos login`

Logs in via the OAuth Device Authorization Grant and caches the resulting
tokens.

```
kairos login --url <URL> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | required | Deployment base URL, e.g. `https://kairos.example.com`. |
| `--issuer <ISSUER>` | string | discovered | OIDC issuer override. Skips RFC 9728 discovery against the deployment. |
| `--tenant <TENANT>` | string | none | Tenant slug, cached and sent as `X-Tenant` on subsequent API calls. |
| `--client-id <CLIENT_ID>` | string | `kairos-cli` | OAuth client id registered for the CLI at the issuer. |
| `--bearer <BEARER>` | `access_token` \| `id_token` | `access_token` | Which token is cached and sent as the API bearer. `id_token` suits issuers whose access token is opaque, such as Google Workspace; `access_token` suits Dex and Keycloak. |

`login` takes neither `--json` nor the cached-deployment form of `--url`.

### `kairos logout`

Removes one deployment's entry from the credential cache.

```
kairos logout [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--url <URL>` | string | the only cached deployment | Deployment whose credentials are forgotten. |

`logout` takes neither `--tenant` nor `--json`.

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
| `move` | no | no | yes | no | no |
| `delete` | yes | yes | yes | yes | yes |
| `restore` | yes | yes | yes | yes | yes |

`documents` has no `transition` verb: documents have no board placement, and
carry an editorial lifecycle instead of a column. `move` exists only on
`tasks`, the only type that sits on a team delivery board. See
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
| `tasks` | `--board <BOARD_ID>` | UUID | required unless `--repo` is given | Delivery board. With `--repo` and no `--board`, the task is routed to the repository's owning team's delivery board. |
| `tasks` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |
| `documents` | `--parent <SHORT_CODE>` | string | required | The workflow item the document supports. Documents take no board; they inherit that item's board for authorization. |
| `adrs` | `--board <BOARD_ID>` | UUID | none | ADR board. Omitting it creates an off-board ADR, which is an org-admin operation. |
| `adrs` | `--column <COLUMN_ID>` | UUID | the board's first column | Column to place it in. |

Options specific to one noun:

| Noun | Option | Type | Default | Description |
|---|---|---|---|---|
| `strategies` | `--hypothesis <HYPOTHESIS>` | string | none | The strategy's hypothesis. |
| `initiatives` | `--complexity <COMPLEXITY>` | `xs` \| `s` \| `m` \| `l` \| `xl` | none | T-shirt sizing. |
| `initiatives` | `--bucket-type <KIND>` | `tech_debt` \| `bug` \| `ad_hoc` | none | Marks the initiative as a bucket of this kind. |
| `tasks` | `--type <TASK_TYPE>` | `task` \| `bug` \| `tech_debt` \| `support` | `task` | Task type. |
| `tasks` | `--work-class <WORK_CLASS>` | `planned` \| `support` | `support` for support-type tasks, `planned` otherwise | Planned/Support lane. |
| `tasks` | `--team <TEAM_ID>` | UUID | the repository's owning team | Owning team. |
| `tasks` | `--repo <REPOSITORY>` | slug or UUID | none | Repository to issue the task against. Routes the task to the owning team's delivery board. |
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

The task lands in the target board's entry column and follows that board's
team. Constraints: `manage_tasks` is required on both boards, and a task bound
to a repository may move only to that repository's owning team's board —
clearing the binding with `kairos repos unbind` is a prerequisite otherwise.
See [Move work between boards](../how-to/move-work-between-boards.md).

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
| `--board <BOARD_ID>` | UUID | none | Restrict to items on this board. |
| `--column <COLUMN_ID>` | UUID | none | Restrict to items in this column. |
| `--team <TEAM_ID>` | UUID | none | Restrict to tasks assigned to this team. |
| `--repo <REPOSITORY>` | slug or UUID | none | Restrict to tasks issued against this repository. |
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
| `--sort <FIELD>` | `created_at` \| `updated_at` \| `title` | `created_at` | Sort field. |
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
kairos boards show <BOARD_ID> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<BOARD_ID>` | UUID | required | Board to show. |

Shows the board's live items grouped by column. Archived items are not shown
and there is no flag to include them.

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
kairos teams create --name <NAME> --slug <SLUG> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--name <NAME>` | string | required | Team name. |
| `--slug <SLUG>` | string | required | Team slug. The team's delivery board becomes `{slug}-delivery`. |
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

| Argument | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |

Shows the owning team, the delivery board, the how-to-work-here description,
and in-flight pull requests.

### `kairos repos create`

```
kairos repos create --forge <FORGE> --name <FULL_NAME> --repo-url <URL> --team <TEAM> [OPTIONS]
```

| Option | Type | Default | Description |
|---|---|---|---|
| `--forge <FORGE>` | `github` \| `gitlab` \| `other` | required | The forge. |
| `--name <FULL_NAME>` | `owner/repo` | required | Must match what the forge sends in webhooks. |
| `--repo-url <URL>` | string | required | Browser URL of the repository. |
| `--team <TEAM>` | slug or UUID | required | Owning team. |
| `--slug <SLUG>` | string | derived from `--name` | Slug. |
| `--default-branch <BRANCH>` | string | `main` | Default branch. |
| `--description <DESCRIPTION>` | string | none | Short "how to work here" blurb for agents. |

Permitted to an org admin or a member of the owning team.

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

Same permission gate as `create`.

### `kairos repos delete`

```
kairos repos delete <REPOSITORY> --confirm [OPTIONS]
```

| Argument / Option | Type | Default | Description |
|---|---|---|---|
| `<REPOSITORY>` | slug or UUID | required | The repository. |
| `--confirm` | flag | off | Required for the removal to happen. |

Org admin only. Refused while any task or webhook connection still references
the repository.

### `kairos repos bind`

```
kairos repos bind <SHORT_CODE> <REPOSITORY> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task. |
| `<REPOSITORY>` | slug or UUID | required | The repository. Must be owned by the team whose board the task sits on. |

### `kairos repos unbind`

```
kairos repos unbind <SHORT_CODE> [OPTIONS]
```

| Argument | Type | Default | Description |
|---|---|---|---|
| `<SHORT_CODE>` | string | required | The task whose repository binding is cleared. |

Repositories are the codebases tickets are issued against. See
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
| `--initial-admin <OIDC_SUB>` | string | the caller | OIDC `sub` of the initial org admin. That user must have logged in at least once. |

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

## Files

| Path | Mode | Contents |
|---|---|---|
| config directory | `0700` | Resolved from `KAIROS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/kairos`, else `$HOME/.config/kairos`. |
| `credentials.json` inside it | `0600` | The token cache, keyed by normalized deployment URL. |

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
