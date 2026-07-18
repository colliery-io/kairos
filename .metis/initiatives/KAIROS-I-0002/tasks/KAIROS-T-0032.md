---
id: m3-engineering-skills-code-review
level: task
title: "M3: Engineering skills - code-review"
short_code: "KAIROS-T-0032"
created_at: 2026-07-10T09:09:17.072446+00:00
updated_at: 2026-07-10T09:42:01.039840+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Engineering skills - code-review

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

Port code-review: the two-axis review (Standards + Spec) with the spec axis reading the originating Kairos task/initiative.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Standards axis: repo conventions + smell baseline per upstream, run as a sub-agent
- [x] Spec axis: fetches the originating item via get_item (argument or inferred), reviews the diff against its acceptance criteria, run as a parallel sub-agent; findings posted back to the Kairos task via edit_item as a review record
- [x] Model-invoked per upstream; registered + router; skill-reviewer run — AMENDED per orchestrator lane rules: this lane records the exact plugin.json/router entries in Status Updates instead of editing `.claude-plugin/plugin.json` or the router (orchestrator merges)

## Implementation Notes

Upstream: skills/engineering/code-review. The A-0012 gate makes this skill the review half of the consumer loop.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Ported. Created `plugin/skills/engineering/code-review/SKILL.md` (single file, no siblings — smell baseline stays inline because every run reaches it). Faithful to upstream's key mechanic: Standards and Spec run as parallel `general-purpose` sub-agents in one message; findings never merged/reranked across axes. Standards axis carries repo-convention discovery (with a checkable done-condition) plus the full 12-smell Fowler baseline and its two binding rules pasted into the sub-agent prompt. Spec axis rebound to Kairos: originating item from argument short code → short codes in commits/branch → session active items (`my_boards` → `board_items`, confirm if ambiguous) → ask; fetched via `get_item`, diff reviewed against its acceptance criteria (content pasted — sub-agents have no MCP access); step 6 posts a structured review record back via `edit_item`, anchored on `## Status Updates` with final-line fallback and a re-fetch retry on anchor miss. Model-invoked (no `disable-model-invocation`). S-0006 tools only: `get_item`, `edit_item`, `my_boards`, `board_items` — all in the spec's tool inventory.
- 2026-07-10: Verification evidence. (1) skill-reviewer (plugin-dev) foreground pass: "Needs Improvement — close to Pass; all fixes are local edits"; 0 blockers, 5 should-fix, 3 nits. All 5 should-fixes + 2 nits applied: description pruned to triggers (53 words), tool names aligned to `my_boards`→`board_items`, no-item skip single-sourced with a local guard at step 6, step-3 completion criterion made checkable, "Why two axes" section collapsed into step 5, unused `search` tool dropped, baseline paste boundary disambiguated. Nit 8 (trigger phrasings) kept as deliberate invocation-recall aid. (2) Frontmatter validated by script: parses, `name: code-review`, model-invoked, all structural markers present (both axes, parallel sub-agents, get_item/edit_item, smells, Status Updates anchor). (3) Lane discipline: only `plugin/skills/engineering/code-review/` touched; no plugin.json/router edits; no git commands. Full behavioral verification against a live Kairos deferred to KAIROS-T-0035 per gate.
- 2026-07-10: ORCHESTRATOR MERGE SNIPPETS (exact). plugin.json — add to the `skills` array (alphabetical within engineering): `"./plugin/skills/engineering/code-review"`. Router (`plugin/skills/meta/kairos/SKILL.md`) — NO entry required: code-review is model-invoked and the router maps only the user-invoked surface (A-0014); the router sync rule is honored by omission. If the orchestrator maintains a README skill table, list code-review under engineering as model-invoked.