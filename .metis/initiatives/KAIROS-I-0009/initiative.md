---
id: git-forge-integration-branch-and
level: initiative
title: "Git Forge Integration - Branch and PR Links on Work Items and Team Rollups"
short_code: "KAIROS-I-0009"
created_at: 2026-09-01T12:58:02.328987+00:00
updated_at: 2026-09-02T02:10:16.261901+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/active"


exit_criteria_met: false
estimated_complexity: L
initiative_id: git-forge-integration-branch-and
---

# Git Forge Integration - Branch and PR Links on Work Items and Team Rollups Initiative

## Context **[REQUIRED]**

PO ask (2026-09-01): *"some form of integration to github/gitlab so that i can associate tickets with specific branches and pull/merge requests — that should probably work also for team landing pages."*

**Starting point: nothing exists.** No VCS integration of any kind — no webhooks, no branch/PR concept, no forge credentials anywhere in the codebase. The only thing already in our favour is that **Kairos short codes are an ideal match key**: `DEMO-T-0002` in a branch name or PR title is exactly the mechanism Jira (`ABC-123`), Linear (`ENG-123`), and Azure Boards (`AB#123`) use. The matching half is free; everything else is new.

**Survey findings that shaped this design** (external research, 2026-09-01):

- **No work tracker models a repo as a first-class entity.** Jira, Linear, and Azure Boards all treat a repo as *integration configuration plus links on the issue*, never a queryable object. Atlassian — with unlimited resources — declined to put a service catalog in Jira, building Compass separately and surfacing it as a single field. That is our permission slip to stay small.
- **Linear's shape is closest to Kairos already**: the team owns repos as configuration, the issue id lives in the branch name, and attribution falls out of the PR's repo. Multiple repos to one team is explicitly supported.
- **In-repo manifests rot.** Backstage's `catalog-info.yaml` fails because changing ownership requires a PR in every affected repo — the data most likely to change is the most expensive to change. What stays accurate is data that *hurts at the moment of use*, and attribution *derived* from branches/PRs rather than typed by hand.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- Work items show the branches and pull/merge requests that reference them, with live state (open/merged/closed/draft).
- Team landing pages roll the same data up: what is in flight across this team's work, across all its repos.
- Both GitHub and GitLab from day one (PO decision), behind one forge abstraction.
- Repos are configuration owned by an org admin, not a modelled entity — one team board naturally serves many repos.

**Non-Goals:**
- **No auto-transitions** (PO decision). PR state never moves cards: Kairos board columns are configurable per A-0002, so there is no universal "In Progress"/"Done" to target. Links only.
- No service/component catalog, ownership graph, dependency edges, or scorecards — that is a developer portal, and explicitly out of scope.
- No in-repo manifest file. Configuration lives in Kairos.
- No commit-level ingestion in v1 (branches + PRs only); no CI/build status; no smart-commit command syntax (`#close`, `#time`).
- No writes back to the forge (no commenting on PRs, no status checks).

## Architecture **[CONDITIONAL: Technically Complex Initiative]**

### Tenant routing — the load-bearing problem

A webhook arrives from GitHub/GitLab with **no tenant context**, and Kairos is multi-tenant with per-tenant schemas. The SCIM surface already solved exactly this (`crates/kairos-server/src/scim/auth.rs`): its token is `kairos_scim_<slug>_<secret>` so "traffic is tenant-scoped by the token, not by subdomain," and it mounts **outside** the OIDC auth → tenant stack (`app.rs`, alongside the deployment-admin router).

Webhooks copy that: the delivery URL carries the tenant and the connection —

```
POST /webhooks/{forge}/{tenant_slug}/{connection_id}
```

— mounted outside the auth→tenant stack, non-`/api` (so outside the S-0005 surface and the openapi route scanner, like `/healthz` and `/metrics`). Authenticity comes from the signature, not the URL.

### Secret handling — derive, never store

GitHub verifies with `X-Hub-Signature-256` (HMAC-SHA256 of the raw body); GitLab compares `X-Gitlab-Token` to a shared secret. **Both require the plaintext secret at verification time**, so the API-key trick (store only a hash) does not apply.

Rather than storing recoverable secrets, **derive** them: `secret = HMAC-SHA256(server_signing_key, "webhook:" || connection_id)`. The secret is shown once at connection creation (like an API key), recomputed on every delivery, and **never persisted**. Rotating a connection = new connection id. This needs one new deployment config value (`KAIROS_WEBHOOK_SIGNING_KEY`, A-0013 style) and stores nothing sensitive at rest.

Every failure mode — unknown tenant, unknown connection, bad signature — returns the same generic response, so the endpoint is not a tenant-enumeration oracle (the SCIM rule, `scim/auth.rs` module docs).

### Storage (tenant schema)

- **`forge_connections`** — `id`, `forge` (`github|gitlab`, `text_enum!`), `repo_full_name` (e.g. `acme/payments-api`), `repo_url`, optional `team_id` (nullable FK → `teams`, for landing-page attribution of repo-level activity), `created_by`, `created_at`, `updated_at`, `deleted_at`. UNIQUE on `(forge, repo_full_name)` where live.
- **`item_links`** — `id`, `item_id` (UUID, **no FK** — the shared UUID space convention, as `item_metadata` does), `connection_id` FK, `kind` (`branch|pull_request`, `text_enum!`), `external_id` (PR number or branch ref), `title`, `url`, `state` (`open|merged|closed|draft`, `text_enum!`), `author`, `forge_updated_at`, timestamps. UNIQUE `(connection_id, kind, external_id)` — the upsert key.

Item links are **derived data**: no `version`, no history rows, no optimistic concurrency (A-0004 does not apply — Kairos is not the author).

### Ingestion pipeline

1. Verify signature → resolve connection + tenant (constant-time compare).
2. Normalize the forge payload into one internal `ForgeEvent` (kind, external id, title, url, state, author, updated-at, candidate text) — the `forge` abstraction lives here, in `kairos-core` as a pure normalizer with per-forge parsers, so payload shapes are unit-testable without a network.
3. Extract short codes with one regex over branch name + PR title + PR body: `[A-Z][A-Z0-9]*-[SITDA]-\d{4}`.
4. Resolve each code through `entity_directory` (live-only, per A-0001). **Unknown or foreign codes are ignored silently** — a branch may legitimately mention a code from another deployment; that is not an error.
5. Upsert `item_links` by `(connection_id, kind, external_id)`, guarded by `forge_updated_at` so **redelivered or out-of-order webhooks cannot regress state** (forges retry; ordering is not guaranteed).
6. Emit a thin `item_links_changed` WS event so open item/team views refresh through the existing T-0074 pattern.

### Read surfaces

- `GET /api/{family}/{code}/links` — the item's branches and PRs.
- `GET /api/teams/{id}/links` — the rollup: links whose item is a task with this `team_id`, or an item on the team's delivery board (reuse the KAIROS-T-0084 `team_work_documents` derivation shape exactly).
- Both open tenant-wide, like the other read surfaces.

### Deployment note

Webhooks require Kairos to be reachable from the forge — fine for the k8s deployment (A-0013 ingress), and localhost dev needs a tunnel. Document this; do not try to solve it in-product.

## Alternatives Considered **[REQUIRED]**

- **Polling the forge API** — works behind a firewall but needs stored forge credentials, rate-limit handling, and is only as fresh as the interval. Rejected by PO decision in favour of webhooks; revisit only if a deployment cannot expose an ingress.
- **CI/agent pushes links in** — no inbound exposure and no forge credentials, but nothing updates when a PR merges unless the pipeline says so, and coverage depends on every repo wiring it up. Rejected as the primary mechanism; the ingestion endpoint could later accept authenticated pushes in the same shape if a firewalled deployment needs it.
- **Auto-transitioning cards on PR events** (Linear's model) — rejected by PO decision: configurable board columns mean no universal target column, and a failed/illegal transition is worse than none.
- **A modelled Service/Repository entity** with ownership and dependencies — rejected: it is a developer portal, and the survey's clearest lesson is that work trackers that stayed out of the catalog business were right to.
- **Storing webhook secrets encrypted at rest** — rejected in favour of deriving them from a server key, which stores nothing sensitive and makes rotation a connection-id change.

## Implementation Plan **[REQUIRED]**

Decomposed 2026-09-01 (PO decisions folded in: webhooks, links-only, both forges):

1. **KAIROS-T-0097** — Schema + connection management: `forge_connections`, `item_links`, models, org-admin CRUD, derived-secret generation, the setup surface that hands back a delivery URL + secret.
2. **KAIROS-T-0098** — Forge payload normalization in kairos-core: GitHub + GitLab parsers → one `ForgeEvent`, short-code extraction, unit-tested against captured fixture payloads (no network).
3. **KAIROS-T-0099** — The webhook endpoint: routing, signature verification, tenant resolution, the idempotent/ordering-safe upsert, silent-ignore semantics, thin WS event.
4. **KAIROS-T-0100** — Read APIs + item Development panel: `/links` endpoints, client DTOs, the item-detail panel with state chips.
5. **KAIROS-T-0101** — Team landing-page rollup: the derived team query and its panel.
6. **KAIROS-T-0102** — e2e + fixtures + docs: seeded links, a spec driving synthetic webhook deliveries end to end, setup documentation naming the reachability requirement.