---
id: tutorials-run-kairos-locally-and
level: task
title: "Tutorials: run Kairos locally, and deploy it to Kubernetes"
short_code: "KAIROS-T-0174"
created_at: 2026-09-23T22:11:36.901564+00:00
updated_at: 2026-09-23T23:06:48.203296+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
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

**2026-09-23 — narrowed to one tutorial, and the second one found a product
defect.**

`tutorials/run-kairos-locally.md` is written and **executed end to end**.
`tutorials/deploy-to-kubernetes.md` is **not written**, deliberately: it cannot
be completed on this machine, and the reason is
[[KAIROS-T-0180]]. Deferred to [[KAIROS-T-0181]], blocked on it.

### The Kubernetes lesson could not be executed

Executing it is how the defect surfaced. A throwaway `kind` cluster, a
`postgres:16`, and a Dex all came up cleanly; `helm install` pulled the
published chart correctly from OCI. Then:

```
Failed to pull image "ghcr.io/colliery-io/kairos:0.1.0": rpc error: code = NotFound
desc = failed to pull and unpack image: no match for platform in manifest: not found
```

The v0.1.0 image is `linux/amd64` only, and an arm64 node has nothing to pull.
`docker pull --platform linux/amd64` works on the host, but `kind load
docker-image` does not rescue it — the kind node's own containerd is arm64 and
rejects the manifest before emulation is consulted. **There is no documentable
workaround**, which is what makes it a defect rather than a limitation.

Writing the lesson anyway was available and was rejected. T5 permits assuming
*stated* prerequisites, so "you need an amd64 cluster" would have been
spec-legal — but T1 promises a guaranteed result and T6 promises it works every
time, and a lesson nobody had completed once would claim both on no evidence.
A tutorial is the one mode where the author takes full responsibility for the
learner's success, so shipping an unrun one is the specific dishonesty this
mode forbids.

### The local tutorial needed a product change to be writable at all

Running Kairos locally meant setting **six environment variables** by hand.
That is not a choice (T2) but it is a step that fails opaquely when one value
is wrong, which is the same problem in practice — and every other workflow in
this repo has an angreal entry point. Added `.angreal/task_dev.py`:
**`angreal dev serve`**.

Building it found a second thing: the obvious `OIDC_AUDIENCE=kairos-web`
**breaks the CLI**, which presents a `kairos-cli` token and gets
`InvalidAudience`. `OIDC_AUDIENCE` is a comma-separated allow-list
(KAIROS-T-0055), so the dev server names all three of `kairos-web`,
`kairos-cli` and `kairos-svc`. A tutorial that used the GUI only would never
have caught it.

### Verified, step by step

Every step was run, and the output in the page is copied from the run, not
composed: `angreal services up`, `db migrate`, `db seed`, `web build`,
`dev serve` (healthz `ok`), `whoami` (alice / demo admin / Platform),
`tasks create --repo payments-api` (→ `DEMO-T-0013`), `boards list`,
`boards show <id>`, `tasks transition --to <column id>`, and the card confirmed
moved into Todo.

The **device-flow login is the one step I did not complete interactively** — it
needs a browser approval. I verified the CLI prints the URL and code and begins
polling, and quoted that real output; the flow end to end is covered by the
passing `crates/kairos-cli/tests/cli_live.rs`.

### Two CLI findings that shaped the page

- `tasks transition --to` takes a **column UUID**, not a name, and
  `boards show` takes a **board UUID**, not a slug — while MCP
  `transition_item` takes a column *name*. The same slug-vs-UUID inconsistency
  as [[KAIROS-T-0150]]. The tutorial works with it rather than around it:
  `boards show` prints each column's id inline, so the learner copies an id
  from output they have just seen, which satisfies T3.
- **Two staged failures were removed.** The first draft had the learner run
  `boards show platform-delivery`, see the 422, then correct it — and
  demonstrated `INVALID_TRANSITION` the same way. Both were instructive, and
  both violate T1/T6: a step that fails by design is a step that does not work.
  The slug limitation is now a plain instruction, and the refusal is a pointer
  to `reference/errors.md`.