# Screenshots

Captured by `e2e/tests/capture-docs-images.spec.ts`, which is `@docs`-tagged
and **excluded from CI**. To recapture:

```sh
angreal services up
angreal db migrate && angreal db seed      # the `demo` tenant these shots show
angreal web build
angreal dev serve                          # in another terminal

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

The last one needs a put-away item that matches the query. The run that
produced it archived `DEMO-T-0011` first and restored it afterwards:

```sh
kairos tasks delete DEMO-T-0011 --confirm   # before
kairos tasks restore DEMO-T-0011            # after
```

## These go stale, deliberately

There is no automated recapture (KAIROS-T-0185, Dylan's call). Two habits keep
that honest:

- **Shoot things that change slowly** — layout, a banner, a badge — rather than
  specific data or exact wording.
- **No screenshot is the only home for a fact.** Every one illustrates prose
  that stands without it, so a stale image is out of date rather than the sole
  source of something wrong. A book whose facts live inside PNGs rots
  invisibly.
