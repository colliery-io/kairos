---
id: dev-stack-on-a-unique-41xxx-port
level: task
title: "Dev stack on a unique 41xxx port block — stop colliding with other local projects"
short_code: "KAIROS-T-0095"
created_at: 2026-09-01T10:02:34.735034+00:00
updated_at: 2026-09-01T10:47:58.528949+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#tech-debt"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Dev stack on a unique 41xxx port block — stop colliding with other local projects

## Objective

Move every HOST-exposed dev-stack port onto a project-unique 41xxx block so Kairos stops colliding with other local projects. Container-internal and shipping defaults stay standard.

## Backlog Item Details

### Type
Tech Debt (chore)

### Priority
P2 — active daily friction across projects.

## Port Scheme (decision)

| Surface | Old | New |
|---|---|---|
| Postgres (host mapping) | 5432 | 41432 (container stays 5432) |
| Dex (issuer/listener/host) | 5558 | 41558 (must match inside+outside — browser and server both dial it) |
| Dev server / GUI + PKCE redirect range | 8080 (8080–8099) | 41080 (41080–41099) |
| e2e API golden-path server | 8188 | 41188 |
| Soak server | 8189 | 41189 |

41xxx: below the ephemeral range (no transient grabs), keeps the recognizable suffixes.

**Unchanged on purpose**: `DEFAULT_BIND_ADDR` 127.0.0.1:8080 (shipping default; helm/deploy assume container port 8080); deploy/** entirely; test fixtures that merely parse hosts (tenant middleware, ws url unit tests); CLI shipped default server URL.

## Acceptance Criteria

## Acceptance Criteria

- [x] Compose, Dex config, and every angreal task default use the 41xxx block; `angreal services up` exposes only 41432/41558.
- [x] All test-tier defaults (kairos-db/server/cli test DATABASE_URLs, ISSUER consts, e2e helpers/config/specs, soak + golden-path defaults) point at the new block.
- [x] Docs that name dev URLs updated (e2e README, gui-conventions, bootstrap skill).
- [x] Full ladder green: unit, integration, e2e.

## Status Updates

- 2026-09-01: Ticketed (PO request), scheme decided, executing.
- 2026-09-01: Sweep applied: compose (41432:5432 mapping; Dex 41558 inside+outside incl. healthcheck), dex config (issuer/listener + the T-0050 redirect range re-enumerated as 41080–41099), task_db/task_test defaults (DATABASE_URL, E2E_ISSUER, GUI 41080, golden-path 41188, soak 41189), every crate test-tier DATABASE_URL/ISSUER const, e2e harness (playwright baseURL, auth helper defaults, spec GUI consts, README), soak + golden-path example defaults, and dev-URL mentions in gui-conventions + bootstrap skill. Deliberately untouched (verified one by one): DEFAULT_BIND_ADDR 8080 (shipping/container default; helm assumes it), deploy/**, CLI shipped default server URL, and host-parsing unit fixtures (tenant middleware, ws url, config parse tests). Dex serves the new issuer; compose exposes exactly 41432/41558; unit clean; integration 32/32 on the new ports; e2e running.
- 2026-09-01: e2e run 1 failed ENVIRONMENTALLY, and the diagnosis validated the ticket: Docker Desktop restarted mid-run (kairos containers vanished with no exit records; docker events only reach back to the fresh daemon), and the `cloacina-demo` stack came up on the new daemon occupying host **8080** and 5556 — the exact old-style ports. On the old scheme the GUI server could not even have bound. Zero overlap with 41xxx confirmed (`docker ps` port scan). Re-running e2e with both stacks up side by side — the honest test of the objective.
- 2026-09-01: COMPLETE. e2e run 2: 9/9 specs passed in 11.9s with the cloacina-demo stack (host 8080 + 5556) running concurrently — full PKCE against Dex on 41558, GUI on 41080, zero cross-project interference. Full ladder on the new block: unit clean, integration 32/32, e2e 9/9.