---
id: retrieval-has-no-human-surface-no
level: task
title: "Retrieval has no human surface: no CLI verb, and the GUI cannot ask what is related"
short_code: "KAIROS-T-0195"
created_at: 2026-09-24T22:03:09.445333+00:00
updated_at: 2026-09-24T22:03:09.445333+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


exit_criteria_met: false
initiative_id: NULL
---

## Objective

`related_work` is reachable over **MCP and REST only**. There is no
`kairos related …` CLI verb, and the GUI can show edge proposals on an item but
cannot *ask* what a piece of work is related to.

Found while writing [[KAIROS-T-0193]]'s documentation, which is where
[[KAIROS-I-0016]] said defects get found: the Diátaxis tutorial step could not be
written. Every other feature in `tutorials/run-kairos-locally.md` is demonstrated
with the CLI or the browser, and this one would have needed `curl` with a
hand-extracted bearer token — which is not a tutorial step, it is an apology.

## Why it matters

[[KAIROS-A-0021]] is explicitly agent-first, and MCP is the surface that matters
most. That is a good reason for the MCP tool to exist *first*; it is not a reason
for a person never to be able to ask.

The asymmetry is the odd part rather than the absence:

- a person **can** see and act on proposals an agent made (the item page panel)
- a person **cannot** ask the question that produces them

So a human can only ever react to what an agent thought of, on an item an agent
happened to look at. "Didn't we try this?" is at least as much a human question,
and it is the one the whole initiative is named after.

## Type

- [x] Feature — new functionality

## Priority

- [x] P2 — worth doing, nothing is broken without it

## Business Justification

- **User value**: a person can ask "what is this related to?" where they already
  are, instead of only seeing what an agent asked on their behalf.
- **Business value**: the confirm/reject loop is what makes the graph improve
  with use ([[KAIROS-T-0192]]), and it currently only ever starts from an agent.
- **Effort**: S for the CLI verb; M with a GUI panel, which wants a design pass —
  a "related work" box that is wrong half the time needs to *read* as suggestions
  rather than as a list of facts, and that is a wording and layout problem more
  than a code one.

## Notes

The rendering already exists and is tested: `render_related_work` produces the
markdown an agent reads, and a CLI verb could print substantially the same thing.
The REST endpoint returns everything a GUI panel needs.

Whoever picks this up should read
[`explanation/finding-related-work.md`](../../../docs/src/explanation/finding-related-work.md)
first. The counter-intuitive part — *a good answer here often looks like a weak
one* — is a presentation problem, and presenting these as though they were search
results would undo the care taken in the wording elsewhere.

## Decision — 2026-09-25 (Dylan)

**GUI panel first.** A "Possibly related" section on the item detail page, where
someone already is when the question occurs to them — rather than a `kairos
related` CLI verb.

Noted against my own recommendation, which was the CLI verb on the grounds that it
was smaller and would unblock the tutorial [[KAIROS-T-0193]] could not write. The
counter-argument is the stronger one: the CLI verb is reachable but not
*discoverable*, and "didn't we try this?" is a question people have while looking
at the work, not while composing a command. A panel is where the question actually
arrives.

Consequences to carry into implementation:

- The tutorial step becomes browser steps rather than a CLI line, so it belongs in
  `run-kairos-locally.md`'s existing browser section.
- The panel needs the proposal wording to survive into a visual design — "possibly
  related", the claim type, and the *why*. The whole point of
  [[KAIROS-A-0021]] rule 5 is that these are proposals, and a UI that renders them
  as a confident list of links would undo that in a way the MCP text cannot.
- `propose_edge` already exists for agents; the panel needs the human side of the
  same flow, which [[KAIROS-T-0192]] built the confirm/reject for. So this is
  mostly a read surface plus a button that already has a backend.
- A CLI verb is still worth having later, and is now explicitly *not* blocking.

This is initiative-sized once the tutorial and UAT journey are counted, so it
wants decomposing rather than doing as one task.
