---
id: m5-review-skills-architecture
level: task
title: "M5: Review skills - architecture-review and diataxis-review"
short_code: "KAIROS-T-0033"
created_at: 2026-07-10T09:09:18.482218+00:00
updated_at: 2026-07-10T09:43:40.953042+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M5: Review skills - architecture-review and diataxis-review

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

The two new review skills driven by the SHIPPED references: architecture-review (plugin/references/architecture-review.md) and diataxis-review (plugin/references/diataxis.md).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] architecture-review: scan → classify by altitude → report per the reference's Section 1-5 (finding format verbatim, anti-pattern self-check); keeps upstream improve-codebase-architecture's HTML-report presentation + grilling loop for the chosen candidate; ADR-conflict checks read Kairos ADRs via search/get_item
- [x] diataxis-review: classify pages per the reference (declared vs actual mode, rule IDs cited in every finding, severity levels); optional filing of findings as Tech-Debt items on the team's delivery board via create_item
- [x] Both user-invoked; each SKILL.md cites its reference via context pointer and never restates the rules (single source of truth); registered + router — AMENDED by orchestrator lane rule: this task does not edit plugin.json or the router; exact registration + router entries recorded in Status Updates for the orchestrator to merge; skill-reviewer run on both skills

## Implementation Notes

The references were rendered from KAIROS-S-0007/S-0008 in T-0006 — the skills consume, never duplicate, them.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Executed (lane: `plugin/skills/review/` only; plugin.json/router untouched per orchestrator amendment).

### Deliverables

- `plugin/skills/review/architecture-review/SKILL.md` — user-invoked; cites `../../../references/architecture-review.md` as single source of truth; process: load contracts (reference + Kairos ADRs via `search`/`get_item` for A2-6..A2-8 and re-litigation rules) → scan (Explore subagent, both altitudes) → classify (Section 2.3/3.3 checklists) → report (Section 4 format, Section 5 self-check, self-contained HTML report in OS temp dir) → grilling loop via `grilling` skill on the picked candidate, with ADR offer via `create_item(item_type: adr)` on load-bearing rejections.
- `plugin/skills/review/architecture-review/HTML-REPORT.md` — upstream improve-codebase-architecture presentation ported: Tailwind + Mermaid CDN scaffold, candidate cards rendering the reference's Section 4 finding fields, before/after diagrams, strength badges (Strong=emerald / Worth exploring=amber / Speculative=slate) + altitude tags, top-recommendation section; vocabulary delegated to reference Section 2.1 (upstream's /codebase-design dependency removed).
- `plugin/skills/review/diataxis-review/SKILL.md` — user-invoked; cites `../../../references/diataxis.md` as single source of truth; process: scope inventory → per-page pass (Section 3 declared/actual, Section 2 criteria, Section 4 anti-patterns) → tree pass (S1–S6, explicit verdict each) → report (Section 6 format/ordering, classification table, explicit clean-tree statement) → optional tech-debt filing: `whoami`/`my_boards` → one `create_item(item_type: task, task_type: tech_debt)` per accepted finding on the delivery board, finding verbatim with rule IDs.
- S-0006 tools only (`search`, `get_item`, `create_item`, `whoami`, `my_boards`); no HTTP/REST assumptions; no services; no Homebrew.

### Evidence

- **skill-reviewer (architecture-review)**: initial FAIL — 3 blocking restatements of reference rule text (HTML-REPORT.md Evidence field wording, design-it-twice trigger clause, "before forming any opinion" clause in SKILL.md); all fixed plus 5 nits (Location field, Wins/ADR-callout framing, vocabulary-carrying examples, step-2 completion criterion) → PASS conditions met.
- **skill-reviewer (diataxis-review)**: PASS, no blocking issues; nits applied (S1–S6 completion criterion added, no-op rationale clause pruned). Process note from review: KAIROS-S-0008 re-render should re-verify section-number citations in this SKILL.md.
- **plugin-validator**: PASS 2/2 — frontmatter valid, names match directories, all relative links resolve on disk; non-registration confirmed as expected lane condition.
- **No-duplication proof**: 6-word-shingle intersection between each reference and each skill file — architecture SKILL.md: only "…the single source of truth for…" (the SSoT declaration itself); HTML-REPORT.md: 0; diataxis SKILL.md: 0. Keyword grep for reference rule terms (pass-through, deletion test, two adapters, learning-oriented, blocking confusion, …) across both SKILL.mds: 0 hits.

### For the orchestrator to merge (registration + router)

`.claude-plugin/plugin.json` — append to `skills` (after the meta entries):

```json
    "./plugin/skills/review/architecture-review",
    "./plugin/skills/review/diataxis-review"
```

`plugin/skills/meta/kairos/SKILL.md` — insert into the router list (after the `/kairos:writing-great-skills` line):

```markdown
- `/kairos:architecture-review` — reviewing a codebase's structure: scan, classify by altitude, an HTML report of deepening candidates, then a grilling loop on the one you pick.
- `/kairos:diataxis-review` — reviewing a documentation tree: classify every page against the Diataxis spec, run the structural pass, optionally file findings as tech debt.
```

Full behavioral verification against a live Kairos deferred to KAIROS-T-0035 per the gate.