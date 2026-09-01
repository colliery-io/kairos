---
id: forge-payload-normalization-in
level: task
title: "Forge payload normalization in kairos-core: GitHub + GitLab parsers, short-code extraction"
short_code: "KAIROS-T-0098"
created_at: 2026-09-01T23:12:30.697892+00:00
updated_at: 2026-09-01T23:12:30.697892+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Forge payload normalization in kairos-core: GitHub + GitLab parsers, short-code extraction

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. Pure logic; no dependency on KAIROS-T-0097, so it can run in parallel. KAIROS-T-0099 consumes it.

## Objective

Turn two different forge webhook payloads into one internal event shape, and pull Kairos short codes out of branch names and PR/MR text — all pure, in `kairos-core`, unit-tested against captured fixture payloads with no network and no database.

## Implementation Notes

- **Where**: new `crates/kairos-core/src/forge.rs`. Core is the rules-only crate (A-0009) — this is exactly the kind of logic that belongs there, and putting it here is what makes the payload shapes testable without booting anything. Follow the `kairos_core::search` / `kairos_core::graph` shape: types + pure functions + exhaustive tests.
- **The normalized event**:
  ```
  ForgeEvent { kind: LinkKind (Branch|PullRequest), external_id: String,
               title: String, url: String, state: LinkState (Open|Merged|Closed|Draft),
               author: String, forge_updated_at: DateTime<Utc>,
               match_text: String }
  ```
  `match_text` is the concatenation of every field short codes may appear in (branch/source-branch name, PR title, PR body) so extraction has a single input.
- **GitHub**: `pull_request` events (`opened`, `edited`, `reopened`, `closed` — merged is `closed` + `pull_request.merged == true`, the classic trap: there is no `merged` action), `draft` → `Draft`; `create`/`delete` events for branch refs; `push` for branch head movement. State mapping must be a table with a test per row.
- **GitLab**: `Merge Request Hook` (`object_attributes.action` = `open|reopen|update|merge|close`, `work_in_progress`/draft flag) and `Push Hook` for branches. Note GitLab's `object_attributes.iid` (project-scoped) is the number humans see — use it, not `id`.
- **Unknown/uninteresting event types normalize to `None`**, not an error — forges send pings and event types we never asked for.
- **Short-code extraction**: one regex, `[A-Z][A-Z0-9]*-[SITDA]-\d{4}`, case-sensitive, deduplicated, order-preserving. Derive the letter set from `kairos_core::short_code::ItemType` rather than hardcoding, so a sixth type cannot silently fail to match. Return `Vec<String>`; resolution against `entity_directory` is KAIROS-T-0099's job (core does no I/O).
- **Fixtures**: capture real payload JSON into `crates/kairos-core/tests/fixtures/forge/` (github_pr_opened.json, github_pr_merged.json, github_push.json, gitlab_mr_open.json, gitlab_mr_merge.json, gitlab_push.json, plus a ping and an unknown event). Redact org/user identifiers. These fixtures are the contract — when a forge changes shape, one test file changes.
- Parsers take `&str` (raw body) + the event-type header value, because KAIROS-T-0099 must verify the HMAC over the **raw bytes** before parsing; do not make the signature path depend on a round-tripped value.

## Acceptance Criteria

- [ ] `kairos-core::forge` normalizes GitHub and GitLab branch + PR/MR payloads into one `ForgeEvent`; unknown/ping events return `None` rather than erroring.
- [ ] GitHub merged-vs-closed is correct (`closed` + `merged: true` → `Merged`), and draft state maps on both forges; one test per state-table row.
- [ ] Short-code extraction finds codes in branch names, PR titles, and PR bodies; deduplicates; ignores lowercase and malformed codes; the type-letter set derives from `ItemType`.
- [ ] Parsers operate on the raw body string so signature verification can precede parsing.
- [ ] Captured fixture payloads live in-repo and every parser test runs against them; no network, no database, runs under `angreal test unit`.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
