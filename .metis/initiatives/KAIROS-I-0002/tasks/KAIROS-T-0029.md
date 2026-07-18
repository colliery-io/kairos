---
id: m3-workflow-skills-triage-and
level: task
title: "M3: Workflow skills - triage and implement"
short_code: "KAIROS-T-0029"
created_at: 2026-07-10T09:09:08.107628+00:00
updated_at: 2026-07-10T09:46:37.287830+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Workflow skills - triage and implement

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

Port triage and implement (workflow bucket): delivery-board backlog triage with native Task/Bug/Tech-Debt types; pick-up-and-execute against a Kairos task with phase transitions and evidence.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] triage: upstream's state-machine discipline rebound to board columns + item types (backlog→todo grooming, bug prioritization, stale-item archival per S-0003's ceremony actions); cross-team blockers escalate via blocks edges + initiative-board visibility, per the vision
- [x] implement: takes a short code (or picks from my_boards todo), transitions todo→active, works the task updating it via edit_item as working memory (the metis-ralph discipline), demonstrates acceptance criteria, transitions active→completed only when done; explicitly instructs recording evidence in the task
- [x] Both user-invoked (implement may also be model-reachable — decide per upstream and A-0014, document); registered + router updated (amended: snippets recorded in Status Updates, orchestrator merges); skill-reviewer run

## Implementation Notes

Upstream: skills/engineering/triage (+ its OUT-OF-SCOPE/AGENT-BRIEF references) and implement. This pair IS the beast-mode loop for consumers.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Authored `plugin/skills/workflow/triage/` (SKILL.md + ACCEPTANCE-CRITERIA.md, rebound from upstream AGENT-BRIEF.md) and `plugin/skills/workflow/implement/` (SKILL.md). Invocation decisions: **triage = user-invoked** (`disable-model-invocation: true`, matches upstream and A-0014). **implement = model-invoked** (no disable flag) — upstream's frontmatter is `disable-model-invocation: true`, but A-0014 explicitly lists `implement` among "disciplines the model must reach autonomously"; the beast-mode loop requires model reach, so the ADR governs and the divergence from upstream frontmatter is deliberate. Consequently implement gets NO router entry (the `/kairos` router maps the user-invoked surface only, per A-0014); triage gets one. Upstream OUT-OF-SCOPE.md's knowledge-base spirit is folded into triage's stale-item archival: append durable why-archived note, then `delete_item confirm:true` (soft delete keeps the record) — chosen over a separate `.out-of-scope/` KB because Kairos keeps institutional memory on the items themselves.
- 2026-07-10: **Lane amendment (orchestrator)**: this task does NOT edit `.claude-plugin/plugin.json` or `plugin/skills/meta/kairos/SKILL.md` (router) — the orchestrator merges. The "registered + router updated" criterion is amended to "exact registration/router snippets recorded here". Snippets:
  - `plugin.json` `skills` array — append after `"./plugin/skills/meta/writing-great-skills"`:
    - `"./plugin/skills/workflow/implement",`
    - `"./plugin/skills/workflow/triage"`
  - Router (`plugin/skills/meta/kairos/SKILL.md`) — insert into the skill list before the `/kairos:kairos` line: `- \`/kairos:triage\` — grooming the delivery board: refine acceptance criteria, prioritize bugs, weigh tech debt against its ~20% allocation, archive stale items, restock Todo, escalate cross-team blockers.` (`implement` intentionally absent from the router: model-invoked.)
- 2026-07-10: **Reviews (skill-reviewer agent, foreground)**. triage: PASS conditional on should-fixes — applied: P0-pull vs refinement contradiction resolved (P0s get expedited criteria as pulled), tech-debt gate made computable (~1 in 5 items across Todo+Active, lead confirms promotions), restock made computable (ask lead for Todo depth, default one item per member), `## Triage Notes` anchor heading defined for all edit_item appends, effort/priority bound to board metadata scales via set_metadata, pointer reworded to "read ACCEPTANCE-CRITERIA.md first", negation-heavy closer trimmed from the reference. Router-omission finding = the lane amendment (orchestrator merges the recorded snippet). implement: PASS — should-fixes applied: description trimmed to identity + distinct triggers, leading word "Implement" front-loaded; triage hand-back given an executable actor (stop, note gap, tell the lead); Blocked section aligned with triage (escalation note on task AND parent initiative for Initiative Board Review visibility); board_items named in the pickup path; negated restatements and "where available" hedge dropped.
- 2026-07-10: **Evidence (AC demonstration)**. AC1 triage: `plugin/skills/workflow/triage/SKILL.md` — columns Backlog→Todo→Blocked→Active→Completed + Task/Bug/Tech Debt types as the state machine; ceremony encodes all five S-0003 grooming actions (refine w/ acceptance criteria, P0 pull + P1–P3 ordering via set_metadata, tech-debt vs ~20% allocation, 90+ day archival via durable why-note + delete_item confirm:true or parked note, Todo restock); Cross-team blockers: link_items(blocks) + Blocked transition + escalation note on item AND parent initiative, per vision. ACCEPTANCE-CRITERIA.md rebinds upstream AGENT-BRIEF (durable/behavioral/verifiable/out-of-scope). AC2 implement: `plugin/skills/workflow/implement/SKILL.md` — short code or my_boards→board_items(Todo) pickup; transition_item to Active before first change; "the task item is your working memory" edit_item discipline; completion gate per A-0012 = repo gates clean (`cargo fmt --check`, `cargo clippy -- -D warnings`, `angreal test unit && angreal test integration`, or repo equivalents) + every criterion demonstrated-not-asserted with command+output recorded on the item + new behavior carries new tests; Active→Completed only after the gate. AC3: invocation decided + documented; snippets recorded above; skill-reviewer PASS on both. Verification: both frontmatters parse via yaml.safe_load (triage: disable-model-invocation true; implement: no disable flag); lane respected — `grep -n workflow .claude-plugin/plugin.json` matches only the description line (no skill entries), `grep -c triage plugin/skills/meta/kairos/SKILL.md` = 0 (router untouched). S-0006-only tool references (whoami, my_boards, board_items, get_item, get_history, search, edit_item, set_metadata, transition_item, link_items, delete_item). Full behavioral verification against a live Kairos deferred to KAIROS-T-0035 per the gate note.