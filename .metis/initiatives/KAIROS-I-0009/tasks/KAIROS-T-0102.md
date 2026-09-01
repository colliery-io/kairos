---
id: forge-integration-e2e-seeded
level: task
title: "Forge integration e2e + seeded fixtures + setup documentation"
short_code: "KAIROS-T-0102"
created_at: 2026-09-01T23:12:38.913970+00:00
updated_at: 2026-09-01T23:12:38.913970+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] Seed provisions demo connections + links covering open/draft/merged/branch; `seed_demo` counts updated and asserted.
- [ ] `forge.spec.ts` drives real signed deliveries: open → merged live over WS → replay stays merged → bad signature is a no-op → team rollup shows only in-flight work. Green without retries.
- [ ] A reusable signed-delivery helper lives in `e2e/helpers/`.
- [ ] Fallout swept: provisioning counts, item-detail specs, seed counts, openapi gate.
- [ ] Setup docs cover both forges, the branch-naming convention, the reachability requirement (including the localhost caveat), and secret rotation.
- [ ] Full ladder green: unit, integration, e2e.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
