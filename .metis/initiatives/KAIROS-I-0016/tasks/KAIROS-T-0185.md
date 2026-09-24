---
id: illustrate-the-book-screenshots
level: task
title: "Illustrate the book: screenshots where the reader is looking at a screen, diagrams where they are not"
short_code: "KAIROS-T-0185"
created_at: 2026-09-24T00:04:12.769766+00:00
updated_at: 2026-09-24T00:13:56.502824+00:00
parent: KAIROS-I-0016
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

The book shipped with **zero images** — no screenshot, no diagram, no `![...]`
anywhere across 36 pages. Add both.

Dylan, 2026-09-23, asked directly whether screenshots were included. They were
not, and it is a gap against the house convention this initiative claimed to
follow: `squire-core`, whose layout and `book.toml` were the model, keeps
`docs/src/images/` with five screenshots. Its structure was copied and the fact
that it illustrates its pages was missed.

**Scope decided by Dylan: both screenshots and diagrams; staleness accepted**
(no recapture automation — see D3).

## Implementation Notes

### D1 — Screenshots, where the reader is looking at a screen

The server runs on `:41080` against the seeded `demo` tenant
(`angreal dev serve`), so every state below is reachable. Sign in as
`alice@kairos.test` / `alice-password`.

Ranked by how much the prose is currently doing a picture's job:

1. **`tutorials/run-kairos-locally.md`** — the strongest case. The lesson says
   "you will land on the boards overview… open Platform Delivery and you will
   see cards sitting in columns." **T3** wants the learner to see what they
   should see, and for a browser step a paragraph is a poor substitute.
   Two shots: the boards overview, and Platform Delivery with cards in columns.
2. **Put-away work** — [[KAIROS-T-0164]] built a gold banner, a badge, write
   affordances disabled *with their reasons beside them*, and a distinct
   treatment for put-away rows and graph nodes. All of it is prose today. The
   vocabulary problem this initiative kept hitting ("archived" meaning two
   unrelated things) is a **visual** distinction in the product and text in the
   book. Shots: an archived item page with its banner and Restore, and the
   search results showing a marked put-away hit.
3. **`how-to/set-up-a-board.md`** and the search toggle — pointing at
   `[data-testid="include-put-away"]` in prose where a screenshot simply shows
   it.

Reference and explanation need screenshots least; do not add them for balance.

### D2 — Diagrams, where the reader is not looking at a screen

Mermaid, client-side, following `brokkr`'s precedent
(`additional-js = ["mermaid.min.js", "mermaid-init.js"]`) rather than adding an
`mdbook-mermaid` preprocessor — no new build dependency, and the book already
builds with nothing but mdBook.

- **`explanation/flight-levels.md`** — the three levels and what connects them.
  This page argues about a shape and currently describes it.
- **`explanation/archiving.md`** — the live → put away → restored lifecycle,
  with the refusals on the edges (read-only while away; `RESTORE_BLOCKED` when
  its home is gone). Six numbered rules are easier to hold as a picture.
- **`explanation/capabilities-and-access.md`** — how a board is resolved for an
  item, including a document reaching its parent through the `supports` edge.
  It is a lookup chain, which is exactly what prose is bad at.

### D3 — Staleness is accepted, and said out loud

**Dylan's call: accept stale.** No capture automation, no e2e-driven refresh.

That is a real trade and the page should not pretend otherwise: a screenshot of
a GUI that has moved on is a quiet lie, and this initiative spent its length
fixing exactly that class of problem in prose. So:

- Put a short note in `docs/src/images/README.md` recording how each shot was
  taken (URL, tenant, user, viewport) so a future recapture is mechanical
  rather than archaeological.
- Prefer shots of things that change slowly — overall layout, a banner, a badge
  — over shots of specific data or exact wording.
- **No screenshot carries information found nowhere else.** If the only place a
  fact appears is inside a PNG, it rots invisibly and fails **R4**. Every shot
  illustrates prose that stands without it.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [ ] Screenshots for the tutorial's two browser moments, an archived item
      page, and a marked put-away search hit.
- [ ] Mermaid diagrams on `flight-levels.md`, `archiving.md` and
      `capabilities-and-access.md`, rendering in the built book.
- [ ] Mermaid wired via `additional-js`, no new build dependency; `angreal docs
      build` clean and the published site renders the diagrams.
- [ ] Every image has alt text that carries the same information (the book must
      work for a reader who cannot see it).
- [ ] `docs/src/images/README.md` records how each shot was taken.
- [ ] No image is the only home for any fact.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.** Published and verified live.

### Diagrams (3)

Mermaid, client-side via `additional-js`, following `brokkr` — no
`mdbook-mermaid` preprocessor, so the book still builds with nothing but
mdBook, which is all `docs.yml` installs.

- `flight-levels.md` — the three levels with `parent` edges solid and
  `informs` dotted, and delivery boards coloured as team-owned. The page argues
  about a shape and previously described it.
- `archiving.md` — the `Live ⇄ PutAway` lifecycle. Two things the picture fixes:
  **there is no third state** (nothing deletes; the sweeper is unwired), and
  **restore is not the inverse of delete** (the delete edge cascades, the
  restore edge does not, and it can refuse outright).
- `capabilities-and-access.md` — board resolution as a flowchart, with the
  "no board → org admin only" fallback coloured distinctly. It is a lookup
  chain, which is what prose is worst at.

`mermaid-init.js` rewrites mdBook's `<pre><code class="language-mermaid">`
into `<pre class="mermaid">` (mermaid does not recognise the former) and picks
its light/dark theme from mdBook's own `<html>` class, so diagrams follow the
reader's theme rather than pinning one.

### Screenshots (3 used, 1 spare)

Captured by `e2e/tests/capture-docs-images.spec.ts` — `@docs`-tagged and
excluded from CI. Written as a Playwright spec rather than a separate harness
because the e2e suite already knows how to do the hard part: a real in-browser
PKCE login against Dex.

- `boards-overview.png` and `platform-delivery-board.png` → the tutorial's two
  browser moments. The board shot earns its place twice over: it shows the
  **Support and Planned lanes** and the **repository chip on every card**, both
  of which the prose had to assert, and the second explains how a task created
  with `--repo` and no board knew where to go.
- `search-put-away-results.png` → `how-to/find-archived-work.md`. The most
  useful shot in the book: `DEMO-T-0011` with a gold **put away** badge beside
  a live result, under a caption reading *"2 on this page · 1 put away"*. The
  vocabulary problem this initiative kept hitting is a **visual** distinction
  in the product and was text in the book.

**The spec asserts the state before shooting**, and that caught a real mistake.
The put-away step waits for `.kairos-archived-badge` rather than a timeout —
because a screenshot of live-only results looks exactly like the feature
working. It failed: clicking the labelled `[data-testid]` wrapper left the
toggle off. The API was correct all along (verified independently via
`kairos search --include-deleted`), so a timeout-based capture would have
shipped a confidently wrong picture.

A second framing fix: the first successful shot put the marked row below the
fold, capturing the toggle and none of the point. The step now scrolls the
badge into view.

### Staleness, accepted and stated

`docs/src/images/README.md` records viewport, user, tenant and the exact state
each shot needs — including that the put-away one requires archiving
`DEMO-T-0011` first and restoring it after, which this run did. **The demo
tenant was left as it was found.**

It also records the two habits that keep accepted staleness honest: shoot what
changes slowly, and **never let a screenshot be the only home for a fact** —
every image illustrates prose that stands without it, so a stale one is out of
date rather than the sole source of something wrong. A book whose facts live
inside PNGs rots invisibly, which is the failure mode this whole initiative
was about.

### Verified published

Docs run green. `images/boards-overview.png` and
`images/search-put-away-results.png` return 200; both fingerprinted mermaid
assets (`theme/mermaid-a43bc1af.min.js`, `theme/mermaid-init-52696e91.js`)
return 200. Worth noting the trap: mdBook **fingerprints `additional-js`
filenames**, so checking `theme/mermaid.min.js` 404s and looks like a broken
build when nothing is wrong.