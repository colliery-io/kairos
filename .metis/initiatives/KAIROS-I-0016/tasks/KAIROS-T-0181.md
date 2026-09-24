---
id: tutorial-deploy-kairos-to
level: task
title: "Tutorial: deploy Kairos to Kubernetes, once the image runs on ARM"
short_code: "KAIROS-T-0181"
created_at: 2026-09-23T23:22:09.606448+00:00
updated_at: 2026-09-23T23:22:09.606448+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0180]
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

`tutorials/deploy-to-kubernetes.md` — from nothing to a running deployment,
using the published chart and image.

**Blocked by [[KAIROS-T-0180]].** Not deferred for effort: the lesson cannot
be completed on an ARM machine, so it cannot be written to the tutorial
contract.

## Why it was split out of [[KAIROS-T-0174]]

T-0174 required both tutorials to be **executed** before closing, because T2
(no step offers a choice) and T6 (repeatable, no dependence on environment
specifics) are not verifiable by reasoning about the steps. Executing this one
is how [[KAIROS-T-0180]] was found: the published v0.1.0 image is `linux/amd64`
only, and an arm64 `kind` node rejects the manifest before anything else
happens.

Writing the lesson anyway was available and was rejected. T5 does permit a
tutorial to assume *stated* prerequisites, so "you need an amd64 cluster" would
have been spec-legal — but T1 promises a guaranteed result and T6 promises it
works every time, and a lesson nobody had completed once would be claiming
both on no evidence. A tutorial is the one mode where the author takes full
responsibility for the learner's success.

## Implementation Notes

The route is already proven up to the image pull, and the working pieces are
worth keeping:

- A throwaway `kind` cluster, a `postgres:16` Deployment and Service, and a
  Dex Deployment with `staticClients` for `kairos-web` and one static
  password — all of which came up cleanly.
- `helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.1.0`
  pulls the chart correctly from the OCI registry.
- Minimum values: `database.url`, `config.oidc.issuerUrl`,
  `config.oidc.audience`. The chart refuses to render without the latter two,
  which is [[KAIROS-A-0016]] working as intended.

**Pin every version** — kind, the Postgres image, the Dex image and the chart.
T6 forbids depending on environment specifics the tutorial does not control,
and "whatever `postgres:latest` is today" is exactly that.

**Do not tell the learner to bring their own Postgres and IdP.** That is a
choice, which T2 forbids. Pin a throwaway of each, and say plainly that they
are throwaway — [[KAIROS-A-0016]]'s "state and identity are the operator's"
is the thing a how-to covers, not a tutorial.

If the full lesson runs long once the image works, **narrow the promised
outcome** rather than handing the learner decisions — stop at "the pods are
running and `/healthz` answers", and leave signing in to a how-to.

## Acceptance Criteria

- [ ] [[KAIROS-T-0180]] is fixed and the image runs on arm64.
- [ ] The tutorial exists and states its outcome up front.
- [ ] **Executed end to end on a clean cluster**, with the result recorded in
      the Status Update. Not reasoned about.
- [ ] No step offers a choice (T2); every step shows what the learner should
      see (T3); every version pinned (T6).
- [ ] `diataxis-review` passes, citing T1–T6 individually.
- [ ] `angreal docs build` clean; the `SUMMARY.md` line uncommented.

## Status Updates

*To be added during implementation*

**2026-09-24 — done.** Commit `c30c200`. Unblocked by fixing
[[KAIROS-T-0180]] rather than by working around it.

`docs/src/tutorials/deploy-to-kubernetes.md`, and the `SUMMARY.md` line that
carried a *"blocked on KAIROS-T-0180"* comment is uncommented.

### Written against a path that was walked, not reasoned about

Every step below was executed on a real `kind` cluster before the page was
written, which is what [[KAIROS-T-0174]] refused to do without: create the
cluster and namespace, a `postgres:16` Deployment and Service, a Dex Deployment
with `staticClients` and one static password, `helm install` from the chart, and
then the checks — pods `1/1 Running`, `/healthz` → `ok`, `/readyz` → `ready`,
the GUI serving 200.

The outputs quoted in the page are from that run.

### Contract decisions

- **Every version pinned** — kind, `postgres:16`, `ghcr.io/dexidp/dex:v2.41.1`,
  the chart at `0.1.1`. **T6** forbids depending on environment specifics the
  lesson does not control, and "whatever `postgres:latest` is today" is exactly
  that.
- **A throwaway Postgres and Dex are pinned rather than offered as a choice.**
  "Bring your own database and IdP" is a decision, and **T2** forbids handing
  the learner one. The page says plainly that neither is suitable for anything
  but a tutorial.
- **The promised outcome is narrowed to a running, reachable deployment** —
  `/healthz`, `/readyz`, the interface. Signing in as a real user with real
  work belongs to the how-to guides; stretching the lesson that far would have
  meant more setup than the outcome justifies.
- **The tenant is empty at the end, and the page says so**, rather than letting
  a reader expect the demo data from the local tutorial. An empty Kairos is the
  honest starting point for a real one.

### What the lesson teaches beyond the steps

`/healthz` and `/readyz` answer different questions, and the page says which to
gate traffic on — `/readyz`, because it is the one that checks the database.
That distinction costs nothing to explain here and is expensive to discover in
production.
