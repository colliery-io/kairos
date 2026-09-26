---
id: close-out-local-auth-the-adr
level: task
title: "Close out local auth: the ADR amendment, the book, a journey, the drift gate"
short_code: "KAIROS-T-0206"
created_at: 2026-09-26T12:45:31.362635+00:00
updated_at: 2026-09-26T12:45:31.362635+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0200, KAIROS-T-0201, KAIROS-T-0202, KAIROS-T-0203, KAIROS-T-0204, KAIROS-T-0205]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0018
---

## Parent Initiative

[[KAIROS-I-0018]]

## Objective

Close the initiative honestly: amend the ADR that said none of this would happen,
teach the book when to use which login, exercise it as a persona would, and make the
drift gate count the new surface.

## Dependencies

[[KAIROS-T-0200]] through [[KAIROS-T-0205]] — the whole feature has to exist before
it can be described truthfully.

## Implementation Notes

### The ADR amendment is the important artefact

[[KAIROS-A-0016]] decided Kairos ships no identity provider and requires a BYO OIDC
issuer. This initiative does not overturn that; it **amends an absolute into a
default**, which is exactly what [[KAIROS-A-0021]] rule 2 did to A-0013's "bring your
own PostgreSQL".

Write it in that style, inline, and make it say:

- what is unchanged — enterprise deployments still bring an issuer, Kairos still owns
  no identity for them, SCIM is still how lifecycle arrives;
- what changed and why the original reasoning did not survive — A-0016 *named* this
  gap ("no turnkey identity for evaluators without any IdP") and proposed a
  documented Dex quickstart as the mitigation; that mitigation was not enough;
- what Kairos now owns that it deliberately did not before: **password storage, and
  the security questions that come with it.** Say it plainly. That is the cost of
  the decision and a future reader deserves to see it acknowledged rather than
  discovered.

### The book

Per [[KAIROS-S-0008]], each mode earns its page:

- **Explanation** — *choosing how people log in*. The one that matters: three options
  (bring an issuer, bundled Dex, local accounts), what each costs, and the honest
  advice that local accounts are for small teams and an IdP is right the moment
  there is a security team to answer to.
- **How-to** — turn local auth on, create a user, reset a password, recover when
  locked out.
- **Reference** — the new variables, `/api/login`, `/api/logout`, `/api/config`'s new
  field. The generated REST pages come from `scripts/render-openapi.py`; check whether
  the new endpoints land in an existing `GROUPS` entry and run `--check` so CI does
  not find the drift first.
- **Tutorial** — `run-kairos-locally.md` currently begins by standing up Dex. Local
  auth could make the first lesson shorter. Decide whether it should, and **record
  the decision either way** — [[KAIROS-T-0193]] set the precedent that "no tutorial
  change" is a legitimate outcome if it is written down.

### The journey

A UAT journey where an operator stands up a deployment with no IdP, bootstraps the
first admin, adds a colleague, and that colleague logs in with a password. That arc
is the whole initiative in one story, and it is the only test that proves the pieces
compose.

### The drift gate

`uat/README.md` documents a surface drift gate over MCP tools and CLI nouns. New CLI
verbs from [[KAIROS-T-0204]] move its denominator; make the numbers agree and record
what it read before and after.

### Expect to find defects here

[[KAIROS-I-0016]] found nine by writing the documentation, and [[KAIROS-T-0193]]
found a sweep bug no smaller test could have. If a page cannot be written honestly,
**file the defect and say so in the Status Updates** — that is the mechanism working.

## Acceptance Criteria

- [ ] [[KAIROS-A-0016]] amended inline, in A-0021's style, naming what Kairos now owns
- [ ] Explanation, how-to and reference pages, added to `SUMMARY.md`
- [ ] The REST reference regenerated and `--check` passing
- [ ] The tutorial decision made and recorded, either way
- [ ] A UAT journey covering bootstrap → add a user → that user logs in
- [ ] The drift gate reads complete, with before/after numbers recorded
- [ ] Any defect found while writing is filed and named in the Status Updates
- [ ] `angreal docs build` green and the full ladder green

## Status Updates

*To be added during implementation*
