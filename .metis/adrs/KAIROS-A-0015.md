---
id: 001-v1-client-architecture-cli-and
level: adr
title: "V1 Client Architecture - CLI and Leptos GUI"
number: 1
short_code: "KAIROS-A-0015"
created_at: 2026-07-08T11:29:02.513106+00:00
updated_at: 2026-07-08T15:00:48.253409+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: V1 Client Architecture - CLI and Leptos GUI

## Context

Dylan decided (2026-07-08) that v1 ships the full client set — MCP (A-0011), a human CLI, and a GUI — and fixed the GUI as **Leptos, served from the root of the backend, styled with Colliery's "Aurora Dark" theme**. This ADR pins the client architectures so agents can build them: rendering mode, code sharing, GUI scope, CLI shape, and distribution.

## Decision

**GUI: Leptos in CSR mode, compiled to WASM, served as static assets at `/` by the server binary, themed Aurora Dark. CLI: a `kairos` clap binary over the shared `kairos-client` crate. All clients share DTO types from one crate.**

### Shared types
Request/response DTOs live in `kairos-client` (or a `kairos-types` sub-crate if WASM feature-gating demands it) and are used by the server handlers, the CLI, the Leptos app, and integration tests. API drift between clients becomes a compile error, not a runtime bug.

### GUI (`kairos-web`)
- **Leptos CSR** — a static WASM bundle built with `cargo-leptos`, embedded in the server image and served at `/` (A-0009, A-0013). No SSR: this is an authenticated app-like tool (kanban boards), where SEO and anonymous first-paint don't apply, and CSR avoids coupling page rendering to auth state on the server
- **Auth**: Authorization Code + PKCE against Keycloak (A-0010); access token held in memory, silent refresh; unauthenticated hits show the login redirect
- **Theme**: Colliery **Aurora Dark** is the design system — consumed as the **`aurora-dark` crate from crates.io** (a normal Cargo dependency of `kairos-web`, version-pinned), per Dylan 2026-07-08. Components consume its tokens (colors, typography, spacing, elevation) only; no hardcoded colors in components. Local working copy for reference: `/Users/dstorey/Desktop/aurora-dark`
- **v1 scope** (matches the API surface, S-0005): login; board views per level with column layout and drag/click transitions (invalid transitions not offered — read from `board_transitions`); item detail with markdown editing and 409-conflict resolution UI (show server version, let user merge); create-from-template flows; unified search; relationship views (parent chain, blockers); admin surfaces — board configuration, members/capabilities, teams, delivery streams, templates/metadata definitions; activity/history views
- **State/data layer**: a thin resource layer over `kairos-client` WASM calls; server is the source of truth; no client cache invalidation cleverness in v1

### CLI (`kairos-cli`)
- Binary name `kairos`; `clap` derive; subcommand nouns mirror the API: `login`, `whoami`, `orgs`, `boards`, `strategies|initiatives|tasks|documents|adrs` (`list/get/create/edit/transition/delete`), `search`, `teams`, `streams`, `admin tenants`
- `login` uses the Device Authorization Grant (A-0010); credentials cached per A-0010
- Output: human-readable tables by default, `--json` for scripting; exit codes: 0 success, 1 API/validation error, 2 auth error
- Distribution: release binaries attached to repo releases + `cargo install`; packaging managers later

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Leptos CSR from server root (chosen) | All-Rust workspace, shared DTOs, zero CORS, one artifact | WASM bundle size; smaller ecosystem than JS; agent familiarity lower than React | Medium | Medium |
| Leptos SSR + hydration | Faster first paint | Server rendering entangled with OAuth state; more moving parts for an authed tool with no SEO need | Medium | High |
| React/TS SPA | Deepest ecosystem and agent training coverage | Second language/toolchain in the workspace; DTO drift returns; contradicts the all-Rust decision | Low | Medium |
| Tauri desktop app | Native feel, metis heritage | Wrong distribution model for a multi-tenant SaaS; per-OS builds | High | High |

## Rationale

1. **All-Rust was chosen deliberately** — one toolchain, one type system across server, CLI, and GUI; shared DTOs convert a whole class of integration bugs into compile errors.
2. **CSR fits the product**: every meaningful screen is behind login and board-shaped; SSR would buy nothing and cost auth complexity.
3. **Served-from-root** eliminates CORS, keeps the deployment single-artifact (A-0013), and guarantees GUI/API version lock.
4. **CLI as a thin client-crate wrapper** means the CLI, tests, and skills-verification harness all exercise the same client code.

## Consequences

### Positive
- GUI, CLI, MCP, and tests all consume the same typed client/DTOs — one place to update per API change
- Single deployable artifact preserved; `docker compose up` includes the full GUI
- Aurora Dark tokens centralize the visual identity; theming later (light mode, tenant branding) is a token-sheet swap

### Negative
- Leptos expertise is scarcer than React for both humans and agents — mitigate with early component conventions (a `plugin`-style reference doc in-repo) and the Playwright smoke tier (A-0012)
- WASM payload needs a size budget and `wasm-opt` in the release build

### Neutral
- GUI live updates come from the `/ws/events` channel (A-0005, added 2026-07-08): board views subscribe (filtered by board_id) and re-fetch changed items on event — no polling
- Aurora Dark ships as the published `aurora-dark` crate (crates.io) — no vendoring; theme updates arrive as ordinary dependency bumps

## Review Schedule

### Review Triggers
- Leptos velocity proves materially worse than expected during Phase B of implementation (re-evaluate before GUI scope grows)
- WASM bundle exceeds budget on mid-tier hardware/network profiles