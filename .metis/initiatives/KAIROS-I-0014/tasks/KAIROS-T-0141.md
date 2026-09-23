---
id: j11-new-kind-of-work-a-template
level: task
title: "J11 new-kind-of-work: a template and a metadata field make support requests first-class, then the field is retired"
short_code: "KAIROS-T-0141"
created_at: 2026-09-23T03:45:50.887850+00:00
updated_at: 2026-09-23T10:32:56.313638+00:00
parent: KAIROS-I-0014
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0014
---

## Parent Initiative

[[KAIROS-I-0014]]

## Implementation Notes

The journey's story, cast and the thing it would catch are in the
initiative's Detailed Design — read that section first; it is the spec.

Harness conventions (unchanged): `journey(id, title, {humans}, body)` and
`step(persona, narration, fn)` from `uat/run/narrate.ts`; personas from
`uat/personas`; surfaces in `uat/surfaces/` (gui, api, cli, mcp, forge);
`ledger.add` everything created, in dependency order; `named()` for every
created slug or title; `step.composeOnly` for anything needing a fresh
tenant or deployment admin. Read a neighbouring journey before writing —
`uat/journeys/board-setup.journey.ts` is the most recent and shows the
current idioms. Selector notes live in the completed task docs of
KAIROS-I-0013 (T-0133 and T-0134 in particular).

If a journey needs a team of its own, give the fixture a suffix no other
journey uses (`mobile`, `ios`, `infra` are taken).

## Acceptance Criteria

- [x] Green in compose (hand-run, and again alongside J9/J10 under the shared
      stack lock): 8 steps, ~2.1s. The report reads as a team inventing a
      process and then trying to un-invent part of it.
- [x] Definition, template, task, document and the stamped value all ledgered;
      teardown clean across every run, including the failed iterations.
- [x] tsc clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `29526a1` (`uat/journeys/new-kind-of-work.journey.ts`).

Product findings, all of them the product being right:

- **A metadata definition cannot be retired while anything references it.**
  `DELETE /api/metadata-definitions/{id}` returns 409 `DEFINITION_IN_USE`
  with `details.item_values` and `details.template_fields`. So the
  initiative's "alice later retires the field — the item keeps its stamped
  value" is not reachable as written: the delete is refused outright. The
  story became the better one — she takes the field OFF THE TEMPLATE so new
  work stops collecting it, the refusal count drops to the item value alone,
  and the stamped value on existing work is untouched. Removing a field from
  a template changes what the NEXT item gets and does not reach backwards.
- **Templates are DOCUMENT templates (A-0003).** `create_item` accepts
  `template` for documents only, so "raises an item from the template" is the
  intake write-up attached to the support task, not the task itself. Only an
  association carrying a `default_value` is stamped at create time; a
  `required` association with no default just signals expectation.
- **A soft-deleted item keeps its `item_metadata` rows.** Teardown therefore
  needs an extra ledger entry that CLEARS the stamped value (`set_metadata`
  with null) before the document is deleted — otherwise the definition can
  never be deleted and every run leaks one.
- A definition created with no `entity_types` applies to every entity type
  (`definition_applies_to` treats an empty scope list as "all"), which is
  what the `/admin/metadata` create form sends.

Selector notes:

- The item page's Metadata panel labels a field by its definition NAME, not
  its slug, and an enum renders as a `<select>` inside
  `.kairos-metadata__field` holding the current value.
- Admin list rows (a definition, a template) are `.cl-stack`s with no class
  of their own; scope to the innermost stack containing the slug —
  `.cl-stack` filtered by `has:` then `.last()`, same trick J9 needs for the
  search page's chip rows.
- The admin screens surface a rejection as `div.cl-alert[role="alert"]`
  carrying the server message plus `code: <CODE>`, which is enough to assert
  a refusal without reading the network.
- The API error envelope nests everything under `error`:
  `body.error.details`, not `body.details`.

**Harness note (all three journeys):** `flock` does not exist on macOS, so
the documented serialisation silently ran nothing. These were re-run to green
together using a `mkdir /tmp/kairos-uat.lock` mutex instead.