---
id: m3-engineering-skills-tdd-and
level: task
title: "M3: Engineering skills - tdd and diagnosing-bugs"
short_code: "KAIROS-T-0030"
created_at: 2026-07-10T09:09:08.546485+00:00
updated_at: 2026-07-10T09:45:57.507584+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Engineering skills - tdd and diagnosing-bugs

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

Port tdd (with its mocking/tests references) and diagnosing-bugs (with its hitl-loop script) into the engineering bucket, Kairos-bound.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] tdd: red-green-refactor discipline intact incl. the reference files; progress notes land in the active Kairos task via edit_item
- [x] diagnosing-bugs: reproduce→minimise→hypothesise→instrument→fix→regression-test loop intact; diagnosis log lands in the Kairos task/bug
- [x] Both model-invoked per upstream; registered + router recorded as merge snippets per lane amendment (see Registration Snippets — orchestrator applies); skill-reviewer run

## Implementation Notes

Upstream: skills/engineering/tdd/* and diagnosing-bugs/*. Minimal rebinding — these are discipline skills; only the work-system touchpoints change.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Registration Snippets (for orchestrator merge)

**Lane amendment**: this task's lane creates the skill directories ONLY — `.claude-plugin/plugin.json` and the `/kairos` router are merged by the orchestrator from the snippets below. The "registered + router" clause of AC-3 is therefore satisfied by recording these exact snippets, not by editing those files here.

**plugin.json** — add to the `skills` array:

```json
    "./plugin/skills/engineering/diagnosing-bugs",
    "./plugin/skills/engineering/tdd",
```

**Router** (`plugin/skills/meta/kairos/SKILL.md`) — model-invoked disciplines are reachable without the router, but it mentions them briefly per its existing style; add before the Sync rule:

```markdown
Model-invoked disciplines (the agent reaches these on its own; naming them works too):

- `tdd` — the red → green test-first loop; slice progress lands on the active Kairos task.
- `diagnosing-bugs` — the phased loop for hard bugs: reproduce → minimise → hypothesise → instrument → fix → regression-test; the diagnosis log lands on the Kairos item.
```

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Read A-0014, meta exemplars, upstream tdd/* and diagnosing-bugs/*. Created `plugin/skills/engineering/tdd/` (SKILL.md + tests.md + mocking.md) and `plugin/skills/engineering/diagnosing-bugs/` (SKILL.md + scripts/hitl-loop.template.sh). tests.md, mocking.md, hitl-loop.template.sh carried over byte-identical (diff-verified); hitl script chmod +x, `bash -n` clean. SKILL.md bodies verbatim per diff except the work-system rebindings: (tdd) upstream CONTEXT.md/ADR line → pull active Kairos task via `get_item` (short code from session context or `my_boards`) + linked ADR documents; new loop rule "Progress lives on the task" — progress note per slice via `edit_item`. (diagnosing-bugs) diagnosis-log paragraph up front — per-phase outcomes appended to the Kairos item via `edit_item`; CONTEXT.md line → `get_item` on the item + linked ADR documents; Phase 6 checklist gains a diagnosis-log-current item; upstream `/improve-codebase-architecture` handoff → `codebase-design` skill (A-0014 name). Both skills keep upstream's trigger-rich descriptions and stay model-invoked (no `disable-model-invocation`). No upstream references remain. Next: skill-reviewer + plugin-validator passes.
- 2026-07-10: Verification gate evidence. **skill-reviewer (foreground): PASS both skills, 0 blocking.** tdd — 1 should-fix (tests.md/mocking.md context-pointer wording names no trigger; upstream-verbatim discipline text, NOT applied per lane constraint "only rebinding = work-system touchpoints" — logged for a follow-up pruning pass), 2 nits (tdd/diagnosing-bugs trigger overlap on bug-fixing; deliberate red-green-refactor tension). diagnosing-bugs — 1 should-fix (`codebase-design` reach doesn't resolve yet; **accepted as forward reference** — codebase-design is in A-0014's model-invoked engineering inventory and lands with its own M-task, same pattern as tdd→code-review which another lane already landed), 2 nits (broken/throwing/failing synonym collapse; ask-vs-create_item ambiguity when no item exists). **plugin-validator (foreground): PASS both** — frontmatter valid, names match directories, all relative references resolve (tdd 2/2, diagnosing-bugs 1/1), hitl script executable with clean `bash -n`, zero warnings; manifest registration out of scope per lane. AC evidence: AC-1 diff vs upstream shows tdd body verbatim except the two Kairos touchpoints (get_item/my_boards context pull; per-slice progress note via edit_item as a loop rule) + tests.md/mocking.md byte-identical. AC-2 diff shows diagnosing-bugs 6-phase loop verbatim except diagnosis-log touchpoints (up-front per-phase edit_item rule; Phase 6 checklist item) and the codebase-design rebinding + hitl-loop.template.sh byte-identical. AC-3 both descriptions upstream-verbatim trigger-rich, no `disable-model-invocation`; plugin.json + router snippets recorded above for orchestrator merge (router/README sync honored via the recorded snippet — A-0014). All ACs met; transitioning to completed.