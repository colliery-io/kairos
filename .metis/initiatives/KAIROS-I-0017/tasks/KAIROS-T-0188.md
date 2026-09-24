---
id: a-disableable-bundled-postgres-in
level: task
title: "A disableable bundled Postgres in the chart, and the A-0016 amendment"
short_code: "KAIROS-T-0188"
created_at: 2026-09-24T02:27:49.817246+00:00
updated_at: 2026-09-24T20:58:44.581817+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0187]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

- [x] `postgresql.enabled` bundles a Postgres carrying pgvector; disabled
      reproduces today's behaviour exactly
- [x] `helm install` with only the two OIDC values produces a working deployment
- [x] Setting both `database.url` and **explicit** `postgresql.enabled: true` is
      refused with a clear message; inheriting the default alongside a named
      database resolves silently and correctly, which is what keeps upgrades safe
- [x] The false README sentence is replaced, and the bundled database is labelled
      for evaluation in README, `values.yaml`, the Helm how-to and the
      configuration reference
- [x] [[KAIROS-A-0013]] and [[KAIROS-A-0016]] record the amendment and point at
      [[KAIROS-A-0021]]
- [x] `deploy/helm/kairos/ci` covers both switch positions
- [x] The remote embedding provider's API key follows the chart's existing-Secret
      pattern, as `DATABASE_URL` does
- [x] Verified on a real cluster in both positions, as v0.1.1 was

## Status Updates

*To be added during implementation*

## Status Updates

### 2026-09-24 — done

A bundled PostgreSQL in the chart, disableable, plus the amendments that stop the
documentation contradicting it.

### Templated, not a subchart

Deciding reason: rule 2 requires **pgvector**, and the usual community Postgres
charts ship an image without it. Depending on one would mean overriding its image
anyway — at which point the dependency buys little and costs a `helm dependency
update` in the release pipeline, a second chart's values surface to document, and
a supply-chain relationship for something that is three objects.

A StatefulSet rather than a Deployment, because this owns a volume and a
Deployment rolling a second pod onto the same PVC is a corruption the operator
gets to discover.

### The design decision the task did not settle: tri-state

The task recommended the bundle default to **on**. Implementing that literally
broke three of the five existing `ci/` values files, because they set
`database.url` and the guard refused "two databases".

That is not a test problem — **every release that already exists sets
`database.url`**, so a plain `enabled: true` default would have failed their next
`helm upgrade`. The task's own Risk Considerations anticipated this and its
wording pointed both ways at once: *treat an explicit `database.url` as evidence
the bundle is not wanted* **and** *fail loudly if both are set*.

Both, resolved by making `postgresql.enabled` tri-state:

| value | effect |
|---|---|
| *(unset, default)* | bundled — **unless** `database.url`/`existingSecret` is set |
| `true` | bundled, and naming an external database is an error |
| `false` | no bundle; exactly the pre-bundle chart |

Unset is what keeps upgrades safe: naming a database *is* the evidence, and it
needs no edit from anyone. Explicit `true` alongside an external database is
still refused, because that is someone asking for two rather than inheriting a
default. All five `ci/` files pass untouched.

### Verified on a real cluster, both positions

Fresh `kind` cluster, Dex from the tutorial, the published `0.1.1` image.

**Bundled** — nothing but the two OIDC values and a tenant:

```
dex-6d556d5f6d-pfbsk              1/1 Running
kairos-bundled-6d98786cd6-d9m47   1/1 Running
kairos-bundled-6d98786cd6-ldc4t   1/1 Running
kairos-bundled-postgresql-0       1/1 Running

pgvector available in the bundled image: vector 0.8.6
healthz: ok   readyz: ready   GUI: 200
```

**External** — `postgresql.enabled: false` against a separate Postgres:

```
healthz: ok   readyz: ready
statefulsets: kairos-bundled-postgresql   (only the other release's — no second database)
```

That last line is the one worth having: the disabled path stands nothing up.

Render-time behaviour, each checked directly: nothing set → bundled; `database.url`
set with `enabled` unset → **no** bundle; explicit `true` + `database.url` →
refused naming both; explicit `false` with no database → refused as before.

### The API key, moved here from KAIROS-T-0189

`embeddings.apiKey` / `embeddings.existingSecret`, handled exactly as
`database.url` is — inline or from a Secret you already keep — because it is the
same kind of thing and an operator who has learned one pattern should not have to
learn a second. Rendered only when configured: the local model needs no key, and
a local Ollama needs none either. `KAIROS_EMBED_PROVIDER` / `_URL` / `_MODEL` go
in the ConfigMap; the key never does.

### Documentation, and the sentence that was false

`deploy/helm/kairos/README.md` claimed *"There is deliberately no Postgres
subchart — a stateful dependency in the chart would contradict A-0016's 'state
and identity are the operator's' posture."* Replaced with what is true, including
the tri-state table and a plain statement that the bundle is for evaluation.

- **[[KAIROS-A-0013]]** carries the amendment: state is still the operator's, an
  absolute became a default.
- **[[KAIROS-A-0016]]** gets a note rather than an amendment, because the thing
  that softened is the *database* half of the posture the chart was quoting.
  Identity is unchanged and remains wholly external — worth saying explicitly, so
  a reader landing there does not conclude more was reversed than was.
- The Helm how-to gains "Trying it out without a database" and "Using a hosted
  embedding model".
- The configuration reference gains both value tables, and names the two env vars
  the chart deliberately does not surface (`KAIROS_EMBED_CACHE`,
  `KAIROS_EMBED_REFRESH_SECS`).

### Gates

`helm lint --strict` clean, all five `ci/` value sets render, `angreal docs build`
clean, REST reference current.