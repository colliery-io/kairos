---
name: diataxis-review
description: Review a documentation tree against the Diataxis spec — classify every page, run the structural pass, and optionally file findings as tech debt.
disable-model-invocation: true
---

# Diataxis Review

[The Diataxis reference](../../../references/diataxis.md) is the single source of truth for this review — the four modes and their criteria, classification heuristics, mode-mixing anti-patterns, structural checks, finding format, and severity levels live there and nowhere else. Read it end to end before opening a page and apply it verbatim; this file only sequences the work.

## 1. Scope

Establish which tree is under review: the path the user gave, else the repo's docs root (ask if ambiguous). List every page in scope before classifying any — the structural pass needs the complete inventory.

## 2. Per-page pass

For each page in scope: determine its declared mode and actual mode(s) with the reference's Section 3 heuristics, then check it against its mode's Section 2 criteria and the Section 4 anti-patterns. Complete when every page in the inventory has a classification and a criteria check — a skipped page is an unfinished review.

## 3. Tree pass

With the full classification in hand, run the reference's Section 5 checks (S1–S6) across the tree. Complete when each of S1–S6 has an explicit verdict — pass or finding.

## 4. Report

Emit findings exactly in the reference's Section 6 format and ordering. Lead with the classification table — page, declared mode, actual distribution — for every page reviewed, findings or not. If the tree is clean, say so explicitly above that table.

## 5. File tech debt (optional)

Offer to file the findings on the team's delivery board. If accepted: resolve the board with `whoami`/`my_boards`, then one `create_item` per accepted finding — `item_type: task`, `task_type: tech_debt`, `board` set to the delivery board, title from the finding's proposed action, content the finding verbatim (rule IDs intact). Report the short codes created.
