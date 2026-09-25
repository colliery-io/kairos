---
id: retrieval-has-no-human-surface-no
level: task
title: "Retrieval has no human surface: no CLI verb, and the GUI cannot ask what is related"
short_code: "KAIROS-T-0195"
created_at: 2026-09-24T22:03:09.445333+00:00
updated_at: 2026-09-25T11:28:50.825537+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


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

### 2026-09-25 — sizing corrected: a task, not an initiative

I said this was initiative-sized and it is not. I had not checked what already
exists, and reasoned from [[KAIROS-T-0193]]'s "retrieval has no human surface" as
though it meant nothing existed — when it meant the *asking* did not.

What is already built:

- **`GET /api/items/{short_code}/related`** ([[KAIROS-T-0191]]) — fully
  documented, bounded, `503 EMBEDDINGS_DISABLED` when embeddings are off rather
  than an empty list dressed as "nothing is related", and a `vector: false` flag
  marking a degraded text-only answer.
- **`RelatedWorkResponse`/`RelatedProposal` DTOs** in `kairos-client`.
- **`EdgeProposalsPanel`** on the item page ([[KAIROS-T-0192]]) — a human already
  sees agent-made proposals there and can confirm or reject them.

What is actually missing:

1. A client method to call `/related` — neither `kairos-client` nor
   `kairos-web/src/api.rs` has one. The DTO is there; nothing fetches it.
2. A "Possibly related" panel on the item page that asks, handling the 503 and the
   degraded case.
3. The tutorial moment, as browser steps.
4. Coverage.

That is one task. Doing it as this ticket rather than decomposing an initiative
around it.
## Status Updates

### 2026-09-25 — a person can ask now

`RelatedWorkPanel` on the item detail page, below the agent-made **Suggested
links** — the same idea from the other direction: one you were handed, one you
asked for.

Scope was much smaller than I first said, and I was wrong about that in a way
worth recording: I called this initiative-sized off [[KAIROS-T-0193]]'s note that
"retrieval has no human surface", without checking. The REST endpoint, the DTOs and
the confirm/reject panel were all already built. What was actually missing was a
client method and a panel. See the sizing correction above.

### Design decisions, and why they are not cosmetic

- **It does not search on page load.** Retrieval costs a vector search per ask, and
  most visits to an item are not someone wondering what it duplicates. A button
  also makes it a question the person asked rather than a claim the page makes,
  which is the framing [[KAIROS-A-0021]] rule 5 wants.
- **No score is shown.** The score is a fused rank comparable within one response
  and nowhere else. On screen a number reads as a confidence whatever the label
  says, and the e2e test asserts no `0.xx` appears in the panel — the one assertion
  most likely to be lost in a future redesign.
- **The empty case is worded, not blank**: "that is this search coming up short
  rather than proof that nothing is related." An empty list is exactly the kind of
  answer people over-read.
- **`503` is not an error.** A deployment with embeddings off gets one dimmed
  sentence, because the person just clicked and deserves an answer rather than
  silence — but not an alert, because nothing is broken.
- **A degraded answer says so.** `vector: false` surfaces the server's own note in
  a warning: still useful, and it will have missed work phrased differently.

### The tutorial moment T-0193 could not write

[[KAIROS-T-0193]] recorded "no tutorial page" *because* there was no human surface,
and filed this ticket instead. `run-kairos-locally.md` now has the step — and it
spends most of its words teaching scepticism rather than clicking, because the
measurement is the interesting part: related pairs average 0.80 similarity,
unrelated pairs reach 0.82, so about half the strongest matches are wrong. A
tutorial that presented this as a magic "related items" feature would teach the
wrong thing about the product.

### I broke the smoke test and blamed the test first

Adding the panel, `angreal test e2e` started failing in `smoke.spec` on a
save-confirmation toast. I assumed a flake, then suspected my panel, then suspected
[[KAIROS-T-0197]]'s auth change — three e2e runs of guessing — before reading
Playwright's DOM snapshot, which showed the toast present but mangled: `Saved â the
item is now at v2.`

**I had corrupted `item.rs`.** A `perl -0777 -i -pe` with a `\x{2014}` escape and no
encoding layer rewrote the whole file's bytes, mangling **89 non-ASCII characters**
— em dashes, ellipses, a checkmark — across comments and UI strings. The
"Wide character in print" warning perl emitted at the time was the tell, and I did
not read it.

Fixed by restoring from git and re-applying through the byte-safe helper I had been
using everywhere else. Verified zero mojibake in every file this branch touches, not
just the one I noticed.

Two lessons worth keeping: the DOM snapshot was the fastest path to the answer and I
reached for it fourth; and a warning from a tool mid-edit is evidence, not noise.

### Coverage, and what it actually proved

`e2e/tests/related-work.spec.ts`: the panel exists, has **not** searched on load,
asks when clicked, and the answer is framed as proposals with a claim pill, a
clickable short code, a *why*, and no score.

It accepts three outcomes — proposals, honest-empty, or not-enabled — because all
three are legitimate and asserting "results appeared" would make the test depend on
the seed's similarity scores, which are not a contract. **But a test that accepts
three outcomes can silently stop asserting the interesting one**, so it prints which
one it got. This run:

```
[related-work] outcome: proposals rendered
```

So the proposal-rendering path is genuinely exercised, against real retrieval on the
GUI server — not the 503 branch. Worth knowing: the e2e tier runs two servers, and
only the GUI one has the embedding cache.

### Gates

lint clean, `angreal web lint` clean, release wasm build green, **403 unit tests**,
integration **47/47**, e2e **17** (one new), uat **22 journeys**, docs build green.

### Still not done

No CLI verb. It is no longer blocking anything — the tutorial has its browser steps
and the panel is the discoverable surface — so `kairos related <code>` is worth
having for scripting and is not worth holding this ticket open for.