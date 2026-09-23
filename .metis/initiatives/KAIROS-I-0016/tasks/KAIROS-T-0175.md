---
id: close-out-the-readme-becomes-a
level: task
title: "Close out: the README becomes a landing page and every page passes diataxis-review"
short_code: "KAIROS-T-0175"
created_at: 2026-09-23T22:11:40.199306+00:00
updated_at: 2026-09-23T23:30:55.055428+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0168, KAIROS-T-0169, KAIROS-T-0170, KAIROS-T-0171, KAIROS-T-0172, KAIROS-T-0173, KAIROS-T-0174, KAIROS-T-0176, KAIROS-T-0179]
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

Reduce the README to a landing page, review the whole book against
[[KAIROS-S-0008]], and confirm it builds and publishes.

## Implementation Notes

**Blocked by every other task in [[KAIROS-I-0016]].**

### The README (D9)

Roughly 80 lines, keeping two things and moving everything else:

- what Kairos is, in a few sentences, and a link to the book;
- how to install the CLI (the v0.1.0 release URLs — now pointing at
  `colliery-io`, fixed during the release);
- a contributor quickstart: clone, `angreal services up`, `angreal test all`.

Everything user-facing moves. Use [[KAIROS-I-0016]] D3's inventory as the
checklist and confirm **every** one of the nine original sections has landed
somewhere — nothing is deleted without a destination.

The README is the one page where a second reading moment is accepted
deliberately: a repo landing page has two unavoidable audiences, someone
evaluating the product and someone about to build it. Note that in a comment
so a later `diataxis-review` does not read it as an oversight.

Contributor docs stay where they are (D6) — `uat/README.md`,
`e2e/README.md`, `docs/gui-conventions.md`, `plugin/README.md` are untouched
and unreferenced by the book. The README's contributor quickstart is what
keeps them reachable.

### The review pass

Run `diataxis-review` over **every** page, citing rule IDs. The skill and
the spec both already exist; a Diátaxis book shipped from the repo that owns
the reviewer, without having run it, would be an odd artifact.

Expect real findings rather than a rubber stamp — especially mode-mixing at
the seams, where a how-to drifts into explaining and a reference page starts
instructing. Fix them; where a rule genuinely does not fit (generated
reference against R4 is the likely one), record it for the spec rather than
bending the page.

### Final checks

- Every cross-link resolves; no links to moved or deleted README anchors
  anywhere in the repo (`deploy/helm/kairos/README.md`, `uat/README.md` and
  `plugin/README.md` may all point at README sections).
- `angreal docs build` clean.
- The published site is live and navigable, verified by loading it.
- The release workflow is still untouched.

## Acceptance Criteria

## Acceptance Criteria

- [ ] README is a landing page plus contributor quickstart, ~80 lines.
- [ ] All nine original README sections accounted for; none dropped.
- [ ] `diataxis-review` run over every page, rule IDs cited, findings fixed
      or recorded.
- [ ] Every cross-link in the repo resolves.
- [ ] The book builds and the published site loads.
- [ ] Release pipeline unchanged.

## Status Updates

*To be added during implementation*

## Notes carried in from other tasks

**2026-09-23, from [[KAIROS-T-0169]] — a review that must be re-run.** Its
delegated `diataxis-review` did not return, and the agent said so plainly
rather than claiming it passed. `reference/mcp-tools.md`, `glossary.md`,
`events.md` and `scim.md` have had fact-level scrutiny but **less mode-level
scrutiny than the other pages**. Re-run `diataxis-review` over those four
specifically; treat the rest of the book's reviews as done.

**From [[KAIROS-T-0168]]:** the README's CLI section is now **the stale copy** —
it predates `restore`, `--include-deleted`, `repos`, `service-accounts`,
`keys` and `tasks move`. Reducing the README to a pointer (D9) fixes this by
construction, but verify rather than assume.

**From [[KAIROS-T-0170]]:** `SUMMARY.md` is **not** generated. If a future
endpoint group appears, `angreal docs api` writes the page and no sidebar
entry, and `--check` will not catch it. Worth stating in the link check.

**From [[KAIROS-T-0171]]:** one cosmetic **S3** finding was deliberately
declined and left here — sidebar entries are bare noun phrases while the H1s
carry the mode signal. The spine wording is the initiative's D4 design and
`SUMMARY.md` was shared with four concurrent tasks, so churning it mid-flight
was the wrong trade. Decide it once, now that nothing else is editing the file.

**Ten "Related guides" links 404 by design** while how-to was unwritten
(recorded in T-0168/T-0169's Status Updates so they are not rediscovered as
defects). They should resolve once [[KAIROS-T-0172]] and [[KAIROS-T-0173]]
land — verify.

**Two tickets were filed outside this initiative** and are not this task's
work: [[KAIROS-T-0177]] (Compose drops the two variables Google Workspace
requires) and [[KAIROS-T-0178]] (MCP `create_item` lags the CLI).
## Status Updates

**2026-09-23 — closed out.** The book is live at
**https://colliery-io.github.io/kairos/** — 36 pages, all four modes, every
page returning 200.

### The README

571 lines → **114**. All nine original sections have destinations; nothing was
dropped. Three stale claims went with the trim, each of which would have
misled someone: the CLI section predated `restore`, `--include-deleted`,
`repos`, `service-accounts` and `tasks move`; a sentence pointed at
`kairos boards --help` "for the grant commands", of which there are none; and
the CI section still explained how to activate CI *"once a remote exists"*,
with steps to create the repository — written before this repo had a remote and
still there after v0.1.0 shipped from it.

Added what was never there: the amd64-only image caveat, beside the `helm
install` command where somebody about to run it on an ARM node will see it.

`introduction.md`'s links, deferred by [[KAIROS-T-0167]] until the pages
existed, are applied and the deferral comment removed.

### The independent review — 7 blocking, ~25 degrading, all fixed

Run by an agent that wrote none of the book, per the initiative's standing
finding. **The finding held exactly**: every blocking defect was a *missing
refusal* or an under-stated `details`/vocabulary — invisible from the prose,
visible only against the code.

Blocking, all fixed:

1. **`manage_*` silently grants `manage_members`.** Matching is textual, so the
   glob that reads as a work-item family also grants board administration —
   including granting other people's capabilities. Filed as [[KAIROS-T-0183]];
   verified independently by running the matcher's algorithm over the
   vocabulary.
2. `VALIDATION` is **400** on `POST /api/search`, not 422 — and the *generated*
   REST page already said 400, so the book contradicted itself.
3. `CONFLICT` is not only optimistic concurrency: three other 409s exist, and a
   client reading `details.current` unconditionally finds nothing.
4. `events.md` documented **six of nine** event values. The missing
   `item_restored` is the one whose own code comment warns that treating it as
   a create "would show a new card with an old version number".
5. The SCIM how-to said non-standard group names "are ignored". They are
   **refused 400** — an IdP pushing its whole catalogue fails per group.
6. Group `DELETE` omitted two permanent refusals, including that a team group
   will not delete while its board holds work.
7. "Errors are always the RFC 7644 envelope" was false four ways, including for
   the `location` URLs the discovery document itself advertises.

**One of the fixes was mine.** `scripts/render-openapi.py`'s `anchor()` folded
every non-alphanumeric to a hyphen, emitting `#listenvelope-strategy` where
mdBook renders `id="listenvelope_strategy"` — so **14 schema links in the
generated pages were dead anchors**, every generic-envelope link scrolling to
the top of the page. Fixed in the generator and regenerated, not hand-edited,
so the CI drift check still holds.

### Filed rather than papered over

- [[KAIROS-T-0183]] — `manage_*` grants board administration
- [[KAIROS-T-0184]] — SCIM: `userName` immutability breaks every PUT an
  Okta-shaped IdP sends; a soft-deleted team **permanently burns its slug**
  (the same plain-`UNIQUE`-under-soft-delete landmine as [[KAIROS-T-0161]], and
  worth auditing in one pass); a case-sensitive no-path PATCH silently swallows
  a deprovision; a `userName` filter searches `external_id`
- `crates/kairos-client/src/types_events.rs` — the `ThinEvent.event` doc
  comment listed 7 of 9 values. **Fixed here**, since it is what an SDK
  consumer reads.

One reported item I am **not** acting on: the review flagged
`discovery.rs`'s `documentationUri` pointing at `github.com/colliery-io/kairos`
as "the colliery naming the project is dropping". That URL is correct —
`colliery-io` is the GitHub organisation, and v0.1.0 published to it.

### Two things the reviewer could not verify, and said so

- **The Okta and Entra ID specifics** in the SCIM how-to are claims about
  third-party defaults, not about Kairos, and no code confirms them. The
  *mechanisms* they rely on were verified (the email-shaped-`userName` fallback
  and the `"True"`/`"False"` coercion, which is itself an Entra-shaped
  accommodation). Someone with an Okta or Entra tenant should confirm the
  vendor details.
- **The tutorial's copied outputs** were not re-run. Everything checkable
  around them holds: `payments-api` is a seeded platform-owned repo, alice is a
  Platform member and org admin, the seeded Backlog holds exactly the two named
  tasks, and every flag and format string used exists.

### Gates

- `angreal docs build` clean; **zero broken file links and zero broken anchors**
  across all 36 pages after the generator fix.
- `angreal test lint` clean; `actionlint` clean on all workflows.
- `scripts/render-openapi.py --check` passes — worth noting because another
  agent edited `api/openapi.rs` doc comments, which feed the spec, and a drift
  there would have failed the CI check added by [[KAIROS-T-0170]].
- `release.yml` untouched, as promised.
- Docs publish run **success**; site and pages in all four modes return 200.

### Known gaps, tracked not hidden

- **One tutorial** for the product. The Kubernetes lesson is
  [[KAIROS-T-0181]], blocked on [[KAIROS-T-0180]].
- **No how-to for consuming `/ws/events`** — a public push surface with
  reference only. A legitimate S1 gap; the reviewer declined to invent the page
  and so do I.
