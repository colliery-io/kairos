---
id: agentic-development-skills-plugin
level: initiative
title: "Agentic Development Skills Plugin (Beast Mode)"
short_code: "KAIROS-I-0002"
created_at: 2026-07-08T11:11:53.218058+00:00
updated_at: 2026-07-13T12:17:09.449841+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: agentic-development-skills-plugin
---

# Agentic Development Skills Plugin (Beast Mode) Initiative

## Context

Kairos is one combined program with two legs that ship from this monorepo (see KAIROS-A-0008):

1. **The service** — the company-facing multi-tenant Flight Levels platform (KAIROS-V-0001, designed under KAIROS-I-0001).
2. **The skills plugin** (this initiative) — the agentic workflow layer OF the Kairos product. Kairos ships day 1 with this skill set; it is a core part of what makes Kairos valuable to the companies that adopt it. The plugin is for Kairos and Kairos alone.

The plugin is a port of [mattpocock/skills](https://github.com/mattpocock/skills) rebound to Kairos, plus new Kairos-native skills. Matt's skills are small, composable engineering disciplines (grilling interviews, TDD, tracer-bullet decomposition, triage, two-axis code review, domain modeling, Ousterhout-style architecture improvement) that assume a generic *issue tracker* (GitHub/GitLab/local markdown). The port's core move: **Kairos IS the issue tracker.** Every place Matt's skills read/write issues, labels, or PRDs, ours read/write Kairos workflow items, phases, boards, and `blocked_by` edges — native concepts in the three-level model, not label conventions. The three levels give the skills something no generic tracker port has: decomposition lands on delivery boards linked to initiatives, triage runs a real backlog with Task/Bug/Tech-Debt types, and blockers escalate along the level structure.

Design philosophy carried over from the source repo (see its `writing-great-skills`): user-invoked vs model-invoked split, router skill over the user-invoked set, per-repo setup skill, progressive disclosure, leading words, aggressive pruning.

Metis is untouched by this work: it remains Dylan's proven local tool, and this repo uses it internally to plan the program. The shipped plugin does not target Metis at all — skills speak to a Kairos deployment through the Kairos MCP server (API-backed, per KAIROS-V-0001).

## Goals & Non-Goals

**Goals:**
- Port the promoted mattpocock/skills engineering + productivity skills, rebinding all issue-tracker touchpoints to Kairos (workflow items, phases, boards, backlog types, `blocked_by`, level escalation)
- New review skills: **architecture-review** (driven by a blended Ousterhout + C4/ADR spec) and **diataxis-review** (documentation review against the Diataxis framework)
- Author and ship the two specs as distributable reference docs inside the plugin: **Architecture Review Spec** and **Diataxis Documentation Spec** (drafted first as specifications in this repo's planning instance)
- **Fast bootstrapping**: one skill that wires a repo/engineer into their company's Kairos deployment — auth (OIDC), tenant/delivery-stream/team selection, angreal init (Kairos-flavored template) for task orchestration, CONTEXT.md seed, plugin defaults
- **Plugin management**: `.claude-plugin/plugin.json` + marketplace manifest so consumers install the plugin alongside their Kairos deployment; a dev-loop/link script for local iteration
- A router skill (the `ask-matt` pattern) mapping the whole user-invoked surface
- Day-1 shipping: the plugin is part of the Kairos v1 release — a Kairos deployment without the skills is an incomplete product

**Non-Goals:**
- Kairos service implementation (KAIROS-I-0001 and follow-on initiatives)
- Porting Matt's `personal/`, `misc/`, `deprecated/`, and `in-progress/` buckets (except where a draft like `wayfinder` informs design)
- Replacing the existing metis plugin's core MCP server/hooks — this plugin layers workflow skills on top of it (overlap resolved during design)
- GUI/web surfaces

## Architecture

### Repo layout (proposed, to confirm in design)

```
kairos/
├── .claude-plugin/plugin.json      # the beast-mode plugin manifest
├── .claude-plugin/marketplace.json # marketplace so `claude plugin marketplace add <repo>` works
├── skills/                         # SKILL.md folders (engineering/, review/, bootstrap/)
├── references/                     # distributable specs: architecture-review.md, diataxis.md
├── scripts/                        # link-skills / dev-loop scripts
├── crates/ (or src/)               # Kairos service (Rust/Axum) — KAIROS-I-0001 follow-on
└── .metis/, .angreal/              # internal planning + dev-task orchestration for building Kairos
```

### Kairos integration

Skills speak to a Kairos deployment through the **Kairos MCP server** — the API-backed MCP client named in the vision. That makes the Kairos MCP tool surface a hard dependency of this initiative: its design (which tools exist, how tenant/team/board context flows, how level-aware operations like decompose-to-delivery-board work) is service-leg work growing out of KAIROS-I-0001's API design. Skill authoring can proceed against the designed tool surface; skill verification needs a running Kairos (local Docker Compose — the `angreal services`/`db` tasks in this repo already scaffold that harness).

This repo's own `.metis/` is internal planning tooling for building Kairos and is not a skill target.

## Detailed Design

### Port map (mattpocock/skills → Kairos)

| Source skill | Kairos skill | Adaptation |
|---|---|---|
| `grilling` (model) | `grilling` | As-is: the core interview loop, reused by other skills |
| `grill-me` (user) | `grill-me` | As-is |
| `grill-with-docs` | `grill-with-docs` | ADR side-effects become Kairos ADRs (attachable at any level — strategy, initiative, task); `CONTEXT.md` stays a repo file |
| `to-prd` | `to-initiative` | Synthesize conversation into a Kairos initiative on a coordination board (discovery phase), with the PRD attached as a supporting document (template-driven, per the vision's document model) |
| `to-issues` | `decompose` | Tracer-bullet vertical slices → tasks on delivery boards, linked to the parent initiative with native `blocked_by` edges; expand–contract for wide refactors |
| `triage` | `triage` | Label state machine → the delivery board's real backlog: Task/Bug/Tech-Debt item types + phase transitions (backlog→todo); cross-team blockers escalate to the initiative board |
| `implement` | `implement` | Picks up a todo task from the team's delivery board, transitions todo→active→completed, records findings in the task; repo is metadata on the task, per the vision |
| `code-review` | `code-review` | Two axes: Standards (repo conventions + smell baseline) and Spec (faithful to the originating Kairos task/initiative), parallel sub-agents |
| `tdd` | `tdd` | Largely as-is; progress notes land in the active Kairos task |
| `diagnosing-bugs` | `diagnosing-bugs` | As-is; diagnosis log lands in the Kairos task/bug |
| `prototype` | `prototype` | As-is |
| `research` | `research` | Cited findings attached as a supporting document on the initiating Kairos item |
| `domain-modeling` | `domain-modeling` | As-is; ADR side-effects go to Kairos ADRs at the appropriate level |
| `codebase-design` | `codebase-design` | As-is: supplies the module/interface/depth/seam vocabulary the architecture spec builds on |
| `improve-codebase-architecture` | `architecture-review` | Rebuilt around the Architecture Review Spec: Ousterhout deepening scan + C4-level system views + ADR-conflict checks against Kairos ADRs; keeps the HTML report + grilling loop |
| `handoff` | `handoff` (thin) | Mostly superseded by Kairos-as-shared-memory (any teammate or agent reads the same boards); retained as a slim cross-agent compactor |
| `ask-matt` | router (name TBD) | Maps the full user-invoked surface for a Kairos-connected engineer |
| `setup-matt-pocock-skills` | `bootstrap` | Reworked entirely: wire repo/engineer to the company Kairos deployment (OIDC auth, tenant, delivery stream, team board), angreal init (template), CONTEXT.md seed, hook/permission defaults |
| `writing-great-skills` | `writing-great-skills` | Ported as the skill-authoring reference so consumers can extend the set |

### New skills (no upstream source)

- **`diataxis-review`** — classify every page in a docs tree into Diataxis modes (tutorial / how-to / reference / explanation), flag mode-mixing and gaps, propose a restructure; findings optionally filed as Tech-Debt items on the team's delivery board. Driven by the shipped Diataxis spec.
- **`architecture-review`** — see port map; driven by the shipped Architecture Review Spec.
- **`bootstrap`** — see port map.

### Distributable specs (authored as Metis specifications first)

- **Architecture Review Spec** — blended: Ousterhout deep-modules discipline (depth, seams, deletion test, interface-is-the-test-surface) for code-level review; C4-style context/container/component views and ADR discipline for system-level review; explicit review checklists per level.
- **Diataxis Documentation Spec** — condensed statement of the Diataxis framework (four modes, the acquisition/application × action/cognition compass, per-mode quality criteria, anti-patterns) suitable for automated review.

Both render into `references/` in the plugin and are cited by their skills via context pointers.

## Key Design Questions

1. **Kairos MCP tool surface** — the tool set skills program against: which tools exist, how tenant/team/board context flows through a session, how level-aware operations (decompose-to-delivery-board, escalate-blocker) are expressed. Designed with/after KAIROS-I-0001's API design; this initiative's binding dependency.
2. **Level & context awareness** — how a skill session knows *whose* boards it's operating on (engineer's team, delivery stream, active initiative); what the SessionStart hook injects (e.g., your team's active/ready tasks, standing bucket initiatives).
3. **Plugin layout & distribution** — manifest at repo root vs `plugin/` subtree; marketplace install vs bundled with a Kairos deployment; versioning the plugin against the service API.
4. **Naming** — plugin name, skill prefixes, router name (Kairos-flavored vocabulary or plain).
5. **Bootstrap surface** — OIDC/auth flow from a CLI skill; which angreal template(s) ship; how much of `.claude/settings` (permissions, hooks) bootstrap is allowed to write.
6. **Spec depth** — how prescriptive the architecture spec's checklists are before they become noise; what the diataxis-review skill does with pre-existing non-Diataxis doc trees.
7. **User-invoked vs model-invoked split** per ported skill (upstream's split is the starting point, not gospel).
8. **Dev harness** — exercising skills against a compose-run Kairos during development, before any production deployment exists.

## Alternatives Considered

- **Bind the skills to local Metis instead of Kairos** — rejected (2026-07-08, Dylan): Metis is a personal tool that already works and stays untouched. The plugin is part of the Kairos *product* — it must ship with Kairos day 1 to make Kairos most valuable to consumers. A Metis-bound plugin serves an audience of one; a Kairos-bound plugin is the product's agentic workflow layer.
- **Standalone plugin repo** — rejected (2026-07-08): monorepo chosen so the plugin ships as part of the product it belongs to (KAIROS-A-0008).
- **Extend the existing metis plugin instead** — rejected: same grounds as the first alternative; the metis plugin serves the personal/local workflow.
- **Adopt mattpocock/skills wholesale (skills.sh install)** — rejected: they bind to generic issue trackers and lack the three-level model Kairos makes native (boards, item types, blocked_by, escalation); the value is the port, not the mirror.
- **Building process-owning frameworks (GSD/BMAD/Spec-Kit style)** — rejected on the same grounds the source repo rejects them: small composable skills keep the human in control.

## Implementation Plan

Proposed phases (pending decomposition approval — tasks created at decompose):

- **Phase A — Foundations**: `git init` this repo; plugin + marketplace manifests; skeleton `skills/`/`references/` layout; author the two specs (drafted as specifications here → rendered references); extend KAIROS-I-0001's API design with the Kairos MCP tool surface the skills program against.
- **Phase B — Core ports**: grilling family, `to-initiative`, `decompose`, `triage`, `implement`, `code-review`, `tdd`, `diagnosing-bugs`, `domain-modeling`, `codebase-design`, `prototype`, `research`, `writing-great-skills` — authored against the designed tool surface; portions that don't touch Kairos (grilling, tdd, prototype, codebase-design) are verifiable immediately.
- **Phase C — New skills**: `architecture-review`, `diataxis-review`, `bootstrap`.
- **Phase D — Surface & verification**: router skill, per-skill docs, dev-loop scripts, install verification, then the acceptance test: full workflow (bootstrap → to-initiative → decompose → implement → code-review) exercised against a compose-run Kairos instance.

Sequencing note: Kairos-bound skills can't be *verified* end-to-end until the service leg delivers a runnable API + MCP server, so KAIROS-I-0001's implementation follow-on is the critical path for Phase D. Skill authoring, specs, and the non-Kairos-touching skills proceed in parallel with service implementation — that's the combined program.

## Progress Log

- **2026-07-08**: Initiative created. Source repo cloned and analyzed; port map drafted; monorepo + combined-program + blended-arch-spec decisions taken with Dylan (recorded in KAIROS-A-0008). Awaiting human review of this design and go-ahead to draft the two specs and decompose.
- **2026-07-08 (later)**: Per Dylan — dropped the "Metis MCP contract seam" framing entirely. Metis is stable and stays as-is. Kairos's purpose is company-scale Flight Levels, not a Metis backend swap.
- **2026-07-08 (final framing)**: Per Dylan — **the plugin is for Kairos and Kairos alone.** It ships day 1 with the Kairos product as its agentic workflow layer; that's a core part of Kairos's consumer value. Skills bind to the Kairos MCP server / three-level board model, not to Metis. Metis is only this repo's internal planning tool. Port map, goals, design questions, alternatives, and sequencing all rewritten to match; KAIROS-A-0008 and the vision updated.
- **2026-07-08 (design-completion pass)**: Dylan directed full design + max ADRs now so implementation can be unleashed to agents. Open design questions resolved: Q1 (MCP tool surface) → **KAIROS-S-0006** authored; Q2 (context awareness) → `whoami`/`my_boards` tools + SessionStart hook + `.claude/kairos.local.md` (A-0011, A-0014); Q3 (layout/distribution) → **KAIROS-A-0014** (plugin/ subtree, marketplace install); Q4 (naming) → plain discipline names, `/kairos` router (Dylan); Q5 (bootstrap surface) → A-0014 + A-0010 (OIDC device/browser flows, angreal init, `.local.md` writes); Q7 (invocation split) → A-0014 fixes the per-skill split; Q8 (dev harness) → A-0012/A-0013 (compose + seeded demo tenant). Q6 (spec depth) → answered by the authored specs **KAIROS-S-0007** (architecture review: two altitudes, A1/A2 checklists, finding format) and **KAIROS-S-0008** (diataxis: mode contracts with citable rule IDs). Remaining before decompose: ADR ratification and Aurora Dark token pointer (A-0015).