---
id: scaffold-the-mdbook-and-publish-it
level: task
title: "Scaffold the mdBook and publish it to Pages"
short_code: "KAIROS-T-0167"
created_at: 2026-09-23T22:11:06.422985+00:00
updated_at: 2026-09-23T22:27:15.769353+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0166]
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

Ship an empty but live book, so every later task has somewhere to land and
the publishing path is proven before there is content to lose.

## Implementation Notes

**Follow `colliery-io/squire-core` rather than inventing a layout** — it is
the most recent of the four sibling books and the cleanest.

```
docs/
├── book.toml
└── src/
    ├── SUMMARY.md
    ├── introduction.md
    ├── tutorials/
    ├── how-to/
    ├── reference/
    └── explanation/
```

### `book.toml`

Model on squire-core's: stock theme (it uses `default-theme = "rust"`,
`preferred-dark-theme = "ayu"`), `git-repository-url`,
`edit-url-template`, `[output.html.fold] enable = true`, search on. **No
custom theme** — `colliery-io/mdbook-colliery` is dormant (last touched
2026-03-14) and none of the four books reference it.

### `introduction.md`

States what the book covers and, per [[KAIROS-I-0016]] D6, what it does
**not**: contributor documentation is out of scope and lives beside the code
(`uat/README.md`, `e2e/README.md`, `docs/gui-conventions.md`,
`plugin/README.md`). Say so plainly rather than letting a reader conclude the
book is everything and the rest is missing.

It should also point the reader at the right mode — the four-way signpost is
the one place a Diátaxis book gets to explain its own shape.

### Publishing

`.github/workflows/docs.yml`, modelled on
`colliery-io/squire-core/.github/workflows/docs.yml`:

- mdBook `0.5.2`, installed by curl from the GitHub release (that repo's
  approach — no action dependency);
- `on: push: branches: [main], paths: ["docs/**", ".github/workflows/docs.yml"]`
  plus `workflow_dispatch`;
- `concurrency: group: docs-publish, cancel-in-progress: true`.

**Difference from squire-core**: Squire pushes the rendered site to a
separate public dist repo because its source is private. Kairos is already
public, so it publishes to its own Pages — use `actions/upload-pages-artifact`
+ `actions/deploy-pages` with `permissions: pages: write, id-token: write`,
which is simpler and needs no PAT. Pages must be enabled with source =
GitHub Actions.

Note the release pipeline is **not** touched: docs ship on `docs/**` pushes,
deliberately decoupled from tags. squire-core's workflow header says exactly
why.

### `angreal docs`

Angreal is how everything in this repo is run, so the book gets a task:
`angreal docs build` and `angreal docs serve`. Follow the existing task
modules in `.angreal/`.

## Acceptance Criteria

## Acceptance Criteria

- [ ] `docs/book.toml` + `src/` with `SUMMARY.md`, `introduction.md` and the
      four mode directories.
- [ ] `introduction.md` states what the book covers and that contributor
      docs are deliberately elsewhere.
- [ ] `angreal docs build` and `angreal docs serve` work.
- [ ] `.github/workflows/docs.yml` publishes to Pages on a `docs/**` push;
      verified by an actual run, not just by reading the YAML.
- [ ] The release workflow is unchanged.
- [ ] `actionlint` clean.

## Status Updates

*To be added during implementation*