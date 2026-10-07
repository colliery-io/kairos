# Build the code index again

Kairos keeps a code index of each repository. The builder of the server
updates it after each push to the default branch. It also makes the first
index of a repository that has none. This page tells you how to see what
the builder did, and how to ask for a new index.

## See the runs of the builder

Each run of the builder leaves a record. A run is a build after a push, a
first build, a build on request, or an upload. Each member can read the
records.

```bash
kairos repos builds <slug>
```

The command shows the runs, newest first. Each run has its trigger,
outcome, commit, count of symbols, and the text of a failure. Use
`--limit <N>` for more runs (100 at most).

The same records are on the page of the repository in the GUI, and at
`GET /api/repositories/{slug}/code-indexes/builds`.

A run can have the text "The server stopped during the run." That run was
active when the server stopped. The next run of the repository replaces
it.

## Ask for a new index

You need the right to change the repository: you are an organization admin,
or a member of the owner team.

```bash
kairos repos reindex <slug>
```

The builder makes a full index of the head of the default branch. The new
index replaces the index of that commit. The command shows the run. Follow
it with `kairos repos builds <slug>`.

An agent asks with the MCP tool `rebuild_code_index`. The REST route is
`POST /api/repositories/{slug}/code-indexes/rebuild`, with no body.

## When the request is refused

| Code | Cause | What to do |
|---|---|---|
| `CODE_INDEX_BUILD_RUNNING` | A run of the repository is active. | Wait for its end. `kairos repos builds <slug>` shows it. |
| `CODE_INDEX_BUILD_OFF` | The repository has the builder off. | Run `kairos repos update <slug> --code-index-build on`, then ask again. |
| `CODE_INDEX_BUILDER_OFF` | The deployment has no builder. | An operator sets `KAIROS_CODE_INDEX_DIR`. See [Configuration](../reference/configuration.md). |
| `FORBIDDEN` | You cannot change the repository. | Ask an organization admin, or a member of the owner team. |

## Related

- [Read a private repository](read-a-private-repository.md): the read
  token that the builder gives to git.
- [Configuration](../reference/configuration.md): the settings of the
  builder.
