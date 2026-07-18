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
  (wires a repo to a deployment; writes `.claude/kairos.local.md`)
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
call `my_boards`/`board_items` over the **authenticated MCP connection** for live board state.
Hooks cannot drive the client's OAuth flow (KAIROS-A-0011) and tokens live with the MCP client
(KAIROS-A-0014), so the hook deliberately never authenticates; offline degrades to a note.

## `.mcp.json` is a template

JSON cannot carry comments, so this note lives here: `plugin/.mcp.json` ships with a
`{{KAIROS_DEPLOYMENT_URL}}` placeholder instead of a real deployment URL. The `/bootstrap`
skill fills it per-repo (KAIROS-A-0014): it asks for the Kairos deployment URL and writes the
concrete MCP endpoint (`<deployment-url>/mcp`) into the consuming repo's configuration, along
with `.claude/kairos.local.md`. Do not replace the placeholder in this file with a real URL —
the template must stay deployment-agnostic.

## `references/` are rendered artifacts

`references/architecture-review.md` and `references/diataxis.md` are rendered from the Metis
specifications KAIROS-S-0007 and KAIROS-S-0008, which remain the source of truth. Never edit
the rendered files directly. To re-render after a spec change, run:

```
scripts/render-references.sh
```
