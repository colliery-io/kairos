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
  - "#phase/completed"


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

- [x] All five guides exist under `## For operators`.
- [x] Each addresses a goal, not a feature, and includes the real edges
      (required values, unsupported IdPs, unwired retention).
- [x] No guide teaches; conceptual detours link to `explanation/`.
- [x] The Helm guide uses the published OCI chart, verified against v0.1.0.
- [x] `diataxis-review` passes on each; H-rule IDs cited per page.
- [x] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

### 2026-09-23 — five operator guides written, reviewed and revised

All five pages are in `docs/src/how-to/` and uncommented under
`## For operators`. `angreal docs build` is clean.

**Verified against the published artifacts, not against the chart source.**
`helm show chart oci://ghcr.io/colliery-io/charts/kairos --version 0.1.0` pulls
`appVersion: 0.1.0`, and every refusal quoted in the Helm guide came out of a
real `helm template` against that pulled chart:

- neither tenancy value → `config.tenancy: set EXACTLY ONE of baseDomain … —
  neither is set`
- both → the same message ending `— both are set`
- no issuer → `config.oidc.issuerUrl is required (KAIROS-A-0016: bring your own
  OIDC issuer)`
- no database → `database: set EITHER database.url (inline) OR
  database.existingSecret`

**Two product facts found while writing, both now in the guides.** Boot applies
**public** migrations only (`crates/kairos-server/src/main.rs`) and `/readyz`
checks only the public schema (`crates/kairos-server/src/metrics.rs`), so a
release carrying a tenant migration comes up reporting *ready* with every
`org_*` schema behind, silently. `kairos-server migrate-tenants` is now a
mandatory post-upgrade and post-restore step in both the Helm and the
backup guides. Second: `kairos-server create-tenant` seeds no org-admin
membership, unlike `POST /api/admin/tenants`, so the provisioning guide steers
it to dev stacks only.

#### `diataxis-review` — per page, with rule IDs

Reviewed against `plugin/references/diataxis.md` §2.2 (H1–H6), §4 and §5. All
five declared How-to by location, title and opening; all five actually How-to
dominant. Findings were real — the systematic one was §4.2, a how-to that
teaches — and all page-level findings are fixed in this commit.

- **`install-with-helm.md`** — H1 ✓ H2 ✓ H3 ✓ (six steps reach a serving
  deployment) H4 ✓✓ (the tenancy fork table plus "apply whichever of these
  describe your deployment"; the reviewer called this the H4 ideal) H6 ✓.
  **Fixed H5/S4:** a `## Where things are` section restated
  `reference/configuration.md`'s probe and endpoint facts, and step 4 restated
  the startup-probe arithmetic — both removed, replaced by the operational
  consequence plus a link to `configuration.md#probes`. **Fixed H3 (minor):**
  wildcard mode required DNS and a certificate that no step created; step 1 now
  says to create them. **Fixed H5:** the upgrade section's forward-only-migration
  rationale cut to the instruction.
- **`configure-an-oidc-issuer.md`** — H2 ✓ H3 ✓ H4 ✓✓ (four per-issuer
  conditionals, plus the Dex 2.43.1 exact-match redirect-URI edge) H6 ✓.
  **Fixed S4/E6:** two sentences reproduced the `KAIROS_WEB_CLIENT_SECRET` and
  `KAIROS_API_BEARER` descriptions from the reference page linked two lines
  above; removed. **Fixed H5/H6:** `## What is not supported` addressed a
  contributor running the acceptance suite as much as an operator — cut to the
  two operational statements (no `client_credentials`; no password form means no
  `angreal test uat`), which the ticket asks for by name. **Fixed H1/S3:** page
  retitled to match its nav label. **Fixed H3 (cosmetic):** a sample line set
  `KAIROS_WEB_CLIENT_ID=kairos-web` under a comment saying not to set it.
- **`provision-a-tenant.md`** — H1 ✓ H2 ✓ H3 ✓ H4 ✓✓ H5 ✓ H6 ✓. The reviewer's
  cleanest page and the pattern the others were brought toward. **Fixed
  cosmetic S4:** the refusal table now cites `reference/errors.md` and reads as
  "do this" rather than "this means"; the slug pattern stays, because it is
  needed at the moment of choosing a slug. Retitled to match its nav label.
- **`connect-a-git-forge.md`** — H1 ✓ (retitled) H3 ✓ H4 ✓ H6 ✓. **Fixed
  H2/H5:** the signing-key prerequisite ended in four sentences of derivation
  rationale; cut to "do not rotate it casually — it invalidates every
  connection". **Fixed H5/§4.2:** `## What this does not do` was rationale and
  trade-off prose; rewritten as `## Limits to expect`, every bullet an
  operational consequence, the *why* linked to
  `explanation/flight-levels.md`. **Fixed S4:** registering a repository was a
  mid-task dead end into reference; the command is now inline in the
  prerequisites.
- **`back-up-and-restore.md`** — H1 ✓ H4 ✓ H6 ✓. Worst offender before revision
  (How-to 60% / Explanation 30%). **Fixed H2:** `## What is state, and what is
  not` stood between the reader and step 1; folded into the preconditions as a
  two-item checklist. **Fixed H5/§4.2/E6:** the retention section was theory —
  which scheduler is not called, what a soft delete is, which tables grow —
  rewritten as `## Size for unbounded growth`, keeping the operator's
  obligation and citing `configuration.md#retention--recognised-but-inert` and
  `explanation/archiving.md` for the rest. The honesty the ticket demands is
  intact: nothing prunes anything in 0.1.0, and configuring an archive target
  will not reclaim a byte. **Fixed H3:** the page had two sequences both
  numbered 1..n; the restore steps are now unnumbered headings under "in this
  order".

#### Tree-level, left open deliberately

- **S1 / S6** — no tutorial exists (`docs/src/tutorials/` empty, both entries
  still commented), which S1 ranks as the top-priority gap. Owned by the
  tutorials task. Reference also dominates 16 : 5 : 5 : 0; the how-to and
  explanation counts rise as the remaining tasks land.
- **S1, three empty explanation cells** — *install/deploy*, *identity/OIDC* and
  *schema-per-tenant tenancy* have how-to and reference but no explanation
  page, which is exactly where the relocated prose above wanted to go. It was
  cut rather than moved, because [[KAIROS-I-0016]] D4 fixes the explanation set
  at five. If the book later grows an explanation page, that material is the
  first candidate.
- **S4, forward links** — `reference/errors.md` (T-0176) and
  `reference/capabilities.md` (T-0176) are linked and do not exist yet. Listed
  for the close-out.

#### For the close-out

`README.md` line ~221 tells the reader to "See `kairos boards --help` for the
grant commands". There are none: `reference/cli.md` documents `boards list` and
`boards show` only, and board capability grants are API and GUI. The README
rewrite should drop that sentence rather than carry it forward.
