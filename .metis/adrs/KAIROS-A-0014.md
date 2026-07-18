---
id: 001-skills-plugin-architecture
level: adr
title: "Skills Plugin Architecture"
number: 1
short_code: "KAIROS-A-0014"
created_at: 2026-07-08T11:28:55.685022+00:00
updated_at: 2026-07-08T15:00:46.765060+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Skills Plugin Architecture

## Context

The skills plugin is Kairos's day-1 agentic workflow layer (KAIROS-I-0002): a Claude Code plugin whose skills operate on Kairos boards via the remote MCP endpoint (A-0011). Its internal architecture — layout, invocation model, naming, hooks, configuration — must be fixed so skills can be authored in parallel by agents without convention drift. The design philosophy is inherited from mattpocock/skills (`writing-great-skills`): small composable skills, user-invoked vs model-invoked split, router over the user-invoked set, progressive disclosure, leading words, aggressive pruning.

## Decision

**One plugin (`kairos`) in the monorepo: plain-named skills split user/model-invoked per upstream, a `/kairos` router, a SessionStart hook for board context, and per-repo configuration written by `/bootstrap`.**

### Layout (in the kairos repo, per KAIROS-A-0008)
```
.claude-plugin/plugin.json        # name: "kairos"; enumerates skills
.claude-plugin/marketplace.json   # installable via `claude plugin marketplace add <repo>`
plugin/
├── skills/
│   ├── workflow/    # to-initiative, decompose, triage, implement, handoff
│   ├── engineering/ # tdd, diagnosing-bugs, prototype, research, code-review,
│   │                # domain-modeling, codebase-design, grill-with-docs
│   ├── review/      # architecture-review, diataxis-review
│   ├── meta/        # kairos (router), bootstrap, grill-me, grilling, writing-great-skills
│   └── <skill>/SKILL.md (+ sibling reference files, progressive disclosure)
├── references/      # architecture-review.md, diataxis.md (rendered from S-0007/S-0008)
├── hooks/           # SessionStart context injection
└── .mcp.json        # template: Kairos MCP URL placeholder, filled by /bootstrap
```

### Invocation model
- Upstream's split is the default: orchestrating skills are user-invoked (`disable-model-invocation: true`): `/kairos` (router), `/bootstrap`, `/grill-me`, `/to-initiative`, `/decompose`, `/triage`, `/architecture-review`, `/diataxis-review`, `/writing-great-skills`
- Disciplines the model must reach autonomously stay model-invoked: `grilling`, `tdd`, `diagnosing-bugs`, `prototype`, `research`, `domain-modeling`, `codebase-design`, `code-review`, `implement`
- Plain discipline names (decided 2026-07-08); the plugin namespace (`kairos:decompose`) provides scoping — no `kairos-` prefixes baked into skill names
- `/kairos` is the router: maps every user-invoked skill to its moment, kept in sync as skills change (a router that lies is a bug)

### Kairos access
Skills reach Kairos **only** through the MCP tools (S-0006). A SKILL.md never embeds HTTP calls or assumes REST shapes. Skills reference tools by name and rely on `whoami`/`my_boards` for context.

### Hooks
- **SessionStart**: queries the configured Kairos deployment for the engineer's context — team, delivery board, active/todo items, standing bucket initiatives — and injects a compact summary. Degrades gracefully (offline → note, not failure)
- No PreToolUse gates in v1; the server's ABAC is the enforcement layer

### Per-repo configuration
`/bootstrap` writes `.claude/kairos.local.md` (YAML frontmatter + notes): deployment URL, tenant slug, delivery stream, team board, default initiative board. Hooks and skills read it; it is gitignored by default (contains org-specific wiring, not secrets — tokens live with the MCP client)

### Skill-authoring standard
`writing-great-skills` (ported) is normative for every skill in the plugin: description discipline, information hierarchy, leading words, no-op pruning. The plugin-validator/skill-review pass is part of the definition of done for skill tasks (A-0012's gate applies).

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| One plugin, split-invocation + router (chosen) | Proven upstream pattern; minimal always-loaded context; consumers get one install | Router needs maintenance discipline | Low | Medium |
| Everything model-invoked | Agent can reach any skill autonomously | Description context load for ~20 skills every turn; trigger collisions | Medium | Low |
| Multiple plugins by concern (workflow/review/etc.) | Selective install | Cross-skill reach breaks at plugin boundaries; consumers juggle versions | Medium | Medium |
| Skills embedded in Kairos server (served remotely) | Version-locked to deployment | No such distribution channel in Claude Code today; blocks other agent hosts | High | High |

## Rationale

1. **The upstream philosophy is the product thesis** — small, composable, human-controlled skills beat process-owning frameworks; this ADR keeps the port faithful where it worked and rebinds only the work-system layer.
2. **MCP-only access makes the plugin host-portable** and keeps authorization entirely server-side — a skill cannot do what the user's token cannot.
3. **`.local.md` per-repo config** follows the established Claude Code plugin-settings pattern and keeps the plugin stateless across repos.
4. **SessionStart injection mirrors what makes the metis plugin effective** (ambient work context) — rebuilt against Kairos boards instead of a local database.

## Consequences

### Positive
- Skill authoring parallelizes safely: layout, invocation, naming, and access rules are all fixed
- Consumers configure exactly one thing (deployment URL) to activate the whole layer
- Plugin versions with the repo/release train — skills and MCP tool surface move together (A-0008)

### Negative
- Router and README must be re-synced whenever the skill set changes (make it part of the skill-task definition of done)
- Claude Code is the primary supported host in v1; other MCP hosts get the tools but not the skills packaging

### Neutral
- Bucket layout (`workflow/engineering/review/meta`) is organizational only — invocation behavior is set per skill
- The two shipped references render from Metis specs S-0007/S-0008; the Metis documents remain the source of truth

## Review Schedule

### Review Triggers
- Claude Code plugin/skill packaging changes materially
- A second agent host with its own skill format gains real adoption among Kairos consumers