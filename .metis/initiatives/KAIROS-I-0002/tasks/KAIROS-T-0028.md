---
id: m3-workflow-skills-to-initiative
level: task
title: "M3: Workflow skills - to-initiative and decompose"
short_code: "KAIROS-T-0028"
created_at: 2026-07-10T09:09:07.770158+00:00
updated_at: 2026-07-10T09:37:22.749047+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Workflow skills - to-initiative and decompose

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

Port to-prd → to-initiative and to-issues → decompose (workflow bucket): conversation → Kairos initiative with attached PRD document; plan → tracer-bullet tasks on delivery boards with native blocked_by edges.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] to-initiative: synthesizes the conversation into create_item(initiative) on a coordination board + create_item(document, template=prd, parent=the initiative); no interview (grilling is separate); uses S-0006 tools by name
- [x] decompose: vertical-slice/tracer-bullet discipline preserved from upstream (incl. expand–contract for wide refactors); creates tasks via create_item with parent linkage, wires blocks edges via link_items, quizzes the user on granularity/dependencies before publishing; publishes in dependency order
- [x] Both user-invoked; registered in plugin.json + router updated (amended per orchestrator lane: registration/router recorded as exact snippets in Status Updates for merge-time application — shared files); skill-reviewer findings addressed/recorded

## Implementation Notes

Upstream sources: skills/engineering/to-prd and to-issues. The Kairos rebind: issues→tasks on the team's delivery board (whoami/my_boards for context), triage labels→none (phases are native), sub-issue linking→parent edges.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: **Completed.** Authored `plugin/skills/workflow/to-initiative/SKILL.md` and `plugin/skills/workflow/decompose/SKILL.md` per KAIROS-A-0014 (user-invoked, `disable-model-invocation: true`, exemplar voice) and KAIROS-S-0006 (tools by name only: `my_boards`, `create_item`, `get_item`, `link_items`, `search`).
- 2026-07-10: **Lane amendment (orchestrator-directed):** five concurrent agents share `.claude-plugin/plugin.json` and the `/kairos` router, so the "registered in plugin.json + router updated" criterion is satisfied by recording the exact snippets here; the orchestrator applies them at merge. AC3 amended accordingly.
- 2026-07-10: **Snippet — plugin.json** (add to the `skills` array):
  ```
      "./plugin/skills/workflow/decompose",
      "./plugin/skills/workflow/to-initiative",
  ```
- 2026-07-10: **Snippet — router** (`plugin/skills/meta/kairos/SKILL.md`, add to the bullet list):
  ```
  - `/kairos:to-initiative` — when a discussed plan is ready to become work: synthesize the conversation into a Kairos initiative with an attached PRD (no interview — grill first if it needs one).
  - `/kairos:decompose` — when an initiative or plan needs breaking into tasks: tracer-bullet vertical slices on the delivery board, quizzed with the user, blocking edges wired, published in dependency order.
  ```
  README sync note for merge: `plugin/README.md`'s "Shipped so far" paragraph should mention the `workflow/` bucket once merged (A-0014 sync rule).
- 2026-07-10: **AC evidence.** (1) to-initiative: no-interview synthesis (routes interrogation to `/kairos:grill-me` before, `/kairos:decompose` after); publishes `create_item(item_type: initiative, board: <coordination board via my_boards>)` + `create_item(item_type: document, template: prd, parent: <initiative>)`; upstream to-prd process (repo exploration, seams check, PRD template verbatim) preserved. (2) decompose: Vertical slice rules and expand–contract Wide refactors kept verbatim in discipline (issue→task rename only); step 4 quizzes user on granularity/dependencies/merge-split before publishing; step 5 publishes in dependency order via `create_item(item_type: task, parent: <initiative>)` + `link_items(relationship: blocks)`; triage labels dropped (columns/phases native), parent body-sections dropped (parent edge native). (3) Both user-invoked; registration recorded as snippets above per lane amendment.
- 2026-07-10: **skill-reviewer passes (both rated Pass).** to-initiative findings addressed: added exhaustive PRD synthesis criterion ("every decision made in the conversation lands in a template section"); wired confirmed seams into Testing Decisions; added `search` pointer for glossary/ADRs; sharpened step-1 exploration criterion; zero-board case asks user; deduped opening line vs description. decompose findings addressed: step 2 retitled "(if not already done)"; exhaustive user-story coverage criterion added (uncovered list shown in quiz); prefactoring lands as its own leading task(s); no-input branch asks user. Recorded-not-changed: router omission handled by snippet above (shared file); pointer-vs-inline prototype split with to-initiative's PRD is intentional (task bodies point, PRDs inline trimmed); Wide refactors kept as one paragraph for verbatim preservation.
- 2026-07-10: Verification: `grep` confirms zero upstream references (issue tracker/triage label/sub-issue/matt-pocock) in both skills; frontmatter valid; only S-0006 tool names used. Live behavioral verification deferred to KAIROS-T-0035 per gate.