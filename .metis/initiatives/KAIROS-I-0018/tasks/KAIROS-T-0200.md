---
id: the-chart-bundles-an-optional-dex
level: task
title: "The chart bundles an optional Dex, the way it bundles PostgreSQL"
short_code: "KAIROS-T-0200"
created_at: 2026-09-26T12:42:27.337713+00:00
updated_at: 2026-09-26T12:42:27.337713+00:00
parent: KAIROS-I-0018
blocked_by: []
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

- [ ] `helm install` with neither `config.oidc.issuerUrl` nor `dex.enabled` set
      renders a Dex and points the server at it
- [ ] `dex.enabled: true` **and** a named issuer is refused at render time, naming
      both values
- [ ] An existing release that names an issuer upgrades with no values edit and gets
      no Dex
- [ ] A new `deploy/helm/kairos/ci/bundled-dex-values.yaml`, and every existing CI
      value set still renders
- [ ] `install-with-helm.md` covers it with the same evaluation-only bluntness the
      bundled database gets, including that it needs an ingress and why
- [ ] `helm lint` clean; `angreal test lint` and the docs build green

## Status Updates

*To be added during implementation*
