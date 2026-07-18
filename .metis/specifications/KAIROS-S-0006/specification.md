---
id: mcp-tool-surface
level: specification
title: "MCP Tool Surface"
short_code: "KAIROS-S-0006"
created_at: 2026-07-08T11:29:07.063805+00:00
updated_at: 2026-07-08T11:29:07.063805+00:00
parent: KAIROS-I-0001
blocked_by: []
archived: false

tags:
  - "#specification"
  - "#phase/discovery"


exit_criteria_met: false
initiative_id: NULL
---

# MCP Tool Surface

## Overview

The MCP tool surface is the contract between the skills plugin (KAIROS-I-0002) and a Kairos deployment. It is served at `/mcp` on the API binary (KAIROS-A-0011), authenticated per KAIROS-A-0010, and implemented as thin wrappers over the same `kairos-core` services as the REST API. Skills reference these tools by name and never assume REST shapes (KAIROS-A-0014). Tool responses are agent-optimized: compact markdown, short codes as primary identifiers, no UUID-only payloads.

Scope: **work-item operations, context, and search.** Board configuration, member/capability management, template/metadata administration, and tenant provisioning are deliberately excluded from v1 MCP — agents work items; humans configure the system through GUI/CLI.

## System Context

### Actors
- **AI agent (via skills plugin)**: the primary consumer; acts as the authenticated engineer with their ABAC capabilities — never with elevated agent identity
- **Any MCP-capable host**: Claude Code is the packaged experience; the tool surface itself is host-agnostic

### External Systems
- **Keycloak (OIDC)**: OAuth flow per A-0010; the endpoint advertises RFC 9728 protected-resource metadata
- **Kairos application services**: tools invoke `kairos-core` in-process (A-0011)

### Boundaries
Inside: item CRUD, transitions, relationships, metadata values, search/traverse, content history, session context. Outside (v1): board/column/transition configuration, capability grants, team/stream management, template/metadata-definition administration, tenant provisioning, activity-log queries.

## Tool Inventory

### Context tools

| Tool | Input | Returns |
|------|-------|---------|
| `whoami` | — | User (name, email), org, teams with roles, boards where the user holds capabilities (with the capability list) |
| `my_boards` | `level?` | Boards visible to the user, grouped by level; for the user's delivery boards includes column names and item counts per column |
| `board_items` | `board` (slug or id), `column?` | Items on a board grouped by column: short code, title, type, assignee-relevant metadata — compact listing |

### Read tools

| Tool | Input | Returns |
|------|-------|---------|
| `get_item` | `short_code` | Any entity resolved by short code: title, type, board/column, content (markdown), version, metadata values, relationships (parent chain, children, blockers, supporting docs), template origin |
| `get_history` | `short_code`, `limit?` | Version list (version, editor, timestamp) and optionally a requested version's content |
| `search` | `q?`, `filter?`, `traverse?`, `sort?`, `limit?`, `offset?` | Mirrors `POST /api/search` (KAIROS-A-0007) exactly — same composable capabilities, results grouped by type in compact listing form |

### Write tools

| Tool | Input | Returns |
|------|-------|---------|
| `create_item` | `item_type` (strategy\|initiative\|task\|document\|adr), `title`, `board?`, `parent?` (short code), `template?`, `content?`, type-specific fields (`task_type`, `hypothesis`, `complexity`, `decision_maker`) | Created item with short code; creates the `parent`/`supports` relationship when `parent` given |
| `update_item` | `short_code`, `title?`, `content`, `version` | Full-content update under optimistic concurrency; on 409 returns current version + content for reconciliation |
| `edit_item` | `short_code`, `search`, `replace`, `replace_all?` | Server-side read-modify-write: applies search/replace to current content, submits with current version, retries once on race; errors if `search` not found or ambiguous (when `replace_all` false) |
| `transition_item` | `short_code`, `to_column` (name or id) | Resolves column by name on the item's board, validates against `board_transitions`; on invalid transition returns the allowed target columns |
| `link_items` | `source`, `target`, `relationship` (parent\|supports\|informs\|supersedes\|blocks) | Creates the edge (org-admin-gated relationships enforced server-side per A-0006) |
| `unlink_items` | `source`, `target`, `relationship` | Removes the edge |
| `set_metadata` | `short_code`, `values` (map of definition slug → value) | Validates against metadata definitions (A-0003); returns resulting metadata set |
| `delete_item` | `short_code`, `confirm` (must be true) | Soft delete with cascade warning: response lists what was cascade-deleted |

## Requirements

### Functional Requirements

| ID | Requirement | Rationale |
|----|-------------|-----------|
| REQ-1.1 | Every tool executes as the authenticated user under full ABAC (A-0006); errors surface as tool errors mirroring API error codes (403/404/409/422 semantics) | Agents are human extensions; no privileged path |
| REQ-1.2 | Tenant context comes exclusively from the connection host (subdomain / single-tenant mode); no tenant parameter on any tool | Prevents cross-tenant confusion or probing |
| REQ-1.3 | Short codes are the identifier in all tool inputs and outputs; UUIDs accepted where a `board` id is natural but never required | Agent ergonomics and log readability |
| REQ-1.4 | `transition_item` failures enumerate valid target columns | Converts dead-ends into self-correcting agent behavior |
| REQ-1.5 | `update_item`/`edit_item` implement optimistic concurrency (A-0004); conflict responses carry enough state to reconcile without extra calls | Agents must handle 409s mechanically |
| REQ-1.6 | List/search responses are compact (title + short code + key fields), with full content only via `get_item` | Token economy in agent contexts |
| REQ-1.7 | Tool schemas are versioned with the server; `initialize` reports server version | Skills/server compatibility diagnosis |

### Non-Functional Requirements

| ID | Requirement | Rationale |
|----|-------------|-----------|
| NFR-1.1 | Tool overhead beyond the underlying service call ≤ 5ms p95 (in-process, no loopback) | Vision's <50ms p95 applies to MCP paths too |
| NFR-1.2 | No server-held session state beyond MCP protocol requirements | Stateless server constraint |
| NFR-1.3 | Every tool invocation logged to the activity trail identically to API calls | Audit parity |

## Decision Log

| ADR | Title | Status | Summary |
|-----|-------|--------|---------|
| KAIROS-A-0011 | Kairos MCP Server - Remote HTTP | draft | Streamable HTTP at `/mcp`, in-process services, rmcp SDK |
| KAIROS-A-0010 | OIDC Authentication | draft | OAuth protected resource → Keycloak; user-scoped sessions |
| KAIROS-A-0006 | ABAC Authorization | draft | Board-scoped capabilities govern every write tool |
| KAIROS-A-0007 | Unified Search | draft | The `search` tool mirrors it 1:1 |
| KAIROS-A-0014 | Skills Plugin Architecture | draft | Skills consume only this surface |

## Constraints

### Technical Constraints
- Tools wrap `kairos-core` services in-process — adding a tool must not introduce logic absent from the service layer
- Tool names and input schemas are frozen by this spec; changes require a spec revision before implementation (A-0011)

### Organizational Constraints
- Administrative operations stay out of the MCP surface in v1 — expansion requires revisiting the agents-work-items boundary deliberately