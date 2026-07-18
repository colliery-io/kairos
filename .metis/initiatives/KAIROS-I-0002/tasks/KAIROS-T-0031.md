---
id: m3-engineering-skills-prototype
level: task
title: "M3: Engineering skills - prototype, research, domain-modeling, codebase-design, grill-with-docs"
short_code: "KAIROS-T-0031"
created_at: 2026-07-10T09:09:09.083590+00:00
updated_at: 2026-07-10T09:51:44.812283+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Engineering skills - prototype, research, domain-modeling, codebase-design, grill-with-docs

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

Port prototype (LOGIC/UI references), research, domain-modeling (ADR/CONTEXT formats), codebase-design (DEEPENING/DESIGN-IT-TWICE), and grill-with-docs into the engineering bucket.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] prototype + codebase-design ported with all reference files; codebase-design's vocabulary kept exactly (it underpins the shipped architecture-review reference)
- [x] research: cited findings saved as a Kairos document attached to the initiating item (create_item document with parent) instead of repo markdown
- [x] domain-modeling + grill-with-docs: CONTEXT.md stays a repo file; ADR side-effects become Kairos ADRs (create_item adr, linked appropriately); grill-with-docs composes grilling + domain-modeling as upstream
- [x] Invocation split per upstream; registered + router updated (amended per orchestrator lane: satisfied by the exact merge snippets recorded in Status Updates — plugin.json/router/README edits are the orchestrator's); skill-reviewer run on each

## Implementation Notes

Upstream: skills/engineering/{prototype,research,domain-modeling,codebase-design,grill-with-docs}. Five skills, one task, because rebinding is light.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Read A-0014, S-0006, meta exemplars (grilling, grill-me, kairos router, writing-great-skills), architecture-review reference, and all upstream sources (SKILL.md + LOGIC/UI, ADR-FORMAT/CONTEXT-FORMAT, DEEPENING/DESIGN-IT-TWICE) from the scratchpad clone. **Lane amendment (orchestrator directive)**: this task creates `plugin/skills/engineering/{prototype,research,domain-modeling,codebase-design,grill-with-docs}/` ONLY — `.claude-plugin/plugin.json`, the `/kairos` router, and `plugin/README.md` are NOT edited here; exact entries are recorded below as snippets for the orchestrator to merge. This amends the "registered + router updated" clause of AC-4: satisfied by recorded snippets, not by edits. Invocation split per upstream frontmatter, verified: prototype/research/domain-modeling/codebase-design model-invoked (no disable flag upstream), grill-with-docs user-invoked (`disable-model-invocation: true` upstream) — matches A-0014. Plan: prototype near-verbatim (capture-the-answer rebound to Kairos item/ADR); research output rebound to `create_item(document, parent: initiating item)` with cited findings; domain-modeling keeps CONTEXT.md/CONTEXT-MAP.md as repo files, ADR side-effects become `create_item(adr)` + `link_items`, ADR-FORMAT.md adapted to Kairos ADR fields (no dirs/numbering); codebase-design vocabulary kept EXACTLY and cross-checked against plugin/references/architecture-review.md; grill-with-docs composes grilling + domain-modeling. Then foreground skill-reviewer per skill (self-review fallback).
- 2026-07-10: All five skills authored in plugin/skills/engineering/. **prototype/** (SKILL.md near-verbatim + LOGIC.md verbatim + UI.md): only deltas are the capture-the-answer rebinds — SKILL.md "When done" and UI.md step 6 now capture to the initiating Kairos item (`edit_item`) or a Kairos ADR (`create_item` item_type: adr) instead of "ADR, issue"; diff vs upstream shows exactly those 2 hunks. **research/**: output rebound to `create_item(item_type: document, parent: <initiating item>)` with cited findings, explicit "not as repo markdown", asks for a home if no initiating item. **domain-modeling/**: CONTEXT.md/CONTEXT-MAP.md stay repo files (CONTEXT-FORMAT.md diff-identical to upstream); docs/adr/ trees removed from the structure diagrams; ADR-FORMAT.md rewritten to Kairos mechanics — create_item(adr) with title/content/decision_maker, `link_items` (informs) to the initiating item, supersedes via `link_items` (supersedes), no manual numbering; upstream's when-to-offer criteria + what-qualifies list kept verbatim. **codebase-design/**: SKILL.md, DEEPENING.md, DESIGN-IT-TWICE.md all diff-identical to upstream — vocabulary kept EXACTLY; cross-checked against plugin/references/architecture-review.md Section 2.1: all eight terms (module, interface, implementation, depth, seam, adapter, leverage, locality), the deletion test, seam discipline (one-adapter-hypothetical/two-real), depth-not-a-line-ratio, and design-it-twice are definitionally consistent — no drift. **grill-with-docs/**: user-invoked (disable-model-invocation: true), composes as upstream: "Invoke the `grilling` skill, applying the `domain-modeling` skill throughout the session." Hygiene grep for mattpocock/upstream refs across all five dirs: CLEAN. Invocation-split grep confirms only grill-with-docs carries the disable flag. All tool references are S-0006 names only (create_item, edit_item, link_items). Note: sibling task's tdd/diagnosing-bugs/code-review landed in the same bucket in parallel; untouched by this lane. Next: 5 foreground skill-reviewer passes.
- 2026-07-10: **Skill-reviewer results (5/5, foreground plugin-dev:skill-reviewer agents)**. **prototype** Needs Improvement → fixed: major (LOGIC.md step 7 omitted the Kairos capture path, contradicting SKILL.md) resolved by making LOGIC.md step 7 and UI.md step 6 defer to SKILL.md's "When done" (single source of truth; also resolves the UI.md-missing-ADR-path minor); recorded, not applied: UI.md's literal `/prototype/<name>` (upstream fidelity; surrounding text already binds the project's routing convention) and broader description triggers (conflicts with one-trigger-per-branch pruning). **research** PASS → major applied: added "spawn with Kairos tool access; agent performs the `create_item` save itself and reports the created document's short code; done when that short code exists under the initiating item and is surfaced" (also closes the completion-criterion minor); recorded, not applied: quoted trigger examples, citation-form spec. **domain-modeling** PASS → minors applied: description now carries literal "glossary in CONTEXT.md" and "ADR" keywords; ADR-FORMAT.md gains "if no such item exists, skip the link" and a multi-context convention (name the bounded context in the ADR title); recorded, not applied: three-criteria wording drift between SKILL.md and ADR-FORMAT.md (inherited from upstream; the gate deliberately lives in SKILL.md). **codebase-design** Needs Improvement → critical finding RECORDED, not applied by design: reviewer flags a surface tension between SKILL.md's internal-seams-used-by-own-tests and architecture-review.md A1-8/Section-5 "private seams"; both files are verbatim renderings of their sources (upstream / KAIROS-S-0007) and this task's AC forbids vocabulary drift — the reconciling reading (a test at an internal seam exercises a smaller module through *its* interface, sanctioned by the scale-agnostic module definition both files share) should be added as a one-clause carve-out in KAIROS-S-0007 then re-rendered; **recommend a follow-up item on S-0007**. Minor applied: DESIGN-IT-TWICE.md's bare CONTEXT.md mention now points at the `domain-modeling` skill. Recorded, not applied: "Agent tool"→"Task tool" (wrong for this plugin — architecture-review SKILL.md uses "Agent tool"), description phrasing, references/-subdir convention (plugin convention is sibling files). **grill-with-docs** PASS, zero findings — reviewer verified both composed skills exist and are model-invoked-reachable, description honestly signals the delta over grill-me.
- 2026-07-10: **Final verification evidence**: tree shows 11 files across the five dirs (prototype/{SKILL,LOGIC,UI}.md, research/SKILL.md, domain-modeling/{SKILL,ADR-FORMAT,CONTEXT-FORMAT}.md, codebase-design/{SKILL,DEEPENING,DESIGN-IT-TWICE}.md, grill-with-docs/SKILL.md). Post-review diffs vs upstream: codebase-design SKILL.md and DEEPENING.md byte-identical, DESIGN-IT-TWICE.md exactly one pointer hunk (no vocabulary touched); CONTEXT-FORMAT.md byte-identical; grep confirms all 8 vocabulary terms defined once each in plugin/references/architecture-review.md, definitionally consistent. Hygiene grep (mattpocock, docs/adr): CLEAN. `claude plugin validate .` → Validation passed (registration itself deferred to orchestrator per lane). Live-Kairos behavioral verification deferred to KAIROS-T-0035 per the Verification Gate.

## Orchestrator merge snippets (lane: this task did NOT edit plugin.json / router / README)

**`.claude-plugin/plugin.json`** — add to the `skills` array:

```json
"./plugin/skills/engineering/codebase-design",
"./plugin/skills/engineering/domain-modeling",
"./plugin/skills/engineering/grill-with-docs",
"./plugin/skills/engineering/prototype",
"./plugin/skills/engineering/research",
```

**`plugin/skills/meta/kairos/SKILL.md` (router)** — only grill-with-docs is user-invoked; add one entry to the list:

```md
- `/kairos:grill-with-docs` — before building, when the design should leave a paper trail: the same relentless interview as grill-me, additionally capturing glossary terms (CONTEXT.md) and Kairos ADRs as decisions crystallise.
```

**`plugin/README.md`** — extend the "Shipped so far" paragraph with: engineering bucket additions `prototype`, `research`, `domain-modeling`, `codebase-design` (all model-invoked) and `grill-with-docs` (user-invoked).