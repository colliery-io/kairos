---
id: how-to-for-operators-deploy
level: task
title: "How-to: for operators — deploy, configure identity, provision, connect a forge, back up"
short_code: "KAIROS-T-0172"
created_at: 2026-09-23T22:11:24.472546+00:00
updated_at: 2026-09-23T22:11:24.472546+00:00
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

Five how-to guides for operators — the audience with the most to get wrong
and the least margin for a guide that only half works.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** How-to mode: the reader is a competent
practitioner at work with a real goal. Address the goal, not the feature, and
include the messy real-world edges. A how-to does **not** teach — link to
explanation instead of digressing.

Grouped under `## For operators` in `SUMMARY.md` (audience is a second-level
grouping inside the mode — see [[KAIROS-I-0016]] D2).

### `how-to/install-with-helm.md`

The chart is published as of v0.1.0:
`helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.1.0`.
Source: `deploy/helm/kairos/README.md` and `values.yaml`. Note the real
edges: the chart bundles **neither** Postgres nor an identity provider by
design (A-0016), and `config.oidc.issuerUrl` / `audience` are required — the
chart refuses to render without them.

### `how-to/configure-an-oidc-issuer.md`

The goal is "point Kairos at my IdP", and the honest answer includes what
does **not** work: the deployment's IdP must offer a password grant for some
tooling, and IdPs without one are not supported by the current pass (see
`uat/README.md`'s note on Google Workspace). Dex is the reference.

### `how-to/provision-a-tenant.md`

`kairos orgs` / the tenant admin surface, and the schema-per-tenant model's
consequence: provisioning creates `org_<slug>` and runs tenant migrations.
Source: `angreal db create-tenant`, `crates/kairos-db/src/lib.rs`'s
`provision_tenant`.

### `how-to/connect-a-git-forge.md`

GitHub and GitLab: connection, webhook, and what arrives. Migrate
`README.md` lines 393–466 — that section is already how-to shaped, so this is
mostly relocation plus the edges it omits.

### `how-to/back-up-and-restore.md`

Postgres is the only state; the image is stateless. Say what to back up
(the database, including every `org_*` schema) and what not to. **Be honest
about retention**: the sweeper that would prune history is not wired into the
server, so nothing prunes today and backups grow unbounded. An operator
needs to know that.

## Acceptance Criteria

- [ ] All five guides exist under `## For operators`.
- [ ] Each addresses a goal, not a feature, and includes the real edges
      (required values, unsupported IdPs, unwired retention).
- [ ] No guide teaches; conceptual detours link to `explanation/`.
- [ ] The Helm guide uses the published OCI chart, verified against v0.1.0.
- [ ] `diataxis-review` passes on each; H-rule IDs cited per page.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
