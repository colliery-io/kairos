---
id: form-inputs-are-unlabelled-for
level: task
title: "Form inputs are unlabelled for assistive technology: TextInput has no label association"
short_code: "KAIROS-T-0198"
created_at: 2026-09-25T02:22:28.709454+00:00
updated_at: 2026-09-25T02:22:28.709454+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# Form inputs are unlabelled for assistive technology: TextInput has no label association

## Objective

Associate every `TextInput` label with its input, so a screen reader announces
the field name and `getByLabel` works in tests. Today neither does.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P2 - Medium (every form in the GUI; no functional loss for sighted mouse users)

### Impact Assessment

- **Affected users**: anyone using a screen reader, and anyone navigating by
  voice. Also every future e2e test author, who will reach for `getByLabel` and
  find it does not work.
- **Where**: `colliery-io-aurora`'s `TextInput` (`src/components.rs`) renders

  ```html
  <div class="cl-field">
    <label class="cl-field__label">Slug</label>
    <input class="cl-input" type="text" ...>
  </div>
  ```

  The `<label>` has no `for`, the `<input>` has no `id`, and there is no
  `aria-label` or `aria-labelledby`. Nothing ties the two together, so the
  accessible name of every such input is **empty**.
- **Expected vs actual**: a screen reader should announce "Slug, edit text".
  It announces an unlabelled text field, and the visible label is read only as
  adjacent static text if at all.

### How it surfaced

[[KAIROS-T-0094]] added an e2e step that filled the page-create form with
`getByLabel('Slug')`. It timed out. The panel and its caption were found, and
`selectOption` on the sibling `<select>` worked — so the form was rendered and
visible, and the locator was the only thing wrong.

That test now locates fields through the `.cl-field` wrapper and its
`.cl-field__label` text, with a comment saying why. **That workaround is the
symptom, not the fix**: a test can navigate the DOM structurally, and a screen
reader cannot.

### This is an upstream component

`TextInput` lives in `colliery-io-aurora`, not in Kairos, so the fix belongs
there and arrives via a version bump. Worth checking the other form components in
the same pass — `Switch`, `Select` and anything else pairing a label with a
control are likely to have the same shape, since this one clearly was not caught
by anything.

The two honest options:

1. **`id` + `for`** — needs a unique id per instance, so the component has to
   generate one (Leptos has no built-in `useId`; a counter or a caller-supplied
   `id` prop both work). Correct, and makes the label clickable, which is a real
   usability gain beyond accessibility.
2. **Wrap the input in the label** — `<label>Slug <input/></label>` associates
   implicitly with no id at all. Smaller change; constrains the styling, since
   the label becomes the input's parent.

## Acceptance Criteria

- [x] A `TextInput` with a label exposes that label as its accessible name
- [~] `getByLabel('Slug')` resolves the input, and the KAIROS-T-0094 e2e
      workaround is replaced with it
- [x] The other aurora form components are audited in the same pass, and any
      with the same gap are fixed or listed here
- [x] Clicking the label focuses the input

## Status Updates

**2026-09-25 — filed while doing [[KAIROS-T-0094]].** Found by a test failing for
the right reason. Not fixed there because it is an upstream component and a
Kairos-side workaround would have hidden it — the GUI has a lot of forms, and this
affects all of them.

## Decision — 2026-09-25 (Dylan)

**Generated `id` + `for`, inside the component.**

`TextInput` mints a unique id per instance and the label points at it. Chosen over
wrapping the input in the label (which associates implicitly but makes the label the
layout parent, constraining the existing `.cl-field` CSS) and over a caller-supplied
`id` prop (which touches every call site and lets one be forgotten).

The deciding point is that only this option makes the label *clickable*, which is a
usability gain for every user rather than only for assistive technology — and it
leaves the markup shape alone, so no stylesheet has to change.

Implementation note: Leptos has no `useId`, so the component needs its own
counter. A process-wide `AtomicUsize` is sufficient — ids only have to be unique
within a document, and the CSR bundle is one document.

This is an upstream change in `colliery-io-aurora`, so it lands there, gets a
release, and reaches Kairos as a version bump. The audit of the other form
components (`Switch`, `Select`, anything else pairing a label with a control) goes
in the same pass — this one clearly was not caught by anything, so the others are
unlikely to be better.

## Status Updates

### 2026-09-25 — fixed upstream, verified downstream, waiting on a publish

`colliery-io-aurora` **0.3.0** is committed in `~/Desktop/aurora-dark`
(`9f43693`), unpushed and untagged. Publishing is CI-on-tag, so the tag push *is*
the publish — held for a decision, since crates.io versions can be yanked but never
removed.

### The audit found more than the ticket described

The ticket was about `TextInput`. Five components had the same defect and two had
worse ones:

| Component | Defect |
|---|---|
| `TextInput`, `Select`, `Textarea`, `NumberInput`, `PasswordInput` | `<label>` with no `for`, control with no `id` — **accessible name empty** |
| `Switch` | **Not a control at all**: a `<span>` with a click handler. No role, no state, not focusable, not keyboard-operable |
| `SegmentedControl` | Selected item conveyed by colour alone |
| `NumberInput` steppers | Unnamed ("▲" is not a name), and could submit a surrounding form |

`Switch` is the one that matters most and was not in the ticket. A keyboard user
could not toggle it and a screen reader saw two pieces of decorative text. It is a
`<button type="button" role="switch">` with `aria-checked` now, taking its
accessible name from the label inside it, with the CSS undoing the browser's button
styling (`font: inherit` included — a button does not inherit font-family) and a
`:focus-visible` ring, because a focusable thing with no focus indicator is its own
bug.

`field_id()` mints ids from a process-wide counter, per the decision above. Two unit
tests: uniqueness across 1000 draws, and valid HTML identifiers. The uniqueness test
guards the subtler failure — two fields sharing an id points a label at the *wrong*
control, which is worse than pointing at nothing because it looks correct.

### The thing I had wrong, and how the verification caught it

I read the defect in the registry's **0.2.0** source and wrote the ticket from it.
Kairos actually declares **0.1.0**. I only found out by patching Kairos at
`[patch.crates-io]` to the local 0.3.0, running e2e, and watching `getByLabel` fail
anyway — because `version = "0.1.0"` means `^0.1.0`, which does not match 0.3.0, so
cargo quietly ignored the patch and kept the registry build.

Had I trusted the reasoning instead of running it, I would have published 0.3.0,
switched Kairos's test to `getByLabel`, and watched CI fail for a reason with no
obvious connection to either change.

### Verified end to end

With the requirement bumped to `0.3.0` *and* the patch in place:

- `cargo check -p kairos-web --target wasm32-unknown-unknown` — clean, so the
  0.1 → 0.3 jump (which includes 0.2.0's `hlin` feature) breaks nothing in Kairos
- `angreal test e2e` — **17 passed**, with `teampages.spec` using
  `create.getByLabel('Slug')` rather than the `.cl-field` wrapper workaround

Both changes are reverted for now: Kairos is back on `0.1.0` with the wrapper, because
a tree declaring an unpublished version does not resolve and CI could not build it.

### The remaining two steps, in order

1. **Publish**: `cd ~/Desktop/aurora-dark && git push && git tag v0.3.0 && git push --tags`
   — `.github/workflows/publish.yml` does the rest.
2. **Then in Kairos**, one line and one test edit:
   - `Cargo.toml`: `colliery-io-aurora` `0.1.0` → `0.3.0`
   - `e2e/tests/teampages.spec.ts`: replace the `.cl-field` wrapper helper with
     `const field = (name: string) => create.getByLabel(name);` and drop the comment
     explaining why it could not be used

Both already exercised together, so this is re-applying a verified change rather
than attempting one.
