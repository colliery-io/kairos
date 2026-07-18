---
id: m3-skills-foundations-meta-bucket
level: task
title: "M3: Skills foundations - meta bucket and conventions"
short_code: "KAIROS-T-0027"
created_at: 2026-07-10T09:08:54.743433+00:00
updated_at: 2026-07-10T09:28:51.808100+00:00
parent: KAIROS-I-0002
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Skills foundations - meta bucket and conventions

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

The plugin's meta bucket and authoring foundations per KAIROS-A-0014: port writing-great-skills, grilling, grill-me, and a thin handoff from mattpocock/skills; stub the /kairos router; register everything in plugin.json.

## Acceptance Criteria

## Acceptance Criteria

- [x] Upstream sources obtained (clone https://github.com/mattpocock/skills if no local copy) and the four skills ported into plugin/skills/meta/ with A-0014's invocation split (grilling model-invoked; grill-me, writing-great-skills user-invoked; handoff user-invoked and rewritten thin — Kairos boards are the persistent memory, handoff only compacts conversation context)
- [x] Ports are Kairos-native: no issue-tracker/mattpocock references; grilling unchanged in discipline; writing-great-skills kept as the normative authoring reference
- [x] /kairos router skill created mapping the CURRENT user-invoked surface (it will grow; include the sync rule in its own text)
- [x] plugin.json skills array lists every shipped skill path; `claude plugin validate` passes; plugin-dev:skill-reviewer run on each new SKILL.md with findings addressed or recorded

## Implementation Notes

References A-0014 (decided), the port map in KAIROS-I-0002. Authoring standard is the ported writing-great-skills itself — apply it to these very ports (descriptions, leading words, pruning).

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Read A-0014, I-0002 port map, S-0006 tool inventory. Upstream clone present in scratchpad. Plan: port grilling (model-invoked, verbatim discipline), grill-me + writing-great-skills (+GLOSSARY.md) user-invoked, handoff rewritten thin (Kairos items = persistent memory), author /kairos router with sync rule; register all five in plugin.json; validate; skill-review each. Note: A-0014's layout comment lists handoff under workflow/, but this task's acceptance criteria explicitly place it in plugin/skills/meta/ — following the task; plugin/README.md bucket comments updated to match.
- 2026-07-10: Authored: grilling (model-invoked, discipline verbatim), grill-me, handoff (thin rewrite: bring items current via edit_item/transition_item, then compact conversation-only context to a temp-dir note keyed by short codes for get_item), kairos router (4 user-invoked entries + sync rule), writing-great-skills + GLOSSARY.md (verbatim, grep confirms zero upstream/author/issue-tracker references). plugin.json skills array lists all 5 directories; `claude plugin validate` passes on both marketplace.json and plugin.json. plugin/README.md re-synced (handoff moved to meta bucket, shipped-skills note + sync rule). 5 plugin-dev:skill-reviewer agents running.
- 2026-07-10: Skill-reviewer results (4/5 in): **grilling** PASS — 1 minor ("'grill' trigger phrases" is meta-language; concrete phrases would match paraphrases better) recorded, not applied: description is part of the proven upstream discipline and the quoted 'grill' keyword carries the match. **grill-me** PASS — 1 minor applied: body now "Invoke the `grilling` skill." (explicit invocation removes imitation variance). **handoff** Needs Improvement — all 3 findings applied: description now names both moves; note gets deterministic overwriting filename `kairos-handoff-<workspace-name>.md` + finish-by-reporting-absolute-path (fixes discoverability + temp-dir sediment); no-op clause added ("if nothing remains, say so and skip the note"). **writing-great-skills** PASS — applied: unbolded `collapse`/`positive` (SKILL.md) and `positive` (GLOSSARY.md) to restore the bold=glossary-defined invariant; recorded, not applied (fidelity to source): "in-skill" vs "in-file" wording drift, External-reference rung nuance, glossary-internal emphasis bolds (sequence/execution/clarity/demand). Router entry for handoff re-checked against new description — still accurate. Router review pending.
- 2026-07-10: **kairos router review (5/5)** — dedicated reviewer agent stalled; rigorous self-review performed per fallback and recorded: description discipline PASS (human one-liner, trigger lists stripped, "router" leading word); surface accuracy PASS (grep shows exactly 4 skills with disable-model-invocation:true, all 4 listed; model-invoked grilling correctly excluded; handoff entry matches post-fix description); no-ops PASS (self-entry deliberate — sync rule claims the ENTIRE surface, omitting itself would make the map lie); duplication PASS (entry summaries duplicate siblings only as routing requires; "user-invoked surface" appears twice, both load-bearing — header vs invariant definition); sediment/sprawl PASS (fresh, 9-line body); negation ACCEPTABLE (defect-framing is a maintainer-facing hard guardrail required by task wording, paired with positive invariant). No changes required. Grill-me's reviewer independently confirmed the router's grill-me entry and sync-rule presence.
- 2026-07-10: **Final verification evidence**: `claude plugin validate .` → "Validating marketplace manifest … ✔ Validation passed" (exit 0); `claude plugin validate .claude-plugin/plugin.json` → "✔ Validation passed" (exit 0). Tree of plugin/skills/meta/: grill-me/SKILL.md, grilling/SKILL.md, handoff/SKILL.md, kairos/SKILL.md, writing-great-skills/{SKILL.md,GLOSSARY.md}. plugin.json skills array lists all 5 directories. grep for mattpocock/upstream/issue-tracker references across meta/: CLEAN. Invocation split verified by grep: grilling model-invoked (no disable flag), other four disable-model-invocation: true. Handoff references only S-0006 tool names (edit_item, transition_item, get_item — reviewer verified against the spec). All acceptance criteria checked off. Live-Kairos behavioral verification deferred to KAIROS-T-0035 per the Verification Gate. Transitioning to completed.