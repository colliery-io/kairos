# kairos plugin

This directory holds the components of the `kairos` Claude Code plugin (manifest lives at the
repo root in `.claude-plugin/plugin.json`; layout per KAIROS-A-0014).

```
plugin/
├── skills/
│   ├── workflow/     # to-initiative, decompose, triage, implement
│   ├── engineering/  # tdd, diagnosing-bugs, prototype, research, code-review, ...
│   ├── review/       # architecture-review, diataxis-review
│   └── meta/         # kairos (router), grill-me, grilling, handoff, writing-great-skills, bootstrap
├── references/       # rendered review specs (see below)
├── hooks/            # SessionStart context injection
└── .mcp.json         # MCP endpoint template (see below)
```

Shipped (KAIROS-T-0027..T-0034): all four buckets plus the SessionStart hook.

- `meta/` — `kairos` (the `/kairos` router), `grilling` (model-invoked), `grill-me`,
  `handoff`, `writing-great-skills` (the normative authoring reference), `bootstrap`
  (wires a repo to a deployment — detects the repository from the git remote, takes the
  team board from the team of the principal — and writes `.claude/kairos.local.md`)
- `workflow/` — `to-initiative`, `decompose`, `triage` (user-invoked), `implement`
  (model-invoked per KAIROS-A-0014)
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
with `.claude/kairos.local.md`. Do not replace the placeholder in this file with a real URL —
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
