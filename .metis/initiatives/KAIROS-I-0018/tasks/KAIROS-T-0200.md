---
id: the-chart-bundles-an-optional-dex
level: task
title: "The chart bundles an optional Dex, the way it bundles PostgreSQL"
short_code: "KAIROS-T-0200"
created_at: 2026-09-26T12:42:27.337713+00:00
updated_at: 2026-09-26T13:00:28.265200+00:00
parent: KAIROS-I-0018
blocked_by: []
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

Make `helm install kairos` with **no OIDC issuer named** bring up a working login,
by standing a Dex up inside the release — the same shape the chart already uses for
PostgreSQL.

No product code. This task is chart templates, values, documentation, and the CI
value sets that prove both switch positions render.

## Implementation Notes

### Copy the PostgreSQL precedent deliberately

[[KAIROS-T-0188]] did this for the database and its decisions transfer almost
unchanged. Read `deploy/helm/kairos/templates/postgresql.yaml`, the `postgresql:`
block in `values.yaml`, and the `kairos.postgresqlEnabled` / `kairos.validateDatabase`
helpers in `_helpers.tpl` before writing anything.

**`dex.enabled` is TRI-STATE, for the same reason `postgresql.enabled` is.** Unset
must mean *on unless the operator names an issuer*, so that every existing release —
all of which set `config.oidc.issuerUrl` — upgrades without an edit and keeps the
issuer it has. `true` alongside a named issuer is refused at render time, exactly as
naming both a bundled and an external database is.

### What the template needs

- A `Deployment` (one replica), a `Service`, and a `ConfigMap` holding Dex's config
  with the static users. The tutorial's `deploy-to-kubernetes.md` already stands Dex
  up by hand — its image pin and shape are the starting point, not a fresh design.
- The two OAuth clients Kairos needs, matching `.angreal/dex/config.yaml`: the public
  `kairos-web` client for the GUI's PKCE login and the `kairos-cli` client for the
  device grant. **Copy from that file**; it is the reference configuration and it
  works.
- `redirectURIs` has to be right or login fails at the issuer with a message that
  points at Dex rather than at the chart. It depends on the ingress host, so the
  helper that resolves it belongs beside `kairos.bundledDatabaseUrl`.
- The in-cluster issuer URL the server should use — `http://<release>-dex:5556/dex`
  — resolved by a helper so `config.oidc.issuerUrl` can be left empty.

### The thing that will bite

Dex's issuer URL must be reachable **and identical** from two places: the server
validating tokens in-cluster, and the browser being redirected to log in. An
in-cluster Service DNS name is not resolvable from a browser. This is the reason the
tutorial exposes Dex, and it is the one part of this task that is not a
copy-paste of the Postgres work — expect to need the ingress host for the browser-facing
issuer and to document that a bundled Dex needs an ingress.

### Say it is evaluation-only, everywhere

The bundled Postgres carries a **"Do not run it in production"** block in
`install-with-helm.md`, and the wording is the model: one replica, static users, a
password in your values file and your release history. A bundled IdP deserves the
same treatment or more, because the failure mode is worse than losing data.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `helm install` with neither `config.oidc.issuerUrl` nor `dex.enabled` set
      renders a Dex and points the server at it
- [x] `dex.enabled: true` **and** a named issuer is refused at render time, naming
      both values
- [x] An existing release that names an issuer upgrades with no values edit and gets
      no Dex
- [x] A new `deploy/helm/kairos/ci/bundled-dex-values.yaml`, and every existing CI
      value set still renders
- [x] `install-with-helm.md` covers it with the same evaluation-only bluntness the
      bundled database gets, including that it needs an ingress and why
- [x] `helm lint` clean; `angreal test lint` and the docs build green

## Status Updates

*To be added during implementation*
## Status Updates

### 2026-09-26 — done, with one deliberate deviation from AC 1

The chart stands a Dex up when no issuer is named. Tri-state `dex.enabled`,
`_helpers.tpl` gains `dexEnabled` / `dexName` / `dexIssuerUrl` / `publicBaseUrl` /
`validateDex`, and `templates/dex.yaml` is a ConfigMap + Service + Deployment.

### The deviation: it also requires an ingress and a password hash

AC 1 said `helm install` with neither value set should render a Dex. It does, but
**only once you also give it `ingress.enabled` and `dex.adminPasswordHash`** — so a
bare `helm install` with nothing at all fails with instructions rather than
succeeding.

Both are deliberate and I would not soften either:

- **The password hash cannot have a default.** A chart cannot bcrypt at render time,
  so a default would mean shipping a *known credential* in every Kairos install.
  Helm's `randAlphaNum` is not an option either: it re-randomises on every upgrade,
  so the password would silently change under the operator. Failing with the exact
  `htpasswd` command to run is the honest answer.
- **The ingress is not incidental** — see below.

The render failures name the value and what to do, in the style of the chart's
existing OIDC `required` messages.

### Why a bundled IdP needs an ingress and a bundled database does not

This was the part the task predicted would bite, and it did.

An OIDC issuer URL is not an address, it is an **identity**: Dex stamps it into every
token's `iss`, and the server rejects any token whose `iss` is not the issuer it was
configured with. So one string has to work from two places that see the cluster
differently — the browser being redirected, and the server validating afterwards.

`http://kairos-dex:5556/dex` cannot be that string, because a browser cannot resolve
it. So Dex is routed at `/dex` on the Kairos ingress and that public URL is used for
both sides, with the server hairpinning out and back. That works and is a further
reason this is evaluation-only. Documented as its own subsection rather than a
footnote, because an operator who does not understand it will try an in-cluster URL
and get a failure that points at Dex.

### Three bugs found while verifying, two of which would have shipped silently

- **`gt (len .Values.ingress.tls) 0` is always true.** `ingress.tls` is a *map* with
  an `enabled` key, not a list of TLS blocks, so `len` counted its keys — every
  deployment would have been told it was on `https`, including plain-HTTP ones. Found
  by rendering with and without TLS and comparing, rather than by reading. Now reads
  `.Values.ingress.tls.enabled`, with a comment naming the mistake so it is not
  reintroduced.
- **The `checksum/config` annotation included `dex.yaml` from inside `dex.yaml`** —
  infinite recursion, and Helm's error is fifteen repetitions of the same frame. It
  hashes the *inputs* now, which is also the more precise trigger.
- **`validateDex` reported the wrong problem.** With `dex.enabled=true` *and* an
  issuer named, the ingress check fired first, so an operator who had asked for two
  issuers was told to enable an ingress. The contradiction is checked first now.

One smaller thing: an early draft of `publicBaseUrl` referenced
`.Values.config.publicUrl`, which does not exist in this chart. Helm resolves a
missing key to nil rather than erroring, so it would have worked while documenting a
value nobody can set. Removed rather than added.

### Verified, each position rendered rather than reasoned about

| Configuration | Result |
|---|---|
| issuer named (existing release) | **0** Dex objects — upgrades keep their issuer, no values edit |
| no issuer, ingress, hash | Dex rendered; `OIDC_ISSUER_URL=http://host/dex`, `OIDC_AUDIENCE=kairos-web,kairos-cli` |
| `dex.enabled=true` + an issuer | refused: "that is two issuers" |
| bundled, no ingress | refused, naming `ingress.enabled=true` and why |
| bundled, no password hash | refused, with the `htpasswd` command |
| TLS on / off | issuer scheme follows it |

### Gates

`helm lint` clean; **all six CI value sets render**, including the new
`ci/bundled-dex-values.yaml`; `angreal test lint` and `angreal docs build` green. No
Rust changed, as the task intended.

### Not done here, on purpose

Nothing verifies this on a live cluster. [[KAIROS-T-0188]] took the bundled database
to a real `kind` cluster in both switch positions, and a bundled IdP deserves the
same — a login that completes end to end is the only proof that the issuer URL is
genuinely reachable from both sides. That belongs in [[KAIROS-T-0206]]'s close-out
with the UAT journey, and is named here so it is not mistaken for finished.