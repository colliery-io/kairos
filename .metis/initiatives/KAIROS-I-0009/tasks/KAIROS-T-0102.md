---
id: forge-integration-e2e-seeded
level: task
title: "Forge integration e2e + seeded fixtures + setup documentation"
short_code: "KAIROS-T-0102"
created_at: 2026-09-01T23:12:38.913970+00:00
updated_at: 2026-09-02T10:48:00.557298+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Forge integration e2e + seeded fixtures + setup documentation

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. Final wave; depends on KAIROS-T-0097..0101.

## Objective

Make the feature demonstrable and regression-proof: seeded demo links, an e2e spec that drives real signed webhook deliveries end to end, the fallout sweep, and the setup documentation an operator actually needs.

## Implementation Notes

- **Seed** (`crates/kairos-db/src/seed.rs`): one `forge_connections` row per demo team (`acme/payments-api` for platform, `acme/portal-web` for web — fictional but plausible), plus a handful of `item_links` against existing demo tasks: one open PR, one draft, one merged, and one branch, spread so both the item panel and the team rollup have content. Update `seed_demo` counts — the recurring lesson from every prior wave: **grep every count assertion at once**, they live in `crates/kairos-db/tests/seed_demo.rs` as one expectations table.
- **e2e `forge.spec.ts`**: the honest test is a real delivery, not seeded rows.
  1. Log in, create a forge connection through the admin API with a minted token (helper), capturing the returned webhook URL + secret.
  2. From the spec, POST a GitHub `pull_request opened` payload to that URL with a correctly computed `X-Hub-Signature-256` (Node's `crypto.createHmac` — this is why the secret is returned at creation).
  3. Assert the Development panel on the referenced item shows the PR as open.
  4. POST the merged payload; assert the chip flips to merged **without a page reload** (the WS path).
  5. Replay the earlier opened payload; assert it stays merged (the ordering guard, browser-visible).
  6. POST with a wrong signature; assert nothing changes.
  7. Check the team landing page "In flight" panel lists the open/draft PR and not the merged one.
- Put the signed-delivery helper in `e2e/helpers/` next to `mintToken`/`patchTeamPage`, so future forge specs reuse it.
- **Fallout sweep**: `tenant_provisioning` table/index counts (T-0097 already bumps them — verify nothing else drifted); the item-detail specs (`smoke`, `progress`, `lifecycle`) now have an extra panel on some items — check any assertion that counts panels or asserts absence; `seed_demo` counts; the openapi route-vs-spec gate (the webhook route is deliberately non-`/api`, so confirm the scanner ignores it rather than newly failing).
- **Docs** — the operator-facing gap, and the part most likely to be skipped:
  - `docs/` page (or the README section that fits the existing structure): how to connect a repo — create the connection, copy URL + secret into GitHub (*Settings → Webhooks*, content type JSON, events: pull requests + pushes) or GitLab (*Settings → Webhooks*, Merge request + Push events), and what "Kairos must be reachable from the forge" means in practice (k8s ingress fine; **localhost dev needs a tunnel** — name one, e.g. `cloudflared`/`ngrok`, without endorsing).
  - Document the branch-naming convention that makes this work (`DEMO-T-0002-short-slug` or a short code anywhere in the PR title/body) — this is the bit engineers must actually do, and nothing enforces it.
  - Note the rotation story and that the secret is shown once.
- Consider mentioning the convention in the bootstrap skill's report step so newly wired repos learn it — small, optional, record if skipped.

## Acceptance Criteria

## Acceptance Criteria

- [x] Seed provisions demo connections + links covering open/draft/merged/branch; `seed_demo` counts updated and asserted.
- [x] `forge.spec.ts` drives real signed deliveries: open → merged live over WS → replay stays merged → bad signature is a no-op → team rollup shows only in-flight work. Green without retries.
- [x] A reusable signed-delivery helper lives in `e2e/helpers/`.
- [x] Fallout swept: provisioning counts, item-detail specs, seed counts, openapi gate.
- [x] Setup docs cover both forges, the branch-naming convention, the reachability requirement (including the localhost caveat), and secret rotation.
- [x] Full ladder green: unit, integration, e2e.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
- 2026-09-02: COMPLETE. Seed: one connection per demo team (GitHub `acme/payments-api` → platform, GitLab `acme/portal-web` → web, both team-attributed) plus four links covering every state the panels render — open PR, branch, merged PR, draft. `SeedError::Forge` variant added. seed_demo asserts connections=2, both forges present, both attributed, links=4, three distinct states, one branch.
- 2026-09-02: e2e: `createForgeConnection` / `deliverGithubWebhook` / `githubPullRequest` helpers in `e2e/helpers/api.ts` (the delivery helper computes a real `X-Hub-Signature-256` with Node's `crypto.createHmac`, and rewrites the returned public `webhook_url` to the local test server's path). `forge.spec.ts` walks: seeded links on the item → register a repo → signed "opened" delivery appears with NO reload → merge flips the chip live → **replay of the earlier open leaves it merged** (the ordering guard, proven in a browser) → a wrongly signed delivery is 401 and changes nothing → the team In flight panel shows open work and hides the merged PR. `.angreal/task_test.py` now sets `KAIROS_PUBLIC_URL` + `KAIROS_WEBHOOK_SIGNING_KEY` for the GUI leg.
- 2026-09-02: **Harness bug found and fixed — worth more than the feature work.** The first full run showed 3 failures (forge, team-lens, teampages) that looked like real regressions. Root cause: a stray dev server of mine still held :41080, so our GUI server died with "Address already in use" while `_wait_for_url(healthz)` happily got a 200 **from the stale process** — the whole suite then ran against a binary predating the `/links` endpoints. The harness now checks `gui_server.poll()` after healthz answers and fails loudly ("something else is already listening"), so this class of confusion cannot recur. With the port free, team-lens and teampages passed untouched, confirming they were never broken.
- 2026-09-02: One genuine test-only fix: the In flight assertion hit a Playwright strict-mode violation because an item can legitimately carry several in-flight links (seeded PR + branch + the delivered one) — scoped to `.first()` rather than asserting uniqueness.
- 2026-09-02: Docs: new README section "Git forge integration (GitHub / GitLab)" covering the branch/PR short-code convention (the part engineers must actually do), `KAIROS_PUBLIC_URL` + `KAIROS_WEBHOOK_SIGNING_KEY`, the reachability requirement with the localhost-needs-a-tunnel caveat, per-forge webhook setup steps and event selections, rotation (new id ⇒ new URL *and* secret), and the deliberate non-goals. Skipped the optional bootstrap-skill mention — recorded here rather than done half-way.
- 2026-09-02: Full ladder green: `angreal test unit` clean, `angreal test integration` 34/34 targets, `angreal test e2e` 10/10 specs with no retries.