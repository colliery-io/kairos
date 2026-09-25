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

- [ ] A `TextInput` with a label exposes that label as its accessible name
- [ ] `getByLabel('Slug')` resolves the input, and the KAIROS-T-0094 e2e
      workaround is replaced with it
- [ ] The other aurora form components are audited in the same pass, and any
      with the same gap are fixed or listed here
- [ ] Clicking the label focuses the input

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
