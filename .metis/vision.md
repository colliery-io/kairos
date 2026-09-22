---
id: project-vision
level: vision
title: "Project Vision"
short_code: "KAIROS-V-0001"
created_at: 2026-03-03T03:38:59.631341+00:00
updated_at: 2026-03-04T01:19:57.707067+00:00
archived: false

tags:
  - "#vision"
  - "#phase/published"


exit_criteria_met: false
strategy_id: NULL
initiative_id: NULL
---

# Kairos: Distributed Flight Levels Work Management

## Purpose

Kairos evolves the Metis Flight Levels methodology from a single-repo, file-based system into a centralized, multi-tenant platform for distributed teams. Where Metis proved that phase-gated work decomposition delivers structured, design-first workflows, Kairos extends this to organizations where work is organized around delivery streams that cut across repositories, teams, and technologies.

Kairos implements a three-level Flight Levels system: **Strategy** (leadership bets), **Initiative** (cross-team coordination), and **Delivery** (team execution). Direction flows down through the levels; information flows up. Each level has its own board, ownership, cadence, and ceremonies.

The name "Kairos" (Greek: the opportune moment) reflects the system's role: surfacing the right work at the right time across organizational boundaries.

## Product/Solution Overview

Kairos is an API-first work management platform built on PostgreSQL with multi-tenant isolation. All clients - CLI, GUI, and MCP servers for AI agents - consume the same HTTP API.

**Target audience:**
- Distributed engineering teams needing shared visibility into strategic work decomposition across delivery streams
- Organizations adopting Flight Levels methodology at scale across multiple teams
- Teams outgrowing single-repo, file-based work tracking

**Key benefits over Metis:**
- Three distinct Flight Levels with clear ownership, ceremonies, and cadences
- Work planned around delivery streams that span repositories, and issued and executed per repository — a team owns many repos, and agents work one repo at a time
- Tenant isolation enables multi-org deployments with hard data boundaries
- Real-time consistency (no sync step between filesystem and database)
- Authentication via any OIDC-compliant IdP (bring your own); authorization in Kairos; SCIM 2.0 provisioning
- Any client (CLI, GUI, AI agents via MCP) accesses the same data through the same API

## Current State

Metis provides a mature, file-based Flight Levels implementation with:
- Hierarchical documents: Vision -> Strategy -> Initiative -> Task + ADR
- Phase state machines with forward-only transitions and exit criteria gates
- SQLite + FTS5 for search, markdown files as source of truth
- MCP server for Claude Code integration
- CLI, GUI (Tauri), and plugin ecosystem

**Limitations driving Kairos:**
- Work is scoped to a single `.metis/` directory within one repository - the repo *is* the organizational boundary, which doesn't reflect how teams actually deliver value
- No concept of delivery streams or workstreams that span multiple repos
- No distinction between strategic, coordination, and execution levels - all documents live in a flat hierarchy
- No ceremonies, cadences, or defined ownership model
- File-based sync (filesystem <-> SQLite) adds complexity and failure modes
- No authentication, authorization, or multi-user access control
- No API layer - all access is through local filesystem or MCP-over-stdio

## Future State

Kairos provides a centralized platform with three Flight Levels:

### Level 3: Strategy (Leadership)
One board, owned by the strategy team (leadership). Contains **Strategies** - hypothesis-driven bets the organization is making. Each strategy is framed as: "we're doing X because we believe Y will happen."

The **Company Vision** is the root document of the organization - everything ultimately maps up to it. It informs this board but doesn't live on it. All strategies are evaluated against it.

Phases: **Draft -> Review -> Active -> Monitoring -> Completed**

Monitoring is the post-execution phase where the hypothesis is validated or invalidated before closing out. Ceremonies: Strategy Review (monthly/quarterly), Strategy-Initiative Sync (monthly).

### Level 2: Initiative (Coordination)
A limited number of boards at this level, each owned by a coordinator. Only **Initiatives** (epics) live here - concrete projects that deliver against strategies. This is the cross-team coordination layer: understanding capacity at the macro level and ensuring the right work flows to the right delivery teams.

The **Social Contract** informs this level - shared agreements across teams about communication, commitments, and coordination norms.

Phases: **Discovery -> Design -> Ready -> Decompose -> Active -> Monitoring -> Completed**

Ready items are pulled JIT into Decompose. Monitoring is post-delivery support through stabilization. **Bucket initiatives** (Tech Debt, Bugs, Ad-Hoc/Service Requests) are standing capacity tracking mechanisms, recreated at whatever cadence fits the organization. Ceremonies: Board Review (weekly/biweekly), Working Sessions (as needed for discovery, decomposition, break-fix).

### Level 1: Delivery (Team Execution)
One board per delivery team, owned by the **team lead**. Three item types: **Task**, **Bug**, **Tech Debt**. Teams are empowered to work however fits them and their cadences. The only requirement: work is trackable and linked to initiatives so flow is visible across levels.

The **Team Charter** informs this board - the team's scope of ownership, working agreements, and internal norms.

Phases: **Backlog -> Todo -> Blocked -> Active -> Completed**

Work enters from two sources: upstream tasks decomposed from initiatives, and team-internal items. Cross-team blockers are escalated to the Initiative Board Review. Ceremonies: Daily Standup, Backlog Grooming/Triage (weekly or per sprint).

### Cross-Level Flow
- **Direction flows down**: Strategies define what to build toward. Initiatives are created to implement active strategies. Tasks are decomposed from initiatives and assigned to delivery teams.
- **Information flows up**: Delivery teams report progress and blockers at Initiative Board Review. Coordinators report initiative status at Strategy-Initiative Sync. Proposals and escalations flow upward.
- **Work enters at each level**: Strategies from leadership proposals or escalations. Initiatives from strategy decomposition. Tasks from initiative decomposition or team-internal needs (bugs, tech debt).

## Major Features

- **Three-level Flight Levels boards**: Strategy, Initiative, and Delivery boards with distinct ownership, phases, and ceremonies at each level
- **Delivery streams plan, repositories execute**: A delivery stream represents a flow of work toward a business outcome and may span many repositories, teams, and technologies; streams and boards are where work is planned and tracked. Repositories are first-class, team-owned entities (KAIROS-A-0019): a team owns many repositories, each task is issued against at most one, and that binding routes the ticket to the owning team's delivery board. Agents execute inside one repository at a time, and can discover other teams' repositories — owner, board, conventions — to file tickets and open PRs across team boundaries.
- **Supporting documents with configurable templates**: Any workflow item (strategy, initiative, task) can have child documents attached to it. Documents are template-driven - the system ships with defaults (PRD, System Context, Architecture Framing, Team Charter, Social Contract, Company Vision) and teams can create their own templates. Documents are the substance of initiative phases: a PRD is produced during Discovery, architecture docs during Design. They're children of the parent they support - not free-floating artifacts.
- **Reference documents**: Governance documents that inform boards but don't live on them:
  - **Company Vision** - the root document of the organization. Everything maps up to it. Strategies are evaluated against it.
  - **Social Contract** (initiative level) - how teams work together, coordination norms
  - **Team Charter** (delivery level) - team scope, ownership, working agreements
- **Bucket initiatives**: Standing initiatives for Tech Debt, Bugs, and Ad-Hoc/Service Requests, recreated quarterly. Provide coarse capacity tracking for ongoing non-epic work.
- **ADRs at any level**: Architecture Decision Records can be children of any entity - a strategy, an initiative, a delivery stream, or even a task. A strategy-level ADR ("adopt Kubernetes org-wide") is different in scope from an initiative-level ADR ("use Avro for serialization") or a team-level ADR ("use sqlx for database access"), but they all follow the same lifecycle (draft -> discussion -> decided -> superseded). The parent determines scope.
- **Hypothesis-driven strategies**: Strategies are framed as hypotheses with a Monitoring phase for validation before completion
- **Multi-tenant API server** (Rust/Axum): RESTful HTTP API with tenant-scoped CRUD for all document types, phase transitions, search, and hierarchy management
- **PostgreSQL with schema-per-tenant isolation**: Each organization gets an isolated schema (`org_{slug}`) with the full document model, preserving data boundaries while sharing infrastructure
- **BYO-IdP authentication + ABAC authorization + SCIM provisioning**: any spec-compliant OIDC issuer for identity (JWT validation via discovery/JWKS — Kairos bundles no IdP, per KAIROS-A-0016); inbound SCIM 2.0 for user/group lifecycle including deprovisioning. Attribute-Based Access Control governs who can do what at each Flight Level - the system provides sensible defaults (e.g., leadership can manage strategies, coordinators manage initiatives, team leads manage delivery boards) with customization for organizations that need different access patterns
- **MCP server (API-backed)**: Same tool interface as Metis's MCP server, but backed by HTTP calls to the Kairos API instead of local file I/O
- **Agentic development skills plugin ("beast mode")**: The agentic workflow layer of the Kairos product, shipping with it from day 1 (KAIROS-I-0002, KAIROS-A-0008). Engineering-discipline skills bound to Kairos boards via the Kairos MCP server — grilling, TDD, tracer-bullet decomposition onto delivery boards, backlog triage, code review against the originating item, domain modeling — plus architecture-review and diataxis-review skills with their distributable specs, and a bootstrap skill that wires an engineer's repo into their company's Kairos deployment (OIDC, tenant, team, angreal defaults). Adopting companies get the full agentic development workflow out of the box; it is a core part of Kairos's consumer value
- **Full-text search**: PostgreSQL tsvector/GIN indexes with tenant-scoped search across all document content

## Success Criteria

- The three Flight Levels (Strategy, Initiative, Delivery) operate as distinct boards with their own ownership, phases, and cadences
- Direction flows down and information flows up across levels with clear linkage (strategy -> initiative -> task)
- A single Kairos deployment can serve multiple tenants with complete data isolation
- Delivery streams can span multiple repositories; each repository has one owning team, each task binds to at most one repository, and an agent in one repository can file work against another team's repository and see the resulting PR linked back
- Any API client (CLI, GUI, MCP) interacts through the same API
- Authentication via OIDC prevents unauthorized access; authorization scopes to tenant boundaries
- Latency for common operations (list, read, transition) is under 50ms p95

## Principles

- **Three levels, clear ownership**: Strategy is owned by leadership. Initiative coordination is owned by coordinators. Delivery is owned by team leads. Access at each level is governed by ABAC with sensible defaults and customization.
- **Direction down, information up**: Strategic direction flows downward through decomposition. Progress, blockers, and proposals flow upward through ceremonies and escalation.
- **Streams and boards plan; repositories are where tickets are issued and executed**: The planning unit is the delivery stream and its boards. The execution unit is the repository — tickets are addressed to a repo, agents work inside a repo, and PRs land in a repo. A team owns many repositories; a repository has exactly one owning team; a task belongs to at most one repository (multi-repo work is decomposed). Any team's agent may file work into another team's Backlog against that team's repository, behind the owning team's triage gate (KAIROS-A-0019).
- **Teams work how they work**: Delivery teams choose their own cadence and process. The only requirement is trackable work linked to initiatives for cross-level visibility.
- **API-first**: The HTTP API is the primary interface. CLI, MCP, and GUI are all equal consumers. No special paths or backdoors.
- **Design is part of delivery**: Planning artifacts (PRDs, system context, architecture docs) are first-class content in Kairos, not external documents you link to. The tool centralizes both the thinking and the tracking.
- **Templates, not types**: Supporting documents are one generic type with configurable templates. The system ships with useful defaults; teams customize for their domain. No document type explosion.
- **Agents are human extensions**: AI agents (via MCP) are a client of the API, just like CLI and GUI. They execute on behalf of humans, not as autonomous actors.
- **Tenant isolation by default**: Data never leaks across tenant boundaries. Schema-per-tenant in PostgreSQL provides hard isolation.
- **Progressive adoption**: Start with a single tenant and grow. Simple to self-host (Docker Compose) and simple to operate.
- **Stateless server**: The API server holds no state beyond the request. All state lives in PostgreSQL.

## Constraints

- **Rust for server and core logic**: Consistency with Metis; performance and correctness guarantees
- **PostgreSQL 16+**: Required for schema isolation, full-text search, and JSON support
- **Identity is external (BYO IdP)**: any spec-compliant OIDC provider; Kairos ships none and builds no custom auth. SCIM 2.0 serves user/group lifecycle. ABAC policies enforce access at each Flight Level. *(Amended 2026-07-10 — previously "Keycloak for OIDC"; see KAIROS-A-0016.)*
- **Single-database deployment initially**: Multi-tenant via schemas within one PostgreSQL instance. Sharding across databases is out of scope for v1.
- **Thin real-time events (v1)**: A tenant-scoped WebSocket channel (`/ws/events`) pushes change notifications so UIs never poll; events are hints (clients re-fetch via the API), fan-out via PostgreSQL LISTEN/NOTIFY. A durable/replayable event stream remains a future enhancement. *(Amended 2026-07-08 — previously "no real-time push in v1"; see KAIROS-A-0005.)*
- **No file-based fallback**: Unlike Metis, Kairos does not maintain markdown files on disk. The database is the sole source of truth.