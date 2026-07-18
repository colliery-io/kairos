---
id: m0-plugin-manifests-and-rendered
level: task
title: "M0: Plugin manifests and rendered references"
short_code: "KAIROS-T-0006"
created_at: 2026-07-08T15:06:01.580655+00:00
updated_at: 2026-07-09T03:05:51.381495+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0001]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M0: Plugin manifests and rendered references

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Plugin skeleton per KAIROS-A-0014: manifests, directory tree, and the two distributable review specs rendered from their Metis sources.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `.claude-plugin/plugin.json` (name `kairos`) and `.claude-plugin/marketplace.json` valid; plugin skill buckets `plugin/skills/{workflow,engineering,review,meta}/` created (empty is fine), `plugin/hooks/` and `plugin/.mcp.json` template (deployment-URL placeholder) in place
- [x] `plugin/references/architecture-review.md` and `plugin/references/diataxis.md` rendered from KAIROS-S-0007/KAIROS-S-0008 bodies (no Metis frontmatter; content otherwise identical), with a documented re-render procedure (script or README note)
- [x] Structure passes the plugin-validator agent / `claude plugin` local validation

## Implementation Notes

References A-0014 (layout), S-0007/S-0008 (source of truth stays in Metis). Skills themselves are KAIROS-I-0002 scope — do not author any SKILL.md here.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-08: Implemented plugin skeleton per KAIROS-A-0014. Files created: `.claude-plugin/plugin.json` (name `kairos`, v0.1.0, author, `skills: []` placeholder — skills are KAIROS-I-0002 scope), `.claude-plugin/marketplace.json` (marketplace `kairos`, one plugin entry, `source: "./"`), `plugin/skills/{workflow,engineering,review,meta}/.gitkeep`, `plugin/hooks/.gitkeep`, `plugin/.mcp.json` (http server template with `{{KAIROS_DEPLOYMENT_URL}}/mcp` placeholder), `plugin/README.md` (documents template + /bootstrap fill flow + re-render procedure), `scripts/render-references.sh`, `plugin/references/{architecture-review,diataxis}.md`.
- 2026-07-08: Evidence — criterion 1: `python3 -m json.tool` parses all three JSON files ("plugin.json-OK / marketplace.json-OK / mcp.json-OK"); tree verified with `find .claude-plugin plugin scripts -type f`. Criterion 2: `scripts/render-references.sh` renders both references from `.metis/specifications/KAIROS-S-000{7,8}/specification.md` (strips frontmatter, keeps H1+body, swaps the render-reference sentence for "Rendered from KAIROS-S-000x (source of truth) — do not edit here."); ran twice, `diff` of consecutive outputs empty → idempotent; `grep -c "renders into the skills plugin"` = 0 in rendered files; content otherwise identical to spec bodies. Criterion 3: `claude plugin validate /Users/dstorey/Desktop/kairos` → "Validation passed" (fixed two initial warnings by adding marketplace description + plugin author); plugin-dev:plugin-validator agent → PASS, 0 critical issues, 5 forward-looking warnings recorded below.
- 2026-07-08: Validator warnings to carry into KAIROS-I-0002/M1 (not M0 defects): (1) skills under `plugin/skills/<bucket>/` are outside the auto-discovery path `<plugin-root>/skills/` — when skills are authored they must be wired via manifest paths or relocated; (2) `"skills": []` may not be a recognized manifest field (documented path fields: commands/agents/hooks/mcpServers) — confirm against docs at M1; (3) `plugin/.mcp.json` is deliberately inert by location (placeholder must never be loaded unrendered); (4) `source: "./"` ships the whole repo as plugin payload (.metis db/logs, .angreal caches) — consider narrowing source or exclusions later; (5) no root README/LICENSE yet.
- 2026-07-08: Verification gate note: this task created only JSON/Markdown/shell artifacts — no Rust or angreal surface touched (Cargo/crates/.angreal owned by other agents), so cargo fmt/clippy and angreal test gates apply to the orchestrator's combined run; no new testable runtime behavior was introduced (render script verified by double-run diff above).