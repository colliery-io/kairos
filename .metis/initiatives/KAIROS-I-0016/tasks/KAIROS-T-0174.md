---
id: tutorials-run-kairos-locally-and
level: task
title: "Tutorials: run Kairos locally, and deploy it to Kubernetes"
short_code: "KAIROS-T-0174"
created_at: 2026-09-23T22:11:36.901564+00:00
updated_at: 2026-09-23T22:11:36.901564+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
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

The two tutorials — the mode Kairos has never had. Today there is no path
for someone who has not used it before.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Tutorial mode is the strictest contract in
S-0008 and the easiest to get wrong. T1–T6, and in particular:

- **T2**: every step is a concrete action, and **no step offers a choice**.
  No "if you prefer", no alternatives, no "you could also". The author
  decides.
- **T3**: the learner sees a result early and often — show the expected
  output so they can self-check.
- **T4**: explanation is minimal. Link out; do not teach here.
- **T6**: repeatable. No step depending on environment specifics or timing
  the tutorial does not pin down.

The obligation is total: the author takes responsibility for the learner's
success, and every step must work every time for every learner. Which means
**both tutorials must be executed start to finish on a clean machine state
before this task closes** — not reasoned about.

### `tutorials/run-kairos-locally.md`

From nothing to a board with the learner's own first piece of work on it,
using the compose stack. The "does this thing do anything?" lesson.

Roughly: start the services, seed the demo tenant, log in, look at a board,
create one task, move it a column. Ends with something visibly theirs.

Pin the versions and commands; `angreal services up`, `angreal db migrate`,
`angreal db seed` are the real path. Avoid the repo-contributor framing —
the learner here wants to see Kairos, not to build it.

### `tutorials/deploy-to-kubernetes.md`

From nothing to a running deployment, using the **published** v0.1.0 chart
and image rather than a checkout:

```
helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.1.0
```

The hard part is honest prerequisites. The chart bundles neither Postgres
nor an IdP (A-0016), and `config.oidc.issuerUrl` / `audience` are required.
T5 says a tutorial may assume only *stated* prerequisites — so state them,
and pin an exact minimal way to satisfy them (a throwaway Postgres and a
Dex, both pinned) rather than saying "bring your own", which is a choice and
would violate T2.

If that makes the lesson too long, the right answer is to narrow the
promised outcome, not to hand the learner a decision.

## Acceptance Criteria

- [ ] Both tutorials exist and state their outcome up front.
- [ ] **Both executed end to end on clean state**, and the transcript or
      result recorded in the Status Update. Not reasoned about.
- [ ] No step offers a choice (T2); every step shows what the learner should
      see (T3).
- [ ] Versions and commands pinned (T6); prerequisites stated (T5).
- [ ] `diataxis-review` passes; T1–T6 cited individually per tutorial.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
