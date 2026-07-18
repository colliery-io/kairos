---
id: v1-clients-cli-and-leptos-gui
level: initiative
title: "V1 Clients - CLI and Leptos GUI"
short_code: "KAIROS-I-0004"
created_at: 2026-07-08T15:04:04.650026+00:00
updated_at: 2026-07-16T00:02:00.160897+00:00
parent: KAIROS-V-0001
blocked_by: [KAIROS-I-0003]
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: v1-clients-cli-and-leptos-gui
---

# V1 Clients - CLI and Leptos GUI Initiative

## Context

Roadmap milestone **M4**: the human client surfaces of Kairos v1 per KAIROS-A-0015 (decided) — the `kairos` CLI and the Leptos GUI. **Blocked by KAIROS-I-0003** (needs the API, `kairos-client`, WS events, and OpenAPI from M2). Decomposition happens when I-0003's M2 nears completion; this document holds scope until then.

## Goals & Non-Goals

**Goals:**
- `kairos` CLI: clap over `kairos-client`; device-flow `login` (A-0010); noun subcommands mirroring the API; table + `--json` output; exit codes 0/1/2; release binaries + `cargo install`
- Leptos GUI (`kairos-web`, CSR): served at `/` by the server binary; **`aurora-dark` crate (crates.io) as the design system** — components consume tokens only; PKCE login via the deployment IdP; board views with drag/click transitions (only valid targets offered) and live `/ws/events` updates; item detail with markdown editing + 409-merge UI; create-from-template; unified search; relationship views; admin surfaces (boards, members/capabilities, teams, streams, templates/metadata); activity/history views
- Playwright smoke suite for GUI critical paths (login via Dex, board view, create/transition, live update) wired into `angreal test e2e`

**Non-Goals:**
- SSR, offline mode, tenant theming, mobile-specific layouts (post-v1)
- New API endpoints — any gap discovered here is filed against the service, not worked around client-side

## Detailed Design

Fixed by KAIROS-A-0015 (decided 2026-07-08). Shared DTOs from `kairos-client`/`kairos-types` make client/server drift a compile error. Leptos component conventions doc is this initiative's first task, before parallel component work fans out.

## Alternatives Considered

Adjudicated in KAIROS-A-0015 (React SPA, Leptos SSR, Tauri — all rejected there).

## Implementation Plan

Decompose when I-0003/M2 is nearing done. Expected shape: CLI (~3 tasks: auth+config, command tree, packaging) ∥ GUI (~8 tasks: conventions+shell+auth, boards, item detail, search, relationships, admin, activity/history, Playwright) — GUI tasks fan out in parallel worktrees after the conventions/shell task lands.

## Progress Log

- **2026-07-08**: Initiative created post-ratification; scoped and parked pending I-0003 M2 (blocked_by edge set).