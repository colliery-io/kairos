---
id: close-out-local-auth-the-adr
level: task
title: "Close out local auth: the ADR amendment, the book, a journey, the drift gate"
short_code: "KAIROS-T-0206"
created_at: 2026-09-26T12:45:31.362635+00:00
updated_at: 2026-09-26T16:37:42.942416+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0200, KAIROS-T-0201, KAIROS-T-0202, KAIROS-T-0203, KAIROS-T-0204, KAIROS-T-0205]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] [[KAIROS-A-0016]] amended inline, in A-0021's style, naming what Kairos now owns
- [x] Explanation, how-to and reference pages, added to `SUMMARY.md`
- [x] The REST reference regenerated and `--check` passing
- [x] The tutorial decision made and recorded, either way
- [x] A UAT journey covering bootstrap → add a user → that user logs in — **partly**;
      the bootstrap half is covered by integration tests instead, with the reason
      recorded below and in the journey's own header
- [x] The drift gate reads complete, with before/after numbers recorded
- [x] Any defect found while writing is filed and named in the Status Updates
- [x] `angreal docs build` green and the full ladder green

## Status Updates

### 2026-09-26 — done

#### The ADR amendment

[[KAIROS-A-0016]] is amended inline, in the shape [[KAIROS-A-0021]] rule 2 used on
[[KAIROS-A-0013]]. **Its own review trigger fired** — it listed *"evaluator friction
data showing the no-bundled-IdP quickstart is a real adoption barrier"* — so the
amendment opens by saying the ADR predicted this and its mitigation was not enough. A
documented Dex quickstart is still a second system to stand up, understand and debug
before you have seen a board.

The amendment names, plainly, the five things Kairos now owns and deliberately did
not: a password database, a brute-force surface, an account-enumeration surface, a
credential lifecycle with no email subsystem, and a bootstrap credential. The Negative
consequence that said *"no turnkey identity for evaluators"* is struck through and
marked as the cost that came due. A new review trigger is added: a security defect in
this surface that the throttle, the uniform failure or the storage-layer revocation did
not prevent — because if keeping local accounts safe starts costing more than an IdP
integration would, the trade has inverted.

#### The book

- **Explanation** — `explanation/choosing-how-people-log-in.md`. A three-row table, then
  the honest advice: if your organisation has an IdP, use it, because every reason you
  have one is a reason not to keep a second set of credentials. Local accounts get a
  "what it is for" and a "what it costs" (no MFA, no offboarding hook, no reset email,
  no reload survival), and a section on **the line** — local accounts are right while one
  person can remember who should have access. It also names break-glass as a genuinely
  good reason to turn them on in an organisation that does have an IdP.
- **How-to** — `how-to/use-local-accounts.md`. Turn it on, create the first admin, create
  an organization, add a colleague, reset a password, end sessions, recover when nobody
  can sign in, turn it off again.
- **Reference** — `configuration.md` (done as each task landed) plus a new generated
  `reference/rest/signing-in.md`.
- All three are in `SUMMARY.md`.

Written in ASD-STE100 style where it is procedural — short sentences, active voice,
imperative steps, one instruction per sentence. [[KAIROS-T-0207]] adopts STE formally
and its ordering against this task was never settled; writing the how-to this way makes
that task a check rather than a rewrite.

#### The tutorial: no change, and why

`run-kairos-locally.md` keeps its Dex. Two reasons, recorded in the tutorial itself
under *A note on the identity provider* so a reader finds it where they would ask:

1. **It would be longer, not shorter.** The Dex costs nothing extra — it comes up in the
   same `angreal services up` that starts the database the reader needs anyway. Local
   auth would add a hash to generate and a first-boot admin to configure.
2. **The first lesson should teach the intended shape**, which is that an organisation
   brings its own issuer. Local accounts are the small-team exception, and a tutorial is
   the wrong place to teach an exception.

The page now links to the explanation for a reader who is here precisely because they
never want to touch an IdP. [[KAIROS-T-0193]]'s precedent applies: "no change" is a
legitimate outcome once it is written down.

#### The journey, and what it honestly does not cover

`uat/journeys/local-login.journey.ts` — nine steps, all green. alice checks the
deployment offers both paths, creates an account for a colleague who has never signed in
anywhere, is refused a short password, watches a wrong password and an unknown email
return identical 401s, sees the colleague sign in and reach five boards, lists their
sessions without seeing a token, resets the password and watches the old session die,
ends every session without changing the password, and logs them out for good.

**The bootstrap half of the arc is not in the journey, deliberately.** T-0206 asked for
an operator standing up a deployment with *no* IdP and bootstrapping the first admin.
The UAT tier runs against one long-lived compose deployment that has a Dex, and the
bootstrap is single-use on an **empty** database by design — a journey needing a second,
fresh deployment would be a harness change, not a journey. Those two steps are asserted
where they can be asserted properly, and the journey's header says so:

- `kairos-db/tests/local_auth.rs::the_bootstrap_admin_is_single_use`
- `kairos-server/tests/local_login.rs::a_deployment_with_no_issuer_still_lets_people_in`

The deployment under test runs local auth **alongside** its Dex, which is the additive
shape [[KAIROS-I-0018]] chose and the one an organisation with an IdP would actually
run.

#### The drift gate: the numbers did not move, and that is the finding

`MCP 20/20 tools, CLI 16/16 nouns, 0 allow-listed` — **before this initiative and
after it**, confirmed against the last four reports on disk.

The task expected T-0204's new verbs to move the denominator. They do not, because the
gate reads MCP `tools/list` and `kairos --help` nouns, and the new verbs are
`kairos-server` subcommands — the operator binary, which nothing counts. Rather than
leave a green gate implying coverage it does not measure, `uat/README.md` now has a
**What the gate does NOT see** section naming its three blind spots (REST endpoints,
operator subcommands, GUI screens) and where each is covered instead, and records that
extending it to operator subcommands would be a reasonable next step.

#### Defects found by writing the documentation

1. **The published REST reference was carrying an implementation note.** `login`'s
   rustdoc explained why the handler takes a whole `Request` instead of an extractor
   tuple — and `scripts/render-openapi.py` puts the rustdoc in front of an operator, so
   *"Takes the whole `Request` rather than an extractor tuple"* was on a user-facing
   page. Rewritten as a caller-facing description, with the reasoning moved to a `//`
   comment that says why it is not a doc comment.
2. **`uat/README.md` documented an `ALLOW` map that no longer existed.** It showed a
   `cli:adrs` entry; the code has `{}` and the gate reports 0 allow-listed. Corrected,
   keeping the entry as an illustration of the shape.
3. **The docs referenced operator commands that were documented nowhere.** Found during
   [[KAIROS-T-0204]] and fixed there: `drop-tenant` existed only in `main.rs`'s module
   docs, so "documented alongside the other destructive operator commands" had nothing
   to point at. `reference/cli.md` now has an **Operator subcommands** section.

None of the three was a code defect, and all three were invisible until something had
to be written truthfully — which is the mechanism [[KAIROS-I-0016]] and
[[KAIROS-T-0193]] set up working again.

#### The ladder

`angreal test lint`, `unit` (11 suites), `integration` (51 suites), `e2e` (19 specs),
`uat` (23 journeys + the drift gate), `web lint`, `web build --release`,
`angreal docs build`, and `render-openapi.py --check` are all green.