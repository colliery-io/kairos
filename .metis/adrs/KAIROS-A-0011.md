---
id: 001-kairos-mcp-server-remote-http-on
level: adr
title: "Kairos MCP Server - Remote HTTP on the API Service"
number: 1
short_code: "KAIROS-A-0011"
created_at: 2026-07-08T11:28:39.735144+00:00
updated_at: 2026-07-08T15:00:37.805096+00:00
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

# ADR-1: Kairos MCP Server - Remote HTTP on the API Service

## Context

AI agents are a first-class, day-1 client of Kairos: the skills plugin (KAIROS-I-0002) is the product's agentic workflow layer, and every skill operates on Kairos boards through MCP. The MCP server's shape determines the consumer install experience, the auth story, and how tools relate to the REST API. The concrete tool list lives in KAIROS-S-0006; this ADR fixes the architecture.

## Decision

**The MCP server is a remote Streamable HTTP endpoint served by the API binary at `/mcp`, authenticated via OAuth (KAIROS-A-0010), with tools calling the same in-process application services as the REST handlers.**

### Transport and mounting
- Streamable HTTP per the current MCP specification, mounted on the axum router at `/mcp` alongside `/api` — one binary, one deployment (KAIROS-A-0009)
- Implemented with the official Rust MCP SDK (`rmcp`), whose streamable-HTTP service mounts as a tower service; version-pinned, protocol revision tracked deliberately
- Tenant context resolves exactly as for `/api`: subdomain Host header → `search_path` (KAIROS-A-0005). An agent connects to `acme.kairos.example/mcp` and is inside `acme`

### Authentication
- The endpoint is an OAuth protected resource: it serves protected-resource metadata (RFC 9728) pointing at the deployment's Keycloak realm; MCP clients (Claude Code et al.) drive the flow
- Bearer tokens are validated by the same middleware as `/api`; the MCP session acts **as the authenticated user** with their ABAC capabilities (A-0006). Agents are human extensions (vision principle) — no ambient agent identity, no elevated agent role

### Tools call services, not HTTP
Tool implementations invoke `kairos-core` application services directly — the same functions the REST handlers call. No self-HTTP loopback. Consequences: identical authorization enforcement, identical validation, one code path to test, and tool responses can be shaped for agents (markdown summaries, compact listings) rather than mirroring JSON payloads.

### Session context
The protocol is stateless on the server beyond MCP protocol requirements. Agent working context (which team/board the engineer belongs to) is not server-held session state — it's discoverable via context tools (`whoami`, `my_boards` in S-0006) and pinned client-side by the plugin's bootstrap/session hooks (KAIROS-A-0014).

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Remote HTTP on the service (chosen) | Zero per-engineer install; one deployable; auth handled by MCP-standard OAuth; always version-matched to the API | Requires MCP clients with HTTP+OAuth support (mainstream ones have it) | Low | Medium |
| Local stdio binary wrapping the API | Works with stdio-only clients | Per-engineer install + upgrade burden on day 1; version skew against server; token plumbing is bespoke | Medium | Medium |
| Both from day 1 | Maximum client compatibility | Double surface to build/test before first ship | Low | High |
| Separate MCP microservice | Independent scaling | Second deployable, must call API over HTTP (loopback tax), duplicated auth handling | Medium | High |

## Rationale

1. **Day-1 consumer experience**: an engineer installs the skills plugin, points it at their company's Kairos URL, authenticates in the browser — no binaries. This is the shortest path from "company adopts Kairos" to "agents working on boards."
2. **In-process services guarantee authz parity.** The moment MCP tools go through their own path, capability checks can drift. Same functions, same checks, provably.
3. **One binary honors the deployment principle** ("simple to self-host") — compose brings up Postgres, Keycloak, and one Kairos container serving GUI, API, and MCP.
4. **A stdio wrapper can be added later without redesign** (it would speak to the same HTTP endpoint) if a client environment demands it — see Review Triggers.

## Consequences

### Positive
- MCP tools ship in lockstep with the API — no client/server version matrix on day 1
- The skills plugin needs only a URL to configure (`.mcp.json` with the deployment's `/mcp`)
- Tool responses can be agent-optimized independently of REST response shapes

### Negative
- Requires MCP-over-HTTP + OAuth capable clients; stdio-only hosts are unsupported in v1
- `rmcp` SDK and MCP protocol revisions become tracked dependencies of the server binary

### Neutral
- Local development uses the compose stack; agents connect to `localhost` with the dev realm (A-0012, A-0013)
- Tool-surface changes are spec changes (S-0006) first, implementation second

## Review Schedule

### Review Triggers
- A consumer environment that cannot use HTTP MCP (air-gapped or stdio-only host) → add the stdio wrapper binary
- MCP protocol transport changes that break streamable HTTP compatibility