# kairos plugin

This directory holds the components of the `kairos` Claude Code plugin (manifest lives at the
repo root in `.claude-plugin/plugin.json`; layout per KAIROS-A-0014).

```
plugin/
├── skills/
│   ├── workflow/     # grill-initiative, to-initiative, decompose, grill-decomposition, triage, onboard-team, implement, ralph, ralph-initiative, cancel-ralph
│   ├── engineering/  # tdd, diagnosing-bugs, prototype, research, code-review, ...
│   ├── review/       # architecture-review, diataxis-review
│   └── meta/         # kairos (router), kairos-vocabulary, grill-me, grilling, handoff, writing-great-skills, bootstrap
├── references/       # rendered review specs (see below)
├── hooks/            # SessionStart context injection, the Ralph loop (Stop hook), the active work after a compaction, the code index, the Kairos rules once per session, the git guard, the memory reminder, the ADRs of a file before an edit
└── bin/              # kairos-code (the code tools over stdio), kairos-channel (the dispatch channel)
```

The manifest registers three MCP servers: `kairos` (the deployment, over HTTP with the agent
key), `kairos-code` (the code index, stdio) and `kairos-dispatch` (the dispatch channel,
stdio). Each is described below.

- `meta/` — `kairos` (the `/kairos` router), `grilling` and `kairos-vocabulary` (model-invoked;
  `kairos-vocabulary` names work only by short codes and Kairos words, with `ITEM-TYPES.md`
  from outside words to item types), `grill-me`,
  `handoff`, `writing-great-skills` (the normative authoring reference), `bootstrap`
  (wires a repo to a deployment — detects the repository from the git remote, takes the
  team board from the team of the principal — and writes `.claude/kairos.local.md`).
  It reads `/api/config` to choose the authentication path: OAuth, or the agent key of the
  user on a deployment with no issuer, or with an issuer that has no dynamic client
  registration (Google). Each person makes their own agent key with
  `kairos keys create` or on the Agent keys page and puts it in `KAIROS_MCP_KEY`, with the
  deployment URL in `KAIROS_URL`, for the `kairos` server of the plugin (below); the key acts as that person
  (KAIROS-A-0024). Do not share a service-account key for Claude Code. That server lists
  the work profile of the MCP tools; for the administration tools, see "The admin tools"
  below.
- `workflow/` — `grill-initiative` (an interview that writes each settled answer into the
  initiative or strategy at once, and never moves it), `to-initiative`, `decompose`,
  `grill-decomposition` (reviews the open tasks of an initiative as a set before a Ralph loop),
  `triage`,
  `ralph`, `ralph-initiative`, `cancel-ralph` (user-invoked), `implement` (model-invoked per
  KAIROS-A-0014)
- `engineering/` — `grill-with-docs` (user-invoked); `tdd`, `diagnosing-bugs`, `prototype`,
  `research`, `domain-modeling`, `codebase-design`, `code-review` (model-invoked)
- `review/` — `architecture-review`, `diataxis-review` (user-invoked, driven by the
  rendered references below)

Every shipped skill directory is listed in `.claude-plugin/plugin.json`, and the `/kairos`
router maps the whole user-invoked surface; changing the skill set without re-syncing both
(and this README) is a defect (KAIROS-A-0014).

## The SessionStart hook holds no credentials

`hooks/hooks.json` registers `hooks/session_start.py` (python3, stdlib only) for SessionStart.
It reads the wiring (absent → silent no-op): `kairos config show --json` gives the
`[[project]]` of the config of the person, else `.claude/kairos.local.md`, with `KAIROS_URL`
first (#270; with no CLI that has the verb, the hook reads the file). It probes `<deployment_url>/healthz`
unauthenticated, and injects the wiring summary as `additionalContext` with an instruction to
pull live state over the **authenticated MCP connection**: with a `repository` wired
(KAIROS-A-0019) that is `get_repository` + `board_items` on the team board narrowed to the
repository — the session's queue; without one, `my_boards`/`board_items` as before. The hint
also says to name a board at create and how to send a request (COLLIERY-A-0023). Its pure parts are unit
tested (`hooks/test_session_start.py`, part of `angreal test unit`).
In a worktree, which has no `.claude/kairos.local.md` (the file is git-excluded), each hook
reads the file of the clone, the folder above `git rev-parse --git-common-dir` (KAIROS-T-0520).
In a board folder (`~/Boards/<board>/`: clones and worktrees, no `.git` of its own) the hook
lists each checkout with its origin remote, its branch and the count of commits behind the
origin default branch, from the local refs with no fetch (KAIROS-T-0521).
When the `kairos` CLI has `kairos repos local` (KAIROS-T-0646), the hook runs it with the
wired `deployment_url`, and each line also names the repository of the clone. The CLI uses
its own credentials. An older CLI, or a CLI that fails, gives the listing of the hook.
Hooks cannot drive the client's OAuth flow (KAIROS-A-0011) and tokens live with the MCP client
(KAIROS-A-0014), so the hook deliberately never authenticates; offline degrades to a note.
That holds on the agent-key path too: the MCP client sends the agent key, and the hook
does not read `KAIROS_MCP_KEY`.

## The Ralph loop runs tasks with no stop between them

`/kairos:ralph <code>... [--max-iterations N]` and `/kairos:ralph-initiative <code>
[--max-iterations N]` run tasks one by one, each through the steps of `implement`
(COLLIERY-T-1861, ported from the Metis plugin 2.3.0). `/kairos:cancel-ralph` stops the loop.

- The skill reads each item with the kairos MCP tools, then runs `hooks/ralph.py start` (or
  `start-initiative`, with the open tasks and their blockers as JSON on stdin). The script
  refuses a missing code, a malformed code, an unknown option and an unknown field, and names
  it. For an initiative it puts each blocker before the task that it blocks, and it skips a
  Blocked task, a task whose blocker is not done, and a cycle.
- The state is one JSON file for each Claude Code session, outside the repository:
  `~/.claude/kairos-ralph/<session_id>.json` (`KAIROS_RALPH_STATE_DIR` overrides the folder).
  The skill gives the session ID with `${CLAUDE_SESSION_ID}`; the Stop hook gets it in its input.
  A start and a cancel remove state files older than 30 days. After `/clear` the session has a
  new ID: `cancel` then finds the one loop of the folder by its `project_dir` (with several, it
  names them and `--loop <session id>` picks one).
- `hooks/ralph.py stop` is the `Stop` hook. With no state for its session it does nothing.
  Otherwise it blocks the stop and gives the next instruction, until the last reply ends with
  `<promise>ALL TASKS COMPLETE</promise>`, `<promise>LOOP STOPPED</promise>` (the agent stopped
  the loop and wrote the reason on the task and its initiative) or `<promise>NEED INPUT</promise>`
  (the agent wrote a question for the user on the task), or until the iteration limit: 25 stops
  by default, `--max-iterations 0` for no limit.
  Each error in the hook lets the session stop.
- Tests: `hooks/test_ralph.py`, part of `angreal test unit`.

## The active work comes back after a compaction

After a context compaction, `hooks/active_work.py compact` gives the agent its active work
again (COLLIERY-T-1862). It names the Active (and Blocked) tasks of this repository that the
session worked on. For each task, it gives the title, the initiative and the next step. It also names the
Ralph loop of the session: the iteration and the tasks left.

- It is a `SessionStart` hook with the matcher `compact`. A `PreCompact` hook cannot do this:
  it has no `additionalContext`, Claude Code writes its stdout to the debug log only, and it
  discards its `systemMessage`. `SessionStart` fires again after each compaction, and its
  `additionalContext` is a system reminder on the next request.
- The hook does not read Kairos and holds no credentials. `hooks/active_work.py record` is a
  `PostToolUse` hook on the kairos `get_item`, `transition_item`, `edit_item`,
  `update_item`, `append_note` and `set_state` tools. It keeps a local record for each session in
  `~/.claude/kairos-work/<session_id>.json` (`KAIROS_WORK_STATE_DIR` overrides the folder).
  The record has the title, repository, column and parent from the `get_item` result. It has
  the column from `transition_item`, and the next step from the text that the agent writes. The next step is
  the text after the last `Next step:` (or `Next:`) in the notes; without one, the last line.
  The hook removes records older than 30 days.
- The record can be stale, so the text always tells the agent to confirm with `get_item`, and
  to find its Active tasks with `board_items` (`repository=<repo>`, `column=Active`). With no
  record, that instruction is all the text. Each error gives no output; the hook never blocks a
  tool call or a compaction.
- Tests: `hooks/test_active_work.py`, part of `angreal test unit`.

## Each session gets the code index

The plugin gives each session the code index of the checkout (COLLIERY-T-1855, COLLIERY-I-0264).

- **The code tools.** `.claude-plugin/plugin.json` registers the local MCP server `kairos-code`
  (stdio). It starts `bin/kairos-code`, which runs `kairos index mcp`. It gives `module_map`,
  `code_search`, `symbol`, `callers`, `callees`, `references`, `path`, `duplicates` and
  `fetch_index`. A
  symbol is chosen by its name, `file` and `line` (KAIROS-T-0417). `references` gives each use
  of a symbol, not only the calls: in Rust, also the uses as a type, a field or an import, from
  SCIP. For the other languages it gives the calls (KAIROS-T-0419). With an ADR short code as
  `symbol`, it gives each place that cites the ADR: a comment, a doc string or a test name
  (KAIROS-T-0703). A session that starts
  outside a checkout (a board folder) has no index open: the agent calls `fetch_index` with the
  path of the checkout, and the server takes the nearest index of the repository from Kairos
  (KAIROS-T-0348). The instructions of the server, and the error of a tool with no open
  checkout, name each clone and worktree below the start folder, with its `origin` remote and
  whether it has a local index (#272). Each `fetch_index` adds a checkout: the tools read the
  last one opened, or the one that their `checkout` argument names, and `code_search` with no
  `checkout` searches each open checkout. If `kairos` is not on PATH, `bin/kairos-code` is an MCP server with no
  tools (COLLIERY-T-2529). Its instructions tell the agent that `kairos` is not on PATH, and
  Claude Code shows no connection error. If `kairos` is older than the plugin
  (`.claude-plugin/plugin.json`), it is the same server with no tools: an old CLI reads an
  old index schema and refuses the index of the server (KAIROS-T-0513). Its instructions
  name both versions and `kairos self-update`. `hooks/session_start.py` says the same in one
  line. A plugin update does not update the CLI: run `kairos self-update` after it.
- **The update at the start.** `hooks/index_update.py start` is a `SessionStart` hook. If
  `kairos` is on PATH and the checkout has `.claude/kairos.local.md`, it starts
  `kairos index update --link-only` as a detached process and does not wait for it. The
  update runs no model: it links the summaries that are in the pool and makes no new ones
  (COLLIERY-T-2529). Summaries come from Kairos, or from a `kairos index update` that a person
  runs. Set `KAIROS_INDEX_SUMMARIZE=1` in the environment of Claude Code to make the background
  update run the model (a `kairos` built with `llama` can then summarize for some minutes with
  about 4 GB of memory). It also tells the agent
  to use the code tools before it reads files. In a board folder with the wiring, it starts
  the same update in each clone and worktree below the folder (#272). Else it does nothing and
  gives no output. The
  update reads the deployment, the repository and the key (`KAIROS_KEY` or `KAIROS_MCP_KEY`)
  itself (COLLIERY-T-1854). The hook holds no credentials.
- **The list of changed files.** `hooks/index_update.py record` is a `PostToolUse` hook on
  `Edit`, `Write`, `MultiEdit` and `NotebookEdit`. It finds the checkout of each edited source
  file (`.rs`, `.py`, `.ts`, `.tsx`, `.js`, `.jsx`, `.go`): the nearest folder above the file with
  `.git`. It adds the path of the file to the list of that checkout. In a board folder, each
  edit goes to the list of its own clone or worktree (KAIROS-T-0524). An edit outside a git
  checkout records nothing. The checkout needs the wiring: its own, the one of its clone, or
  the one of the session folder above it. The background job takes the list
  first, writes it to the log, runs the update, and clears the list only when the update
  succeeds. The update finds each changed file by its content hash, also with no list, so the
  list does not make the update faster today.
- **The update after edits.** After an edit, `record` starts a detached
  `index_update.py debounce` for the checkout of the file, if none waits already
  (`debounce.lock`). This job waits until no edit came for 30 seconds. Then it waits for an
  update that runs, and it runs the same update as the start. An edit during the update makes
  it run again. So a burst of edits gives one update. The code tools are current about a
  minute after the last edit (KAIROS-T-0524). `KAIROS_INDEX_DEBOUNCE_SECONDS` sets the
  number of seconds. `off` stops the update after an edit. With no `kairos` on PATH, no job
  starts.
- **The result of the last update.** After each update, the background job writes
  `last-update.json` in the state folder: the exit code, the time, the command and the refusal.
  The refusal is the last `error: ` line of the output of `kairos` (KAIROS-T-0519). The next
  `index_update.py start` reads this file before it starts a new update. `active_work.py
  compact` reads it after a compaction. When the update failed, each hook gives the agent one
  line: the exit code, the refusal, the command to run again and the log. A success, no file, a
  file that does not parse, or a result older than 7 days gives no line.
- **The state** is outside the checkout: `~/.claude/kairos-index/<name>-<hash>/` holds
  `changed-files`, `update.log` (and `update.log.1`), `last-update.json`, `lock`, `last-edit` and
  `debounce.lock`. `KAIROS_INDEX_STATE_DIR` overrides the parent folder. Only one update runs at
  a time for each checkout.
- Tests: `hooks/test_index_update.py` (also `bin/kairos-code`), part of `angreal test unit`.

## The dispatch channel pushes a delivery into the session

`bin/kairos-channel` is the MCP server `kairos-dispatch` of the manifest (KAIROS-T-0429): a
Claude Code channel over stdio. It listens on a local port for the signed deliveries of the
dispatch rules of a board (KAIROS-T-0385, KAIROS-T-0427), checks the HMAC signature, the time
and the delivery id of each one, and pushes the event into the running session; the session
then works the task with the Kairos MCP tools. The event carries facts only (the code, the
board, the column, the type, the lane, the event, the rule, the blocker); the session reads
the item itself.

- The config of a board is `~/.claude/channels/kairos/<name>.json`, mode 600, where `<name>`
  is `KAIROS_CHANNEL_BOARD`, else the name of the folder of the session. Write it with
  `bin/kairos-channel setup <name> --port <port>`, the secret of the target on stdin; the
  target comes from `kairos boards dispatch add-target <board> --hook-url http://127.0.0.1:<port>/`.
  With no config, the server serves MCP with no listener (KAIROS-T-0544). In a folder with no
  Kairos wiring it gives no instructions; in a wired folder its instructions give the three
  steps to turn dispatch on, with the full path of `kairos-channel`.
- The listener answers 503 until the client sends `notifications/initialized`; Kairos tries
  again. The client's `initialize` does not show whether the session loaded the channel, so
  a session started without the flag still takes the deliveries and drops them.
- The delivery ids are kept in `<name>.seen.json` beside the config (mode 600): a restart does
  not push a delivery twice. The session that listens writes `<name>.owner.json` (pid, folder,
  start time); a second session that finds the port taken names it.
- The instructions name the intake steps (KAIROS-T-0543) and the push policy (KAIROS-A-0046).
- Channels are in research preview: the session starts with
  `claude --dangerously-load-development-channels plugin:kairos@kairos`. The how-to
  "Receive dispatch in a Claude Code session" in the book has the steps.
- The secret is never printed or logged. Python stdlib only.
- Tests: `hooks/test_channel.py`, part of `angreal test unit`. One of them checks that the
  how-to names the flags that the CLI has (KAIROS-T-0545).

## The Kairos rules come once per session

`hooks/kairos_rules.py` is a `PreToolUse` hook on the kairos MCP tools, on `Skill` and on
Bash (KAIROS-T-0498, COLLIERY-I-0631). On the first Kairos tool call of a session (an
`mcp__plugin_kairos_*` or `mcp__kairos*` tool, a `kairos*` skill, or a `kairos` command) it
gives the Kairos rules as `additionalContext`, once: cite each item as a link
`<deployment_url>/items/<CODE>` (the bare code when no URL is wired), name work by short code
and Kairos words, ground a plan in the ADRs and documents of the repository and the initiative
(COLLIERY-I-0627; the routes are `get_repository` and `team_pages`, KAIROS-T-0599), put a decision about the system in Kairos and not in local memory
(COLLIERY-I-0628), and mark what was not checked as "not checked". The short codes in the
rules are links too.

- The marker is `~/.claude/kairos-rules/<session_id>-<RULES_VERSION>` (`KAIROS_RULES_STATE_DIR`
  overrides the folder). A marker means silence. Bump `RULES_VERSION` when the text changes:
  each session then gets the new text once. Markers older than 30 days are removed.
- It reads `deployment_url` from the wiring with `session_start.wiring` (#270).
  No credentials, no network.
- Tests: `hooks/test_kairos_rules.py`, part of `angreal test unit`.

## The git guard asks before a push, a merge, a rebase, a tag and a rewrite

`hooks/git_guard.py` is a `PreToolUse` hook on Bash (KAIROS-T-0497, COLLIERY-I-0631). It
asks every time; it does not look for a "go" in the conversation. It splits the command line
into its simple commands (`&&`, `;`, `|`, a subshell, `$(...)`), looks inside `sh -c`,
`bash -c`, `zsh -c` and `eval` (KAIROS-T-0548), and asks for: `git push` (but the push of
a feature branch, below),
`git pull` (any form: a merge or a rebase), `git merge`, `git rebase`, `git tag <name>` (a
list is silent), `git reset --hard`, `git commit --amend`, `git clean -f`, `git checkout --
<path>`, `git restore` (not `--staged`), `git branch -D` of a remote-tracking branch, any `git`
command with `--force` or `--force-with-lease`, `gh pr merge`, `gh release
create|edit|delete` and `gh api` with a path that ends in `/merge`. The answer is
`permissionDecision: "ask"` with a one-line reason that names the command; anything else gives
no output. `git status`, `git fetch`, `git clean -n`, a `grep` for "push" and an `echo` of the
words are silent. The hook reads no file and holds no credentials.

The push policy (KAIROS-T-0542): an agent pushes the branch of its task and opens a draft PR;
a merge, a release and a tag need the user's go. So `git [-C <path>] push [-u|--set-upstream]
origin <branch>` is silent when `<branch>` is one local branch, not a tag, and not the default
branch (`origin/HEAD`; `main` and `master` always count). A force flag, a `+` or `:` refspec,
`HEAD`, `--delete`, `--tags`, `--all`, any other option, a `git -c`, a `VAR=value` before
`git`, a `remote.origin.push` refspec, or a `cd` in the line asks. For that check the hook
runs read-only git commands (`symbolic-ref`, `show-ref`, `config --get-all`) in the folder of
the command; when they fail, it asks.

- Tests: `hooks/test_git_guard.py`, part of `angreal test unit`.

## A write to local memory gets a reminder

`hooks/memory_reminder.py` is a `PreToolUse` hook on `Write`, `Edit` and `MultiEdit`
(KAIROS-T-0502, COLLIERY-I-0628). When the file is in a memory folder
(`.claude/projects/<name>/memory/`, or `CLAUDE_MEMORY_DIR`), it reminds: a fact about the
system under work belongs in Kairos, in an ADR (`create_item` with `item_type: adr` on the
`adr_board` of the wiring), the spec document or a note on the initiative; local memory holds
the preferences and the tooling of the person. It reminds and never refuses: the output is
`additionalContext` only, with no `permissionDecision`, so the normal permission flow of the
tool decides. It does not read the text of the write.

- Once per session per file: `~/.claude/kairos-memory/<session_id>.json` holds the paths
  (`KAIROS_MEMORY_STATE_DIR` overrides the folder; records older than 30 days are removed).
- Tests: `hooks/test_memory_reminder.py`, part of `angreal test unit`.

## Before an edit, the agent sees the ADRs that the file cites

`hooks/adr_context.py` is a `PreToolUse` hook on `Write`, `Edit` and `MultiEdit`
(KAIROS-T-0702, COLLIERY-I-0659). When the file cites ADRs by short code (for example
`KAIROS-A-0031`), it gives the agent one block before the edit: the code, the column and the
title of each cited ADR, the successor of a superseded ADR, the amendments, the revisit
condition and date (`revisit_when`, `revisit_on`, KAIROS-A-0070), and a line for a code that
no ADR has. The states come from `kairos adrs check <file> --json`, with the login
or the key of the person.

- It runs in a wired checkout only: the nearest folder with `.git` above the file has the
  wiring, or the session folder above it has.
- Once per session per file: `~/.claude/kairos-adr/<session_id>.json` holds the files that got
  the block and the states of the ADRs that the session read, so the hook reads an ADR once
  per session (`KAIROS_ADR_STATE_DIR` overrides the folder; records older than 30 days are
  removed).
- It fails open. A file that cites nothing, a new file, no `kairos` on PATH, no deployment, a
  skipped check, a timeout (8 seconds) or an error gives no output. The output is
  `additionalContext` only, with no `permissionDecision`: the hook never blocks an edit.
- Tests: `hooks/test_adr_context.py`, part of `angreal test unit`.

## Sessions are scoped to a team board and a repository

`/kairos:bootstrap` records the checkout's repository (`repository:` in
`.claude/kairos.local.md`, matched from `git remote get-url origin` against the tenant's
directory), the delivery board of the team of the principal that connects (`team_board:`), and the
ADR board of that team, or of the organization when the team has none (`adr_board:`).
The team decides the board of a task; the repository is an optional link that says where the
code is (COLLIERY-A-0023). Every repository has exactly one owning team, and a task on any
team's board may link to it. `implement` works the team board filtered by this repository and
finds the work of other teams here with `search`, `decompose` names a board for every task it
creates, `triage` grooms the tasks that link to this repository by default, and `kairos:code-review`
flags a change whose task links to a different repository. Work for a different team is a
**request** to the delivery board of that team: entry column, support lane (recipe in
`skills/workflow/implement/CROSS-TEAM-FILING.md`, which the `/kairos` router points at).

## The plugin has its own `kairos` server

`.claude-plugin/plugin.json` declares a `kairos` stdio server, `bin/kairos-mcp`. It runs
`kairos mcp-proxy` (#270), which forwards MCP over stdio to `/mcp` of the deployment of the
project. The deployment is the first of: `KAIROS_URL`, the `[[project]]` of the origin remote
of the session folder in `~/.config/kairos/config.toml`, `.claude/kairos.local.md`, `[default]`
of that file, and the deployment of `kairos login`. So two repositories can use two
deployments in one Claude Code, with no `.mcp.json` and no direnv. The credential is
`KAIROS_KEY` or `KAIROS_MCP_KEY` (the agent-key path, KAIROS-A-0024), else the token of
`kairos login`, which the proxy refreshes. An OAuth deployment therefore works through the plugin
server after `kairos login`; a user-scope entry (`claude mcp add --scope user`) still works,
but then disable one of the two servers so the session does not have the tools twice.
When the deployment is unavailable (502, 503 or 504, a timeout, or a reset, for example
during an upgrade), the proxy gives a plain error with `retryable: true` in its data. The
proxy sends a read again two times. It also sends a write with a `request_key` again two times:
the server gives the first answer to the same key and writes nothing new, so the data says
`"retry": "replay"`. It sends an idempotent write (a set, a patch or a replace, such as
`set_state` or `set_metadata`, as `kairos_client::mcp_tools` marks it) again two times: a
second call gives the same state, so the data says `"retry": "idempotent"`. A retry of a
`set_state` with `expected_updated_at` that the first send applied is refused with
`CONFLICT` and the current state. It does not send any other write with no key again: read
the item to see if the write was applied before you retry it. `KAIROS_TIMEOUT` sets the request timeout in
seconds (default 60).
With an older `kairos`, or none on PATH, `bin/kairos-mcp` is an MCP server with no tools whose
instructions say why, as `bin/kairos-code` does. Before #270 the server was HTTP at
`${KAIROS_URL}/mcp`, and `/mcp` showed it as failed when `KAIROS_URL` was not set.
The hooks match the tools of both names (`mcp__kairos__*` and `mcp__plugin_kairos_kairos__*`).

### The admin tools

The `kairos` server of the plugin lists the work profile of the MCP tools. The administration
tools (teams, boards, columns and transitions, grants, metadata, dispatch rule writes, team page
writes, org read tokens, purge) are in the admin profile, `/mcp?profile=admin`. Claude Code calls
only the tools that a server lists. To get the admin tools, add a second proxy with the admin
profile, one time for each user, then start a new session:

```sh
claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin
```

It uses the same deployment and credential as the plugin server: `kairos login`, or an agent key
in `KAIROS_KEY` or `KAIROS_MCP_KEY`. Its tools are `mcp__kairos-admin__*`. `--profile all` lists
each tool. The plugin does not ship this server, so a session that does no administration does
not get the text of the admin tools. To remove it: `claude mcp remove --scope user kairos-admin`.
Without the server, the CLI does the same work with the same login (`kairos teams create`,
`kairos boards`, `kairos teams members`, `kairos teams pages`). The skill `onboard-team`
(user-invoked: only the user can start it, with `/kairos:onboard-team`) uses these tools.

## The connection belongs to the user

`/kairos:bootstrap` writes no MCP config in the repository by default. The connection is the
plugin server (an agent key or `kairos login`) or a user-scope entry (OAuth), and each
repository of the user uses it. By default the skill writes the wiring as a `[[project]]` of
`~/.config/kairos/config.toml` (`kairos config project`), so the repository gets no file; it
writes `.claude/kairos.local.md` only when the user asks for it or the CLI cannot. A second
deployment is a `[[project]]` with its own `deployment`, not a project `.mcp.json`.

## `references/` are rendered artifacts

Three files, each rendered from the Kairos document that is its source of truth. They were
Metis specifications until 2026-09-26, and citations of the old code still mean the same text:

| Rendered | Source | Was |
|---|---|---|
| `references/architecture-review.md` | COLLIERY-D-0007 | KAIROS-S-0007 |
| `references/diataxis.md` | COLLIERY-D-0008 | KAIROS-S-0008 |
| `references/simplified-technical-english.md` | COLLIERY-D-0009 | KAIROS-S-0009 |

Never edit the rendered files directly. The script reads the documents over the REST API, so it
needs the deployment reachable and `KAIROS_KEY` (or `KAIROS_MCP_KEY`) set. To re-render after a
spec change, run:

```
scripts/render-references.sh
```
