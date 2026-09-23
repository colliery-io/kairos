---
id: close-out-the-readme-becomes-a
level: task
title: "Close out: the README becomes a landing page and every page passes diataxis-review"
short_code: "KAIROS-T-0175"
created_at: 2026-09-23T22:11:40.199306+00:00
updated_at: 2026-09-23T22:11:40.199306+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0168, KAIROS-T-0169, KAIROS-T-0170, KAIROS-T-0171, KAIROS-T-0172, KAIROS-T-0173, KAIROS-T-0174]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] README is a landing page plus contributor quickstart, ~80 lines.
- [ ] All nine original README sections accounted for; none dropped.
- [ ] `diataxis-review` run over every page, rule IDs cited, findings fixed
      or recorded.
- [ ] Every cross-link in the repo resolves.
- [ ] The book builds and the published site loads.
- [ ] Release pipeline unchanged.

## Status Updates

*To be added during implementation*
