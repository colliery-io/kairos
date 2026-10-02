# kairos plugin

This directory holds the components of the `kairos` Claude Code plugin (manifest lives at the
repo root in `.claude-plugin/plugin.json`; layout per KAIROS-A-0014).

```
plugin/
├── skills/
│   ├── workflow/     # grill-initiative, to-initiative, decompose, grill-decomposition, triage, implement, ralph, ralph-initiative, cancel-ralph
│   ├── engineering/  # tdd, diagnosing-bugs, prototype, research, code-review, ...
│   ├── review/       # architecture-review, diataxis-review
│   └── meta/         # kairos (router), kairos-vocabulary, grill-me, grilling, handoff, writing-great-skills, bootstrap
├── references/       # rendered review specs (see below)
├── hooks/            # SessionStart context injection, the Ralph loop (Stop hook), the active work after a compaction
└── .mcp.json         # MCP endpoint template (see below)
```

Shipped (KAIROS-T-0027..T-0034): all four buckets plus the SessionStart hook.

- `meta/` — `kairos` (the `/kairos` router), `grilling` and `kairos-vocabulary` (model-invoked;
  `kairos-vocabulary` names work only by short codes and Kairos words, with `ITEM-TYPES.md`
  from outside words to item types), `grill-me`,
  `handoff`, `writing-great-skills` (the normative authoring reference), `bootstrap`
  (wires a repo to a deployment — detects the repository from the git remote, takes the
  team board from the team of the principal — and writes `.claude/kairos.local.md`).
  It reads `/api/config` to choose the authentication path: OAuth, or a service account
  with an API key on a deployment with no issuer.
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
It reads `.claude/kairos.local.md` (absent → silent no-op), probes `<deployment_url>/healthz`
unauthenticated, and injects the wiring summary as `additionalContext` with an instruction to
pull live state over the **authenticated MCP connection**: with a `repository` wired
(KAIROS-A-0019) that is `get_repository` + `board_items` on the team board narrowed to the
repository — the session's queue; without one, `my_boards`/`board_items` as before. The hint
also says to name a board at create and how to send a request (COLLIERY-A-0023). Its pure parts are unit
tested (`hooks/test_session_start.py`, part of `angreal test unit`).
Hooks cannot drive the client's OAuth flow (KAIROS-A-0011) and tokens live with the MCP client
(KAIROS-A-0014), so the hook deliberately never authenticates; offline degrades to a note.
That holds on the service-account path too: the MCP client sends the API key, and the hook
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
- `hooks/ralph.py stop` is the `Stop` hook. With no state for its session it does nothing.
  Otherwise it blocks the stop and gives the next instruction, until the last reply ends with
  `<promise>ALL TASKS COMPLETE</promise>` or `<promise>LOOP STOPPED</promise>` (the agent stopped
  the loop and wrote the reason on the task and its initiative), or until the iteration limit.
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
  `PostToolUse` hook on the kairos `get_item`, `transition_item`, `edit_item` and
  `update_item` tools. It keeps a local record for each session in
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

## Sessions are scoped to a team board and a repository

`/kairos:bootstrap` records the checkout's repository (`repository:` in
`.claude/kairos.local.md`, matched from `git remote get-url origin` against the tenant's
directory) and the delivery board of the team of the principal that connects (`team_board:`).
The team decides the board of a task; the repository is an optional link that says where the
code is (COLLIERY-A-0023). Every repository has exactly one owning team, and a task on any
team's board may link to it. `implement` works the team board filtered by this repository and
finds the work of other teams here with `search`, `decompose` names a board for every task it
creates, `triage` grooms the tasks that link to this repository by default, and `code-review`
flags a change whose task links to a different repository. Work for a different team is a
**request** to the delivery board of that team: entry column, support lane (recipe in
`skills/workflow/implement/CROSS-TEAM-FILING.md`, which the `/kairos` router points at).

## `.mcp.json` is a template

JSON cannot carry comments, so this note lives here: `plugin/.mcp.json` ships with a
`{{KAIROS_DEPLOYMENT_URL}}` placeholder instead of a real deployment URL. The `/bootstrap`
skill fills it per-repo (KAIROS-A-0014): it asks for the Kairos deployment URL and writes the
concrete MCP endpoint (`<deployment-url>/mcp`) into the consuming repo's configuration, along
with `.claude/kairos.local.md`. On a deployment with no issuer it also writes the header
`"Authorization": "Bearer ${KAIROS_MCP_KEY}"`. The header holds the name of an environment
variable, never the API key (`skills/meta/bootstrap/SERVICE-ACCOUNT.md`). Do not replace the placeholder in this file with a real URL —
the template must stay deployment-agnostic.

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
