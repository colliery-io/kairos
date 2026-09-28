# Screenshots

Captured by `e2e/tests/capture-docs-images.spec.ts`, which is `@docs`-tagged
and **excluded from CI**. To recapture:

```sh
angreal docs images
```

The task starts the dev stack, makes a new demo seed, builds the GUI, starts a
server on :41080, runs the capture, and stops the stack (COLLIERY-T-0252). It
is destructive for the `demo` tenant of the dev database, as `angreal test e2e`
is. Port 41080 must be free. Then look at each image, and at the text near it
in the book, before you commit.

To run the capture by hand against a server that runs already:

```sh
cd e2e && npx playwright test capture-docs-images --grep @docs
```

| File | Page | State it needs |
|---|---|---|
| `boards-overview.png` | `tutorials/run-kairos-locally.md` | `/boards`, signed in as alice |
| `platform-delivery-board.png` | `tutorials/run-kairos-locally.md` | `/boards/platform-delivery` |
| `search-put-away-toggle.png` | (unused — kept as the toggle's default state) | `/search`, untouched |
| `search-put-away-results.png` | `how-to/find-archived-work.md` | `/search`, query `invoice`, toggle on, **and an archived task matching it** |

Viewport is 1440×900, signed in as `alice@kairos.test` / `alice-password`
against the `demo` tenant.

The last one needs a put-away item that matches the query. The capture
archives `DEMO-T-0011` after the image of the board, and restores it at the
end.

Two runs on one day give the same images. The search results show the date of
the seed in the column CREATED, so that image changes when the day changes.

## These go stale, deliberately

No gate makes the images again (KAIROS-T-0185, Dylan's call): a person runs
`angreal docs images` after a change of the demo seed or of the GUI. Two habits
keep that honest:

- **Shoot things that change slowly** — layout, a banner, a badge — rather than
  specific data or exact wording.
- **No screenshot is the only home for a fact.** Every one illustrates prose
  that stands without it, so a stale image is out of date rather than the sole
  source of something wrong. A book whose facts live inside PNGs rots
  invisibly.
