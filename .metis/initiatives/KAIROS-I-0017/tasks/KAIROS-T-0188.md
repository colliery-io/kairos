---
id: a-disableable-bundled-postgres-in
level: task
title: "A disableable bundled Postgres in the chart, and the A-0016 amendment"
short_code: "KAIROS-T-0188"
created_at: 2026-09-24T02:27:49.817246+00:00
updated_at: 2026-09-24T02:27:49.817246+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0187]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0017
---

## Parent Initiative

[[KAIROS-I-0017]]

## Objective

The Helm chart gains an **optional, disableable** PostgreSQL, because
[[KAIROS-A-0021]] rule 2 requires pgvector and "bring your own Postgres, and it
must have an extension your provider may not have enabled" is a worse first
experience than a database in the chart.

And the chart's stated reason for having none becomes false, so the documentation
has to change with it.

## Implementation Notes

### Technical Approach

`deploy/helm/kairos/`:

- a Postgres dependency in `Chart.yaml`, guarded by `postgresql.enabled`
- when enabled, `database.url` is derived rather than required, so a minimal
  `values.yaml` needs only the two OIDC values
- when disabled, today's behaviour exactly: `database.url` or
  `database.existingSecret` required, and the chart refuses to render without one
- the image must carry pgvector either way

### The sentence to fix

`deploy/helm/kairos/README.md` line 22 currently reads:

> There is deliberately no Postgres subchart — a stateful dependency in the
> chart would contradict A-0016's "state and identity are the operator's"
> posture.

Rule 2 amends this. Replace it with what is now true: the posture is unchanged in
substance — state is still the operator's and the bundled database is a
convenience they can decline — but the earlier absolute is now a default. Say
plainly that the bundled Postgres is **for evaluation**, and that a production
deployment should disable it and point at a managed database, with the pgvector
requirement stated.

[[KAIROS-A-0016]] itself needs the amendment recorded — the ADR says identity is
external and A-0013 covers state; the amended claim is A-0013's absolute and
A-0016's quoted posture. Both places that a reader could land on need to point at
A-0021 rather than silently disagree with the chart.

### Default: on or off?

Recommendation: **on**, because the audience for `helm install` with no values
file is someone evaluating, and a default that cannot start is not a default. A
production deployment sets values anyway, and the README's first table entry can
be the switch.

### Dependencies

[[KAIROS-T-0187]] — the extension requirement has to exist before the chart can
promise to satisfy it.

### Risk Considerations

- A bundled stateful dependency invites someone to run it in production by
  accident. Mitigation is documentation, not mechanism: say it in the README, in
  `values.yaml` comments beside the switch, and in the how-to guide.
- `helm upgrade` on an existing release must not create a database beside the
  external one it is already using. Existing releases set `database.url`, so the
  chart should treat an explicit `database.url` as evidence the bundle is not
  wanted and fail loudly if both are set, rather than quietly preferring one.

## Acceptance Criteria

- [ ] `postgresql.enabled` bundles a Postgres carrying pgvector; disabled
      reproduces today's behaviour exactly
- [ ] `helm install` with only the two OIDC values produces a working deployment
- [ ] Setting both `database.url` and `postgresql.enabled` is refused with a
      clear message
- [ ] The false README sentence is replaced, and the bundled database is labelled
      for evaluation in README, `values.yaml` and the Helm how-to guide
- [ ] [[KAIROS-A-0013]] and [[KAIROS-A-0016]] record the amendment and point at
      [[KAIROS-A-0021]]
- [ ] `deploy/helm/kairos/ci` covers both switch positions
- [ ] The remote embedding provider's API key follows the chart's existing-Secret
      pattern, as `DATABASE_URL` does — moved here from [[KAIROS-T-0189]], which
      had no chart change to hang it on
- [ ] Verified on a real cluster in both positions, as v0.1.1 was

## Status Updates

*To be added during implementation*
