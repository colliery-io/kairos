# Code Index

> Generated: 2026-09-23T12:55:58Z | 276 files | Python, Rust, TypeScript

## Project Structure

```
├── crates/
│   ├── kairos-cli/
│   │   ├── src/
│   │   │   ├── commands/
│   │   │   │   ├── admin.rs
│   │   │   │   ├── boards.rs
│   │   │   │   ├── entities.rs
│   │   │   │   ├── keys.rs
│   │   │   │   ├── members.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── orgs.rs
│   │   │   │   ├── repos.rs
│   │   │   │   ├── search.rs
│   │   │   │   ├── service_accounts.rs
│   │   │   │   ├── streams.rs
│   │   │   │   └── teams.rs
│   │   │   ├── context.rs
│   │   │   ├── credentials.rs
│   │   │   ├── error.rs
│   │   │   ├── main.rs
│   │   │   ├── oidc.rs
│   │   │   ├── provider.rs
│   │   │   └── table.rs
│   │   └── tests/
│   │       ├── cli_live.rs
│   │       └── cli_tree_live.rs
│   ├── kairos-client/
│   │   └── src/
│   │       ├── client.rs
│   │       ├── error.rs
│   │       ├── lib.rs
│   │       ├── types.rs
│   │       ├── types_events.rs
│   │       ├── types_forge.rs
│   │       ├── types_graph.rs
│   │       ├── types_meta.rs
│   │       ├── types_org.rs
│   │       ├── types_repositories.rs
│   │       ├── types_search.rs
│   │       ├── types_service_accounts.rs
│   │       ├── types_team_pages.rs
│   │       └── ws.rs
│   ├── kairos-core/
│   │   └── src/
│   │       ├── abac.rs
│   │       ├── board.rs
│   │       ├── forge.rs
│   │       ├── graph.rs
│   │       ├── items.rs
│   │       ├── lib.rs
│   │       ├── repositories.rs
│   │       ├── retention.rs
│   │       ├── search.rs
│   │       └── short_code.rs
│   ├── kairos-db/
│   │   ├── src/
│   │   │   ├── abac.rs
│   │   │   ├── api_keys.rs
│   │   │   ├── boards.rs
│   │   │   ├── events.rs
│   │   │   ├── forge.rs
│   │   │   ├── graph.rs
│   │   │   ├── items.rs
│   │   │   ├── lib.rs
│   │   │   ├── migrations.rs
│   │   │   ├── models/
│   │   │   │   ├── boards.rs
│   │   │   │   ├── enums.rs
│   │   │   │   ├── forge.rs
│   │   │   │   ├── graph.rs
│   │   │   │   ├── items.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── public.rs
│   │   │   │   ├── repositories.rs
│   │   │   │   ├── team_pages.rs
│   │   │   │   ├── teams.rs
│   │   │   │   └── templates.rs
│   │   │   ├── pool.rs
│   │   │   ├── repositories.rs
│   │   │   ├── retention.rs
│   │   │   ├── schema.rs
│   │   │   ├── scim.rs
│   │   │   ├── search.rs
│   │   │   ├── seed.rs
│   │   │   ├── service_accounts.rs
│   │   │   ├── team_pages.rs
│   │   │   └── tenant.rs
│   │   └── tests/
│   │       ├── abac.rs
│   │       ├── api_keys.rs
│   │       ├── board_move.rs
│   │       ├── board_rules.rs
│   │       ├── graph.rs
│   │       ├── isolation.rs
│   │       ├── models_roundtrip.rs
│   │       ├── public_migrations.rs
│   │       ├── repositories_migration.rs
│   │       ├── retention.rs
│   │       ├── search.rs
│   │       ├── seed_demo.rs
│   │       ├── team_pages.rs
│   │       ├── tenant_provisioning.rs
│   │       └── write_path.rs
│   ├── kairos-server/
│   │   ├── examples/
│   │   │   └── e2e_golden_path.rs
│   │   ├── src/
│   │   │   ├── api/
│   │   │   │   ├── adrs.rs
│   │   │   │   ├── cascade.rs
│   │   │   │   ├── convert.rs
│   │   │   │   ├── convert_meta.rs
│   │   │   │   ├── convert_org.rs
│   │   │   │   ├── documents.rs
│   │   │   │   ├── initiatives.rs
│   │   │   │   ├── meta/
│   │   │   │   │   ├── activity.rs
│   │   │   │   │   ├── definitions.rs
│   │   │   │   │   ├── history.rs
│   │   │   │   │   ├── metadata.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── relationships.rs
│   │   │   │   │   ├── restore.rs
│   │   │   │   │   └── templates.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── openapi.rs
│   │   │   │   ├── org/
│   │   │   │   │   ├── admin.rs
│   │   │   │   │   ├── boards.rs
│   │   │   │   │   ├── forge.rs
│   │   │   │   │   ├── members.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── repositories.rs
│   │   │   │   │   ├── streams.rs
│   │   │   │   │   ├── team_pages.rs
│   │   │   │   │   └── teams.rs
│   │   │   │   ├── search.rs
│   │   │   │   ├── strategies.rs
│   │   │   │   └── tasks.rs
│   │   │   ├── app.rs
│   │   │   ├── blocking.rs
│   │   │   ├── config.rs
│   │   │   ├── error.rs
│   │   │   ├── forge/
│   │   │   │   ├── auth.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── webhook.rs
│   │   │   ├── lib.rs
│   │   │   ├── main.rs
│   │   │   ├── mcp/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── service.rs
│   │   │   │   └── tools.rs
│   │   │   ├── metrics.rs
│   │   │   ├── middleware/
│   │   │   │   ├── auth.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── tenant.rs
│   │   │   ├── scim/
│   │   │   │   ├── auth.rs
│   │   │   │   ├── discovery.rs
│   │   │   │   ├── error.rs
│   │   │   │   ├── groups.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── tokens.rs
│   │   │   │   └── users.rs
│   │   │   ├── service_accounts/
│   │   │   │   ├── auth.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── routes.rs
│   │   │   ├── web.rs
│   │   │   └── ws.rs
│   │   └── tests/
│   │       ├── api_key_auth.rs
│   │       ├── archived_hidden.rs
│   │       ├── cascade_preview.rs
│   │       ├── client_roundtrip.rs
│   │       ├── common/
│   │       │   └── mod.rs
│   │       ├── entities.rs
│   │       ├── file_backlog.rs
│   │       ├── forge_connections.rs
│   │       ├── forge_webhook.rs
│   │       ├── mcp.rs
│   │       ├── meta.rs
│   │       ├── metrics.rs
│   │       ├── middleware.rs
│   │       ├── openapi.rs
│   │       ├── org_endpoints.rs
│   │       ├── repositories_api.rs
│   │       ├── scim.rs
│   │       ├── search_endpoint.rs
│   │       ├── service_account_mgmt.rs
│   │       ├── task_move.rs
│   │       ├── task_repositories.rs
│   │       ├── team_pages.rs
│   │       ├── tenant_isolation.rs
│   │       ├── web.rs
│   │       └── ws_events.rs
│   ├── kairos-soak/
│   │   └── src/
│   │       ├── auth.rs
│   │       ├── config.rs
│   │       ├── main.rs
│   │       ├── mcp.rs
│   │       ├── prom.rs
│   │       ├── report.rs
│   │       ├── stats.rs
│   │       ├── workforce.rs
│   │       └── world.rs
│   └── kairos-web/
│       └── src/
│           ├── api.rs
│           ├── app.rs
│           ├── auth.rs
│           ├── lib.rs
│           ├── main.rs
│           ├── pages/
│           │   ├── activity.rs
│           │   ├── admin/
│           │   │   ├── api.rs
│           │   │   ├── boards.rs
│           │   │   ├── capabilities.rs
│           │   │   ├── gating.rs
│           │   │   ├── members.rs
│           │   │   ├── metadata.rs
│           │   │   ├── repositories.rs
│           │   │   ├── streams.rs
│           │   │   ├── teams.rs
│           │   │   └── templates.rs
│           │   ├── admin.rs
│           │   ├── boards/
│           │   │   ├── data.rs
│           │   │   └── live.rs
│           │   ├── boards.rs
│           │   ├── copy_link.rs
│           │   ├── editor.rs
│           │   ├── item/
│           │   │   ├── api.rs
│           │   │   ├── create_doc.rs
│           │   │   ├── delete.rs
│           │   │   ├── editor.rs
│           │   │   ├── markdown.rs
│           │   │   └── metadata.rs
│           │   ├── item.rs
│           │   ├── repositories/
│           │   │   └── api.rs
│           │   ├── repositories.rs
│           │   ├── search/
│           │   │   ├── data.rs
│           │   │   ├── graph.rs
│           │   │   ├── graph_layout.rs
│           │   │   └── relationships.rs
│           │   ├── search.rs
│           │   ├── teams/
│           │   │   ├── api.rs
│           │   │   └── doc.rs
│           │   └── teams.rs
│           └── pages.rs
├── e2e/
│   ├── helpers/
│   │   ├── api.ts
│   │   ├── auth.ts
│   │   └── drag.ts
│   ├── playwright.config.ts
│   └── tests/
│       ├── archived.spec.ts
│       ├── drag.spec.ts
│       ├── forge.spec.ts
│       ├── graph.spec.ts
│       ├── lanes.spec.ts
│       ├── lifecycle.spec.ts
│       ├── metadata.spec.ts
│       ├── progress.spec.ts
│       ├── repositories.spec.ts
│       ├── smoke.spec.ts
│       ├── team-lens.spec.ts
│       └── teampages.spec.ts
├── plugin/
│   └── hooks/
│       ├── session_start.py
│       └── test_session_start.py
└── uat/
    ├── checks/
    │   └── zz-surface-coverage.check.ts
    ├── fixtures/
    │   └── team.ts
    ├── journeys/
    │   ├── agent-loop.journey.ts
    │   ├── audit-trail.journey.ts
    │   ├── board-setup.journey.ts
    │   ├── cross-team.journey.ts
    │   ├── decision-record.journey.ts
    │   ├── explorer.journey.ts
    │   ├── first-week.journey.ts
    │   ├── growing-team.journey.ts
    │   ├── housekeeping.journey.ts
    │   ├── incident.journey.ts
    │   ├── machine-access.journey.ts
    │   ├── new-kind-of-work.journey.ts
    │   ├── onboarding.journey.ts
    │   ├── operations.journey.ts
    │   ├── planning.journey.ts
    │   ├── quarterly-review.journey.ts
    │   ├── reorg.journey.ts
    │   ├── second-tenant.journey.ts
    │   ├── smoke.journey.ts
    │   └── team-knowledge.journey.ts
    ├── personas/
    │   ├── credentials.ts
    │   └── index.ts
    ├── playwright.config.ts
    ├── run/
    │   ├── context.ts
    │   ├── coverage.ts
    │   ├── ledger.ts
    │   ├── narrate.ts
    │   └── reporter.ts
    └── surfaces/
        ├── api.ts
        ├── auth.ts
        ├── cli.ts
        ├── forge.ts
        ├── gui.ts
        └── mcp.ts
```

## Modules

### crates/kairos-cli/src/commands

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-cli/src/commands/admin.rs

- pub `AdminCommand` enum L14-18 — `Tenants` — Deployment-administration operations.
- pub `TenantsCommand` enum L22-50 — `List | Create | Delete` — Provision, list, and drop tenants.
- pub `run` function L53-57 — `(self) -> Result<(), CliError>` — server's `KAIROS_DEPLOYMENT_ADMINS`, otherwise 403 with that guidance).
- pub `run` function L61-139 — `(self) -> Result<(), CliError>` — server's `KAIROS_DEPLOYMENT_ADMINS`, otherwise 403 with that guidance).
-  `AdminCommand` type L52-58 — `= AdminCommand` — server's `KAIROS_DEPLOYMENT_ADMINS`, otherwise 403 with that guidance).
-  `TenantsCommand` type L60-140 — `= TenantsCommand` — server's `KAIROS_DEPLOYMENT_ADMINS`, otherwise 403 with that guidance).

#### crates/kairos-cli/src/commands/boards.rs

- pub `BoardsCommand` enum L14-24 — `List | Show` — Operations on boards.
- pub `run` function L27-66 — `(self) -> Result<(), CliError>` — `GET /api/boards` + `GET /api/boards/{id}/items`).
-  `BoardsCommand` type L26-67 — `= BoardsCommand` — `GET /api/boards` + `GET /api/boards/{id}/items`).
-  `print_board_items` function L71-115 — `(items: &BoardItemsResponse)` — The human board rendering: one section per column (in position order),

#### crates/kairos-cli/src/commands/entities.rs

- pub `ListArgs` struct L28-37 — `{ limit: Option<i64>, offset: Option<i64>, common: Common }` — `?limit=&offset=` pagination flags for the `list` verbs.
- pub `page` function L40-45 — `(&self) -> Pagination` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `EntityListArgs` struct L57-70 — `{ limit: Option<i64>, offset: Option<i64>, include_deleted: bool, common: Common...` — `list` arguments for the five entity families: pagination plus the
- pub `query` function L73-79 — `(&self) -> ListQuery` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `GetArgs` struct L84-89 — `{ short_code: String, common: Common }` — Arguments of the `get` verbs.
- pub `EditArgs` struct L96-114 — `{ short_code: String, title: Option<String>, content: Option<String>, content_fi...` — Arguments of the `edit` verbs — the KAIROS-A-0004 optimistic-concurrency
- pub `build_request` function L118-147 — `( &self, current_version: i32, current_content: &str, ) -> Result<UpdateContentR...` — The PATCH body: flags merged over the fetched current entity.
- pub `TransitionArgs` struct L152-161 — `{ short_code: String, to_column: String, common: Common }` — Arguments of the `transition` verbs.
- pub `MoveArgs` struct L165-176 — `{ short_code: String, to_board: String, common: Common }` — Arguments of `kairos tasks move` (KAIROS-I-0012).
- pub `DeleteArgs` struct L180-188 — `{ short_code: String, confirm: bool, common: Common }` — Arguments of the `delete` verbs (soft delete, KAIROS-A-0001 cascade).
- pub `require_confirm` function L191-199 — `(confirm: bool, what: &str) -> Result<(), CliError>` — The client-side `--confirm` guard for destructive verbs.
- pub `EntityView` interface L208-228 — `{ fn short_code(), fn title(), fn version(), fn content(), fn column_id(), fn ar...` — The rendering surface the five entity DTOs share: identity, versioning,
- pub `emit_list` function L461-492 — `( common: &Common, envelope: &ListEnvelope<T>, ) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_get` function L494-508 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_created` function L510-522 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_edited` function L524-535 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_transitioned` function L537-548 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_moved` function L553-562 — `(common: &Common, task: &Task) -> Result<(), CliError>` — Render a board move (KAIROS-I-0012).
- pub `emit_restored` function L565-582 — `(common: &Common, response: &RestoreResponse) -> Result<(), CliError>` — `restore` output: what came back, and what deliberately did not.
- pub `emit_deleted` function L584-599 — `(common: &Common, response: &DeleteResponse) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `StrategyCreateArgs` struct L607-625 — `{ board: String, column: Option<String>, title: String, content: String, hypothe...` — Arguments of `kairos strategies create`.
- pub `InitiativeCreateArgs` struct L641-662 — `{ board: String, column: Option<String>, title: String, content: String, complex...` — Arguments of `kairos initiatives create`.
- pub `TaskCreateArgs` struct L679-710 — `{ board: Option<String>, column: Option<String>, title: String, content: String,...` — Arguments of `kairos tasks create`.
- pub `DocumentCreateArgs` struct L729-746 — `{ title: String, parent: String, content: Option<String>, template: Option<Strin...` — Arguments of `kairos documents create`.
- pub `AdrCreateArgs` struct L761-782 — `{ title: String, board: Option<String>, column: Option<String>, content: String,...` — Arguments of `kairos adrs create`.
-  `ListArgs` type L39-46 — `= ListArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `EntityListArgs` type L72-80 — `= EntityListArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `EditArgs` type L116-148 — `= EditArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L210 — `: &'static str` — Human noun ("task", "initiative", ...).
-  `HEADERS` variable L212 — `: &'static [&'static str]` — Column headers of the `list` table.
-  `or_dash` function L230-232 — `(value: &Option<String>) -> String` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Strategy` type L234-275 — `impl EntityView for Strategy` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L235 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L236 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L238-240 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L241-243 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L244-246 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L247-249 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L250-252 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `archived_at` function L253-255 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L256-263 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L264-274 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Initiative` type L277-322 — `impl EntityView for Initiative` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L278 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L279-280 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L282-284 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L285-287 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L288-290 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L291-293 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L294-296 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `archived_at` function L297-299 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L300-309 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L310-321 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Task` type L324-367 — `impl EntityView for Task` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L325 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L326 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L328-330 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L331-333 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L334-336 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L337-339 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L340-342 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `archived_at` function L343-345 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L346-354 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L355-366 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Document` type L369-410 — `impl EntityView for Document` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L370 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L371 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L373-375 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L376-378 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L379-381 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L382-384 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L385-387 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `archived_at` function L388-390 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L391-399 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L400-409 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Adr` type L412-455 — `impl EntityView for Adr` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L413 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L414 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L416-418 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L419-421 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L422-424 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L425-427 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L428-430 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `archived_at` function L431-433 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L434-442 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L443-454 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `StrategyCreateArgs` type L627-637 — `= StrategyCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L628-636 — `(&self) -> CreateStrategyRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `InitiativeCreateArgs` type L664-675 — `= InitiativeCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L665-674 — `(&self) -> CreateInitiativeRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `TaskCreateArgs` type L712-725 — `= TaskCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L713-724 — `(&self) -> CreateTaskRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `DocumentCreateArgs` type L748-757 — `= DocumentCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L749-756 — `(&self) -> CreateDocumentRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `AdrCreateArgs` type L784-795 — `= AdrCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L785-794 — `(&self) -> CreateAdrRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `entity_family_cli` macro L807-907 — `-` — Generate the per-family `Subcommand` enum and its `run` dispatcher over
-  `tests` module L975-1032 — `-` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `edit_request_building` function L983-1022 — `()` — The edit flow's request building: flags merge over the fetched
-  `delete_requires_confirm` function L1026-1031 — `()` — Deletes refuse to run without --confirm.

#### crates/kairos-cli/src/commands/keys.rs

- pub `KeysCommand` enum L13-49 — `Create | List | Revoke` — API keys for a service account (org-admin operations).
- pub `run` function L67-123 — `(self) -> Result<(), CliError>` — KAIROS-T-0060).
-  `key_table` function L51-64 — `(keys: &[ApiKey]) -> Table` — KAIROS-T-0060).
-  `KeysCommand` type L66-124 — `= KeysCommand` — KAIROS-T-0060).

#### crates/kairos-cli/src/commands/members.rs

- pub `MembersCommand` enum L13-50 — `List | Add | SetRole | Remove` — Organization membership (org-admin operations).
- pub `run` function L67-137 — `(self) -> Result<(), CliError>` — org-admin gated; the last-admin guard surfaces as 422 `LAST_ADMIN`).
-  `member_table` function L52-64 — `(members: &[OrgMember]) -> Table` — org-admin gated; the last-admin guard surfaces as 422 `LAST_ADMIN`).
-  `MembersCommand` type L66-138 — `= MembersCommand` — org-admin gated; the last-admin guard surfaces as 422 `LAST_ADMIN`).

#### crates/kairos-cli/src/commands/mod.rs

- pub `admin` module L5 — `-` — KAIROS-A-0015): one module per noun, all thin veneers — flag plumbing
- pub `boards` module L6 — `-` — and rendering only, no business logic.
- pub `entities` module L7 — `-` — and rendering only, no business logic.
- pub `keys` module L8 — `-` — and rendering only, no business logic.
- pub `members` module L9 — `-` — and rendering only, no business logic.
- pub `orgs` module L10 — `-` — and rendering only, no business logic.
- pub `repos` module L11 — `-` — and rendering only, no business logic.
- pub `search` module L12 — `-` — and rendering only, no business logic.
- pub `service_accounts` module L13 — `-` — and rendering only, no business logic.
- pub `streams` module L14 — `-` — and rendering only, no business logic.
- pub `teams` module L15 — `-` — and rendering only, no business logic.

#### crates/kairos-cli/src/commands/orgs.rs

- pub `OrgsCommand` enum L12-19 — `Show` — Organization info for your credentials.
- pub `run` function L22-36 — `(self) -> Result<(), CliError>` — KAIROS-T-0037).
-  `OrgsCommand` type L21-37 — `= OrgsCommand` — KAIROS-T-0037).

#### crates/kairos-cli/src/commands/repos.rs

- pub `ReposCommand` enum L17-106 — `List | Get | Create | Update | Delete | Bind | Unbind` — Operations on repositories.
- pub `run` function L135-315 — `(self) -> Result<(), CliError>` — it (and to the owning team's delivery board).
-  `repo_table` function L108-132 — `(repos: &[Repository]) -> Table` — it (and to the owning team's delivery board).
-  `ReposCommand` type L134-316 — `= ReposCommand` — it (and to the owning team's delivery board).

#### crates/kairos-cli/src/commands/search.rs

- pub `SearchArgs` struct L21-101 — `{ query: Option<String>, entity_type: Vec<String>, board: Option<String>, column...` — Search and traverse all entity types (POST /api/search).
- pub `build_request` function L133-214 — `(&self) -> Result<SearchRequest, CliError>` — Compose the S-0005 request body from the flags (or take
- pub `run` function L273-282 — `(self) -> Result<(), CliError>` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `SearchArgs` type L103-283 — `= SearchArgs` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `has_flag_query` function L106-129 — `(&self) -> bool` — Whether any flag other than `--query-json` (and the pagination
-  `parse_metadata` function L217-231 — `(&self) -> Result<Option<BTreeMap<String, String>>, CliError>` — `--metadata k=v` pairs into the S-0005 metadata map.
-  `build_traverse` function L236-271 — `(&self) -> Result<Option<SearchTraverse>, CliError>` — The traverse clause: `--from`/`--from-id` anchor it; the companion
-  `non_empty` function L285-287 — `(values: &[String]) -> Option<Vec<String>>` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `print_results` function L294-328 — `(response: &SearchResponse)` — The human rendering: one CODE/TITLE/VER section per non-empty group.
-  `section` function L295-313 — `(label: &str, items: &[T])` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `tests` module L331-481 — `-` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `flags_compose_the_s0005_body` function L337-374 — `()` — The filter flags compose the S-0005 body: metadata k=v parsing,
-  `traverse_flags` function L380-418 — `()` — Traversal flags: anchored by --from, direction defaults to
-  `repo_alone_is_a_filter` function L425-436 — `()` — `kairos search --repo <slug>` on its own is a complete request
-  `query_json_escape_hatch` function L442-480 — `()` — --query-json is the verbatim escape hatch: parsed as the full

#### crates/kairos-cli/src/commands/service_accounts.rs

- pub `ServiceAccountsCommand` enum L14-40 — `Create | List | Delete` — Service accounts (org-admin operations).
- pub `run` function L51-97 — `(self) -> Result<(), CliError>` — `kairos-client`.
-  `account_table` function L42-48 — `(accounts: &[ServiceAccount]) -> Table` — `kairos-client`.
-  `ServiceAccountsCommand` type L50-98 — `= ServiceAccountsCommand` — `kairos-client`.

#### crates/kairos-cli/src/commands/streams.rs

- pub `StreamsCommand` enum L15-68 — `List | Get | Create | Update | Delete | Teams` — Operations on delivery streams.
- pub `StreamTeamsCommand` enum L72-100 — `List | Add | Remove` — Team membership of one delivery stream.
- pub `run` function L119-218 — `(self) -> Result<(), CliError>` — `/api/delivery-streams`).
- pub `run` function L222-275 — `(self) -> Result<(), CliError>` — `/api/delivery-streams`).
-  `stream_table` function L102-116 — `(streams: &[DeliveryStream]) -> Table` — `/api/delivery-streams`).
-  `StreamsCommand` type L118-219 — `= StreamsCommand` — `/api/delivery-streams`).
-  `StreamTeamsCommand` type L221-276 — `= StreamTeamsCommand` — `/api/delivery-streams`).

#### crates/kairos-cli/src/commands/teams.rs

- pub `TeamsCommand` enum L15-69 — `List | Get | Create | Update | Delete | Members` — Operations on teams.
- pub `TeamMembersCommand` enum L73-101 — `List | Add | Remove` — Membership of one team.
- pub `run` function L134-233 — `(self) -> Result<(), CliError>` — creating a team also creates its delivery board, KAIROS-A-0002).
- pub `run` function L237-284 — `(self) -> Result<(), CliError>` — creating a team also creates its delivery board, KAIROS-A-0002).
-  `team_table` function L104-118 — `(teams: &[Team]) -> Table` — The shared team table (list + single-row `get`).
-  `member_table` function L120-131 — `(members: &[TeamMember]) -> Table` — creating a team also creates its delivery board, KAIROS-A-0002).
-  `TeamsCommand` type L133-234 — `= TeamsCommand` — creating a team also creates its delivery board, KAIROS-A-0002).
-  `TeamMembersCommand` type L236-285 — `= TeamMembersCommand` — creating a team also creates its delivery board, KAIROS-A-0002).

### crates/kairos-cli/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-cli/src/context.rs

- pub `Common` struct L17-27 — `{ url: Option<String>, tenant: Option<String>, json: bool }` — Flags shared by every command that talks to the API.
- pub `client` function L32-44 — `(common: &Common) -> Result<KairosClient, CliError>` — A `KairosClient` for the resolved deployment, drawing (auto-refreshing)
- pub `print_json` function L47-54 — `(value: &T) -> Result<(), CliError>` — Pretty-print a DTO as JSON (the `--json` scripting surface).

#### crates/kairos-cli/src/credentials.rs

- pub `EXPIRY_SKEW_SECONDS` variable L18 — `: u64` — Refresh this many seconds BEFORE the recorded expiry, so a token never
- pub `DeploymentCredentials` struct L22-42 — `{ access_token: String, refresh_token: Option<String>, expires_at: u64, issuer: ...` — One deployment's cached credentials.
- pub `needs_refresh` function L47-49 — `(&self, now: u64) -> bool` — True when the access token is expired (or within the skew window)
- pub `CredentialStore` struct L54-60 — `{ version: u32, deployments: BTreeMap<String, DeploymentCredentials> }` — The on-disk credential store: `{ "version": 1, "deployments": { url: … } }`.
- pub `unix_now` function L76-81 — `() -> u64` — Unix seconds now.
- pub `normalize_url` function L85-87 — `(url: &str) -> String` — Normalize a deployment URL into a cache key: trim whitespace and
- pub `config_dir` function L90-109 — `() -> Result<PathBuf, CliError>` — The kairos config directory (see module docs for resolution order).
- pub `credentials_path` function L112-114 — `() -> Result<PathBuf, CliError>` — `credentials.json` inside the config directory.
- pub `load` function L119-140 — `(path: &Path) -> Result<CredentialStore, CliError>` — Load the store from `path`.
- pub `save` function L144-159 — `(path: &Path, store: &CredentialStore) -> Result<(), CliError>` — Write the store to `path` atomically (temp file + rename) with mode
- pub `entry_for` function L201-208 — `(store: &CredentialStore, url: &str) -> Result<DeploymentCredentials, CliError>` — The cached entry for `url` (normalized), or the actionable "not logged
- pub `resolve_deployment` function L212-230 — `(store: &CredentialStore, url: Option<&str>) -> Result<String, CliError>` — Resolve which deployment a command targets: the explicit `--url`, or
-  `DeploymentCredentials` type L44-50 — `= DeploymentCredentials` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `CredentialStore` type L62-69 — `impl Default for CredentialStore` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `default` function L63-68 — `() -> Self` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `default_version` function L71-73 — `() -> u32` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `create_private_dir` function L162-176 — `(dir: &Path) -> Result<(), CliError>` — Create `dir` (and parents) with owner-only permissions.
-  `write_private_file` function L179-197 — `(path: &Path, contents: &str) -> std::io::Result<()>` — Write `contents` to `path` with mode 0600 (KAIROS-A-0010).
-  `tests` module L233-371 — `-` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `scratch_dir` function L237-244 — `(name: &str) -> PathBuf` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `entry` function L246-256 — `(tenant: Option<&str>) -> DeploymentCredentials` — (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.
-  `save_load_round_trip_with_0600` function L261-301 — `()` — Round trip: save writes 0600 under a 0700 dir; load returns the
-  `missing_and_corrupted_cache` function L306-319 — `()` — Missing file = empty store; corrupted file = auth error (exit 2)
-  `needs_refresh_skew` function L324-333 — `()` — The skew window: fresh tokens pass, tokens expiring within the
-  `deployment_resolution` function L338-370 — `()` — --url picking: explicit wins, single entry is implied, none is an

#### crates/kairos-cli/src/error.rs

- pub `EXIT_FAILURE` variable L8 — `: u8` — Exit code for API/validation/transport failures (KAIROS-A-0015).
- pub `EXIT_AUTH` variable L11 — `: u8` — Exit code for authentication failures (KAIROS-A-0015): missing/expired
- pub `CliError` enum L15-23 — `Auth | Failure` — A failed CLI invocation.
- pub `exit_code` function L27-32 — `(&self) -> u8` — The KAIROS-A-0015 process exit code for this error.
-  `CliError` type L25-33 — `= CliError` — 0 success · 1 API/validation error · 2 auth error.
-  `CliError` type L35-132 — `= CliError` — 0 success · 1 API/validation error · 2 auth error.
-  `from` function L43-131 — `(err: ApiError) -> Self` — Map the typed client error to the exit-code contract: 401s and
-  `tests` module L135-271 — `-` — 0 success · 1 API/validation error · 2 auth error.
-  `exit_code_contract` function L142-172 — `()` — 401s and token-provider failures exit 2; other API errors exit 1
-  `forbidden_names_the_capability` function L177-187 — `()` — 403 with a KAIROS-A-0006 capability check names the capability
-  `forbidden_deployment_admin_hint` function L192-204 — `()` — The deployment-admin gate's 403 points at KAIROS_DEPLOYMENT_ADMINS
-  `conflict_renders_current_version_guidance` function L209-222 — `()` — 409 CONFLICT renders the server-current version guidance
-  `invalid_transition_lists_allowed_targets` function L226-254 — `()` — 422 INVALID_TRANSITION lists the allowed target columns, name + id.
-  `last_admin_is_surfaced_with_guidance` function L258-270 — `()` — 422 LAST_ADMIN gets the keep-one-admin guidance (KAIROS-T-0037).

#### crates/kairos-cli/src/main.rs

-  `commands` module L26 — `-` — `kairos-client`), per KAIROS-A-0015.
-  `context` module L27 — `-` — 2 auth error.
-  `credentials` module L28 — `-` — 2 auth error.
-  `error` module L29 — `-` — 2 auth error.
-  `oidc` module L30 — `-` — 2 auth error.
-  `provider` module L31 — `-` — 2 auth error.
-  `table` module L32 — `-` — 2 auth error.
-  `Cli` struct L66-69 — `{ command: Command }` — 2 auth error.
-  `Command` enum L72-155 — `Login | Logout | Whoami | Orgs | Boards | Strategies | Initiatives | Tasks | Doc...` — 2 auth error.
-  `main` function L158-167 — `() -> ExitCode` — 2 auth error.
-  `run` function L169-196 — `(command: Command) -> Result<(), CliError>` — 2 auth error.
-  `login` function L200-292 — `( url: &str, issuer_override: Option<&str>, tenant: Option<String>, client_id: S...` — `kairos login` — discover the issuer, run the device grant, cache the
-  `load_store_for_login` function L296-307 — `(path: &std::path::Path) -> CredentialStore` — The store to merge a fresh login into: a corrupted cache is NOT fatal
-  `logout` function L310-334 — `(url: Option<&str>) -> Result<(), CliError>` — `kairos logout` — drop the deployment's entry from the cache.
-  `whoami` function L337-364 — `( url: Option<&str>, tenant_override: Option<String>, json: bool, ) -> Result<()...` — `kairos whoami` — the identity probe via `kairos-client`.
-  `print_identity` function L368-383 — `(identity: &WhoamiResponse)` — The human-readable `whoami` rendering: user, org, role, teams
-  `tests` module L386-693 — `-` — 2 auth error.
-  `smoke` function L390-392 — `()` — 2 auth error.
-  `cli_parses` function L397-458 — `()` — The clap surface parses per KAIROS-A-0015: login/logout/whoami with
-  `command_tree_parses` function L463-692 — `()` — The KAIROS-T-0037 command tree parses: every A-0015 noun with its

#### crates/kairos-cli/src/oidc.rs

- pub `SCOPES` variable L19 — `: &str` — The scopes requested at login: identity claims + `offline_access` for
- pub `ApiBearer` enum L28-36 — `AccessToken | IdToken` — Which OIDC token the CLI caches and sends as the `/api` bearer
- pub `IssuerEndpoints` struct L44-47 — `{ token_endpoint: String, device_authorization_endpoint: String }` — The endpoints the device flow needs, from OIDC discovery.
- pub `DeviceAuthorization` struct L51-63 — `{ device_code: String, user_code: String, verification_uri: String, verification...` — RFC 8628 device authorization response.
- pub `TokenResponse` struct L67-77 — `{ access_token: String, id_token: Option<String>, refresh_token: Option<String>,...` — A successful token response (device grant or refresh grant).
- pub `bearer_for` function L83-88 — `(&self, kind: ApiBearer) -> Option<&str>` — The token to cache/send as the `/api` bearer for the given selection.
- pub `PollOutcome` enum L93-106 — `Token | Pending | SlowDown | Denied | Expired | Fatal` — One poll of the token endpoint, classified per RFC 8628 §3.5.
- pub `classify_poll_response` function L109-141 — `(status: u16, body: &str) -> PollOutcome` — Classify a token-endpoint poll response (pure; unit-tested).
- pub `discover_issuer` function L145-184 — `( http: &reqwest::Client, deployment_url: &str, ) -> Result<String, CliError>` — Discover the deployment's OIDC issuer from its RFC 9728
- pub `discover_endpoints` function L187-229 — `( http: &reqwest::Client, issuer: &str, ) -> Result<IssuerEndpoints, CliError>` — OIDC discovery: the issuer's token + device-authorization endpoints.
- pub `start_device_grant` function L232-259 — `( http: &reqwest::Client, endpoints: &IssuerEndpoints, client_id: &str, ) -> Res...` — Start the Device Authorization Grant (RFC 8628 §3.1).
- pub `poll_device_grant` function L263-313 — `( http: &reqwest::Client, endpoints: &IssuerEndpoints, client_id: &str, grant: &...` — Poll the token endpoint until the user approves, declines, or the code
- pub `refresh_grant` function L317-342 — `( http: &reqwest::Client, token_endpoint: &str, client_id: &str, refresh_token: ...` — The refresh grant (RFC 6749 §6).
-  `PROTECTED_RESOURCE_PATH` variable L40 — `: &str` — The path-suffixed RFC 9728 well-known document for the `/mcp` resource
-  `TokenResponse` type L79-89 — `= TokenResponse` — 4.
-  `OAuthError` struct L119-123 — `{ error: String, error_description: Option<String> }` — 4.
-  `ProtectedResource` struct L166-169 — `{ authorization_servers: Vec<String> }` — 4.
-  `Discovery` struct L207-211 — `{ token_endpoint: String, device_authorization_endpoint: Option<String> }` — 4.
-  `tests` module L345-431 — `-` — 4.
-  `poll_classification` function L350-401 — `()` — Every RFC 8628 poll outcome classifies correctly.
-  `token_response_without_refresh` function L406-410 — `()` — A missing refresh_token in a token response stays None (some IdPs
-  `bearer_selection` function L415-430 — `()` — `bearer_for` picks the right token; `id_token` mode fails cleanly when

#### crates/kairos-cli/src/provider.rs

- pub `CachedTokenProvider` struct L19-28 — `{ path: PathBuf, deployment: String, http: reqwest::Client, refresh_lock: Mutex<...` — A refreshing token provider over one deployment's cache entry.
- pub `new` function L31-38 — `(path: PathBuf, deployment: String) -> Self` — `main` maps to exit code 2 (KAIROS-A-0015).
-  `CachedTokenProvider` type L30-112 — `= CachedTokenProvider` — `main` maps to exit code 2 (KAIROS-A-0015).
-  `current_token` function L42-111 — `(&self) -> Result<String, Error>` — The current access token, refreshed (and persisted) when it is
-  `CachedTokenProvider` type L114-118 — `impl TokenProvider for CachedTokenProvider` — `main` maps to exit code 2 (KAIROS-A-0015).
-  `bearer_token` function L115-117 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — `main` maps to exit code 2 (KAIROS-A-0015).

#### crates/kairos-cli/src/table.rs

- pub `Table` struct L7-10 — `{ headers: Vec<String>, rows: Vec<Vec<String>> }` — A simple text table: headers plus rows, rendered with columns padded to
- pub `new` function L14-19 — `(headers: &[&str]) -> Self` — A table with the given column headers.
- pub `row` function L23-25 — `(&mut self, cells: Vec<String>)` — Append one row.
- pub `render` function L29-69 — `(&self) -> String` — Render with each column padded to its widest cell (headers
-  `Table` type L12-70 — `= Table` — dependency; `--json` is the scripting surface).
-  `tests` module L73-99 — `-` — dependency; `--json` is the scripting surface).
-  `renders_aligned_columns` function L79-91 — `()` — Columns align to the widest cell, headers included; no trailing
-  `renders_empty_table` function L95-98 — `()` — A header-only table renders just the header line.

### crates/kairos-cli/tests

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-cli/tests/cli_live.rs

-  `SCRATCH_DB` variable L42 — `: &str` — Uniquely named scratch database for this test binary (shared-services
-  `ISSUER` variable L44 — `: &str` — The live dev/test issuer (`.angreal/dex/config.yaml`).
-  `AUDIENCE` variable L47 — `: &str` — Tokens are minted through the `kairos-cli` public client, so `aud` is
-  `TENANT` variable L48 — `: &str` — credentials to the form's URL, landing on `/device/callback`.
-  `DEFAULT_DATABASE_URL` variable L49 — `: &str` — credentials to the form's URL, landing on `/device/callback`.
-  `admin_database_url` function L51-53 — `() -> String` — credentials to the form's URL, landing on `/device/callback`.
-  `with_database` function L55-58 — `(url: &str, db_name: &str) -> String` — credentials to the form's URL, landing on `/device/callback`.
-  `run_cli` function L62-75 — `(config_dir: &std::path::Path, args: &[&str]) -> (i32, String, String)` — Run the `kairos` binary with `KAIROS_CONFIG_DIR` pointed at the test's
-  `approve_device_grant` function L79-123 — `(user_code: &str)` — Approve a pending device grant headlessly: submit the user code, then
-  `alice_password_token` function L127-149 — `() -> String` — A real alice token via the password grant (JIT provisioning + fixture
-  `cli_login_whoami_refresh_logout_live` function L152-433 — `()` — credentials to the form's URL, landing on `/device/callback`.

#### crates/kairos-cli/tests/cli_tree_live.rs

-  `SCRATCH_DB` variable L34 — `: &str` — Uniquely named scratch database for this test binary (shared-services
-  `ISSUER` variable L36 — `: &str` — The live dev/test issuer (`.angreal/dex/config.yaml`).
-  `AUDIENCE` variable L37 — `: &str` — members list → orgs show → teams list.
-  `TENANT` variable L38 — `: &str` — members list → orgs show → teams list.
-  `DEFAULT_DATABASE_URL` variable L39 — `: &str` — members list → orgs show → teams list.
-  `admin_database_url` function L41-43 — `() -> String` — members list → orgs show → teams list.
-  `with_database` function L45-48 — `(url: &str, db_name: &str) -> String` — members list → orgs show → teams list.
-  `run_cli` function L52-65 — `(config_dir: &std::path::Path, args: &[&str]) -> (i32, String, String)` — Run the `kairos` binary with `KAIROS_CONFIG_DIR` pointed at the test's
-  `approve_device_grant` function L70-107 — `(user_code: &str)` — Approve a pending device grant headlessly (T-0036's proven recipe):
-  `alice_password_token` function L111-133 — `() -> String` — A real alice token via the password grant (JIT provisioning + fixture
-  `cli_login` function L137-172 — `(config_dir: &std::path::Path, base_url: &str)` — Drive `kairos login` with the headless device-grant approval; panics on
-  `cli_command_tree_golden_path_live` function L175-636 — `()` — members list → orgs show → teams list.

### crates/kairos-client/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-client/src/client.rs

- pub `TokenProvider` interface L60-64 — `{ fn bearer_token() }` — Supplies the bearer token for each request.
- pub `StaticToken` struct L68 — `-` — A fixed, never-refreshed token.
- pub `EntityKind` enum L80-86 — `Strategy | Initiative | Task | Document | Adr` — The five S-0005 entity families, for the endpoints that are generic
- pub `path_segment` function L90-98 — `(self) -> &'static str` — The URL path segment of the family (`/api/{segment}/...`).
- pub `KairosClient` struct L109-116 — `{ http: reqwest::Client, base_url: String, token: Arc<dyn TokenProvider>, tenant...` — The typed Kairos API client.
- pub `new` function L129-140 — `(base_url: impl Into<String>, token: Arc<dyn TokenProvider>) -> Self` — A client against `base_url` drawing tokens from `token`.
- pub `with_static_token` function L143-145 — `(base_url: impl Into<String>, token: impl Into<String>) -> Self` — A client with a fixed bearer token (integration tests).
- pub `with_tenant` function L150-153 — `(mut self, slug: impl Into<String>) -> Self` — Send `X-Tenant: {slug}` on every request (dev/test tenant
- pub `base_url` function L156-158 — `(&self) -> &str` — The configured base URL (no trailing slash).
- pub `tenant` function L161-163 — `(&self) -> Option<&str>` — The configured `X-Tenant` value, if any.
- pub `raw_request` function L296-318 — `( &self, method: Method, path_and_query: &str, body: Option<&Value>, ) -> Result...` — Protocol-level escape hatch: send an authorized request with an
- pub `set_task_work_class` function L448-460 — `( &self, short_code: &str, work_class: &str, ) -> Result<Task, Error>` — `POST /api/tasks/{short_code}/work-class` — move a task between
- pub `move_task` function L468-476 — `(&self, short_code: &str, board: &str) -> Result<Task, Error>` — `PUT /api/tasks/{short_code}/repository` — bind the task to a
- pub `set_task_repository` function L478-490 — `( &self, short_code: &str, repository: Option<&str>, ) -> Result<Task, Error>` — typed surface deliberately cannot express.
- pub `board_items_for_repository` function L495-506 — `( &self, board_id: &str, repository: &str, ) -> Result<BoardItemsResponse, Error...` — `GET /api/boards/{id}/items?repository=` — the board narrowed to
- pub `list_boards` function L536-538 — `(&self, page: Pagination) -> Result<ListEnvelope<Board>, Error>` — `GET /api/boards`.
- pub `create_board` function L542-544 — `(&self, request: &CreateBoardRequest) -> Result<BoardDetail, Error>` — `POST /api/boards` — create a board seeded from the system defaults
- pub `get_board` function L547-549 — `(&self, board_id: &str) -> Result<BoardDetail, Error>` — `GET /api/boards/{id}` — board + columns + transition graph.
- pub `update_board` function L552-559 — `( &self, board_id: &str, request: &UpdateBoardRequest, ) -> Result<Board, Error>` — `PATCH /api/boards/{id}` — board settings.
- pub `delete_board` function L562-564 — `(&self, board_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/boards/{id}` (only when empty).
- pub `board_items` function L567-569 — `(&self, board_id: &str) -> Result<BoardItemsResponse, Error>` — `GET /api/boards/{id}/items` — every live item grouped by column.
- pub `board_items_including_archived` function L576-584 — `( &self, board_id: &str, ) -> Result<BoardItemsResponse, Error>` — `GET /api/boards/{id}/items?include_deleted=true` — the board as it
- pub `list_columns` function L587-589 — `(&self, board_id: &str) -> Result<Vec<BoardColumn>, Error>` — `GET /api/boards/{id}/columns`.
- pub `add_column` function L592-599 — `( &self, board_id: &str, request: &CreateColumnRequest, ) -> Result<BoardColumn,...` — `POST /api/boards/{id}/columns`.
- pub `update_column` function L602-613 — `( &self, board_id: &str, column_id: &str, request: &UpdateColumnRequest, ) -> Re...` — `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
- pub `remove_column` function L616-623 — `( &self, board_id: &str, column_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/boards/{id}/columns/{col_id}` (only when empty).
- pub `list_transitions` function L626-629 — `(&self, board_id: &str) -> Result<Vec<BoardTransition>, Error>` — `GET /api/boards/{id}/transitions`.
- pub `add_transition` function L632-639 — `( &self, board_id: &str, request: &CreateTransitionRequest, ) -> Result<BoardTra...` — `POST /api/boards/{id}/transitions`.
- pub `remove_transition` function L642-651 — `( &self, board_id: &str, transition_id: &str, ) -> Result<OrgDeleteResponse, Err...` — `DELETE /api/boards/{id}/transitions/{transition_id}`.
- pub `list_board_members` function L654-656 — `(&self, board_id: &str) -> Result<Vec<BoardMember>, Error>` — `GET /api/boards/{id}/members` — members + capability grants.
- pub `add_board_member` function L660-667 — `( &self, board_id: &str, request: &AddBoardMemberRequest, ) -> Result<BoardMembe...` — `POST /api/boards/{id}/members` — grant capabilities
- pub `replace_capabilities` function L671-682 — `( &self, board_id: &str, user_id: &str, request: &ReplaceCapabilitiesRequest, ) ...` — `PATCH /api/boards/{id}/members/{user_id}` — replace the full
- pub `remove_board_member` function L685-692 — `( &self, board_id: &str, user_id: &str, ) -> Result<RemoveBoardMemberResponse, E...` — `DELETE /api/boards/{id}/members/{user_id}` — revoke everything.
- pub `list_teams` function L697-699 — `(&self, page: Pagination) -> Result<ListEnvelope<Team>, Error>` — `GET /api/teams`.
- pub `create_team` function L703-705 — `(&self, request: &CreateTeamRequest) -> Result<Team, Error>` — `POST /api/teams` — creates the team AND its delivery board
- pub `get_team` function L708-710 — `(&self, team_id: &str) -> Result<Team, Error>` — `GET /api/teams/{id}`.
- pub `update_team` function L713-719 — `( &self, team_id: &str, request: &UpdateTeamRequest, ) -> Result<Team, Error>` — `PATCH /api/teams/{id}`.
- pub `delete_team` function L722-724 — `(&self, team_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/teams/{id}`.
- pub `list_team_members` function L727-729 — `(&self, team_id: &str) -> Result<Vec<TeamMember>, Error>` — `GET /api/teams/{id}/members`.
- pub `add_team_member` function L732-739 — `( &self, team_id: &str, request: &AddTeamMemberRequest, ) -> Result<TeamMember, ...` — `POST /api/teams/{id}/members`.
- pub `remove_team_member` function L742-749 — `( &self, team_id: &str, user_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/teams/{id}/members/{user_id}`.
- pub `list_streams` function L754-759 — `( &self, page: Pagination, ) -> Result<ListEnvelope<DeliveryStream>, Error>` — `GET /api/delivery-streams`.
- pub `create_stream` function L762-767 — `( &self, request: &CreateStreamRequest, ) -> Result<DeliveryStream, Error>` — `POST /api/delivery-streams`.
- pub `get_stream` function L770-773 — `(&self, stream_id: &str) -> Result<DeliveryStream, Error>` — `GET /api/delivery-streams/{id}`.
- pub `update_stream` function L776-783 — `( &self, stream_id: &str, request: &UpdateStreamRequest, ) -> Result<DeliveryStr...` — `PATCH /api/delivery-streams/{id}`.
- pub `delete_stream` function L786-789 — `(&self, stream_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/delivery-streams/{id}`.
- pub `list_stream_teams` function L792-795 — `(&self, stream_id: &str) -> Result<Vec<Team>, Error>` — `GET /api/delivery-streams/{id}/teams`.
- pub `add_stream_team` function L798-805 — `( &self, stream_id: &str, request: &AddStreamTeamRequest, ) -> Result<OrgDeleteR...` — `POST /api/delivery-streams/{id}/teams`.
- pub `remove_stream_team` function L808-817 — `( &self, stream_id: &str, team_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
- pub `create_service_account` function L822-827 — `( &self, request: &crate::types_service_accounts::CreateServiceAccountRequest, )...` — `POST /api/service-accounts` — create a machine principal (org-admin).
- pub `list_service_accounts` function L830-834 — `( &self, ) -> Result<crate::types_service_accounts::ServiceAccountList, Error>` — `GET /api/service-accounts` — list the org's service accounts.
- pub `delete_service_account` function L837-842 — `( &self, id: &str, ) -> Result<crate::types_service_accounts::Deleted, Error>` — `DELETE /api/service-accounts/{id}` — delete a service account + its keys.
- pub `create_api_key` function L845-855 — `( &self, service_account_id: &str, request: &crate::types_service_accounts::Crea...` — `POST /api/service-accounts/{id}/keys` — mint a key (raw returned once).
- pub `list_api_keys` function L858-864 — `( &self, service_account_id: &str, ) -> Result<crate::types_service_accounts::Ap...` — `GET /api/service-accounts/{id}/keys` — list a service account's keys.
- pub `revoke_api_key` function L867-876 — `( &self, service_account_id: &str, key_id: &str, ) -> Result<crate::types_servic...` — `DELETE /api/service-accounts/{id}/keys/{key_id}` — revoke a key.
- pub `list_org_members` function L881-886 — `( &self, page: Pagination, ) -> Result<ListEnvelope<OrgMember>, Error>` — `GET /api/members`.
- pub `add_org_member` function L890-892 — `(&self, request: &AddOrgMemberRequest) -> Result<OrgMember, Error>` — `POST /api/members` — add by email (the user must have logged in
- pub `update_org_member` function L896-903 — `( &self, user_id: &str, request: &UpdateOrgMemberRequest, ) -> Result<OrgMember,...` — `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN`
- pub `remove_org_member` function L906-908 — `(&self, user_id: &str) -> Result<RemoveOrgMemberResponse, Error>` — `DELETE /api/members/{user_id}`.
- pub `list_tenants` function L914-919 — `( &self, page: Pagination, ) -> Result<ListEnvelope<TenantSummary>, Error>` — `GET /api/admin/tenants` (deployment-admin only; cross-tenant, no
- pub `create_tenant` function L922-927 — `( &self, request: &CreateTenantRequest, ) -> Result<TenantCreatedResponse, Error...` — `POST /api/admin/tenants` — provision a tenant (KAIROS-T-0008).
- pub `delete_tenant` function L932-943 — `( &self, slug: &str, confirm: Option<bool>, ) -> Result<TenantDeletedResponse, E...` — `DELETE /api/admin/tenants/{slug}` — destructive; requires
- pub `relationships` function L949-956 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<ItemRelationshipsRespon...` — `GET /api/{family}/{short_code}/relationships` — both directions,
- pub `set_document_lifecycle` function L961-973 — `( &self, short_code: &str, lifecycle: &str, ) -> Result<Document, Error>` — `PATCH /api/documents/{short_code}/lifecycle` — set a document's
- pub `children_progress` function L978-985 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<ChildrenProgressRespons...` — `GET /api/{family}/{short_code}/children-progress` — the direct
- pub `item_links` function L989-995 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<Vec<crate::types_forge:...` — `GET /api/{family}/{short_code}/links` — the branches and
- pub `team_links` function L999-1009 — `( &self, team_id: &str, states: Option<&str>, ) -> Result<Vec<crate::types_forge...` — `GET /api/teams/{id}/links` — the team's in-flight forge links
- pub `list_forge_connections` function L1012-1016 — `( &self, ) -> Result<Vec<crate::types_forge::ForgeConnection>, Error>` — `GET /api/forge-connections` (KAIROS-T-0097).
- pub `get_forge_connection` function L1019-1024 — `( &self, id: &str, ) -> Result<crate::types_forge::ForgeConnection, Error>` — `GET /api/forge-connections/{id}`.
- pub `create_forge_connection` function L1028-1033 — `( &self, request: &crate::types_forge::CreateForgeConnectionRequest, ) -> Result...` — `POST /api/forge-connections` (org admin) — the response carries
- pub `list_repositories` function L1039-1047 — `( &self, team: Option<&str>, ) -> Result<Vec<crate::types_repositories::Reposito...` — `GET /api/repositories[?team=]` — the repository directory (open
- pub `get_repository` function L1051-1056 — `( &self, reference: &str, ) -> Result<crate::types_repositories::RepositoryDetai...` — `GET /api/repositories/{slug}` — the repository, its webhook
- pub `create_repository` function L1060-1065 — `( &self, request: &crate::types_repositories::CreateRepositoryRequest, ) -> Resu...` — `POST /api/repositories` — register a repository under its owning
- pub `update_repository` function L1068-1075 — `( &self, reference: &str, request: &crate::types_repositories::UpdateRepositoryR...` — `PATCH /api/repositories/{slug}`.
- pub `delete_repository` function L1078-1080 — `(&self, reference: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/repositories/{slug}` (org admin; 409 while referenced).
- pub `delete_forge_connection` function L1083-1085 — `(&self, id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/forge-connections/{id}` (org admin).
- pub `rotate_forge_connection` function L1089-1098 — `( &self, id: &str, ) -> Result<crate::types_forge::CreatedForgeConnection, Error...` — `POST /api/forge-connections/{id}/rotate` — mints a new connection
- pub `get_item_graph` function L1103-1114 — `( &self, kind: EntityKind, short_code: &str, depth: Option<u32>, ) -> Result<cra...` — `GET /api/{family}/{short_code}/graph?depth=N` — the focal
- pub `create_relationship` function L1117-1122 — `( &self, request: &CreateRelationshipRequest, ) -> Result<Relationship, Error>` — `POST /api/relationships` (org admin, KAIROS-A-0006).
- pub `delete_relationship` function L1125-1131 — `( &self, relationship_id: &str, ) -> Result<DeletedResponse, Error>` — `DELETE /api/relationships/{id}` (org admin).
- pub `metadata` function L1136-1143 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<ItemMetadataResponse, E...` — `GET /api/{family}/{short_code}/metadata`.
- pub `update_metadata` function L1147-1155 — `( &self, kind: EntityKind, short_code: &str, request: &UpdateMetadataRequest, ) ...` — `PATCH /api/{family}/{short_code}/metadata` — typed upsert; `null`
- pub `get_team_by_slug` function L1162-1164 — `(&self, slug: &str) -> Result<Team, Error>` — `GET /api/teams/by-slug/{slug}`.
- pub `list_team_pages` function L1168-1170 — `(&self, team_id: &str) -> Result<Vec<TeamPage>, Error>` — `GET /api/teams/{id}/pages` — the team's live page tree as a flat
- pub `get_team_page` function L1173-1176 — `(&self, team_id: &str, page_id: &str) -> Result<TeamPage, Error>` — `GET /api/teams/{id}/pages/{page_id}`.
- pub `create_team_page` function L1179-1186 — `( &self, team_id: &str, request: &CreateTeamPageRequest, ) -> Result<TeamPage, E...` — `POST /api/teams/{id}/pages` (team member or org admin).
- pub `update_team_page` function L1190-1198 — `( &self, team_id: &str, page_id: &str, request: &UpdateTeamPageRequest, ) -> Res...` — `PATCH /api/teams/{id}/pages/{page_id}` — content edit
- pub `delete_team_page` function L1202-1209 — `( &self, team_id: &str, page_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/teams/{id}/pages/{page_id}` (soft; folders must be
- pub `list_team_work_documents` function L1214-1220 — `( &self, team_id: &str, ) -> Result<Vec<TeamWorkDocument>, Error>` — `GET /api/teams/{id}/announcements` — pinned first, newest first.
- pub `list_team_announcements` function L1222-1228 — `( &self, team_id: &str, ) -> Result<Vec<TeamAnnouncement>, Error>` — typed surface deliberately cannot express.
- pub `create_team_announcement` function L1232-1239 — `( &self, team_id: &str, request: &CreateTeamAnnouncementRequest, ) -> Result<Tea...` — `POST /api/teams/{id}/announcements` (team member or org admin;
- pub `delete_team_announcement` function L1243-1252 — `( &self, team_id: &str, announcement_id: &str, ) -> Result<OrgDeleteResponse, Er...` — `DELETE /api/teams/{id}/announcements/{announcement_id}` (author
- pub `list_metadata_definitions` function L1255-1260 — `( &self, page: Pagination, ) -> Result<ListEnvelope<MetadataDefinition>, Error>` — `GET /api/metadata-definitions`.
- pub `list_metadata_definitions_for` function L1265-1288 — `( &self, page: Pagination, entity_type: Option<&str>, ) -> Result<ListEnvelope<M...` — `GET /api/metadata-definitions?entity_type=…` — the catalog in
- pub `get_metadata_definition` function L1291-1293 — `(&self, id: &str) -> Result<MetadataDefinition, Error>` — `GET /api/metadata-definitions/{id}`.
- pub `create_metadata_definition` function L1296-1302 — `( &self, request: &CreateMetadataDefinitionRequest, ) -> Result<MetadataDefiniti...` — `POST /api/metadata-definitions` (org admin).
- pub `update_metadata_definition` function L1305-1312 — `( &self, id: &str, request: &UpdateMetadataDefinitionRequest, ) -> Result<Metada...` — `PATCH /api/metadata-definitions/{id}` (org admin).
- pub `delete_metadata_definition` function L1316-1319 — `(&self, id: &str) -> Result<DeletedResponse, Error>` — `DELETE /api/metadata-definitions/{id}` (org admin; 409
- pub `list_templates` function L1324-1326 — `(&self, page: Pagination) -> Result<ListEnvelope<Template>, Error>` — `GET /api/templates`.
- pub `get_template` function L1329-1331 — `(&self, id: &str) -> Result<TemplateDetail, Error>` — `GET /api/templates/{id}` — template + metadata associations.
- pub `create_template` function L1334-1339 — `( &self, request: &CreateTemplateRequest, ) -> Result<TemplateDetail, Error>` — `POST /api/templates` (org admin).
- pub `update_template` function L1342-1348 — `( &self, id: &str, request: &UpdateTemplateRequest, ) -> Result<TemplateDetail, ...` — `PATCH /api/templates/{id}` (org admin).
- pub `delete_template` function L1351-1353 — `(&self, id: &str) -> Result<DeletedResponse, Error>` — `DELETE /api/templates/{id}` (org admin; hard delete).
- pub `history` function L1359-1375 — `( &self, kind: EntityKind, short_code: &str, limit: Option<i64>, offset: Option<...` — `GET /api/{family}/{short_code}/history` — the version list,
- pub `history_snapshot` function L1379-1394 — `( &self, kind: EntityKind, short_code: &str, version: i32, ) -> Result<HistorySn...` — `GET /api/{family}/{short_code}/history?version=N` — one full
- pub `cascade_preview` function L1402-1409 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<CascadePreviewResponse,...` — `GET /api/{family}/{short_code}/cascade-preview` — the AUTHORITATIVE
- pub `activity` function L1414-1419 — `( &self, query: &ActivityQuery, ) -> Result<ListEnvelope<ActivityEntry>, Error>` — `GET /api/activity` with combinable filters (S-0005).
- pub `search` function L1424-1426 — `(&self, request: &SearchRequest) -> Result<SearchResponse, Error>` — `POST /api/search` — full-text + filter + traverse composition.
- pub `whoami` function L1431-1433 — `(&self) -> Result<WhoamiResponse, Error>` — `GET /api/whoami` — the resolved user/tenant/teams identity probe.
-  `StaticToken` type L70-75 — `impl TokenProvider for StaticToken` — typed surface deliberately cannot express.
-  `bearer_token` function L71-74 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — typed surface deliberately cannot express.
-  `EntityKind` type L88-99 — `= EntityKind` — typed surface deliberately cannot express.
-  `EntityKind` type L101-105 — `= EntityKind` — typed surface deliberately cannot express.
-  `fmt` function L102-104 — `(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` — typed surface deliberately cannot express.
-  `KairosClient` type L118-125 — `= KairosClient` — typed surface deliberately cannot express.
-  `fmt` function L119-124 — `(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` — typed surface deliberately cannot express.
-  `KairosClient` type L127-319 — `= KairosClient` — typed surface deliberately cannot express.
-  `url` function L165-167 — `(&self, path: &str) -> String` — typed surface deliberately cannot express.
-  `authorize` function L170-180 — `( &self, builder: reqwest::RequestBuilder, ) -> Result<reqwest::RequestBuilder, ...` — Attach the bearer token and the optional X-Tenant header.
-  `execute` function L186-205 — `( &self, context: String, expect: u16, builder: reqwest::RequestBuilder, ) -> Re...` — Send an authorized request; decode the EXACT expected success
-  `get` function L207-210 — `(&self, path: &str) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `get_query` function L212-223 — `( &self, path: &str, query: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `post_created` function L226-237 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — POST expecting 201 Created (the S-0005 create family).
-  `post_ok` function L241-252 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — POST expecting 200 OK (actions on existing resources, e.g.
-  `patch` function L254-265 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `put_ok` function L269-280 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — PUT expecting 200 OK (whole-field replacement, e.g.
-  `delete` function L282-289 — `(&self, path: &str) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `entity_family` macro L322-381 — `-` — The five entity CRUD families (S-0005; KAIROS-T-0018 contracts).
-  `entity_transition` macro L384-402 — `-` — `POST /api/{family}/{short_code}/transition` for the on-board families.
-  `KairosClient` type L404-1434 — `= KairosClient` — typed surface deliberately cannot express.
-  `DefinitionQuery` struct L1271-1278 — `{ limit: Option<i64>, offset: Option<i64>, entity_type: Option<&'a str> }` — typed surface deliberately cannot express.

#### crates/kairos-client/src/error.rs

- pub `Error` enum L28-134 — `Unauthorized | Forbidden | NotFound | Conflict | InvalidTransition | Validation ...` — A failed client call: an API rejection (typed from the S-0005
- pub `from_envelope` function L138-182 — `(status: u16, envelope: ErrorEnvelope) -> Self` — Map an S-0005 envelope + HTTP status to the typed variant.
- pub `status` function L186-197 — `(&self) -> Option<u16>` — The HTTP status of an API rejection (`None` for transport/decode
- pub `code` function L200-211 — `(&self) -> Option<&str>` — The S-0005 envelope code of an API rejection.
- pub `message` function L214-225 — `(&self) -> Option<&str>` — The S-0005 envelope message of an API rejection.
- pub `details` function L228-239 — `(&self) -> Option<&Value>` — The S-0005 envelope `details` of an API rejection.
-  `Error` type L136-240 — `= Error` — `details` value, so no information is lost relative to the raw body.
-  `tests` module L243-329 — `-` — `details` value, so no information is lost relative to the raw body.
-  `envelope` function L247-252 — `(code: &str, message: &str, details: Value) -> ErrorEnvelope` — `details` value, so no information is lost relative to the raw body.
-  `status_code_mapping` function L256-328 — `()` — Every S-0005 family maps to its variant with the payload extracted.

#### crates/kairos-client/src/lib.rs

- pub `client` module L11 — `-` — CLI (`kairos-cli`) and integration tests per KAIROS-A-0009.
- pub `error` module L12 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types` module L13 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_forge` module L14 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_graph` module L15 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_org` module L16 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_repositories` module L17 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_search` module L18 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_service_accounts` module L19 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_team_pages` module L20 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `ws` module L21 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_meta` module L23 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_events` module L25 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `CRATE_NAME` variable L32 — `: &str` — Placeholder marker kept for early consumers (KAIROS-I-0003).
-  `tests` module L35-63 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
-  `smoke` function L39-41 — `()` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
-  `envelopes_round_trip` function L45-62 — `()` — The envelopes round-trip through serde with the S-0005 field names.

#### crates/kairos-client/src/types.rs

- pub `Strategy` struct L29-59 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — A strategy (Flight Level 3), as returned by `/api/strategies`.
- pub `Initiative` struct L63-97 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — An initiative (Flight Level 2), as returned by `/api/initiatives`.
- pub `Task` struct L101-145 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — A task/bug/tech-debt item (Flight Level 1), as returned by `/api/tasks`.
- pub `Document` struct L153-183 — `{ id: String, short_code: String, title: String, content: String, template_id: O...` — A supporting document, as returned by `/api/documents`.
- pub `Adr` struct L188-219 — `{ id: String, short_code: String, title: String, content: String, board_id: Opti...` — An Architecture Decision Record, as returned by `/api/adrs`.
- pub `CreateStrategyRequest` struct L227-239 — `{ board_id: String, column_id: Option<String>, title: String, content: String, h...` — Body of `POST /api/strategies`.
- pub `CreateInitiativeRequest` struct L243-260 — `{ board_id: String, column_id: Option<String>, title: String, content: String, c...` — Body of `POST /api/initiatives`.
- pub `CreateTaskRequest` struct L264-295 — `{ board_id: Option<String>, column_id: Option<String>, title: String, content: S...` — Body of `POST /api/tasks`.
- pub `SetLifecycleRequest` struct L303-306 — `{ lifecycle: String }` — Body of `POST /api/tasks/{short_code}/work-class` (KAIROS-T-0077): move
- pub `SetWorkClassRequest` struct L310-313 — `{ work_class: String }` — transitions — the board rules engine is never consulted.
- pub `CreateDocumentRequest` struct L320-333 — `{ title: String, content: Option<String>, template_id: Option<String>, parent_sh...` — Body of `POST /api/documents`.
- pub `CreateAdrRequest` struct L339-356 — `{ board_id: Option<String>, column_id: Option<String>, title: String, content: S...` — Body of `POST /api/adrs`.
- pub `UpdateContentRequest` struct L363-371 — `{ title: Option<String>, content: String, version: i32 }` — Body of `PATCH /api/{family}/{short_code}` — the KAIROS-A-0004
- pub `TransitionRequest` struct L375-380 — `{ to_column_id: String }` — Body of `POST /api/{family}/{short_code}/transition`.
- pub `MoveTaskRequest` struct L386-388 — `{ board: String }` — Body of `POST /api/tasks/{short_code}/move` (KAIROS-I-0012): the
- pub `ListEnvelope` struct L396-406 — `{ items: Vec<T>, total: i64, limit: i64, offset: i64 }` — The S-0005 list envelope: `{items, total, limit, offset}`.
- pub `Pagination` struct L412-419 — `{ limit: Option<i64>, offset: Option<i64> }` — `?limit=&offset=` pagination for list endpoints (S-0005: the only list
- pub `ListQuery` struct L434-447 — `{ limit: Option<i64>, offset: Option<i64>, include_deleted: bool }` — `?limit=&offset=&include_deleted=` — the query of the five entity
- pub `including_archived` function L452-457 — `() -> Self` — The archived-inclusive whole-family listing.
- pub `DeleteResponse` struct L473-480 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — Response of `DELETE /api/{family}/{short_code}` — the soft delete and
- pub `RestoreResponse` struct L490-497 — `{ short_code: String, still_archived_count: i64, still_archived_short_codes: Vec...` — Response of `POST /api/{entity_type}/{short_code}/restore`
- pub `CascadePreviewResponse` struct L507-515 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — Response of `GET /api/{entity_type}/{short_code}/cascade-preview`
- pub `ErrorEnvelope` struct L519-521 — `{ error: ErrorBody }` — The S-0005 error envelope: `{"error": {"code", "message", "details"}}`.
- pub `ErrorBody` struct L525-536 — `{ code: String, message: String, details: serde_json::Value }` — The `error` object of [`ErrorEnvelope`].
-  `ListQuery` type L449-458 — `= ListQuery` — `kairos-server` (`api::convert`), keeping this crate free of diesel.
-  `ListQuery` type L460-468 — `= ListQuery` — `kairos-server` (`api::convert`), keeping this crate free of diesel.
-  `from` function L461-467 — `(page: Pagination) -> Self` — `kairos-server` (`api::convert`), keeping this crate free of diesel.

#### crates/kairos-client/src/types_events.rs

- pub `ThinEvent` struct L19-37 — `{ event: String, entity_type: String, short_code: String, board_id: Option<Strin...` — One thin change event pushed by the server (S-0005 shape).
- pub `SubscribeRequest` struct L42-45 — `{ subscribe: SubscribeFilter }` — Client → server message: `{"subscribe": {"board_id": "uuid"}}` filters
- pub `SubscribeFilter` struct L49-54 — `{ board_id: Option<String> }` — The [`SubscribeRequest`] filter body.

#### crates/kairos-client/src/types_forge.rs

- pub `ForgeConnection` struct L12-21 — `{ id: String, forge: String, repository: crate::types_repositories::RepositoryRe...` — The webhook wiring of one repository (KAIROS-T-0106 re-key: repo
- pub `CreateForgeConnectionRequest` struct L27-29 — `{ repository: String }` — Body of `POST /api/forge-connections`: connect webhooks for a
- pub `CreatedForgeConnection` struct L36-44 — `{ connection: ForgeConnection, webhook_url: String, webhook_secret: String }` — Response of connection creation and rotation: the connection plus the
- pub `TeamLink` struct L50-70 — `{ kind: String, external_id: String, title: String, url: String, state: String, ...` — One row of a team's in-flight rollup (KAIROS-T-0101): a link plus the
- pub `ItemLink` struct L74-95 — `{ id: String, item_id: String, kind: String, external_id: String, title: String,...` — One branch or pull/merge request linked to a work item.

#### crates/kairos-client/src/types_graph.rs

- pub `GraphNode` struct L11-36 — `{ id: String, short_code: String, entity_type: String, title: String, status: St...` — One hydrated node of the focal subgraph.
- pub `GraphEdge` struct L40-49 — `{ source_id: String, target_id: String, relationship: String, depth: i32 }` — One typed directed edge between two returned nodes.
- pub `GraphResponse` struct L53-67 — `{ focus: String, depth: u32, nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }` — Response of `GET /api/{family}/{code}/graph`.

#### crates/kairos-client/src/types_meta.rs

- pub `RelatedItem` struct L22-42 — `{ relationship_id: String, id: String, short_code: String, entity_type: String, ...` — One hydrated neighbor of an item in the relationship graph.
- pub `RelationshipGroup` struct L46-51 — `{ relationship: String, items: Vec<RelatedItem> }` — All of an item's neighbors under ONE relationship type in one direction.
- pub `ItemRelationshipsResponse` struct L56-65 — `{ short_code: String, outgoing: Vec<RelationshipGroup>, incoming: Vec<Relationsh...` — Response of `GET /api/{entity_type}/{short_code}/relationships`: both
- pub `ChildrenProgressResponse` struct L73-87 — `{ short_code: String, total: i64, done: i64, has_done_columns: bool, by_column: ...` — Response of `GET /api/{entity_type}/{short_code}/children-progress`
- pub `ChildColumnProgress` struct L91-99 — `{ column_id: String, column_name: String, board_id: String, is_done: bool, count...` — One column bucket of a children-progress rollup (KAIROS-T-0080).
- pub `CreateRelationshipRequest` struct L103-110 — `{ source_short_code: String, target_short_code: String, relationship: String }` — Body of `POST /api/relationships` (org admin only, KAIROS-A-0006).
- pub `Relationship` struct L114-125 — `{ id: String, source_id: String, target_id: String, relationship: String, create...` — A relationship edge, as returned by `POST /api/relationships`.
- pub `MetadataValue` struct L133-144 — `{ definition_id: String, slug: String, name: String, field_type: String, value: ...` — One typed metadata value on an item, hydrated with its definition.
- pub `ItemMetadataResponse` struct L148-153 — `{ short_code: String, values: Vec<MetadataValue> }` — Response of `GET/PATCH /api/{entity_type}/{short_code}/metadata`.
- pub `UpdateMetadataRequest` struct L160-163 — `{ values: BTreeMap<String, Option<String>> }` — Body of `PATCH /api/{entity_type}/{short_code}/metadata`: definition
- pub `MetadataDefinition` struct L171-191 — `{ id: String, name: String, slug: String, field_type: String, is_system_default:...` — A metadata field definition, as returned by `/api/metadata-definitions`.
- pub `CreateMetadataDefinitionRequest` struct L195-207 — `{ name: String, slug: String, field_type: String, enum_options: Vec<String>, ent...` — Body of `POST /api/metadata-definitions` (org admin).
- pub `UpdateMetadataDefinitionRequest` struct L213-225 — `{ name: Option<String>, slug: Option<String>, enum_options: Option<Vec<String>>,...` — Body of `PATCH /api/metadata-definitions/{id}` (org admin).
- pub `Template` struct L233-246 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — A document template, as returned by `GET /api/templates`.
- pub `TemplateMetadataField` struct L251-265 — `{ definition_id: String, slug: String, name: String, field_type: String, enum_op...` — One metadata field a template carries (`template_metadata` hydrated
- pub `TemplateDetail` struct L270-284 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — Response of `GET /api/templates/{id}`: the template plus its associated
- pub `TemplateMetadataEntry` struct L288-296 — `{ definition_slug: String, default_value: Option<String>, required: bool }` — One template ↔ metadata-definition association in a template write.
- pub `CreateTemplateRequest` struct L300-309 — `{ name: String, slug: String, content: String, metadata: Vec<TemplateMetadataEnt...` — Body of `POST /api/templates` (org admin).
- pub `UpdateTemplateRequest` struct L314-324 — `{ name: Option<String>, slug: Option<String>, content: Option<String>, metadata:...` — Body of `PATCH /api/templates/{id}` (org admin).
- pub `DeletedResponse` struct L330-333 — `{ id: String }` — Response of the T-0020 hard-delete endpoints
- pub `HistoryVersion` struct L341-348 — `{ version: i32, edited_by: String, edited_at: String }` — One row of `GET /api/{entity_type}/{short_code}/history`.
- pub `HistorySnapshot` struct L353-363 — `{ version: i32, title: String, content: String, edited_by: String, edited_at: St...` — Response of `GET /api/{entity_type}/{short_code}/history?version=N`:
- pub `HistoryQuery` struct L368-378 — `{ version: Option<i32>, limit: Option<i64>, offset: Option<i64> }` — Query of `GET /api/{entity_type}/{short_code}/history`.
- pub `ActivityEntry` struct L386-402 — `{ id: String, actor_id: String, action: String, entity_id: Option<String>, entit...` — One `activity_log` row, as returned by `GET /api/activity`.
- pub `ActivityQuery` struct L407-426 — `{ entity_id: Option<String>, actor_id: Option<String>, action: Option<String>, s...` — Query of `GET /api/activity` (S-0005: all filters combinable).
-  `tests` module L429-479 — `-` — owns the model → DTO encoding.
-  `update_metadata_request_round_trips` function L435-442 — `()` — The metadata PATCH body round-trips: string values stay, `null`
-  `relationships_response_shape` function L446-478 — `()` — Grouped relationships serialize with the S-0005 field names.

#### crates/kairos-client/src/types_org.rs

- pub `Board` struct L22-35 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — A board (`/api/boards` list element).
- pub `BoardColumn` struct L39-68 — `{ id: String, board_id: String, name: String, position: i32, created_at: String,...` — A board column.
- pub `BoardTransition` struct L72-81 — `{ id: String, board_id: String, from_column_id: String, to_column_id: String }` — An allowed column-to-column transition edge.
- pub `BoardDetail` struct L86-93 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — Board detail: the board plus its full configuration
- pub `CreateBoardRequest` struct L98-106 — `{ name: String, slug: String, board_level: String, team_id: Option<String> }` — Body of `POST /api/boards`: creates a board seeded with the system
- pub `UpdateBoardRequest` struct L110-115 — `{ name: Option<String>, slug: Option<String> }` — Body of `PATCH /api/boards/{id}` (board settings).
- pub `BoardColumnItems` struct L120-126 — `{ column: BoardColumn, strategies: Vec<Strategy>, initiatives: Vec<Initiative>, ...` — One column's items in the `GET /api/boards/{id}/items` view: every live
- pub `BoardItemsResponse` struct L131-144 — `{ board: Board, columns: Vec<BoardColumnItems>, children_progress: std::collecti...` — Response of `GET /api/boards/{id}/items`: all items on the board,
- pub `BlocksCounts` struct L149-154 — `{ blocked_by: i64, blocks: i64 }` — Dependency counts behind a board card's blocked-by/blocks badges
- pub `ProgressCounts` struct L159-166 — `{ done: i64, total: i64, has_done: bool }` — A `(done, total)` children rollup (KAIROS-T-0080).
- pub `CreateColumnRequest` struct L170-175 — `{ name: String, position: i32 }` — Body of `POST /api/boards/{id}/columns`.
- pub `UpdateColumnRequest` struct L180-191 — `{ name: Option<String>, position: Option<i32>, is_done: Option<bool> }` — Body of `PATCH /api/boards/{id}/columns/{col_id}` — rename, move,
- pub `CreateTransitionRequest` struct L195-200 — `{ from_column_id: String, to_column_id: String }` — Body of `POST /api/boards/{id}/transitions`.
- pub `BoardMember` struct L209-216 — `{ user_id: String, email: String, display_name: String, capabilities: Vec<String...` — A board member and their capability grants
- pub `AddBoardMemberRequest` struct L220-226 — `{ user_id: String, capabilities: Vec<String> }` — Body of `POST /api/boards/{id}/members`.
- pub `ReplaceCapabilitiesRequest` struct L231-235 — `{ capabilities: Vec<String> }` — Body of `PATCH /api/boards/{id}/members/{user_id}` — replaces the user's
- pub `RemoveBoardMemberResponse` struct L240-245 — `{ user_id: String, revoked_capabilities: Vec<String> }` — Response of `DELETE /api/boards/{id}/members/{user_id}` — full
- pub `OrgDeleteResponse` struct L250-254 — `{ id: String, deleted: bool }` — Generic delete acknowledgement for organizational resources (boards,
- pub `Team` struct L264-277 — `{ id: String, name: String, slug: String, team_type: String, delivery_board_id: ...` — A team (`/api/teams`).
- pub `CreateTeamRequest` struct L283-290 — `{ name: String, slug: String, team_type: Option<String> }` — Body of `POST /api/teams`.
- pub `UpdateTeamRequest` struct L294-301 — `{ name: Option<String>, slug: Option<String>, team_type: Option<String> }` — Body of `PATCH /api/teams/{id}`.
- pub `TeamMember` struct L305-312 — `{ user_id: String, email: String, display_name: String, joined_at: String }` — A team member (`GET /api/teams/{id}/members` element).
- pub `AddTeamMemberRequest` struct L316-319 — `{ user_id: String }` — Body of `POST /api/teams/{id}/members`.
- pub `DeliveryStream` struct L327-337 — `{ id: String, name: String, slug: String, description: Option<String>, created_a...` — A delivery stream (`/api/delivery-streams`).
- pub `CreateStreamRequest` struct L341-346 — `{ name: String, slug: String, description: Option<String> }` — Body of `POST /api/delivery-streams`.
- pub `UpdateStreamRequest` struct L350-357 — `{ name: Option<String>, slug: Option<String>, description: Option<String> }` — Body of `PATCH /api/delivery-streams/{id}`.
- pub `AddStreamTeamRequest` struct L361-364 — `{ team_id: String }` — Body of `POST /api/delivery-streams/{id}/teams`.
- pub `OrgMember` struct L372-383 — `{ user_id: String, external_id: String, email: String, display_name: String, rol...` — An organization member (`GET /api/members` element).
- pub `AddOrgMemberRequest` struct L389-394 — `{ email: String, role: Option<String> }` — Body of `POST /api/members`.
- pub `UpdateOrgMemberRequest` struct L399-402 — `{ role: String }` — Body of `PATCH /api/members/{user_id}` — role change.
- pub `RemoveOrgMemberResponse` struct L406-410 — `{ user_id: String, removed: bool }` — Response of `DELETE /api/members/{user_id}`.
- pub `WhoamiResponse` struct L419-439 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — Response of `GET /api/whoami`: everything the auth → tenant stack
- pub `WhoamiRepository` struct L443-453 — `{ id: String, slug: String, forge: String, repo_full_name: String, team_slug: St...` — One repository of [`WhoamiResponse::repositories`].
- pub `WhoamiBoardCapabilities` struct L459-467 — `{ board_id: String, board_slug: String, grants: Vec<String> }` — One board on which the caller holds explicit capability grants
- pub `WhoamiUser` struct L471-478 — `{ id: String, external_id: String, email: String, display_name: String }` — The `user` object of [`WhoamiResponse`].
- pub `WhoamiOrganization` struct L482-488 — `{ id: String, slug: String, role: String }` — The `organization` object of [`WhoamiResponse`].
- pub `WhoamiTeam` struct L492-497 — `{ id: String, slug: String, name: String }` — One team of [`WhoamiResponse::teams`].
- pub `CreateTenantRequest` struct L506-516 — `{ slug: String, name: String, initial_admin_external_id: Option<String> }` — Body of `POST /api/admin/tenants` (deployment-admin only; see
- pub `TenantInitialAdmin` struct L520-528 — `{ user_id: String, external_id: String, email: String, role: String }` — The org-admin membership created with a new tenant.
- pub `TenantCreatedResponse` struct L533-545 — `{ slug: String, schema: String, migrations_applied: Vec<String>, boards_created:...` — Response of `POST /api/admin/tenants` — the T-0008 provisioning report
- pub `TenantSummary` struct L549-554 — `{ slug: String, name: String, schema_exists: bool }` — A provisioned tenant (`GET /api/admin/tenants` element).
- pub `TenantDeletedResponse` struct L558-561 — `{ slug: String, dropped: bool }` — Response of `DELETE /api/admin/tenants/{slug}`.

#### crates/kairos-client/src/types_repositories.rs

- pub `RepositoryRef` struct L11-22 — `{ id: String, slug: String, forge: String, repo_full_name: String, team_id: Stri...` — The repository a task is bound to, embedded on [`super::types::Task`]
- pub `SetTaskRepositoryRequest` struct L28-31 — `{ repository: Option<String> }` — Body of `PUT /api/tasks/{short_code}/repository` — bind the task to a
- pub `Repository` struct L35-63 — `{ id: String, slug: String, forge: String, repo_full_name: String, repo_url: Str...` — One repository, as returned by `/api/repositories` (KAIROS-T-0106).
- pub `RepositoryTeam` struct L67-72 — `{ id: String, slug: String, name: String }` — The owning team, embedded on [`Repository`].
- pub `RepositoryDetail` struct L77-90 — `{ repository: Repository, connection_id: Option<String>, stale_tasks: i64, in_fl...` — `GET /api/repositories/{slug}`: the repository plus its webhook
- pub `CreateRepositoryRequest` struct L94-113 — `{ slug: Option<String>, forge: String, repo_full_name: String, repo_url: String,...` — Body of `POST /api/repositories`.
- pub `UpdateRepositoryRequest` struct L119-131 — `{ slug: Option<String>, repo_url: Option<String>, default_branch: Option<String>...` — Body of `PATCH /api/repositories/{slug}` — every field optional;

#### crates/kairos-client/src/types_search.rs

- pub `SearchRequest` struct L33-54 — `{ q: Option<String>, filter: Option<SearchFilter>, traverse: Option<SearchTraver...` — Body of `POST /api/search`.
- pub `SearchFilter` struct L60-116 — `{ entity_type: Option<Vec<String>>, board_id: Option<String>, column_id: Option<...` — The `filter` capability.
- pub `SearchTraverse` struct L122-136 — `{ from: SearchTraverseFrom, relationships: Vec<String>, direction: String, depth...` — The `traverse` capability: recursive walk of the relationship graph from
- pub `SearchTraverseFrom` struct L141-148 — `{ short_code: Option<String>, id: Option<String> }` — `traverse.from`: exactly one of `short_code`/`id`.
- pub `SearchSort` struct L154-159 — `{ field: String, order: String }` — The `sort` clause, applied to the combined cross-type result set before
- pub `SearchResponse` struct L169-179 — `{ results: SearchResultGroups, total: i64, limit: i64, offset: i64 }` — Response of `POST /api/search`: results grouped by entity type plus the
- pub `SearchResultGroups` struct L185-201 — `{ strategies: Vec<Strategy>, initiatives: Vec<Initiative>, tasks: Vec<Task>, doc...` — The `results` object: one fully-typed group per entity type, each group
-  `tests` module L204-273 — `-` — at the boundary, never silently ignored (matching the core model).
-  `s0005_request_round_trips` function L210-241 — `()` — The S-0005 Unified Search request example parses field for field and
-  `unknown_fields_are_rejected` function L245-253 — `()` — Unknown fields are rejected, mirroring the core model.
-  `empty_groups_are_omitted` function L258-272 — `()` — Empty groups vanish from the serialized response; present groups and

#### crates/kairos-client/src/types_service_accounts.rs

- pub `CreateServiceAccountRequest` struct L9-11 — `{ name: String }` — `POST /api/service-accounts` body.
- pub `ServiceAccount` struct L15-19 — `{ id: String, name: String, created_at: String }` — A service account (never carries a secret).
- pub `ServiceAccountList` struct L23-26 — `{ items: Vec<ServiceAccount>, total: i64 }` — `GET /api/service-accounts` envelope.
- pub `CreateApiKeyRequest` struct L30-35 — `{ name: String, expires_at: Option<String> }` — `POST /api/service-accounts/{id}/keys` body.
- pub `ApiKeyCreated` struct L40-47 — `{ id: String, name: String, key: String, prefix: String, created_at: String, exp...` — `POST /api/service-accounts/{id}/keys` response — the ONLY place the raw
- pub `ApiKey` struct L51-59 — `{ id: String, name: String, prefix: String, created_at: String, expires_at: Opti...` — One key row (`GET /api/service-accounts/{id}/keys`) — prefix only.
- pub `ApiKeyList` struct L63-66 — `{ items: Vec<ApiKey>, total: i64 }` — `GET /api/service-accounts/{id}/keys` envelope.
- pub `Deleted` struct L70-73 — `{ id: String, deleted: bool }` — The `DELETE` response for a service account or a key.

#### crates/kairos-client/src/types_team_pages.rs

- pub `TeamPage` struct L13-37 — `{ id: String, team_id: String, parent_id: Option<String>, kind: String, slug: St...` — One node of a team's page tree, as returned by
- pub `CreateTeamPageRequest` struct L41-56 — `{ parent_id: Option<String>, kind: String, slug: String, title: String, content:...` — Body of `POST /api/teams/{id}/pages`.
- pub `UpdateTeamPageRequest` struct L63-88 — `{ title: Option<String>, content: Option<String>, version: Option<i32>, slug: Op...` — Body of `PATCH /api/teams/{id}/pages/{page_id}` — EITHER a
- pub `TeamAnnouncement` struct L92-105 — `{ id: String, team_id: String, body: String, pinned: bool, created_by: String, c...` — One team announcement (append-only; no edit anywhere).
- pub `TeamWorkDocument` struct L112-123 — `{ short_code: String, title: String, lifecycle: String, parent_short_code: Strin...` — One row of `GET /api/teams/{id}/work-documents` (KAIROS-T-0084): a
- pub `CreateTeamAnnouncementRequest` struct L127-133 — `{ body: String, pinned: bool }` — Body of `POST /api/teams/{id}/announcements`.

#### crates/kairos-client/src/ws.rs

- pub `EventStream` struct L26-28 — `{ socket: WebSocketStream<MaybeTlsStream<TcpStream>> }` — An open `/ws/events` connection.
- pub `connect_events` function L41-74 — `(&self) -> Result<EventStream, Error>` — Open the tenant-scoped `/ws/events` channel with this client's
- pub `subscribe_board` function L80-85 — `(&mut self, board_id: &str) -> Result<(), Error>` — Filter subsequent events to one board
- pub `subscribe_all` function L89-91 — `(&mut self) -> Result<(), Error>` — Clear the board filter (`{"subscribe": {}}` — deliver every tenant
- pub `next_event` function L109-115 — `(&mut self) -> Result<ThinEvent, Error>` — The next event as a typed [`ThinEvent`], skipping ping/pong frames.
- pub `next_event_raw` function L119-136 — `(&mut self) -> Result<Value, Error>` — The next event as raw JSON — for wire-shape assertions the typed
- pub `close` function L139-144 — `(mut self) -> Result<(), Error>` — Send a close frame and drain the socket.
-  `EventStream` type L30-34 — `= EventStream` — via the `Authorization` header.
-  `fmt` function L31-33 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — via the `Authorization` header.
-  `KairosClient` type L36-75 — `= KairosClient` — via the `Authorization` header.
-  `EventStream` type L77-145 — `= EventStream` — via the `Authorization` header.
-  `send_subscribe` function L93-105 — `(&mut self, filter: SubscribeFilter) -> Result<(), Error>` — via the `Authorization` header.
-  `ws_base_url` function L148-158 — `(base_url: &str) -> Result<String, Error>` — `http(s)://…` → `ws(s)://…`.
-  `tests` module L161-176 — `-` — via the `Authorization` header.
-  `ws_base_url_scheme_mapping` function L165-175 — `()` — via the `Authorization` header.

### crates/kairos-core/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-core/src/abac.rs

- pub `MANAGE_STRATEGIES` variable L41 — `: &str` — Create, update, delete strategies.
- pub `MANAGE_INITIATIVES` variable L43 — `: &str` — Create, update, delete initiatives.
- pub `MANAGE_TASKS` variable L45 — `: &str` — Create, update, delete tasks.
- pub `MANAGE_DOCUMENTS` variable L47 — `: &str` — Create, update, delete documents.
- pub `MANAGE_ADRS` variable L49 — `: &str` — Create, update, delete ADRs.
- pub `TRANSITION_ITEMS` variable L51 — `: &str` — Move items between board columns.
- pub `CONFIGURE_BOARDS` variable L53 — `: &str` — Modify board columns, transitions, settings.
- pub `CONFIGURE_TEMPLATES` variable L55 — `: &str` — Create/modify templates and metadata definitions.
- pub `CONFIGURE_METADATA` variable L57 — `: &str` — Create/modify metadata definitions.
- pub `MANAGE_MEMBERS` variable L59 — `: &str` — Add/remove users from the board, grant/revoke capabilities.
- pub `GLOB_ALL` variable L62 — `: &str` — Glob: all capabilities on the board (full access).
- pub `GLOB_MANAGE` variable L64 — `: &str` — Glob: all `manage_*` capabilities.
- pub `GLOB_CONFIGURE` variable L66 — `: &str` — Glob: all `configure_*` capabilities.
- pub `GLOB_TRANSITION` variable L69 — `: &str` — Glob: all `transition_*` capabilities (currently just
- pub `CAPABILITIES` variable L72-83 — `: &[&str]` — Every specific (non-glob) capability in the A-0006 vocabulary.
- pub `GLOBS` variable L86 — `: &[&str]` — Every sanctioned glob form (trailing-`*` only, per A-0006).
- pub `TEAM_IMPLIED_CAPABILITIES` variable L93 — `: &[&str]` — The capabilities IMPLIED by membership of a board's owning team
- pub `team_implies` function L99-101 — `(required: &str) -> bool` — Does membership of the board's owning team satisfy `required` on its
- pub `FILE_BACKLOG` variable L110 — `: &str` — Cross-team Backlog filing (KAIROS-T-0105, A-0019 §4 amending A-0006):
- pub `COMPUTED_CAPABILITIES` variable L115 — `: &[&str]` — Every capability that is COMPUTED rather than granted — never in the
- pub `COLLABORATIVE_RELATIONSHIPS` variable L126 — `: &[&str]` — Relationship types a NON-admin may write between items they can
- pub `is_collaborative_relationship` function L129-131 — `(relationship: &str) -> bool` — May a non-admin write `relationship` (see [`COLLABORATIVE_RELATIONSHIPS`])?
- pub `TenantConfigResource` enum L141-148 — `Templates | MetadataDefinitions | Relationships` — Tenant-wide configuration resources that live on no board.
- pub `org_admin_only` function L156-158 — `(self) -> bool` — Whether writes to this resource require `organization_members.role =
- pub `capability_matches` function L174-181 — `(granted: &str, required: &str) -> bool` — Does the stored grant `granted` satisfy the `required` capability?
- pub `is_authorized` function L209-213 — `(grants: &[String], required: &str) -> bool` — The A-0006 decision over a user's loaded grants for one board: allowed
-  `TenantConfigResource` type L150-159 — `= TenantConfigResource` — grant time by API-layer validation, not relied upon.
-  `glob_match` function L185-204 — `(pattern: &str, text: &str) -> bool` — LIKE-style match: `*` in `pattern` matches any (possibly empty) sequence;
-  `tests` module L216-425 — `-` — grant time by API-layer validation, not relied upon.
-  `exact_capability_matches_itself_only` function L222-234 — `()` — grant time by API-layer validation, not relied upon.
-  `bare_star_grants_every_capability` function L239-246 — `()` — grant time by API-layer validation, not relied upon.
-  `manage_glob_matches_manage_capabilities_only` function L251-271 — `()` — grant time by API-layer validation, not relied upon.
-  `configure_and_transition_globs` function L274-282 — `()` — grant time by API-layer validation, not relied upon.
-  `non_matching_prefixes_are_rejected` function L285-291 — `()` — grant time by API-layer validation, not relied upon.
-  `empty_strings_mirror_sql_like` function L296-306 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_percent_is_literal_not_wildcard` function L311-319 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_underscore_is_literal_not_single_char_wildcard` function L322-328 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_backslash_is_literal` function L331-336 — `()` — grant time by API-layer validation, not relied upon.
-  `embedded_star_wildcards_like_the_sql_translation` function L339-352 — `()` — grant time by API-layer validation, not relied upon.
-  `is_authorized_is_a_whitelist_over_grants` function L357-369 — `()` — grant time by API-layer validation, not relied upon.
-  `tenant_config_resources_are_org_admin_only` function L374-382 — `()` — grant time by API-layer validation, not relied upon.
-  `team_implies_only_the_delivery_set` function L388-403 — `()` — KAIROS-T-0072: team membership implies exactly the day-to-day
-  `file_backlog_is_computed_never_grantable_never_team_implied` function L406-424 — `()` — grant time by API-layer validation, not relied upon.

#### crates/kairos-core/src/board.rs

- pub `Column` struct L13-19 — `{ id: Uuid, name: String, position: i32 }` — A board column as loaded from `board_columns` (the rule-relevant subset).
- pub `Transition` struct L23-26 — `{ from_column_id: Uuid, to_column_id: Uuid }` — An allowed transition edge as loaded from `board_transitions`.
- pub `ColumnRef` struct L31-34 — `{ id: Uuid, name: String }` — A column reference carried inside errors and warnings (id + name is what
- pub `TransitionError` enum L67-88 — `UnknownFromColumn | UnknownToColumn | NotAllowed` — Why a requested item transition is invalid.
- pub `can_transition` function L95-115 — `( columns: &[Column], transitions: &[Transition], from: Uuid, to: Uuid, ) -> Res...` — A move from `from` to `to` is allowed iff a matching transition edge
- pub `ColumnRuleError` enum L126-163 — `UnknownColumn | EmptyName | DuplicateName | DuplicatePosition | NegativePosition...` — Why a board configuration change is invalid (KAIROS-A-0002
- pub `check_add_column` function L167-185 — `( columns: &[Column], name: &str, position: i32, ) -> Result<(), ColumnRuleError...` — Rule check for adding a column: non-empty name, unique name, unique
- pub `check_rename_column` function L189-205 — `( columns: &[Column], column_id: Uuid, new_name: &str, ) -> Result<(), ColumnRul...` — Rule check for renaming a column: column exists, new name non-empty and
- pub `check_remove_column` function L211-225 — `( columns: &[Column], column_id: Uuid, item_count: u64, ) -> Result<(), ColumnRu...` — Rule check for removing a column: column exists and no items occupy it.
- pub `check_reorder_columns` function L230-255 — `( columns: &[Column], new_order: &[Uuid], ) -> Result<Vec<(Uuid, i32)>, ColumnRu...` — Rule check for reordering: `new_order` must list every column of the
- pub `check_add_transition` function L260-278 — `( columns: &[Column], transitions: &[Transition], from: Uuid, to: Uuid, ) -> Res...` — Rule check for adding a transition edge: both endpoints are columns of
- pub `dead_end_columns` function L289-296 — `(columns: &[Column], transitions: &[Transition]) -> Vec<ColumnRef>` — Columns with no outbound transitions, ordered by position — a
- pub `DefaultBoardConfig` struct L305-311 — `{ columns: Vec<String>, transitions: Vec<(String, String)> }` — A parsed, validated `public.system_board_defaults` configuration:
- pub `DefaultConfigError` enum L315-337 — `NoColumns | EmptyColumnName | DuplicateColumn | MalformedTransition | UnknownTra...` — Why a `system_board_defaults` row is malformed.
- pub `parse_default_config` function L352-402 — `( columns_text: &str, transitions_text: &str, ) -> Result<DefaultBoardConfig, De...` — Parse and validate a `system_board_defaults` row's `columns`
-  `ColumnRef` type L36-43 — `= ColumnRef` — these rules, and persists the outcome.
-  `of` function L37-42 — `(column: &Column) -> Self` — these rules, and persists the outcome.
-  `find_column` function L45-47 — `(columns: &[Column], id: Uuid) -> Option<&Column>` — these rules, and persists the outcome.
-  `allowed_targets` function L50-59 — `(columns: &[Column], transitions: &[Transition], from: Uuid) -> Vec<ColumnRef>` — The allowed target columns out of `from`, ordered by column position.
-  `parse_transition_line` function L340-347 — `(line: &str) -> Option<(&str, &str)>` — Parse one `"From -> To"` line from `system_board_defaults.transitions`.
-  `tests` module L405-741 — `-` — these rules, and persists the outcome.
-  `fixture` function L409-457 — `() -> (Vec<Column>, Vec<Transition>, [Uuid; 4])` — A little board: A(0) -> B(1) -> C(2), plus B <-> D(3) (bidirectional).
-  `transition_allowed_iff_edge_exists` function L462-481 — `()` — these rules, and persists the outcome.
-  `invalid_transition_carries_allowed_targets_in_position_order` function L484-508 — `()` — these rules, and persists the outcome.
-  `transition_from_dead_end_reports_empty_allowed_targets` function L511-521 — `()` — these rules, and persists the outcome.
-  `transition_with_unknown_columns_is_typed` function L524-535 — `()` — these rules, and persists the outcome.
-  `add_column_requires_unique_name_and_position` function L540-559 — `()` — these rules, and persists the outcome.
-  `rename_column_enforces_existence_and_uniqueness` function L562-580 — `()` — these rules, and persists the outcome.
-  `remove_column_only_when_empty` function L583-600 — `()` — these rules, and persists the outcome.
-  `reorder_must_cover_every_column_exactly_once` function L603-629 — `()` — these rules, and persists the outcome.
-  `add_transition_mirrors_ddl_constraints` function L632-656 — `()` — these rules, and persists the outcome.
-  `dead_end_detection_flags_columns_without_outbound_edges` function L661-680 — `()` — these rules, and persists the outcome.
-  `parse_default_config_round_trips_the_a0002_delivery_shape` function L685-701 — `()` — these rules, and persists the outcome.
-  `parse_default_config_rejects_malformed_rows` function L704-740 — `()` — these rules, and persists the outcome.

#### crates/kairos-core/src/forge.rs

- pub `LinkKind` enum L30-33 — `Branch | PullRequest` — What a link points at.
- pub `as_str` function L37-42 — `(self) -> &'static str` — The wire/storage string (matches `item_links.kind`).
- pub `LinkState` enum L47-52 — `Open | Merged | Closed | Draft` — The forge-side state of a link.
- pub `as_str` function L56-63 — `(self) -> &'static str` — The wire/storage string (matches `item_links.state`).
- pub `ForgeEvent` struct L68-85 — `{ kind: LinkKind, external_id: String, title: String, url: String, state: LinkSt...` — One normalized forge event, ready to become an `item_links` row.
- pub `parse_github` function L89-142 — `(event_type: &str, body: &str) -> Option<ForgeEvent>` — Parse a GitHub delivery.
- pub `parse_gitlab` function L145-196 — `(event_type: &str, body: &str) -> Option<ForgeEvent>` — Parse a GitLab delivery.
- pub `extract_short_codes` function L217-259 — `(text: &str) -> Vec<String>` — Every Kairos short code in `text`, deduplicated, in order of first
-  `LinkKind` type L35-43 — `= LinkKind` — retry a delivery that will never succeed.
-  `LinkState` type L54-64 — `= LinkState` — retry a delivery that will never succeed.
-  `parse_time` function L200-208 — `(raw: Option<&str>) -> Option<DateTime<Utc>>` — GitLab timestamps are not always RFC 3339 (`2026-09-01 10:00:00 UTC`),
-  `tests` module L262-448 — `-` — retry a delivery that will never succeed.
-  `fixture` function L265-271 — `(name: &str) -> String` — retry a delivery that will never succeed.
-  `extracts_codes_from_branches_titles_and_bodies` function L276-290 — `()` — retry a delivery that will never succeed.
-  `deduplicates_preserving_first_appearance` function L293-298 — `()` — retry a delivery that will never succeed.
-  `rejects_lookalikes` function L301-318 — `()` — retry a delivery that will never succeed.
-  `matches_codes_embedded_in_branch_names` function L324-333 — `()` — A trailing slug is NORMAL, not a lookalike: branch names carry one
-  `trims_surrounding_punctuation` function L336-340 — `()` — retry a delivery that will never succeed.
-  `github_pull_request_opened` function L345-358 — `()` — retry a delivery that will never succeed.
-  `github_merged_is_closed_plus_merged_flag` function L362-370 — `()` — The trap: GitHub reports a merge as `closed` + `merged: true`.
-  `github_closed_without_merge_is_closed` function L373-377 — `()` — retry a delivery that will never succeed.
-  `github_draft_is_draft` function L380-384 — `()` — retry a delivery that will never succeed.
-  `github_branch_creation` function L387-393 — `()` — retry a delivery that will never succeed.
-  `github_tag_creation_and_pings_are_ignored` function L396-402 — `()` — retry a delivery that will never succeed.
-  `gitlab_merge_request_opened` function L407-416 — `()` — retry a delivery that will never succeed.
-  `gitlab_merged_and_draft_states` function L419-426 — `()` — retry a delivery that will never succeed.
-  `gitlab_push_hook_branch_only` function L429-436 — `()` — retry a delivery that will never succeed.
-  `gitlab_accepts_its_non_rfc3339_timestamps` function L439-447 — `()` — retry a delivery that will never succeed.

#### crates/kairos-core/src/graph.rs

- pub `Relationship` enum L42-53 — `Parent | Supports | Informs | Supersedes | Blocks` — The five KAIROS-A-0001 edge types.
- pub `ALL` variable L57-63 — `: &'static [Relationship]` — Every variant, in declaration order.
- pub `as_str` function L66-74 — `(self) -> &'static str` — The TEXT value stored in `item_relationships.relationship`.
- pub `requires_acyclicity` function L81-83 — `(self) -> bool` — Whether this edge type must stay acyclic (KAIROS-A-0001: "cycle
- pub `GraphRuleError` struct L97-106 — `{ relationship: Relationship, source_type: ItemType, target_type: ItemType, rule...` — A `(relationship, source_type, target_type)` combination outside the
- pub `check_link` function L139-167 — `( relationship: Relationship, source_type: ItemType, target_type: ItemType, ) ->...` — The KAIROS-A-0001 type-rule matrix: whether a `relationship` edge may
- pub `Edge` struct L172-177 — `{ source_id: Uuid, target_id: Uuid }` — A directed edge of ONE relationship type from `item_relationships`
- pub `would_create_cycle` function L184-201 — `(edges: &[Edge], source: Uuid, target: Uuid) -> bool` — Whether adding `source → target` to `edges` (all existing edges of the
-  `Relationship` type L55-84 — `= Relationship` — cross-type dependencies between workflow items.
-  `Relationship` type L86-90 — `= Relationship` — cross-type dependencies between workflow items.
-  `fmt` function L87-89 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — cross-type dependencies between workflow items.
-  `rule_text` function L110-124 — `(relationship: Relationship) -> &'static str` — The allowed shape of each relationship, as prose (used in
-  `is_workflow` function L128-133 — `(item_type: ItemType) -> bool` — Whether this entity type is a workflow item (lives in the flight-level
-  `tests` module L204-374 — `-` — cross-type dependencies between workflow items.
-  `is_allowed` function L211-223 — `(relationship: Relationship, source: ItemType, target: ItemType) -> bool` — The COMPLETE allowed set: every `(relationship, source, target)`
-  `full_matrix_exhaustive` function L226-252 — `()` — cross-type dependencies between workflow items.
-  `matrix_spot_checks_match_a0001` function L255-286 — `()` — cross-type dependencies between workflow items.
-  `rule_error_displays_the_shape` function L289-296 — `()` — cross-type dependencies between workflow items.
-  `acyclicity_applies_to_parent_and_blocks_only` function L299-306 — `()` — cross-type dependencies between workflow items.
-  `edge` function L308-313 — `(source_id: Uuid, target_id: Uuid) -> Edge` — cross-type dependencies between workflow items.
-  `direct_cycle_detected` function L316-327 — `()` — cross-type dependencies between workflow items.
-  `transitive_cycle_detected` function L330-340 — `()` — cross-type dependencies between workflow items.
-  `diamond_is_not_a_cycle` function L343-362 — `()` — cross-type dependencies between workflow items.
-  `cycle_check_tolerates_malformed_existing_cycles` function L365-373 — `()` — cross-type dependencies between workflow items.

#### crates/kairos-core/src/items.rs

- pub `ParentEdge` struct L15-20 — `{ parent_id: Uuid, child_id: Uuid }` — A `parent` edge from the `item_relationships` graph: `parent_id` is the
- pub `cascade_descendants` function L28-41 — `(root: Uuid, edges: &[ParentEdge]) -> Vec<Uuid>` — The soft-delete cascade set (KAIROS-A-0001 "deleting a parent cascades
- pub `VersionCheck` enum L45-58 — `Proceed | Conflict` — The optimistic-concurrency decision for a content edit (KAIROS-A-0004).
- pub `check_version` function L68-76 — `(current_version: i32, expected_version: i32) -> VersionCheck` — Decide whether a content edit based on `expected_version` may be applied
- pub `children_progress_counts` function L83-91 — `(by_column: &[(bool, i64)]) -> (i64, i64)` — Sum a children-by-column rollup into `(done, total)` counts
-  `tests` module L94-205 — `-` — and persists the outcome (the same layering as `board`/`abac`).
-  `version_check_matches_a0004_contract` function L98-113 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `cascade_walks_chain` function L116-133 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `cascade_ignores_unrelated_edges_and_dedups_diamonds` function L136-171 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `children_progress_counts_sums_done_columns` function L177-185 — `()` — KAIROS-T-0080: the done count is exactly the occupants of
-  `cascade_tolerates_cycles_and_excludes_root` function L188-204 — `()` — and persists the outcome (the same layering as `board`/`abac`).

#### crates/kairos-core/src/lib.rs

- pub `abac` module L7 — `-` — transitions, ABAC capability checks, and short-code generation.
- pub `board` module L8 — `-` — `kairos-db` and `kairos-server`.
- pub `forge` module L9 — `-` — `kairos-db` and `kairos-server`.
- pub `graph` module L10 — `-` — `kairos-db` and `kairos-server`.
- pub `items` module L11 — `-` — `kairos-db` and `kairos-server`.
- pub `repositories` module L12 — `-` — `kairos-db` and `kairos-server`.
- pub `retention` module L13 — `-` — `kairos-db` and `kairos-server`.
- pub `search` module L14 — `-` — `kairos-db` and `kairos-server`.
- pub `short_code` module L15 — `-` — `kairos-db` and `kairos-server`.

#### crates/kairos-core/src/repositories.rs

- pub `is_valid_slug` function L10-20 — `(slug: &str) -> bool` — Whether `slug` is a valid repository slug:
- pub `slug_from_full_name` function L37-56 — `(full_name: &str) -> String` — Derive a slug from a forge full name (`acme/payments-api` ->
-  `looks_like_uuid` function L23-30 — `(s: &str) -> bool` — `8-4-4-4-12` lowercase hex — the canonical UUID text form.
-  `tests` module L59-103 — `-` — write path and the API surfaces them as 422s.
-  `slug_vocabulary` function L63-75 — `()` — write path and the API surfaces them as 422s.
-  `derives_slug_from_full_name` function L78-102 — `()` — write path and the API surfaces them as 422s.

#### crates/kairos-core/src/retention.rs

- pub `ENV_HISTORY_HOT_DAYS` variable L38 — `: &str` — Env var: hot-window length in days (default 90).
- pub `ENV_HISTORY_KEEP_LATEST` variable L40 — `: &str` — Env var: latest-N guard size (default 5).
- pub `ENV_ACTIVITY_RETENTION_DAYS` variable L42 — `: &str` — Env var: `activity_log` retention window in days (default 365).
- pub `ENV_ARCHIVE_TARGET` variable L45 — `: &str` — Env var: archive target (filesystem path or `s3://…` URL); unset/empty
- pub `ENV_RETENTION_MODE` variable L48 — `: &str` — Env var: retention mode, one of `archive|discard|off` (default
- pub `RetentionConfigError` enum L52-59 — `InvalidInt | InvalidMode` — Errors from parsing the retention environment configuration.
- pub `RetentionMode` enum L64-75 — `Archive | Discard | Off` — `KAIROS_RETENTION_MODE` (KAIROS-A-0004): what the sweeper does with
- pub `as_str` function L79-85 — `(self) -> &'static str` — The `KAIROS_RETENTION_MODE` string for this mode.
- pub `ArchiveTarget` enum L116-124 — `Filesystem | S3` — `KAIROS_ARCHIVE_TARGET` (KAIROS-A-0004): where pruned rows are offloaded
- pub `parse` function L131-141 — `(raw: &str) -> Option<ArchiveTarget>` — Parse an archive target string.
- pub `RetentionConfig` struct L147-160 — `{ history_hot_days: u32, history_keep_latest: u32, activity_retention_days: u32,...` — The KAIROS-A-0004 retention policy knobs (env surface per
- pub `from_lookup` function L189-210 — `(lookup: F) -> Result<Self, RetentionConfigError>` — Build the config from an injected variable lookup (pure — the unit
- pub `from_env` function L213-215 — `() -> Result<Self, RetentionConfigError>` — [`RetentionConfig::from_lookup`] over the process environment.
- pub `HistoryRowMeta` struct L222-226 — `{ item_id: Uuid, version: i32, edited_at: DateTime<Utc> }` — The planner's view of one `item_history` row: `(item_id, version,
- pub `PruneKey` struct L231-234 — `{ item_id: Uuid, version: i32 }` — A row the planner selected for offload-then-prune, identified by the
- pub `plan_history_compaction` function L250-301 — `( rows: &[HistoryRowMeta], now: DateTime<Utc>, config: &RetentionConfig, ) -> Ve...` — The pure KAIROS-A-0004 compaction planner: given every history row's
-  `RetentionMode` type L77-86 — `= RetentionMode` — decides WHICH rows are candidates.
-  `RetentionMode` type L88-92 — `= RetentionMode` — decides WHICH rows are candidates.
-  `fmt` function L89-91 — `(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` — decides WHICH rows are candidates.
-  `RetentionMode` type L94-111 — `impl FromStr for RetentionMode` — decides WHICH rows are candidates.
-  `Err` type L95 — `= RetentionConfigError` — decides WHICH rows are candidates.
-  `from_str` function L97-110 — `(s: &str) -> Result<Self, Self::Err>` — decides WHICH rows are candidates.
-  `ArchiveTarget` type L126-142 — `= ArchiveTarget` — decides WHICH rows are candidates.
-  `RetentionConfig` type L162-172 — `impl Default for RetentionConfig` — decides WHICH rows are candidates.
-  `default` function L163-171 — `() -> Self` — decides WHICH rows are candidates.
-  `parse_u32` function L174-182 — `(var: &'static str, value: &str) -> Result<u32, RetentionConfigError>` — decides WHICH rows are candidates.
-  `RetentionConfig` type L184-216 — `= RetentionConfig` — decides WHICH rows are candidates.
-  `tests` module L304-584 — `-` — decides WHICH rows are candidates.
-  `now` function L312-314 — `() -> DateTime<Utc>` — Fixed injected clock for every planner test.
-  `at` function L316-318 — `(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc>` — decides WHICH rows are candidates.
-  `row` function L320-326 — `(item: Uuid, version: i32, edited_at: DateTime<Utc>) -> HistoryRowMeta` — decides WHICH rows are candidates.
-  `config` function L328-334 — `(hot_days: u32, keep_latest: u32) -> RetentionConfig` — decides WHICH rows are candidates.
-  `pruned_versions` function L336-341 — `(plan: &[PruneKey], item: Uuid) -> Vec<i32>` — decides WHICH rows are candidates.
-  `config_defaults_when_nothing_is_set` function L346-358 — `()` — decides WHICH rows are candidates.
-  `config_reads_every_documented_env_var` function L361-381 — `()` — decides WHICH rows are candidates.
-  `config_rejects_bad_integers_and_modes` function L384-407 — `()` — decides WHICH rows are candidates.
-  `mode_parsing_is_case_insensitive_and_trimmed` function L410-417 — `()` — decides WHICH rows are candidates.
-  `archive_target_parsing` function L420-439 — `()` — decides WHICH rows are candidates.
-  `empty_history_plans_nothing` function L444-446 — `()` — decides WHICH rows are candidates.
-  `hot_window_rows_are_never_candidates` function L449-459 — `()` — decides WHICH rows are candidates.
-  `one_second_past_the_cutoff_is_a_candidate` function L462-473 — `()` — decides WHICH rows are candidates.
-  `old_months_thin_to_first_and_last` function L476-491 — `()` — decides WHICH rows are candidates.
-  `calendar_month_boundaries_are_respected` function L494-509 — `()` — decides WHICH rows are candidates.
-  `latest_n_guard_overrides_compaction_regardless_of_age` function L512-522 — `()` — decides WHICH rows are candidates.
-  `items_with_at_most_keep_latest_versions_are_untouched` function L525-531 — `()` — decides WHICH rows are candidates.
-  `single_version_items_are_never_pruned` function L534-540 — `()` — decides WHICH rows are candidates.
-  `months_with_two_or_fewer_old_rows_are_untouched` function L543-551 — `()` — decides WHICH rows are candidates.
-  `items_are_planned_independently_and_output_is_sorted` function L554-568 — `()` — decides WHICH rows are candidates.
-  `same_calendar_month_of_different_years_are_distinct_groups` function L571-583 — `()` — decides WHICH rows are candidates.

#### crates/kairos-core/src/search.rs

- pub `MAX_TRAVERSE_DEPTH` variable L48 — `: u32` — Server cap on `traverse.depth` (A-0007: "server-capped to prevent
- pub `MAX_LIMIT` variable L51 — `: i64` — Server cap on `limit`.
- pub `DEFAULT_LIMIT` variable L55 — `: i64` — Default `limit` when the request omits it (the S-0005 examples' page
- pub `SearchRequest` struct L66-85 — `{ q: Option<String>, filter: Option<SearchFilter>, traverse: Option<Traverse>, s...` — The `POST /api/search` request body (KAIROS-A-0007 / S-0005).
- pub `effective_limit` function L89-91 — `(&self) -> i64` — The page size to use: `limit` or [`DEFAULT_LIMIT`].
- pub `effective_offset` function L94-96 — `(&self) -> i64` — The offset to use: `offset` or 0.
- pub `effective_sort` function L99-104 — `(&self) -> Sort` — The sort to use: `sort` or `created_at desc`.
- pub `SearchFilter` struct L111-157 — `{ entity_type: Option<Vec<SearchEntityType>>, board_id: Option<Uuid>, column_id:...` — The `filter` capability (A-0007: fields are AND with each other; array
- pub `is_constraining` function L169-182 — `(&self) -> bool` — Whether this filter says anything at all — the at-least-one-capability
- pub `SearchEntityType` enum L188-199 — `Strategy | Initiative | Task | Document | Adr` — The `filter.entity_type` vocabulary (the five S-0004 entity tables).
- pub `item_type` function L203-211 — `(self) -> ItemType` — The corresponding [`ItemType`].
- pub `SearchTaskType` enum L217-226 — `Task | Bug | TechDebt | Support` — The `filter.task_type` vocabulary (`tasks.task_type` CHECK set).
- pub `as_str` function L230-237 — `(self) -> &'static str` — The TEXT value stored in `tasks.task_type`.
- pub `SearchWorkClass` enum L243-248 — `Planned | Support` — A `tasks.work_class` lane in a search filter (KAIROS-T-0077).
- pub `as_str` function L252-257 — `(self) -> &'static str` — The TEXT value stored in `tasks.work_class`.
- pub `Traverse` struct L264-278 — `{ from: TraverseFrom, relationships: Vec<SearchRelationship>, direction: Directi...` — The `traverse` capability (A-0007: recursive walk of
- pub `TraverseFrom` struct L284-291 — `{ short_code: Option<String>, id: Option<Uuid> }` — `traverse.from`: exactly one of `short_code`/`id` (enforced by
- pub `SearchRelationship` enum L297-308 — `Parent | Supports | Informs | Supersedes | Blocks` — The `traverse.relationships` vocabulary (`item_relationships.
- pub `as_str` function L312-320 — `(self) -> &'static str` — The TEXT value stored in `item_relationships.relationship`.
- pub `Direction` enum L326-333 — `Outbound | Inbound | Both` — `traverse.direction` (A-0007).
- pub `Sort` struct L339-344 — `{ field: SortField, order: SortOrder }` — The `sort` clause; applies to the combined cross-type result set before
- pub `SortField` enum L350-357 — `CreatedAt | UpdatedAt | Title` — Sortable fields — attributes every entity type carries, so the combined
- pub `SortOrder` enum L362-367 — `Asc | Desc` — Sort direction.
- pub `SearchValidationError` enum L375-437 — `NoCapability | BlankQuery | EmptyEntityTypes | EmptyTaskTypes | EmptyWorkClasses...` — A structurally invalid search request (HTTP 400 at the API layer).
- pub `validate` function L442-523 — `(request: &SearchRequest) -> Result<(), SearchValidationError>` — Validate a [`SearchRequest`] against the module-docs contract.
- pub `metadata_like_pattern` function L533-545 — `(value: &str) -> String` — Translate a metadata filter value into a SQL LIKE pattern (module docs):
-  `SearchRequest` type L87-105 — `= SearchRequest` — translates to a LIKE pattern equivalent to equality.
-  `SearchFilter` type L159-183 — `= SearchFilter` — translates to a LIKE pattern equivalent to equality.
-  `SearchEntityType` type L201-212 — `= SearchEntityType` — translates to a LIKE pattern equivalent to equality.
-  `SearchTaskType` type L228-238 — `= SearchTaskType` — translates to a LIKE pattern equivalent to equality.
-  `SearchWorkClass` type L250-258 — `= SearchWorkClass` — translates to a LIKE pattern equivalent to equality.
-  `SearchRelationship` type L310-321 — `= SearchRelationship` — translates to a LIKE pattern equivalent to equality.
-  `tests` module L548-970 — `-` — translates to a LIKE pattern equivalent to equality.
-  `q` function L551-556 — `(text: &str) -> SearchRequest` — translates to a LIKE pattern equivalent to equality.
-  `traverse` function L558-568 — `(depth: Option<u32>) -> Traverse` — translates to a LIKE pattern equivalent to equality.
-  `empty_request_is_no_capability` function L573-578 — `()` — translates to a LIKE pattern equivalent to equality.
-  `empty_filter_is_no_capability` function L581-597 — `()` — translates to a LIKE pattern equivalent to equality.
-  `include_deleted_alone_is_a_capability` function L605-616 — `()` — KAIROS-T-0157 / KAIROS-A-0020: `include_deleted` on its own is a
-  `each_capability_alone_is_valid` function L619-634 — `()` — translates to a LIKE pattern equivalent to equality.
-  `blank_q_is_rejected_even_alongside_other_capabilities` function L637-643 — `()` — translates to a LIKE pattern equivalent to equality.
-  `empty_or_lists_are_rejected` function L648-682 — `()` — translates to a LIKE pattern equivalent to equality.
-  `work_class_filter_alone_is_a_capability` function L687-696 — `()` — KAIROS-T-0077: a work_class-only filter IS constraining — it must
-  `blank_metadata_keys_are_rejected` function L699-711 — `()` — translates to a LIKE pattern equivalent to equality.
-  `date_ranges_must_be_sane` function L714-751 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_depth_is_required_and_capped` function L756-775 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_from_names_exactly_one_reference` function L778-806 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_relationships_must_be_non_empty` function L809-821 — `()` — translates to a LIKE pattern equivalent to equality.
-  `limit_is_capped_and_offset_non_negative` function L826-855 — `()` — translates to a LIKE pattern equivalent to equality.
-  `effective_defaults` function L858-865 — `()` — translates to a LIKE pattern equivalent to equality.
-  `full_s0005_request_deserializes` function L870-918 — `()` — translates to a LIKE pattern equivalent to equality.
-  `unknown_vocabulary_is_rejected_by_serde` function L921-951 — `()` — translates to a LIKE pattern equivalent to equality.
-  `metadata_like_pattern_translates_globs_and_escapes_metacharacters` function L956-969 — `()` — translates to a LIKE pattern equivalent to equality.

#### crates/kairos-core/src/short_code.rs

- pub `ItemType` enum L23-34 — `Strategy | Initiative | Task | Document | Adr` — The five content-bearing entity types that carry short codes, versions,
- pub `ALL` variable L38-44 — `: &'static [ItemType]` — Every variant, in declaration order.
- pub `letter` function L47-55 — `(self) -> char` — The S-0004 type letter used in short codes (`S`/`I`/`T`/`D`/`A`).
- pub `entity_type` function L59-67 — `(self) -> &'static str` — The `entity_type` string used in `activity_log` and the
- pub `format_short_code` function L79-81 — `(prefix: &str, item_type: ItemType, number: i64) -> String` — Render a short code as `{PREFIX}-{TYPE_LETTER}-{NNNN}` (S-0004): the
- pub `default_prefix` function L91-96 — `(slug: &str) -> String` — The DEFAULT per-tenant short-code prefix: the organization slug
-  `ItemType` type L36-68 — `= ItemType` — default at the call site, not the format.
-  `ItemType` type L70-74 — `= ItemType` — default at the call site, not the format.
-  `fmt` function L71-73 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — default at the call site, not the format.
-  `tests` module L99-141 — `-` — default at the call site, not the format.
-  `type_letters_match_s0004` function L103-108 — `()` — default at the call site, not the format.
-  `short_code_zero_pads_to_four` function L111-118 — `()` — default at the call site, not the format.
-  `short_code_grows_past_9999_without_truncation` function L121-130 — `()` — default at the call site, not the format.
-  `default_prefix_uppercases_and_sanitizes` function L133-140 — `()` — default at the call site, not the format.

### crates/kairos-db/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-db/src/abac.rs

- pub `AbacError` enum L46-70 — `EmptyCapability | AlreadyGranted | GrantNotFound | Database` — Errors from capability checks, grants, revocations, or resolution.
- pub `check_capability` function L94-127 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, required: &str, ) -> R...` — The KAIROS-A-0006 access check as ONE indexed query (no N+1): EXISTS over
- pub `is_org_admin` function L132-148 — `( conn: &mut PgConnection, org_slug: &str, user_id: Uuid, ) -> Result<bool, Abac...` — Whether `user_id` is an org admin (`public.organization_members.role =
- pub `is_org_member` function L151-166 — `( conn: &mut PgConnection, org_slug: &str, user_id: Uuid, ) -> Result<bool, Abac...` — Whether `user_id` is a member (any role) of the organization `org_slug`.
- pub `check_file_backlog` function L174-194 — `( conn: &mut PgConnection, org_slug: &str, board_id: Uuid, user_id: Uuid, ) -> R...` — The COMPUTED `file_backlog` capability (KAIROS-T-0105, A-0019 §4): any
- pub `authorize` function L202-216 — `( conn: &mut PgConnection, org_slug: &str, board_id: Uuid, user_id: Uuid, requir...` — The combined KAIROS-A-0006 write-authorization decision for a board
- pub `grant_capability` function L244-285 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, capability: &str, gran...` — Grant `capability` to `user_id` on `board_id`, writing the
- pub `revoke_capability` function L290-324 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, capability: &str, revo...` — Revoke `capability` from `user_id` on `board_id`, writing the
- pub `resolve_authorization_board` function L389-422 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Option<Uuid>, AbacError>` — Which board authorizes writes to `item_id` (KAIROS-A-0006 access-check
- pub `item_created_by` function L430-450 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<Option<Uuid>, AbacError>` — Who created a workflow item or document, if it exists (KAIROS-T-0111):
-  `BoolRow` struct L73-76 — `{ authorized: bool }` — mutation (KAIROS-A-0004/S-0004 action list).
-  `log_capability_activity` function L221-239 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, board_id: Uui...` — Insert one `activity_log` row (current tenant schema; same shape as
-  `board_of_workflow_item` function L337-369 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Option<Uuid>, DieselError>` — The board a workflow item (strategy/initiative/task/ADR) sits on.
-  `try_table` macro L343-354 — `-` — mutation (KAIROS-A-0004/S-0004 action list).
-  `try_table` macro L432-443 — `-` — mutation (KAIROS-A-0004/S-0004 action list).

#### crates/kairos-db/src/api_keys.rs

- pub `ApiKey` struct L25-44 — `{ id: Uuid, user_id: Uuid, name: String, token_hash: String, prefix: String, cre...` — An API key row (`api_keys`, tenant schema).
- pub `is_valid_at` function L50-52 — `(&self, now: DateTime<Utc>) -> bool` — Whether the key is usable at `now`: not revoked and not past expiry.
- pub `NewApiKey` struct L59-66 — `{ user_id: Uuid, name: String, token_hash: String, prefix: String, created_by: U...` — Insert for [`ApiKey`]; `id`/`created_at` come from column defaults and
- pub `create_key` function L69-74 — `(conn: &mut PgConnection, new: NewApiKey) -> QueryResult<ApiKey>` — Insert a new key row (connection must be tenant-pinned).
- pub `find_by_hash` function L78-84 — `(conn: &mut PgConnection, token_hash: &str) -> QueryResult<Option<ApiKey>>` — The key row with this hash, if any.
- pub `list_keys` function L88-94 — `(conn: &mut PgConnection, user_id: Uuid) -> QueryResult<Vec<ApiKey>>` — All key rows for one service account, newest first (revoked included — the
- pub `find_key` function L97-103 — `(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<ApiKey>>` — The key row with this id, if any.
- pub `revoke_key` function L106-111 — `(conn: &mut PgConnection, id: Uuid) -> QueryResult<ApiKey>` — Mark a key revoked (idempotence is the caller's policy).
- pub `touch_last_used` function L115-119 — `(conn: &mut PgConnection, id: Uuid) -> QueryResult<usize>` — Record that a key was just used.
-  `ApiKey` type L46-53 — `= ApiKey` — auth path keeps its failures uniform (KAIROS-T-0058).

#### crates/kairos-db/src/boards.rs

- pub `BoardError` enum L39-97 — `BoardNotFound | ColumnNotFound | ItemNotFound | ItemNotOnBoard | SameBoard | Not...` — Errors from board creation, item transitions, or board configuration.
- pub `create_board` function L214-282 — `( conn: &mut PgConnection, level: BoardLevel, name: &str, slug: &str, team_id: O...` — NULL, so system-provisioned boards write no activity row).
- pub `TaskMove` struct L391-396 — `{ from_board_id: Uuid, board_id: Uuid, column_id: Uuid, team_id: Option<Uuid> }` — The placement a task has after [`move_task`].
- pub `move_task` function L405-513 — `( conn: &mut PgConnection, task_id: Uuid, to_board_id: Uuid, actor_id: Uuid, ) -...` — Move a live task to another live **delivery** board: it lands in the
- pub `transition_adr` function L518-575 — `( conn: &mut PgConnection, item_id: Uuid, to_column_id: Uuid, actor_id: Uuid, ) ...` — Move an ADR to another column of its board.
- pub `add_column` function L583-618 — `( conn: &mut PgConnection, board_id: Uuid, name: &str, position: i32, actor_id: ...` — Add a column to a board (unique name and position enforced by
- pub `set_column_done` function L624-656 — `( conn: &mut PgConnection, column_id: Uuid, is_done: bool, actor_id: Uuid, ) -> ...` — Rename a column (name uniqueness enforced by
- pub `rename_column` function L658-691 — `( conn: &mut PgConnection, column_id: Uuid, new_name: &str, actor_id: Uuid, ) ->...` — NULL, so system-provisioned boards write no activity row).
- pub `remove_column` function L707-738 — `( conn: &mut PgConnection, column_id: Uuid, actor_id: Uuid, ) -> Result<(), Boar...` — Remove a column: a SOFT delete (KAIROS-T-0161), allowed when no LIVE
- pub `reorder_columns` function L743-790 — `( conn: &mut PgConnection, board_id: Uuid, new_order: &[Uuid], actor_id: Uuid, )...` — Reorder a board's columns.
- pub `add_transition` function L795-830 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Add a transition edge to a board
- pub `remove_transition` function L833-872 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Remove a transition edge from a board.
- pub `dead_end_columns` function L878-884 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<Vec<rules::ColumnRef>, Bo...` — Columns of `board_id` with no outbound transitions, in position order —
- pub `entry_column` function L942-951 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Option<Uuid>, DieselError>` — The board's ENTRY column — lowest position, the one creation defaults
-  `log_activity` function L100-118 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Op...` — Insert one `activity_log` row (current tenant schema).
-  `load_board_rules` function L133-180 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<(Vec<rules::Column>, Vec<...` — Load a board's LIVE columns and the transition edges between them, as
-  `column_name` function L182-188 — `(columns: &[rules::Column], id: Uuid) -> Result<String, BoardError>` — NULL, so system-provisioned boards write no activity row).
-  `seeded_done_column` function L207-212 — `(level: BoardLevel, name: &str) -> bool` — Create a board in the current tenant schema, seeding its columns and
-  `validate_transition` function L290-302 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Validate a move against the board's transition graph and return the
-  `transition_item_fn` macro L309-364 — `-` — Generate `transition_<entity>` for an item table with NOT NULL
-  `column_board_id` function L889-898 — `(conn: &mut PgConnection, column_id: Uuid) -> Result<Uuid, BoardError>` — The `board_id` of a LIVE column, or [`BoardError::ColumnNotFound`] —
-  `count_items_in_column` function L911-936 — `(conn: &mut PgConnection, column_id: Uuid) -> Result<u64, BoardError>` — How many LIVE workflow items occupy `column_id`, across every entity

#### crates/kairos-db/src/events.rs

- pub `EVENT_CHANNEL` variable L53 — `: &str` — The one NOTIFY channel every Kairos event travels on (A-0005 §5).
- pub `EventKind` enum L57-82 — `ItemCreated | ItemUpdated | ItemTransitioned | ItemMoved | ItemDeleted | ItemRes...` — The S-0005 event vocabulary.
- pub `as_str` function L86-98 — `(self) -> &'static str` — The wire name (`event` field).
- pub `ThinEvent` struct L103-116 — `{ event: EventKind, entity_type: String, short_code: String, board_id: Option<Uu...` — One thin change event (S-0005 shape, before tenant tagging).
- pub `emit_event` function L138-163 — `(conn: &mut PgConnection, event: &ThinEvent) -> Result<(), DieselError>` — Emit `event` on [`EVENT_CHANNEL`], tagged with the current connection's
- pub `ItemPlacement` type L166 — `= (String, Option<Uuid>, Option<Uuid>)` — [`item_placement`]'s row: `(short_code, board_id, column_id)`.
- pub `item_placement` function L172-193 — `( conn: &mut PgConnection, entity_type: &str, item_id: Uuid, ) -> Result<Option<...` — `(short_code, board_id, column_id)` of an item by id, straight from its
- pub `emit_item_event_by_id` function L198-219 — `( conn: &mut PgConnection, event: EventKind, entity_type: &str, item_id: Uuid, a...` — Convenience for call sites that only hold an item id: look up the
-  `EventKind` type L84-99 — `= EventKind` — short-code prefix.
-  `SchemaName` struct L119-122 — `{ name: String }` — short-code prefix.
-  `PlacementRow` struct L125-132 — `{ short_code: String, board_id: Option<Uuid>, column_id: Option<Uuid> }` — short-code prefix.

#### crates/kairos-db/src/forge.rs

- pub `ForgeError` enum L33-53 — `ConnectionNotFound | RepoAlreadyConnected | Repository | ForgeMismatch | Databas...` — Errors from the forge-connection services.
- pub `ConnectionWithRepo` struct L57-60 — `{ connection: ForgeConnection, repository: Repository }` — A connection joined to its repository — the read shape the API renders.
- pub `list_connections` function L63-78 — `(conn: &mut PgConnection) -> Result<Vec<ConnectionWithRepo>, ForgeError>` — Every live connection with its repository, newest first.
- pub `load_connection` function L81-90 — `(conn: &mut PgConnection, id: Uuid) -> Result<ForgeConnection, ForgeError>` — One live connection, or [`ForgeError::ConnectionNotFound`].
- pub `load_connection_with_repo` function L93-110 — `( conn: &mut PgConnection, id: Uuid, ) -> Result<ConnectionWithRepo, ForgeError>` — One live connection with its repository.
- pub `find_connection_for_repository` function L113-124 — `( conn: &mut PgConnection, repository_id: Uuid, ) -> Result<Option<ForgeConnecti...` — The live connection of one repository, if any.
- pub `create_connection` function L128-151 — `( conn: &mut PgConnection, input: NewForgeConnection, ) -> Result<ForgeConnectio...` — Wire a webhook connection onto a repository.
- pub `delete_connection` function L156-166 — `(conn: &mut PgConnection, id: Uuid) -> Result<(), ForgeError>` — Soft-delete a connection.
- pub `upsert_link` function L182-212 — `(conn: &mut PgConnection, link: NewItemLink) -> Result<bool, ForgeError>` — Insert or advance one link, ORDERING-SAFELY.
- pub `delete_link` function L217-234 — `( conn: &mut PgConnection, connection_id: Uuid, kind: LinkKind, external_id: &st...` — Drop a link between one item and one PR/branch — used when a pull
- pub `linked_items` function L238-251 — `( conn: &mut PgConnection, connection_id: Uuid, kind: LinkKind, external_id: &st...` — The item ids currently linked to one PR/branch — the "which links
- pub `LinkWithRepo` struct L256-260 — `{ link: ItemLink, forge: Forge, repo_full_name: String }` — One link joined to its repository — the read shape both the item panel
- pub `links_for_item` function L264-288 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Vec<LinkWithRepo>, ForgeEr...` — Every link on one item, PRs before branches then newest first (the
- pub `links_for_items` function L292-320 — `( conn: &mut PgConnection, item_ids: &[Uuid], states: &[LinkState], limit: i64, ...` — Links in the given states across a set of items — the team rollup's
- pub `links_for_connection_team` function L325-353 — `( conn: &mut PgConnection, team_id: Uuid, states: &[LinkState], limit: i64, ) ->...` — Links in the given states belonging to repositories OWNED by a team —
- pub `new_link` function L357-379 — `( item_id: Uuid, connection_id: Uuid, kind: LinkKind, external_id: String, title...` — Convenience for callers building a link from a normalized event.

#### crates/kairos-db/src/graph.rs

- pub `GraphError` enum L92-131 — `ItemNotFound | SelfLink | Rule | CycleDetected | AlreadyLinked | NotLinked | Dat...` — Errors from the relationship-graph services.
- pub `link_items` function L246-316 — `( conn: &mut PgConnection, source_id: Uuid, target_id: Uuid, relationship: Relat...` — Create a `relationship` edge from `source_id` to `target_id`
- pub `unlink_items` function L324-381 — `( conn: &mut PgConnection, source_id: Uuid, target_id: Uuid, relationship: Relat...` — Remove the `relationship` edge from `source_id` to `target_id`, in ONE
- pub `Neighbor` struct L387-404 — `{ relationship: RelationshipType, id: Uuid, short_code: String, entity_type: Ite...` — One neighbor of an item in the relationship graph, hydrated through
- pub `ItemRelationships` struct L409-418 — `{ outgoing: Vec<Neighbor>, incoming: Vec<Neighbor> }` — Both directions of an item's relationships, each grouped by
- pub `relationships_for` function L492-500 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<ItemRelationships, GraphEr...` — Every relationship touching `item_id`, in BOTH directions, grouped by
- pub `ChildColumnCount` struct L508-524 — `{ column_id: Uuid, column_name: String, board_id: Uuid, is_done: bool, board_has...` — One column bucket of a parent's direct children.
- pub `children_progress` function L551-571 — `( conn: &mut PgConnection, parent_id: Uuid, ) -> Result<Vec<ChildColumnCount>, D...` — Direct `parent`-edge children of `parent_id`, grouped by their board
- pub `ProgressCounts` struct L589-593 — `{ done: i64, total: i64, has_done: bool }` — A parent's `(done, total, has_done_semantics)` children rollup.
- pub `board_children_progress` function L604-638 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<std::collections::HashMap...` — Children rollups for EVERY item on `board_id` that has direct
- pub `TeamWorkDocument` struct L647-660 — `{ short_code: String, title: String, lifecycle: String, parent_short_code: Strin...` — One document attached to a team's work, with its supports-parent for
- pub `team_work_documents` function L671-693 — `( conn: &mut PgConnection, team_id: Uuid, delivery_board: Option<Uuid>, ) -> Res...` — The documents attached to a team's WORK (KAIROS-T-0084): live documents
- pub `SubgraphNode` struct L704-717 — `{ id: Uuid, short_code: String, entity_type: ItemType, title: String, status: St...` — One hydrated node of a focal subgraph.
- pub `SubgraphEdge` struct L723-728 — `{ source_id: Uuid, target_id: Uuid, relationship: RelationshipType, depth: i32 }` — One typed directed edge between two visible subgraph nodes.
- pub `item_subgraph` function L790-891 — `( conn: &mut PgConnection, root: Uuid, depth: u32, ) -> Result<(Vec<SubgraphNode...` — The focal subgraph around `root` (KAIROS-T-0088): every node reachable
- pub `BlocksCounts` struct L899-904 — `{ blocked_by: i64, blocks: i64 }` — The dependency counts one board card shows (KAIROS-T-0091).
- pub `blocks_summary` function L928-960 — `( conn: &mut PgConnection, ids: &[Uuid], ) -> Result<std::collections::HashMap<U...` — Blocked-by/blocks counts for a set of items in ONE grouped query
- pub `team_link_rollup` function L977-1005 — `( conn: &mut PgConnection, team_id: Uuid, delivery_board: Option<Uuid>, states: ...` — Forge links across a team's WORK (KAIROS-T-0101).
- pub `TeamLinkRow` struct L1010-1035 — `{ id: Uuid, kind: String, external_id: String, title: String, url: String, state...` — One row of [`team_link_rollup`] — the link plus the repo and the work
- pub `repository_link_rollup` function L1040-1063 — `( conn: &mut PgConnection, repository_id: Uuid, states: &[&str], limit: i64, ) -...` — Forge links on ONE repository (KAIROS-T-0106): the repository detail's
-  `core_relationship` function L135-143 — `(relationship: RelationshipType) -> rules::Relationship` — The pure mirror of a stored [`RelationshipType`] (kairos-core carries no
-  `parse_entity_type` function L147-160 — `(value: &str) -> Result<ItemType, GraphError>` — Parse an `entity_directory.entity_type` value.
-  `DirectoryRow` struct L163-168 — `{ entity_type: String, short_code: String }` — [`crate::abac::grant_capability`].
-  `resolve_entity` function L175-188 — `( conn: &mut PgConnection, id: Uuid, ) -> Result<Option<(ItemType, String)>, Gra...` — Resolve a UUID to its live entity type and short code via
-  `log_relationship_activity` function L192-210 — `( conn: &mut PgConnection, actor: Uuid, action: ActivityAction, relationship: Re...` — Insert one relationship `activity_log` row: `entity_id`/`entity_type`
-  `load_edges` function L214-230 — `( conn: &mut PgConnection, relationship: RelationshipType, ) -> Result<Vec<rules...` — All existing edges of ONE relationship type in the current tenant
-  `NeighborRow` struct L421-434 — `{ relationship: RelationshipType, id: Uuid, short_code: String, entity_type: Str...` — [`crate::abac::grant_capability`].
-  `NeighborRow` type L436-447 — `= NeighborRow` — [`crate::abac::grant_capability`].
-  `into_neighbor` function L437-446 — `(self) -> Result<Neighbor, GraphError>` — [`crate::abac::grant_capability`].
-  `neighbors_of` function L463-479 — `( conn: &mut PgConnection, item_id: Uuid, own_column: &str, other_column: &str, ...` — One direction of [`relationships_for`]: edges where `item_id` sits in
-  `CHILD_COLUMNS_SQL` variable L540-544 — `: &str` — The live workflow-item id → column_id union the progress queries join
-  `BoardProgressRow` struct L574-583 — `{ parent_id: Uuid, is_done: bool, board_has_done: bool, count: i64 }` — [`crate::abac::grant_capability`].
-  `DepthRow` struct L731-736 — `{ id: Uuid, depth: i32 }` — [`crate::abac::grant_capability`].
-  `NodeHydrationRow` struct L739-754 — `{ id: Uuid, short_code: String, entity_type: String, title: String, status: Stri...` — [`crate::abac::grant_capability`].
-  `EdgeRow` struct L757-764 — `{ source_id: Uuid, target_id: Uuid, relationship: RelationshipType }` — [`crate::abac::grant_capability`].
-  `BlocksRow` struct L907-914 — `{ id: Uuid, blocked_by: i64, blocks: i64 }` — [`crate::abac::grant_capability`].

#### crates/kairos-db/src/items.rs

- pub `ItemError` enum L80-121 — `ItemNotFound | VersionConflict | HistoryNotFound | BoardNotFound | BoardHasNoCol...` — Errors from the item write path.
- pub `next_short_code` function L156-171 — `(conn: &mut PgConnection, item_type: ItemType) -> Result<String, ItemError>` — Allocate the next short code for `item_type` in the current tenant
- pub `ContentUpdate` struct L304-308 — `{ new_title: Option<&'a str>, new_content: &'a str, expected_version: i32 }` — A content edit for [`update_item_content`]: `new_title = None` keeps the
- pub `CreateStrategy` struct L485-492 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, hyp...` — Input for [`create_strategy`].
- pub `create_strategy` function L497-529 — `( conn: &mut PgConnection, input: CreateStrategy<'_>, actor: Uuid, ) -> Result<S...` — Create a strategy: assign the next `{PREFIX}-S-{NNNN}` short code,
- pub `CreateInitiative` struct L535-543 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, com...` — Input for [`create_initiative`].
- pub `create_initiative` function L546-580 — `( conn: &mut PgConnection, input: CreateInitiative<'_>, actor: Uuid, ) -> Result...` — Create an initiative (see [`create_strategy`] for the shared contract).
- pub `CreateTask` struct L584-598 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, tas...` — Input for [`create_task`].
- pub `create_task` function L602-637 — `( conn: &mut PgConnection, input: CreateTask<'_>, actor: Uuid, ) -> Result<Task,...` — Create a task/bug/tech-debt item (see [`create_strategy`] for the
- pub `set_task_work_class` function L645-689 — `( conn: &mut PgConnection, task_id: Uuid, work_class: WorkClass, actor: Uuid, ) ...` — Set a task's Planned/Support lane (KAIROS-T-0077).
- pub `set_task_repository` function L697-760 — `( conn: &mut PgConnection, task_id: Uuid, repository_id: Option<Uuid>, actor: Uu...` — Bind a task to a repository, or clear it (KAIROS-T-0103, A-0019).
- pub `definition_applies_to` function L767-778 — `( conn: &mut PgConnection, definition_id: Uuid, entity_type: &str, ) -> Result<b...` — Whether a metadata definition applies to `entity_type`
- pub `set_document_lifecycle` function L787-837 — `( conn: &mut PgConnection, document_id: Uuid, lifecycle: DocumentLifecycle, acto...` — Set a document's editorial lifecycle (KAIROS-T-0078).
- pub `CreateDocument` struct L841-850 — `{ title: &'a str, content: Option<&'a str>, template_id: Option<Uuid> }` — Input for [`create_document`].
- pub `create_document` function L856-932 — `( conn: &mut PgConnection, input: CreateDocument<'_>, actor: Uuid, ) -> Result<D...` — Create a document (documents do not live on boards).
- pub `CreateAdr` struct L940-947 — `{ board_id: Option<Uuid>, column_id: Option<Uuid>, title: &'a str, content: &'a ...` — Input for [`create_adr`].
- pub `create_adr` function L951-987 — `( conn: &mut PgConnection, input: CreateAdr<'_>, actor: Uuid, ) -> Result<Adr, I...` — Create an ADR (see [`create_strategy`] for the shared contract and
- pub `update_item_content` function L1001-1048 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, update: ContentUp...` — Apply a content edit with optimistic concurrency (KAIROS-A-0004), in ONE
- pub `rollback_item` function L1057-1096 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, to_version: i32, ...` — Roll an item back to a historical snapshot (KAIROS-A-0004 "rollback by
- pub `SoftDeleteOutcome` struct L1104-1110 — `{ root_short_code: String, cascaded_short_codes: Vec<String> }` — What [`soft_delete_item`] deleted.
- pub `CascadePreview` struct L1117-1124 — `{ root_short_code: String, cascaded_short_codes: Vec<String> }` — The AUTHORITATIVE pre-delete cascade set (KAIROS-T-0051): what a
- pub `preview_cascade` function L1135-1183 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Casca...` — Preview the KAIROS-A-0001 soft-delete cascade WITHOUT mutating anything
- pub `soft_delete_item` function L1201-1281 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, actor: Uuid, ) ->...` — Soft-delete an item and cascade to its descendants (KAIROS-A-0001), in
- pub `RestoreOutcome` struct L1465-1471 — `{ short_code: String, still_archived_descendants: Vec<String> }` — What a [`restore_item`] put back.
- pub `RestoreBlockers` struct L1476-1479 — `{ missing: Vec<String> }` — Why a restore was refused: everything missing that the item needs in
- pub `restore_item` function L1500-1572 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, actor: Uuid, ) ->...` — Put an archived item back (KAIROS-A-0020): clear `deleted_at` on the
-  `sequence_name` function L128-136 — `(item_type: ItemType) -> &'static str` — The S-0004 sequence backing each entity type's short-code numbers.
-  `SeqValue` struct L139-142 — `{ value: i64 }` — count and the cascaded short codes.
-  `SchemaName` struct L145-148 — `{ name: String }` — count and the cascaded short codes.
-  `log_activity` function L178-196 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Uu...` — Insert one `activity_log` row (current tenant schema).
-  `insert_history` function L199-217 — `( conn: &mut PgConnection, item_id: Uuid, version: i32, title: &str, content: &s...` — Append one `item_history` snapshot (append-only, KAIROS-A-0004).
-  `finish_create` function L223-248 — `( conn: &mut PgConnection, actor: Uuid, item_type: ItemType, item_id: Uuid, code...` — Create-time bookkeeping shared by all five create services: the v1
-  `resolve_column` function L253-294 — `( conn: &mut PgConnection, board_id: Uuid, column_id: Option<Uuid>, ) -> Result<...` — Resolve an item's column placement on `board_id`: an explicit
-  `content_table_ops` macro L313-409 — `-` — Generate the three per-table primitives the generic write path
-  `apply_content_update` function L448-462 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, update: &ContentU...` — Dispatch the atomic content UPDATE to the item's table.
-  `load_live_content` function L465-477 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Optio...` — Dispatch the live-row load to the item's table.
-  `archived_short_code` function L1286-1309 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Optio...` — The short code of an ARCHIVED row of this type, or `None` if the id is
-  `archived_in` macro L1291-1301 — `-` — count and the cascaded short codes.
-  `any_archived_short_code` function L1313-1323 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Option<String>, DieselErro...` — The short code of an archived row with this id in ANY of the five
-  `restore_row` function L1326-1352 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, actor: Uuid, ) ->...` — Clear `deleted_at` on one row.
-  `restore_in` macro L1332-1344 — `-` — count and the cascaded short codes.
-  `restore_blockers` function L1359-1461 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Vec<S...` — Everything an archived item needs back before it can be live, that is

#### crates/kairos-db/src/lib.rs

- pub `abac` module L9 — `-` — models, typed queries, embedded migrations (public + tenant trees), and
- pub `api_keys` module L10 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `boards` module L11 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `events` module L12 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `forge` module L13 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `graph` module L14 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `items` module L15 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `migrations` module L16 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `models` module L17 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `pool` module L18 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `repositories` module L19 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `retention` module L20 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `schema` module L21 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `scim` module L22 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `search` module L23 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `seed` module L24 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `service_accounts` module L25 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `team_pages` module L26 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `tenant` module L27 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.

#### crates/kairos-db/src/migrations.rs

- pub `PUBLIC_MIGRATIONS` variable L30 — `: EmbeddedMigrations` — The embedded public-schema migration tree (`crates/kairos-db/migrations/public`).
- pub `TENANT_MIGRATIONS` variable L37 — `: EmbeddedMigrations` — The embedded tenant-schema migration tree (`crates/kairos-db/migrations/tenant`).
- pub `MigrationError` enum L41-49 — `Connection | Migration` — Errors from establishing a migration connection or applying migrations.
- pub `establish_migration_connection` function L54-56 — `(database_url: &str) -> Result<PgConnection, MigrationError>` — Establish the dedicated synchronous connection used for running
- pub `run_public_migrations` function L67-72 — `(conn: &mut PgConnection) -> Result<Vec<String>, MigrationError>` — Run all pending public-schema migrations on `conn`.
- pub `has_pending_public_migrations` function L79-82 — `(conn: &mut PgConnection) -> Result<bool, MigrationError>` — Whether the embedded public-schema migration tree has any migration not

#### crates/kairos-db/src/pool.rs

- pub `PgPool` type L48 — `= Pool<AsyncPgConnection>` — The shared async pool type (bb8 over diesel-async's manager).
- pub `PoolError` enum L56-71 — `InvalidSlug | Build | Checkout | SearchPath` — Errors from building the pool or checking connections out.
- pub `TenantPool` struct L77-79 — `{ pool: PgPool }` — A shared connection pool that serves every tenant, pinning
- pub `new` function L92-96 — `(database_url: &str, max_size: u32) -> Result<Self, PoolError>` — Build a pool of at most `max_size` connections against
- pub `tenant` function L104-116 — `(&self, slug: &str) -> Result<TenantConnection, PoolError>` — Check out a connection pinned to `slug`'s tenant schema:
- pub `public_conn` function L120-124 — `(&self) -> Result<OwnedConnection, PoolError>` — Check out a connection pinned to `search_path = public` (no tenant
- pub `raw_pool` function L130-132 — `(&self) -> &PgPool` — The underlying bb8 pool.
- pub `TenantConnection` struct L138-143 — `{ conn: Option<OwnedConnection>, schema: String }` — A pooled connection pinned to one tenant's schema.
- pub `schema` function L147-149 — `(&self) -> &str` — The schema this connection is pinned to (`org_{slug}`).
- pub `release` function L153-158 — `(mut self) -> Result<(), PoolError>` — Reset `search_path` and return the connection to the pool,
-  `OwnedConnection` type L52 — `= PooledConnection<'static, AsyncPgConnection>` — An owned checkout from the pool (`'static`: not borrowing the pool, so
-  `TenantPool` type L81-87 — `= TenantPool` — is defense-in-depth for anything that bypasses it.
-  `fmt` function L82-86 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — is defense-in-depth for anything that bypasses it.
-  `TenantPool` type L89-133 — `= TenantPool` — is defense-in-depth for anything that bypasses it.
-  `TenantConnection` type L145-159 — `= TenantConnection` — is defense-in-depth for anything that bypasses it.
-  `TenantConnection` type L161-169 — `impl Deref for TenantConnection` — is defense-in-depth for anything that bypasses it.
-  `Target` type L162 — `= AsyncPgConnection` — is defense-in-depth for anything that bypasses it.
-  `deref` function L164-168 — `(&self) -> &Self::Target` — is defense-in-depth for anything that bypasses it.
-  `TenantConnection` type L171-177 — `impl DerefMut for TenantConnection` — is defense-in-depth for anything that bypasses it.
-  `deref_mut` function L172-176 — `(&mut self) -> &mut Self::Target` — is defense-in-depth for anything that bypasses it.
-  `TenantConnection` type L179-199 — `impl Drop for TenantConnection` — is defense-in-depth for anything that bypasses it.
-  `drop` function L180-198 — `(&mut self)` — is defense-in-depth for anything that bypasses it.

#### crates/kairos-db/src/repositories.rs

- pub `RepositoryError` enum L29-88 — `NotFound | SlugNotFound | InvalidSlug | SlugTaken | AlreadyRegistered | TeamNotF...` — Errors from the repository services.
- pub `TaskRoute` struct L92-96 — `{ board_id: Uuid, team_id: Option<Uuid>, repository_id: Option<Uuid> }` — Where a task write lands (A-0019 §2): the board, the team, the repo.
- pub `route_task` function L109-151 — `( conn: &mut PgConnection, board_id: Option<Uuid>, team_id: Option<Uuid>, reposi...` — THE routing decision for a task write (A-0019 §2; KAIROS-T-0112 moved
- pub `list` function L154-169 — `( conn: &mut PgConnection, team_id: Option<Uuid>, ) -> Result<Vec<Repository>, R...` — Every live repository, optionally one team's, by slug.
- pub `load` function L172-181 — `(conn: &mut PgConnection, id: Uuid) -> Result<Repository, RepositoryError>` — One live repository by id, or [`RepositoryError::NotFound`].
- pub `load_by_slug` function L184-193 — `(conn: &mut PgConnection, slug: &str) -> Result<Repository, RepositoryError>` — One live repository by slug, or [`RepositoryError::SlugNotFound`].
- pub `resolve` function L196-201 — `(conn: &mut PgConnection, reference: &str) -> Result<Repository, RepositoryError...` — Resolve a repository reference as clients write it: a slug, or a UUID.
- pub `find_by_forge_name` function L205-218 — `( conn: &mut PgConnection, forge: Forge, repo_full_name: &str, ) -> Result<Optio...` — The live repository for a `(forge, full name)`, if any — what
- pub `create` function L222-248 — `( conn: &mut PgConnection, input: NewRepository, ) -> Result<Repository, Reposit...` — Register a repository.
- pub `update` function L254-287 — `( conn: &mut PgConnection, id: Uuid, mut changes: RepositoryChangeset, actor: Uu...` — Edit a repository's mutable fields (slug, URL, default branch, owning
- pub `soft_delete` function L291-320 — `(conn: &mut PgConnection, id: Uuid, actor: Uuid) -> Result<(), RepositoryError>` — Soft-delete a repository.
- pub `stale_tasks` function L326-347 — `(conn: &mut PgConnection, repo: &Repository) -> Result<i64, RepositoryError>` — Live tasks bound to the repository whose `team_id` or `board_id` no
- pub `references` function L350-363 — `(conn: &mut PgConnection, id: Uuid) -> Result<(i64, i64), RepositoryError>` — How many live tasks and live connections reference the repository.
- pub `delivery_board_for_team` function L368-386 — `( conn: &mut PgConnection, team_id: Uuid, ) -> Result<Uuid, RepositoryError>` — THE routing helper (A-0019 §2): the one live delivery board of a team.
- pub `RepositoryCounts` struct L442-449 — `{ repository_id: Uuid, open_tasks: i64, has_webhook: bool }` — Per-repository counts the directory renders (KAIROS-T-0106), ONE query
- pub `counts` function L453-473 — `( conn: &mut PgConnection, ids: &[Uuid], ) -> Result<Vec<RepositoryCounts>, Repo...` — [`RepositoryCounts`] for every id in `ids` (repositories with no tasks
-  `require_team` function L389-400 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<(), RepositoryError>` — 422-worthy check that a live team exists.
-  `map_unique` function L404-418 — `(e: DieselError, slug: &str, forge: Forge, repo: &str) -> RepositoryError` — The two partial unique indexes → typed errors.
-  `log_activity` function L420-436 — `( conn: &mut PgConnection, actor_id: Uuid, repository_id: Uuid, details: String,...` — the typed [`RepositoryError::InUse`] says what is still attached.

#### crates/kairos-db/src/retention.rs

- pub `RetentionError` enum L73-104 — `InvalidSlug | TenantNotFound | ArchiveTargetNotImplemented | ArchiveIo | Seriali...` — Errors from the retention sweeper.
- pub `TableCounts` struct L109-117 — `{ rows_archived: u64, rows_pruned: u64, warnings: u64 }` — Per-table sweep counters (the KAIROS-A-0013 metrics seam — see module
- pub `SweepReport` struct L121-133 — `{ slug: String, mode: RetentionMode, item_history: TableCounts, activity_log: Ta...` — What one [`sweep_tenant`] run did.
- pub `sweep_tenant` function L293-319 — `( conn: &mut PgConnection, slug: &str, config: &RetentionConfig, now: DateTime<U...` — Run one retention sweep for one tenant (KAIROS-A-0004 tiers, mode
- pub `sweep_all_tenants` function L475-492 — `( conn: &mut PgConnection, config: &RetentionConfig, now: DateTime<Utc>, ) -> Re...` — Fleet sweep: run [`sweep_tenant`] for every provisioned organization,
- pub `spawn_retention_loop` function L506-523 — `( interval: std::time::Duration, mut tick: F, ) -> tokio::task::JoinHandle<()>` — Spawn the in-process retention scheduler (KAIROS-A-0004: "in-process
-  `SweepReport` type L135-161 — `= SweepReport` — taken here on purpose.
-  `new` function L136-144 — `(slug: &str, mode: RetentionMode) -> Self` — taken here on purpose.
-  `details` function L148-160 — `(&self) -> String` — The `retention_sweep` activity row's `details` payload: per-table
-  `HistoryArchiveRow` struct L169-177 — `{ id: Uuid, item_id: Uuid, version: i32, title: &'a str, content: &'a str, edite...` — Full-fidelity `item_history` archive row (every DDL column).
-  `from` function L180-190 — `(row: &'a ItemHistory) -> Self` — taken here on purpose.
-  `ActivityRow` type L197-205 — `= ( Uuid, Uuid, String, Option<Uuid>, Option<String>, String, DateTime<Utc>, )` — A loaded `activity_log` row.
-  `ActivityArchiveRow` struct L209-217 — `{ id: Uuid, actor_id: Uuid, action: &'a str, entity_id: Option<Uuid>, entity_typ...` — Full-fidelity `activity_log` archive row (every DDL column).
-  `from` function L220-230 — `(row: &'a ActivityRow) -> Self` — taken here on purpose.
-  `archive_path` function L235-239 — `(base: &Path, slug: &str, table: &str, now: DateTime<Utc>) -> PathBuf` — `{target}/{tenant}/{table}/{timestamp}.ndjson` (timestamp = the injected
-  `write_ndjson` function L245-264 — `(path: &Path, lines: &[String]) -> Result<(), RetentionError>` — Write NDJSON lines to a NEW file and fsync it before returning — the
-  `BoolRow` struct L271-274 — `{ present: bool }` — taken here on purpose.
-  `schema_exists` function L276-282 — `(conn: &mut PgConnection, schema: &str) -> Result<bool, DieselError>` — taken here on purpose.
-  `sweep_current_schema` function L322-468 — `( conn: &mut PgConnection, slug: &str, config: &RetentionConfig, now: DateTime<U...` — The sweep body; assumes `search_path` is pinned to the tenant schema.

#### crates/kairos-db/src/scim.rs

- pub `ScimToken` struct L23-34 — `{ id: Uuid, name: String, token_hash: String, created_by: Uuid, created_at: Date...` — A SCIM bearer token row (`scim_tokens`, tenant schema).
- pub `NewScimToken` struct L39-43 — `{ name: String, token_hash: String, created_by: Uuid }` — Insert for [`ScimToken`]; `id`/`created_at` come from column defaults.
- pub `create_token` function L46-51 — `(conn: &mut PgConnection, new: NewScimToken) -> QueryResult<ScimToken>` — Insert a new token row (connection must be tenant-pinned).
- pub `list_tokens` function L55-60 — `(conn: &mut PgConnection) -> QueryResult<Vec<ScimToken>>` — All token rows for the pinned tenant, newest first (revoked included —
- pub `find_token` function L63-69 — `(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<ScimToken>>` — The token row with this id, if any.
- pub `revoke_token` function L73-78 — `(conn: &mut PgConnection, id: Uuid) -> QueryResult<ScimToken>` — Mark a token revoked (idempotence is the caller's policy: the returned

#### crates/kairos-db/src/search.rs

- pub `SearchError` enum L93-107 — `Invalid | TraverseRootNotFound | Database` — Errors from the search pipeline.
- pub `SearchResults` struct L113-130 — `{ strategies: Vec<Strategy>, initiatives: Vec<Initiative>, tasks: Vec<Task>, doc...` — Search results grouped by entity type (A-0007 response shape), each
- pub `SearchStats` struct L136-141 — `{ hydration_queries: usize, total_queries: usize }` — Query-count instrumentation for one `execute_search` run — the proof
- pub `execute_search` function L145-150 — `( conn: &mut PgConnection, request: &SearchRequest, ) -> Result<SearchResults, S...` — Run the A-0007 search pipeline (module docs) for `request` in the
- pub `execute_search_with_stats` function L154-262 — `( conn: &mut PgConnection, request: &SearchRequest, ) -> Result<(SearchResults, ...` — [`execute_search`], also returning the [`SearchStats`] query counters
-  `IdRow` struct L269-272 — `{ id: Uuid }` — read-only, so no transaction is opened.
-  `resolve_root` function L279-314 — `( conn: &mut PgConnection, from: &TraverseFrom, include_deleted: bool, stats: &m...` — Resolve `traverse.from` to an entity id via `entity_directory`.
-  `traverse_ids` function L320-369 — `( conn: &mut PgConnection, root: Uuid, traverse: &Traverse, stats: &mut SearchSt...` — Recursive-CTE walk from `root` (module docs, step 1).
-  `text_match_ids` function L380-399 — `( conn: &mut PgConnection, q: &str, include_deleted: bool, stats: &mut SearchSta...` — Full-text match via the `searchable_items` view (module docs, step 2).
-  `metadata_match_ids` function L405-431 — `( conn: &mut PgConnection, metadata: &std::collections::BTreeMap<String, String>...` — Ids satisfying EVERY metadata entry (module docs, step 3): one query,
-  `TypedIdRow` struct L438-443 — `{ id: Uuid, entity_type: String }` — read-only, so no transaction is opened.
-  `parse_entity_type` function L448-461 — `(value: &str) -> Result<ItemType, SearchError>` — Parse an `entity_type` literal from type resolution.
-  `partition_by_type` function L469-498 — `( conn: &mut PgConnection, ids: &HashSet<Uuid>, include_deleted: bool, stats: &m...` — Resolve candidate ids to `(id, entity_type)` and partition by type
-  `applicable_types` function L504-536 — `(filter: Option<&SearchFilter>) -> Vec<ItemType>` — Which entity types can match the structural filter at all (module docs,
-  `model_task_type` function L545-552 — `(task_type: SearchTaskType) -> TaskType` — The stored counterpart of a [`SearchTaskType`].
-  `model_work_class` function L555-560 — `(work_class: SearchWorkClass) -> WorkClass` — The stored counterpart of a [`SearchWorkClass`] (KAIROS-T-0077).
-  `hydrate_strategies` function L562-596 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_initiatives` function L598-637 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_tasks` function L639-688 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_documents` function L690-718 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_adrs` function L720-754 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `AnyItem` enum L761-767 — `Strategy | Initiative | Task | Document | Adr` — One hydrated row of any entity type, for the combined sort.
-  `AnyItem` type L769-809 — `= AnyItem` — read-only, so no transaction is opened.
-  `created_at` function L770-778 — `(&self) -> DateTime<Utc>` — read-only, so no transaction is opened.
-  `updated_at` function L780-788 — `(&self) -> DateTime<Utc>` — read-only, so no transaction is opened.
-  `title` function L790-798 — `(&self) -> &str` — read-only, so no transaction is opened.
-  `short_code` function L800-808 — `(&self) -> &str` — read-only, so no transaction is opened.
-  `sort_items` function L813-826 — `(items: &mut [AnyItem], sort: Sort)` — Sort the combined rows by the requested field/order, tie-broken by

#### crates/kairos-db/src/seed.rs

- pub `DEMO_SLUG` variable L78 — `: &str` — The demo tenant slug (schema `org_demo`, short-code prefix `DEMO`).
- pub `DEMO_NAME` variable L81 — `: &str` — The demo tenant display name.
- pub `DEMO_USERS` variable L91-113 — `: [(&str, &str, &str, OrgRole); 3]` — The demo users: `(external_id, email, display_name, org_role)`.
- pub `SeedError` enum L117-149 — `AlreadySeeded | Tenant | Item | Graph | Board | TeamPage | Forge | Repository | ...` — Errors from [`seed_demo`].
- pub `SeedDemoReport` struct L153-188 — `{ slug: String, schema: String, recreated: bool, users: usize, teams: usize, boa...` — What [`seed_demo`] created.
- pub `demo_tenant_exists` function L197-205 — `(conn: &mut PgConnection) -> Result<bool, SeedError>` — Whether the demo tenant exists (org row or schema).
- pub `seed_demo` function L339-1083 — `(conn: &mut PgConnection, force: bool) -> Result<SeedDemoReport, SeedError>` — Seed the demo tenant (see module docs for the full inventory and the
-  `BoolRow` struct L191-194 — `{ present: bool }` — ever touched; other tenants and users are invisible to this module.
-  `column_id` function L210-218 — `(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Result<Uuid, SeedError>` — A live board column's id by board + name (seeded default columns).
-  `board_id_of` function L222-229 — `(conn: &mut PgConnection, level: BoardLevel) -> Result<Uuid, SeedError>` — The tenant's board id for a level (unique for strategy/initiative/adr
-  `metadata_definition_id` function L233-239 — `(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError>` — The tenant's metadata definition id by slug (copied from system
-  `template_id` function L242-248 — `(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError>` — The tenant's template id by slug (copied from system defaults).
-  `upsert_demo_users` function L252-276 — `(conn: &mut PgConnection) -> Result<Vec<Uuid>, SeedError>` — Upsert the three demo users (keyed on `external_id`, exactly like JIT
-  `seed_team` function L280-318 — `( conn: &mut PgConnection, name: &str, slug: &str, team_type: TeamType, member_i...` — Create a team + its delivery board (the same shape as the API's team
-  `stamp_priority` function L321-335 — `( conn: &mut PgConnection, priority_def: Uuid, item_id: Uuid, value: &str, ) -> ...` — Stamp one `priority` metadata value on an item.
-  `SeedTask` struct L589-603 — `{ board: Uuid, column: &'a str, title: &'a str, content: &'a str, task_type: Tas...` — ever touched; other tenants and users are invisible to this module.

#### crates/kairos-db/src/service_accounts.rs

- pub `create_service_account` function L23-51 — `( conn: &mut PgConnection, org_id: Uuid, name: &str, ) -> QueryResult<User>` — Create a service account: a `users` row (`kind='service_account'`, synthetic
- pub `list_service_accounts` function L54-62 — `(conn: &mut PgConnection, org_id: Uuid) -> QueryResult<Vec<User>>` — All service accounts belonging to `org_id`, newest first.
- pub `find_service_account` function L67-80 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> QueryResult<Option<...` — The service account `user_id`, but ONLY if it is a service account AND a
- pub `delete_service_account` function L86-106 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> QueryResult<()>` — Delete a service account and everything attached to it: its API keys and

#### crates/kairos-db/src/team_pages.rs

- pub `seed_team_scaffold` function L108-176 — `( conn: &mut PgConnection, team_id: Uuid, actor: Uuid, ) -> Result<(), DieselErr...` — Seed a team's opinionated scaffold (idempotent): the protected Team
- pub `MAX_CONTENT_BYTES` variable L186 — `: usize` — Content bytes cap for team pages and announcements (KAIROS-I-0007
- pub `TeamPageError` enum L190-224 — `PageNotFound | SlugConflict | BadParent | ProtectedPage | FolderNotEmpty | Versi...` — Typed rejections of the team-page write paths.
- pub `load_page` function L227-241 — `( conn: &mut PgConnection, team_id: Uuid, page_id: Uuid, ) -> Result<TeamPage, T...` — Load a live page of a team, or the typed not-found.
- pub `list_pages` function L245-253 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<TeamPage>, DieselError>` — A team's live page tree, parents-with-position order (flat list; the
- pub `CreatePage` struct L280-287 — `{ parent_id: Option<Uuid>, kind: TeamPageKind, slug: &'a str, title: &'a str, co...` — Input of [`create_page`].
- pub `create_page` function L291-337 — `( conn: &mut PgConnection, team_id: Uuid, input: CreatePage<'_>, actor: Uuid, ) ...` — Create a page or folder; pages get the v1 history baseline.
- pub `update_page_content` function L343-392 — `( conn: &mut PgConnection, team_id: Uuid, page_id: Uuid, new_title: Option<&str>...` — Version-checked title/content edit (the A-0004 pattern): the UPDATE
- pub `rename_move_page` function L396-447 — `( conn: &mut PgConnection, team_id: Uuid, page_id: Uuid, new_slug: Option<&str>,...` — Rename and/or move a page (slug, parent, position).
- pub `soft_delete_page` function L451-483 — `( conn: &mut PgConnection, team_id: Uuid, page_id: Uuid, actor: Uuid, ) -> Resul...` — Soft-delete a page; folders must be empty of live children.
- pub `list_announcements` function L486-496 — `( conn: &mut PgConnection, team_id: Uuid, ) -> Result<Vec<TeamAnnouncement>, Die...` — A team's announcements, pinned first then newest first.
-  `CHARTER_CONTENT` variable L22-23 — `: &str` — The charter's templated skeleton (mirrors the `team_charter` system
-  `SUPPORT_OVERVIEW_CONTENT` variable L26-27 — `: &str` — The Support Processes index page's starter content.
-  `DOC_SECTIONS` variable L31-38 — `: &[(&str, &str)]` — The Documentation section folders, in display order (the diataxis
-  `ensure_node` function L44-101 — `( conn: &mut PgConnection, team_id: Uuid, parent_id: Option<Uuid>, kind: TeamPag...` — Insert one scaffold node if no live-or-deleted sibling with the slug
-  `check_parent` function L256-267 — `( conn: &mut PgConnection, team_id: Uuid, parent_id: Uuid, ) -> Result<(), TeamP...` — Validate a prospective parent: a live folder of the same team.
-  `check_size` function L269-276 — `(content: &str) -> Result<(), TeamPageError>` — scaffolded teams converge.

#### crates/kairos-db/src/tenant.rs

- pub `TenantError` enum L43-69 — `InvalidSlug | AlreadyExists | NotFound | ConfirmationRequired | Migration | Boar...` — Errors from tenant provisioning, fleet migration, or teardown.
- pub `TenantProvisionReport` struct L73-87 — `{ slug: String, schema: String, migrations_applied: Vec<String>, boards_created:...` — What [`provision_tenant`] created.
- pub `TenantMigrationOutcome` struct L91-98 — `{ slug: String, schema: String, applied: Vec<String> }` — One tenant's outcome from [`migrate_all_tenants`].
- pub `TenantInfo` struct L102-109 — `{ slug: String, name: String, schema_exists: bool }` — A row from [`list_tenants`].
- pub `is_valid_slug` function L247-254 — `(slug: &str) -> bool` — Whether `slug` matches the KAIROS-S-0004 organization slug pattern
- pub `tenant_schema_name` function L265-267 — `(slug: &str) -> String` — The schema name for an organization slug: `org_{slug}`.
- pub `seed_system_defaults` function L281-284 — `(conn: &mut PgConnection) -> Result<(), TenantError>` — Idempotently seed the `public.system_*` default rows (see
- pub `provision_tenant` function L300-402 — `( conn: &mut PgConnection, slug: &str, name: &str, ) -> Result<TenantProvisionRe...` — Provision a new tenant (KAIROS-A-0001 application-level provisioning):
- pub `drop_tenant` function L409-426 — `(conn: &mut PgConnection, slug: &str, confirm: bool) -> Result<(), TenantError>` — Drop a tenant: remove the `org_{slug}` schema (CASCADE) and delete the
- pub `list_tenants` function L430-445 — `(conn: &mut PgConnection) -> Result<Vec<TenantInfo>, TenantError>` — List provisioned tenants from `public.organizations`, with a
- pub `migrate_all_tenants` function L452-488 — `( conn: &mut PgConnection, ) -> Result<Vec<TenantMigrationOutcome>, TenantError>` — Fleet operation: run pending tenant migrations in every provisioned
-  `PROVISION_BOARDS` variable L115-119 — `: [(BoardLevel, &str, &str); 3]` — The default boards created at provision time: `(board_level, name, slug)`.
-  `SEED_SYSTEM_DEFAULTS_SQL` variable L128-224 — `: &str` — Idempotent seed of the `public.system_*` default rows (KAIROS-A-0002
-  `BoolRow` struct L227-230 — `{ present: bool }` — (Interpretation recorded in KAIROS-T-0008.)
-  `TenantRow` struct L233-240 — `{ slug: String, name: String, schema_exists: bool }` — (Interpretation recorded in KAIROS-T-0008.)
-  `validated_slug` function L256-262 — `(slug: &str) -> Result<(), TenantError>` — (Interpretation recorded in KAIROS-T-0008.)
-  `schema_exists` function L269-275 — `(conn: &mut PgConnection, schema: &str) -> Result<bool, TenantError>` — (Interpretation recorded in KAIROS-T-0008.)
-  `tests` module L491-527 — `-` — (Interpretation recorded in KAIROS-T-0008.)
-  `slug_validation_matches_s0004_pattern` function L495-521 — `()` — (Interpretation recorded in KAIROS-T-0008.)
-  `tenant_schema_name_prefixes_org` function L524-526 — `()` — (Interpretation recorded in KAIROS-T-0008.)

### crates/kairos-db/src/models

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-db/src/models/boards.rs

- pub `Board` struct L23-33 — `{ id: Uuid, name: String, slug: String, board_level: BoardLevel, team_id: Option...` — A configurable board (`boards`).
- pub `NewBoard` struct L38-43 — `{ name: String, slug: String, board_level: BoardLevel, team_id: Option<Uuid> }` — Insert for [`Board`].
- pub `BoardChangeset` struct L48-55 — `{ name: Option<String>, slug: Option<String>, board_level: Option<BoardLevel>, t...` — Partial update for [`Board`].
- pub `BoardColumn` struct L66-83 — `{ id: Uuid, board_id: Uuid, name: String, position: i32, created_at: DateTime<Ut...` — A board column (`board_columns`).
- pub `NewBoardColumn` struct L88-93 — `{ board_id: Uuid, name: String, position: i32, is_done: bool }` — Insert for [`BoardColumn`].
- pub `BoardColumnChangeset` struct L98-104 — `{ name: Option<String>, position: Option<i32>, is_done: Option<bool>, deleted_at...` — Partial update for [`BoardColumn`].
- pub `BoardTransition` struct L115-120 — `{ id: Uuid, board_id: Uuid, from_column_id: Uuid, to_column_id: Uuid }` — An allowed column-to-column transition (`board_transitions`).
- pub `NewBoardTransition` struct L125-129 — `{ board_id: Uuid, from_column_id: Uuid, to_column_id: Uuid }` — Insert for [`BoardTransition`].
- pub `BoardTransitionChangeset` struct L134-137 — `{ from_column_id: Option<Uuid>, to_column_id: Option<Uuid> }` — Partial update for [`BoardTransition`] (rewiring an edge).
- pub `BoardMemberCapability` struct L152-158 — `{ board_id: Uuid, user_id: Uuid, capability: String, granted_at: DateTime<Utc>, ...` — A board-scoped capability grant (`board_member_capabilities`,
- pub `NewBoardMemberCapability` struct L163-168 — `{ board_id: Uuid, user_id: Uuid, capability: String, granted_by: Uuid }` — Insert for [`BoardMemberCapability`].
- pub `BoardMemberCapabilityChangeset` struct L174-177 — `{ granted_at: Option<DateTime<Utc>>, granted_by: Option<Uuid> }` — Partial update for [`BoardMemberCapability`] (re-attribution; grants are

#### crates/kairos-db/src/models/enums.rs

- pub `UnknownEnumValue` struct L21-26 — `{ enum_name: &'static str, value: String }` — A TEXT value read from the database that is not a member of the enum's
-  `text_enum` macro L30-92 — `-` — Declare a TEXT-backed enum: variants, their database strings, `FromStr`
-  `tests` module L264-399 — `-` — than defaulting.
-  `assert_text_enum` macro L269-292 — `-` — Round-trip every variant through its TEXT representation and reject
-  `org_role_round_trip_and_rejection` function L295-297 — `()` — than defaulting.
-  `field_type_round_trip_and_rejection` function L300-302 — `()` — than defaulting.
-  `team_type_round_trip_and_rejection` function L305-315 — `()` — than defaulting.
-  `board_level_round_trip_and_rejection` function L318-320 — `()` — than defaulting.
-  `task_type_round_trip_and_rejection` function L323-325 — `()` — than defaulting.
-  `work_class_round_trip_and_rejection` function L328-330 — `()` — than defaulting.
-  `team_page_kind_round_trip_and_rejection` function L333-335 — `()` — than defaulting.
-  `forge_round_trip_and_rejection` function L338-340 — `()` — than defaulting.
-  `link_kind_round_trip_and_rejection` function L343-345 — `()` — than defaulting.
-  `link_state_round_trip_and_rejection` function L348-350 — `()` — than defaulting.
-  `document_lifecycle_round_trip_and_rejection` function L353-358 — `()` — than defaulting.
-  `complexity_round_trip_and_rejection` function L361-363 — `()` — than defaulting.
-  `bucket_type_round_trip_and_rejection` function L366-368 — `()` — than defaulting.
-  `relationship_type_round_trip_and_rejection` function L371-376 — `()` — than defaulting.
-  `activity_action_round_trip_and_rejection` function L379-398 — `()` — than defaulting.

#### crates/kairos-db/src/models/forge.rs

- pub `ForgeConnection` struct L28-36 — `{ id: Uuid, forge: Forge, created_by: Uuid, deleted_at: Option<DateTime<Utc>>, c...` — The webhook wiring OF a repository (`forge_connections`, re-keyed on
- pub `NewForgeConnection` struct L41-45 — `{ forge: Forge, repository_id: Uuid, created_by: Uuid }` — Insert for [`ForgeConnection`].
- pub `ForgeConnectionChangeset` struct L51-54 — `{ deleted_at: Option<Option<DateTime<Utc>>>, updated_at: Option<DateTime<Utc>> }` — Partial update for [`ForgeConnection`] (nothing but liveness is
- pub `ItemLink` struct L63-78 — `{ id: Uuid, item_id: Uuid, connection_id: Uuid, kind: LinkKind, external_id: Str...` — One branch or pull/merge request associated with a work item
- pub `NewItemLink` struct L83-93 — `{ item_id: Uuid, connection_id: Uuid, kind: LinkKind, external_id: String, title...` — Insert for [`ItemLink`] (also the upsert payload).

#### crates/kairos-db/src/models/graph.rs

- pub `ItemRelationship` struct L22-28 — `{ id: Uuid, source_id: Uuid, target_id: Uuid, relationship: RelationshipType, cr...` — A graph edge between two entities (`item_relationships`).
- pub `NewItemRelationship` struct L33-37 — `{ source_id: Uuid, target_id: Uuid, relationship: RelationshipType }` — Insert for [`ItemRelationship`].
- pub `ItemRelationshipChangeset` struct L43-45 — `{ relationship: Option<RelationshipType> }` — Partial update for [`ItemRelationship`] (edges are usually
- pub `ItemHistory` struct L55-63 — `{ id: Uuid, item_id: Uuid, version: i32, title: String, content: String, edited_...` — An append-only content version snapshot (`item_history`, KAIROS-A-0004).
- pub `NewItemHistory` struct L69-75 — `{ item_id: Uuid, version: i32, title: String, content: String, edited_by: Uuid }` — Insert for [`ItemHistory`].
- pub `ActivityLogEntry` struct L86-98 — `{ id: Uuid, actor_id: Uuid, action: ActivityAction, entity_id: Option<Uuid>, ent...` — An audit-trail entry (`activity_log`, KAIROS-A-0004): transitions,
- pub `NewActivityLogEntry` struct L104-110 — `{ actor_id: Uuid, action: ActivityAction, entity_id: Option<Uuid>, entity_type: ...` — Insert for [`ActivityLogEntry`].

#### crates/kairos-db/src/models/items.rs

- pub `Strategy` struct L29-43 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Uuid, ...` — A strategy (Flight Level 3, `strategies`).
- pub `NewStrategy` struct L48-57 — `{ short_code: String, title: String, content: String, board_id: Uuid, column_id:...` — Insert for [`Strategy`]; `id`/`version`/timestamps come from defaults.
- pub `StrategyChangeset` struct L62-72 — `{ title: Option<String>, content: Option<String>, board_id: Option<Uuid>, column...` — Partial update for [`Strategy`].
- pub `Initiative` struct L85-101 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Uuid, ...` — An initiative (Flight Level 2, `initiatives`).
- pub `NewInitiative` struct L106-117 — `{ short_code: String, title: String, content: String, board_id: Uuid, column_id:...` — Insert for [`Initiative`].
- pub `InitiativeChangeset` struct L122-134 — `{ title: Option<String>, content: Option<String>, board_id: Option<Uuid>, column...` — Partial update for [`Initiative`].
- pub `Task` struct L147-167 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Uuid, ...` — A task/bug/tech-debt item (Flight Level 1, `tasks`).
- pub `NewTask` struct L172-184 — `{ short_code: String, title: String, content: String, board_id: Uuid, column_id:...` — Insert for [`Task`].
- pub `TaskChangeset` struct L189-201 — `{ title: Option<String>, content: Option<String>, board_id: Option<Uuid>, column...` — Partial update for [`Task`].
- pub `Document` struct L213-227 — `{ id: Uuid, short_code: String, title: String, content: String, template_id: Opt...` — A supporting document (`documents`); child of any entity via the
- pub `NewDocument` struct L232-239 — `{ short_code: String, title: String, content: String, template_id: Option<Uuid>,...` — Insert for [`Document`].
- pub `DocumentChangeset` struct L244-252 — `{ title: Option<String>, content: Option<String>, template_id: Option<Option<Uui...` — Partial update for [`Document`].
- pub `Adr` struct L265-280 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Option...` — An Architecture Decision Record (`adrs`).
- pub `NewAdr` struct L285-295 — `{ short_code: String, title: String, content: String, board_id: Option<Uuid>, co...` — Insert for [`Adr`].
- pub `AdrChangeset` struct L300-311 — `{ title: Option<String>, content: Option<String>, board_id: Option<Option<Uuid>>...` — Partial update for [`Adr`].

#### crates/kairos-db/src/models/mod.rs

- pub `boards` module L18 — `-` — table family:
- pub `enums` module L19 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `forge` module L20 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `graph` module L21 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `items` module L22 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `public` module L23 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `repositories` module L24 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `team_pages` module L25 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `teams` module L26 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `templates` module L27 — `-` — (`team_delivery_streams`) deliberately have none.

#### crates/kairos-db/src/models/public.rs

- pub `Organization` struct L27-33 — `{ id: Uuid, name: String, slug: String, created_at: DateTime<Utc>, updated_at: D...` — A tenant organization (`public.organizations`).
- pub `NewOrganization` struct L38-41 — `{ name: String, slug: String }` — Insert for [`Organization`]; `id`/timestamps come from column defaults.
- pub `OrganizationChangeset` struct L46-50 — `{ name: Option<String>, slug: Option<String>, updated_at: Option<DateTime<Utc>> ...` — Partial update for [`Organization`] (`None` = leave unchanged).
- pub `USER_KIND_HUMAN` variable L57 — `: &str` — `users.kind` value for an ordinary OIDC-backed human (the default).
- pub `USER_KIND_SERVICE_ACCOUNT` variable L60 — `: &str` — `users.kind` value for a service-account principal (KAIROS-A-0017): a
- pub `User` struct L69-78 — `{ id: Uuid, external_id: String, email: String, display_name: String, kind: Stri...` — A user principal (`public.users`) — an OIDC-backed human
- pub `is_service_account` function L82-84 — `(&self) -> bool` — True when this principal is a service account (KAIROS-A-0017).
- pub `NewUser` struct L91-95 — `{ external_id: String, email: String, display_name: String }` — Insert for a human [`User`].
- pub `NewServiceAccountUser` struct L102-107 — `{ external_id: String, email: String, display_name: String, kind: String }` — Insert for a service-account [`User`] (KAIROS-A-0017): sets
- pub `new` function L113-124 — `( external_id: impl Into<String>, email: impl Into<String>, display_name: impl I...` — A service-account insert with a synthetic `external_id` and the
- pub `UserChangeset` struct L130-135 — `{ external_id: Option<String>, email: Option<String>, display_name: Option<Strin...` — Partial update for [`User`].
- pub `OrganizationMember` struct L148-153 — `{ organization_id: Uuid, user_id: Uuid, role: OrgRole, joined_at: DateTime<Utc> ...` — Org membership + role (`public.organization_members`, composite PK).
- pub `NewOrganizationMember` struct L158-162 — `{ organization_id: Uuid, user_id: Uuid, role: OrgRole }` — Insert for [`OrganizationMember`].
- pub `OrganizationMemberChangeset` struct L167-169 — `{ role: Option<OrgRole> }` — Partial update for [`OrganizationMember`] (role changes).
- pub `SystemTemplate` struct L180-187 — `{ id: Uuid, name: String, slug: String, content: String, created_at: DateTime<Ut...` — System default template (`public.system_templates`), copied into tenant
- pub `NewSystemTemplate` struct L192-196 — `{ name: String, slug: String, content: String }` — Insert for [`SystemTemplate`].
- pub `SystemTemplateChangeset` struct L201-206 — `{ name: Option<String>, slug: Option<String>, content: Option<String>, updated_a...` — Partial update for [`SystemTemplate`].
- pub `SystemMetadataDefinition` struct L216-223 — `{ id: Uuid, name: String, slug: String, field_type: FieldType, created_at: DateT...` — System default metadata definition (`public.system_metadata_definitions`).
- pub `NewSystemMetadataDefinition` struct L228-232 — `{ name: String, slug: String, field_type: FieldType }` — Insert for [`SystemMetadataDefinition`].
- pub `SystemMetadataDefinitionChangeset` struct L237-242 — `{ name: Option<String>, slug: Option<String>, field_type: Option<FieldType>, upd...` — Partial update for [`SystemMetadataDefinition`].
- pub `SystemMetadataEnumOption` struct L254-259 — `{ id: Uuid, metadata_definition_id: Uuid, value: String, position: i32 }` — Enum option for a system metadata definition
- pub `NewSystemMetadataEnumOption` struct L264-268 — `{ metadata_definition_id: Uuid, value: String, position: i32 }` — Insert for [`SystemMetadataEnumOption`].
- pub `SystemMetadataEnumOptionChangeset` struct L273-276 — `{ value: Option<String>, position: Option<i32> }` — Partial update for [`SystemMetadataEnumOption`].
- pub `SystemTemplateMetadata` struct L289-295 — `{ id: Uuid, template_id: Uuid, metadata_definition_id: Uuid, default_value: Opti...` — Template-to-metadata association for system defaults
- pub `NewSystemTemplateMetadata` struct L300-305 — `{ template_id: Uuid, metadata_definition_id: Uuid, default_value: Option<String>...` — Insert for [`SystemTemplateMetadata`].
- pub `SystemTemplateMetadataChangeset` struct L311-314 — `{ default_value: Option<Option<String>>, required: Option<bool> }` — Partial update for [`SystemTemplateMetadata`].
- pub `SystemBoardDefault` struct L326-334 — `{ id: Uuid, board_level: BoardLevel, columns: String, transitions: String }` — Default board configuration per flight level
- pub `NewSystemBoardDefault` struct L339-344 — `{ board_level: BoardLevel, columns: String, transitions: String }` — Insert for [`SystemBoardDefault`].
- pub `SystemBoardDefaultChangeset` struct L349-354 — `{ board_level: Option<BoardLevel>, columns: Option<String>, transitions: Option<...` — Partial update for [`SystemBoardDefault`].
-  `User` type L80-85 — `= User` — the tenant `search_path` pinned on it.
-  `NewServiceAccountUser` type L109-125 — `= NewServiceAccountUser` — the tenant `search_path` pinned on it.

#### crates/kairos-db/src/models/repositories.rs

- pub `Repository` struct L23-41 — `{ id: Uuid, slug: String, forge: Forge, repo_full_name: String, repo_url: String...` — One repository (`repositories`).
- pub `NewRepository` struct L46-56 — `{ slug: String, forge: Forge, repo_full_name: String, repo_url: String, default_...` — Insert for [`Repository`].
- pub `RepositoryChangeset` struct L63-71 — `{ slug: Option<String>, repo_url: Option<String>, default_branch: Option<String>...` — Partial update for [`Repository`].

#### crates/kairos-db/src/models/team_pages.rs

- pub `TeamPage` struct L20-36 — `{ id: Uuid, team_id: Uuid, parent_id: Option<Uuid>, kind: TeamPageKind, slug: St...` — One node of a team's page tree (`team_pages`): a folder or a markdown
- pub `NewTeamPage` struct L41-52 — `{ team_id: Uuid, parent_id: Option<Uuid>, kind: TeamPageKind, slug: String, titl...` — Insert for [`TeamPage`].
- pub `TeamPageChangeset` struct L57-67 — `{ parent_id: Option<Option<Uuid>>, slug: Option<String>, title: Option<String>, ...` — Partial update for [`TeamPage`].
- pub `TeamPageHistory` struct L75-83 — `{ id: Uuid, page_id: Uuid, version: i32, title: String, content: String, edited_...` — One version snapshot of a team page (`team_page_history`, append-only;
- pub `NewTeamPageHistory` struct L88-94 — `{ page_id: Uuid, version: i32, title: String, content: String, edited_by: Uuid }` — Insert for [`TeamPageHistory`].
- pub `TeamAnnouncement` struct L102-109 — `{ id: Uuid, team_id: Uuid, body: String, pinned: bool, created_by: Uuid, created...` — A one-way team announcement (`team_announcements`, append-only by
- pub `NewTeamAnnouncement` struct L114-119 — `{ team_id: Uuid, body: String, pinned: bool, created_by: Uuid }` — Insert for [`TeamAnnouncement`].

#### crates/kairos-db/src/models/teams.rs

- pub `Team` struct L23-31 — `{ id: Uuid, name: String, slug: String, team_type: TeamType, deleted_at: Option<...` — A delivery team (`teams`).
- pub `NewTeam` struct L36-40 — `{ name: String, slug: String, team_type: TeamType }` — Insert for [`Team`].
- pub `TeamChangeset` struct L46-52 — `{ name: Option<String>, slug: Option<String>, team_type: Option<TeamType>, delet...` — Partial update for [`Team`].
- pub `TeamMember` struct L65-69 — `{ team_id: Uuid, user_id: Uuid, joined_at: DateTime<Utc> }` — Team membership (`team_members`, composite PK).
- pub `NewTeamMember` struct L74-77 — `{ team_id: Uuid, user_id: Uuid }` — Insert for [`TeamMember`].
- pub `TeamMemberChangeset` struct L82-84 — `{ joined_at: Option<DateTime<Utc>> }` — Partial update for [`TeamMember`].
- pub `DeliveryStream` struct L94-102 — `{ id: Uuid, name: String, slug: String, description: Option<String>, deleted_at:...` — A delivery stream (`delivery_streams`).
- pub `NewDeliveryStream` struct L107-111 — `{ name: String, slug: String, description: Option<String> }` — Insert for [`DeliveryStream`].
- pub `DeliveryStreamChangeset` struct L116-122 — `{ name: Option<String>, slug: Option<String>, description: Option<Option<String>...` — Partial update for [`DeliveryStream`].
- pub `TeamDeliveryStream` struct L139-142 — `{ team_id: Uuid, delivery_stream_id: Uuid }` — (`org_{slug}, public`; see [`crate::pool`]).

#### crates/kairos-db/src/models/templates.rs

- pub `Template` struct L24-32 — `{ id: Uuid, name: String, slug: String, content: String, is_system_default: bool...` — A tenant document template (`templates`), seeded from
- pub `NewTemplate` struct L37-42 — `{ name: String, slug: String, content: String, is_system_default: bool }` — Insert for [`Template`].
- pub `TemplateChangeset` struct L47-53 — `{ name: Option<String>, slug: Option<String>, content: Option<String>, is_system...` — Partial update for [`Template`].
- pub `MetadataDefinition` struct L63-71 — `{ id: Uuid, name: String, slug: String, field_type: FieldType, is_system_default...` — A tenant metadata field definition (`metadata_definitions`).
- pub `NewMetadataDefinition` struct L76-81 — `{ name: String, slug: String, field_type: FieldType, is_system_default: bool }` — Insert for [`MetadataDefinition`].
- pub `MetadataDefinitionChangeset` struct L86-92 — `{ name: Option<String>, slug: Option<String>, field_type: Option<FieldType>, is_...` — Partial update for [`MetadataDefinition`].
- pub `MetadataDefinitionScope` struct L107-110 — `{ metadata_definition_id: Uuid, entity_type: String }` — One entity type a metadata definition applies to
- pub `MetadataEnumOption` struct L121-126 — `{ id: Uuid, metadata_definition_id: Uuid, value: String, position: i32 }` — An enum option for a metadata definition (`metadata_enum_options`).
- pub `NewMetadataEnumOption` struct L131-135 — `{ metadata_definition_id: Uuid, value: String, position: i32 }` — Insert for [`MetadataEnumOption`].
- pub `MetadataEnumOptionChangeset` struct L140-143 — `{ value: Option<String>, position: Option<i32> }` — Partial update for [`MetadataEnumOption`].
- pub `TemplateMetadata` struct L155-161 — `{ id: Uuid, template_id: Uuid, metadata_definition_id: Uuid, default_value: Opti...` — Which metadata fields a template carries (`template_metadata`).
- pub `NewTemplateMetadata` struct L166-171 — `{ template_id: Uuid, metadata_definition_id: Uuid, default_value: Option<String>...` — Insert for [`TemplateMetadata`].
- pub `TemplateMetadataChangeset` struct L177-180 — `{ default_value: Option<Option<String>>, required: Option<bool> }` — Partial update for [`TemplateMetadata`].
- pub `ItemMetadata` struct L192-197 — `{ id: Uuid, item_id: Uuid, metadata_definition_id: Uuid, value: String }` — A metadata value on an entity (`item_metadata`).
- pub `NewItemMetadata` struct L202-206 — `{ item_id: Uuid, metadata_definition_id: Uuid, value: String }` — Insert for [`ItemMetadata`].
- pub `ItemMetadataChangeset` struct L211-213 — `{ value: Option<String> }` — Partial update for [`ItemMetadata`].

### crates/kairos-db/tests

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-db/tests/abac.rs

-  `DEFAULT_DATABASE_URL` variable L37 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L39 — `: &str` — off-board ADRs, orphan documents, and unknown ids resolving to None
-  `admin_database_url` function L41-43 — `() -> String` — off-board ADRs, orphan documents, and unknown ids resolving to None
-  `with_database` function L46-51 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `insert_user` function L53-64 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — off-board ADRs, orphan documents, and unknown ids resolving to None
-  `board_id_by_slug` function L67-73 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `first_column` function L76-83 — `(conn: &mut PgConnection, board: Uuid) -> Uuid` — The first column of a board (position order) — items need a placement.
-  `activity_details` function L86-94 — `(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String>` — All `activity_log.details` values for (action, entity_id), in order.
-  `assert_check` function L98-116 — `( conn: &mut PgConnection, board: Uuid, user: Uuid, granted: &str, required: &st...` — Assert the SQL check agrees with the pure core matcher for one stored
-  `abac_capability_lifecycle` function L119-611 — `()` — off-board ADRs, orphan documents, and unknown ids resolving to None
-  `team_membership_implies_delivery_capabilities` function L618-747 — `()` — KAIROS-T-0072 (A-0006 amendment): membership of a board's owning team
-  `TEAM_SCRATCH_DB` variable L621 — `: &str` — off-board ADRs, orphan documents, and unknown ids resolving to None
-  `archived_items_resolve_the_same_capabilities_as_live_ones` function L758-917 — `()` — KAIROS-T-0153 / [KAIROS-A-0020]: archiving is a visibility default, not a
-  `ARCHIVE_SCRATCH_DB` variable L764 — `: &str` — off-board ADRs, orphan documents, and unknown ids resolving to None

#### crates/kairos-db/tests/api_keys.rs

-  `DEFAULT_DATABASE_URL` variable L20 — `: &str` — without colliding.
-  `SLUG` variable L21 — `: &str` — without colliding.
-  `admin_url` function L23-25 — `() -> String` — without colliding.
-  `with_database` function L27-30 — `(url: &str, db: &str) -> String` — without colliding.
-  `setup` function L34-50 — `(db: &str) -> PgConnection` — Drop+recreate the named scratch DB, run public migrations, provision one
-  `teardown` function L52-55 — `(db: &str)` — without colliding.
-  `api_key_round_trip` function L58-123 — `()` — without colliding.
-  `expiry_is_honored` function L126-160 — `()` — without colliding.

#### crates/kairos-db/tests/board_move.rs

-  `DEFAULT_DATABASE_URL` variable L25 — `: &str` — own scratch database like the other kairos-db integration tests.
-  `SCRATCH_DB` variable L26 — `: &str` — own scratch database like the other kairos-db integration tests.
-  `admin_database_url` function L28-30 — `() -> String` — own scratch database like the other kairos-db integration tests.
-  `with_database` function L32-37 — `(url: &str, db_name: &str) -> String` — own scratch database like the other kairos-db integration tests.
-  `insert_user` function L39-51 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — own scratch database like the other kairos-db integration tests.
-  `seed_team` function L53-75 — `(conn: &mut PgConnection, name: &str, slug: &str, actor: Uuid) -> (Uuid, Uuid)` — own scratch database like the other kairos-db integration tests.
-  `column_name` function L77-84 — `(conn: &mut PgConnection, column_id: Uuid) -> String` — own scratch database like the other kairos-db integration tests.
-  `activity_details` function L86-94 — `(conn: &mut PgConnection, item: Uuid) -> Vec<String>` — own scratch database like the other kairos-db integration tests.
-  `move_task_between_delivery_boards` function L97-245 — `()` — own scratch database like the other kairos-db integration tests.

#### crates/kairos-db/tests/board_rules.rs

-  `DEFAULT_DATABASE_URL` variable L40 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L42 — `: &str` — removed
-  `admin_database_url` function L44-46 — `() -> String` — removed
-  `with_database` function L49-54 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `board_id_by_slug` function L57-63 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `column_id_by_name` function L68-76 — `(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid` — The LIVE column named `name` on `board`.
-  `column_names` function L79-87 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Live column names of `board` in position order — what the board is.
-  `column_name_of` function L91-97 — `(conn: &mut PgConnection, column: Uuid) -> String` — The name stored on a column row, removed ones included — the audit
-  `NameRow` struct L100-103 — `{ name: String }` — removed
-  `transition_pairs` function L109-123 — `(conn: &mut PgConnection, board: Uuid) -> BTreeSet<String>` — Transition pairs `"From -> To"` for a board (current search_path
-  `live_transition_pairs` function L128-143 — `(conn: &mut PgConnection, board: Uuid) -> BTreeSet<String>` — Transition pairs between LIVE columns only — the edges a move is
-  `pairs` function L145-147 — `(list: &[(&str, &str)]) -> BTreeSet<String>` — removed
-  `item_column` macro L150-158 — `-` — The `column_id` an item row currently occupies.
-  `activity_details` function L161-169 — `(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String>` — All `activity_log.details` values for (action, entity_id).
-  `dead_end_names` function L171-177 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — removed
-  `board_rules_lifecycle` function L180-755 — `()` — removed

#### crates/kairos-db/tests/graph.rs

-  `DEFAULT_DATABASE_URL` variable L42 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L44 — `: &str` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `admin_database_url` function L46-48 — `() -> String` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `with_database` function L51-56 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `insert_user` function L58-69 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `board_id_by_slug` function L72-78 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `relationship_activity` function L82-90 — `(conn: &mut PgConnection, action: ActivityAction) -> Vec<String>` — All `activity_log.details` for relationship actions (`entity_id` is
-  `edge_count` function L93-106 — `( conn: &mut PgConnection, source: Uuid, target: Uuid, relationship: Relationshi...` — How many edges `(source, target, relationship)` exist.
-  `assert_rule_rejected` function L110-136 — `( conn: &mut PgConnection, source: Uuid, target: Uuid, relationship: Relationshi...` — Assert a link attempt is rejected by the type-rule matrix with the
-  `ExplainRow` struct L141-143 — `{ line: String }` — One EXPLAIN output line.
-  `ExplainRow` type L145-153 — `= ExplainRow` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `build` function L146-152 — `( row: &impl diesel::row::NamedRow<'a, diesel::pg::Pg>, ) -> diesel::deserialize...` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `explain` function L159-167 — `(conn: &mut PgConnection, sql: &str) -> String` — The EXPLAIN plan for `sql`, one string.
-  `relationship_graph_service` function L170-737 — `()` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `children_progress_rollups` function L744-922 — `()` — KAIROS-T-0080: the children-progress rollups — per-parent grouping,
-  `PROGRESS_DB` variable L745 — `: &str` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `focal_subgraph_contract` function L941-1196 — `()` — `item_subgraph`: depth bounding with min-depth per node, cross-links
-  `SUBGRAPH_DB` variable L942 — `: &str` — (`idx_item_relationships_source` / `idx_item_relationships_target`)
-  `archived_children_are_listed_marked_but_never_counted` function L1219-1370 — `()` — The case that motivated KAIROS-T-0158: **an initiative with one live
-  `NEIGHBOUR_DB` variable L1220 — `: &str` — (`idx_item_relationships_source` / `idx_item_relationships_target`)

#### crates/kairos-db/tests/isolation.rs

-  `DEFAULT_DATABASE_URL` variable L73 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `TENANTS` variable L77 — `: [&str; 2]` — The two tenants every test provisions.
-  `admin_database_url` function L79-81 — `() -> String` — by construction; each resolves only within its own tenant, never cross.
-  `with_database` function L84-89 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `scratch_database` function L93-115 — `(db_name: &str) -> String` — Drop + recreate `db_name`, run public migrations, provision `acme` and
-  `drop_scratch_database` function L117-123 — `(db_name: &str)` — by construction; each resolves only within its own tenant, never cross.
-  `tenant_conn` function L127-131 — `(url: &str, slug: &str) -> PgConnection` — A fresh connection pinned to `org_{slug}` — the exact mechanism the pool
-  `pin` function L134-138 — `(conn: &mut PgConnection, slug: &str)` — (Re)pin an existing connection to `org_{slug}`.
-  `insert_user` function L140-151 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — by construction; each resolves only within its own tenant, never cross.
-  `board_id_by_slug` function L153-159 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — by construction; each resolves only within its own tenant, never cross.
-  `first_column` function L161-168 — `(conn: &mut PgConnection, board: Uuid) -> Uuid` — by construction; each resolves only within its own tenant, never cross.
-  `metadata_def` function L170-176 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — by construction; each resolves only within its own tenant, never cross.
-  `CountRow` struct L179-182 — `{ n: i64 }` — by construction; each resolves only within its own tenant, never cross.
-  `count_where_id` function L186-194 — `(conn: &mut PgConnection, table: &str, column: &str, id: Uuid) -> i64` — `count(*)` of `sql_fragment` (a full `SELECT ...
-  `directory_id_by_code` function L201-214 — `(conn: &mut PgConnection, short_code: &str) -> Option<Uuid>` — The id `entity_directory` resolves for `short_code` in the CURRENT tenant
-  `IdRow` struct L203-206 — `{ id: Uuid }` — by construction; each resolves only within its own tenant, never cross.
-  `Seed` struct L218-231 — `{ marker: String, strategy: Uuid, initiative: Uuid, task: Uuid, document: Uuid, ...` — Every seeded item in one tenant, with the short codes services assigned
-  `COLLIDE_CODE` variable L234 — `: &str` — The short code deliberately shared, byte-for-byte, across both tenants.
-  `seed` function L241-397 — `(conn: &mut PgConnection, slug: &str, user: Uuid) -> Seed` — Seed one tenant (connection already pinned to it).
-  `run_search` function L403-407 — `(conn: &mut PgConnection, request: serde_json::Value) -> SearchResults` — by construction; each resolves only within its own tenant, never cross.
-  `all_result_ids` function L409-417 — `(results: &SearchResults) -> Vec<Uuid>` — by construction; each resolves only within its own tenant, never cross.
-  `result_count` function L419-421 — `(results: &SearchResults) -> usize` — by construction; each resolves only within its own tenant, never cross.
-  `CHECKSUM_TABLES` variable L431-442 — `: [&str; 10]` — The tables whose contents must be byte-identical before and after an
-  `table_fingerprint` function L445-459 — `(conn: &mut PgConnection, table: &str) -> String` — `count:md5` fingerprint of one table in the CURRENT tenant schema.
-  `Fp` struct L447-450 — `{ fp: String }` — by construction; each resolves only within its own tenant, never cross.
-  `fingerprint_all` function L463-468 — `(conn: &mut PgConnection) -> Vec<(String, String)>` — Fingerprint every [`CHECKSUM_TABLES`] table (connection pinned to the
-  `cross_read_visibility_sweep` function L475-672 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L476 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `cross_write_battery` function L679-795 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L680 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `short_code_collision` function L802-869 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L803 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `pool_stress` module L880-1039 — `-` — Barrier-synchronized concurrent rounds over ONE small pool, alternating
-  `ROUNDS` variable L895 — `: usize` — by construction; each resolves only within its own tenant, never cross.
-  `TitleRow` struct L898-901 — `{ title: String }` — by construction; each resolves only within its own tenant, never cross.
-  `TenantFixture` struct L906-909 — `{ board: Uuid, column: Uuid }` — Per-tenant board + first column, resolved once (sync) for the async
-  `pool_reuse_stress` function L912-1038 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L913 — `: &str` — by construction; each resolves only within its own tenant, never cross.

#### crates/kairos-db/tests/models_roundtrip.rs

- pub `scratch_database` function L62-85 — `(db_name: &str, tenants: &[&str]) -> String` — Drop + recreate `db_name`, run public migrations, provision
- pub `drop_scratch_database` function L87-93 — `(db_name: &str)` — KAIROS-T-0016 adversarial suite.
-  `setup` module L40-94 — `-` — Synchronous scratch-database plumbing (own module so the sync
-  `DEFAULT_DATABASE_URL` variable L46 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `admin_database_url` function L48-50 — `() -> String` — KAIROS-T-0016 adversarial suite.
-  `with_database` function L53-58 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `SearchPathRow` struct L97-100 — `{ search_path: String }` — KAIROS-T-0016 adversarial suite.
-  `show_search_path` function L102-108 — `(conn: &mut diesel_async::AsyncPgConnection) -> String` — KAIROS-T-0016 adversarial suite.
-  `PidRow` struct L111-114 — `{ pid: i32 }` — KAIROS-T-0016 adversarial suite.
-  `backend_pid` function L116-122 — `(conn: &mut diesel_async::AsyncPgConnection) -> i32` — KAIROS-T-0016 adversarial suite.
-  `board_with_columns` function L125-143 — `( conn: &mut diesel_async::AsyncPgConnection, level: BoardLevel, ) -> (Board, Ve...` — The board (by level) and its columns, in position order.
-  `models_round_trip` function L146-529 — `()` — KAIROS-T-0016 adversarial suite.
-  `DB` variable L147 — `: &str` — KAIROS-T-0016 adversarial suite.
-  `pool_isolation_interleaved` function L532-670 — `()` — KAIROS-T-0016 adversarial suite.
-  `DB` variable L533 — `: &str` — KAIROS-T-0016 adversarial suite.
-  `team_names` function L555-562 — `(conn: &mut kairos_db::TenantConnection) -> Vec<String>` — KAIROS-T-0016 adversarial suite.

#### crates/kairos-db/tests/public_migrations.rs

-  `DEFAULT_DATABASE_URL` variable L22 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L24 — `: &str` — touches the dev `kairos` database or interferes with other tests.
-  `EXPECTED_TABLES` variable L28-38 — `: [&str; 9]` — The public-schema tables: the 8 defined by KAIROS-S-0004 plus the
-  `admin_database_url` function L40-42 — `() -> String` — touches the dev `kairos` database or interferes with other tests.
-  `with_database` function L45-50 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `TableName` struct L53-56 — `{ table_name: String }` — touches the dev `kairos` database or interferes with other tests.
-  `public_base_tables` function L58-73 — `(conn: &mut PgConnection) -> Vec<String>` — touches the dev `kairos` database or interferes with other tests.
-  `assert_database_error_kind` function L76-85 — `( result: Result<T, DieselError>, kind: DatabaseErrorKind, context: &str, )` — Expect a database error of `kind` from `result`.
-  `public_migrations_from_empty_database` function L88-154 — `()` — touches the dev `kairos` database or interferes with other tests.

#### crates/kairos-db/tests/repositories_migration.rs

-  `DEFAULT_DATABASE_URL` variable L24 — `: &str` — the scratch database `kairos_repositories_migration_test`.
-  `SCRATCH_DB` variable L25 — `: &str` — the scratch database `kairos_repositories_migration_test`.
-  `DOWN_SQL` variable L26 — `: &str` — the scratch database `kairos_repositories_migration_test`.
-  `REPOSITORIES_VERSION` variable L32 — `: &str` — The diesel bookkeeping version of the migration above — its directory
-  `admin_database_url` function L34-36 — `() -> String` — the scratch database `kairos_repositories_migration_test`.
-  `with_database` function L38-41 — `(url: &str, db_name: &str) -> String` — the scratch database `kairos_repositories_migration_test`.
-  `CountRow` struct L44-47 — `{ count: i64 }` — the scratch database `kairos_repositories_migration_test`.
-  `TextRow` struct L50-53 — `{ value: String }` — the scratch database `kairos_repositories_migration_test`.
-  `count` function L55-60 — `(conn: &mut PgConnection, sql: &str) -> i64` — the scratch database `kairos_repositories_migration_test`.
-  `texts` function L62-69 — `(conn: &mut PgConnection, sql: &str) -> Vec<String>` — the scratch database `kairos_repositories_migration_test`.
-  `revert_repositories_migration` function L74-90 — `(conn: &mut PgConnection)` — Put `org_acme` back into the pre-`repositories` shape: run the down
-  `old_connection` function L93-117 — `( conn: &mut PgConnection, forge: &str, full_name: &str, team: Option<Uuid>, del...` — Insert one OLD-shape connection (the columns the migration drops).
-  `repositories_migration_on_populated_tables` function L120-343 — `()` — the scratch database `kairos_repositories_migration_test`.

#### crates/kairos-db/tests/retention.rs

-  `DEFAULT_DATABASE_URL` variable L53 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L55 — `: &str` — scheduler test, no database)
-  `admin_database_url` function L57-59 — `() -> String` — scheduler test, no database)
-  `with_database` function L62-67 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `pin` function L72-76 — `(conn: &mut PgConnection, schema: &str)` — Pin the connection's `search_path` to a tenant schema (+ public).
-  `insert_user` function L78-89 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — scheduler test, no database)
-  `board_id_by_slug` function L91-97 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — scheduler test, no database)
-  `at` function L99-101 — `(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc>` — scheduler test, no database)
-  `seed_strategy` function L104-138 — `( conn: &mut PgConnection, board: Uuid, name: &str, versions: i32, actor: Uuid, ...` — Create a strategy with `versions` content versions (create + updates).
-  `backdate_history` function L142-152 — `(conn: &mut PgConnection, item: Uuid, version: i32, ts: DateTime<Utc>)` — Fabricate a snapshot's age (the KAIROS-T-0015 injected-clock seeding:
-  `history_versions` function L155-162 — `(conn: &mut PgConnection, item: Uuid) -> Vec<i32>` — Remaining history versions of an item, ascending.
-  `total_history_rows` function L164-169 — `(conn: &mut PgConnection) -> i64` — scheduler test, no database)
-  `sweep_details` function L172-179 — `(conn: &mut PgConnection) -> Vec<String>` — `retention_sweep` audit rows' details, in `occurred_at` order.
-  `old_activity_count` function L181-187 — `(conn: &mut PgConnection, cutoff: DateTime<Utc>) -> i64` — scheduler test, no database)
-  `counts` function L189-195 — `(archived: u64, pruned: u64, warnings: u64) -> TableCounts` — scheduler test, no database)
-  `read_ndjson` function L198-205 — `(path: &std::path::Path) -> Vec<serde_json::Value>` — Parse an NDJSON archive file into one `serde_json::Value` per line.
-  `json_ts` function L207-213 — `(value: &serde_json::Value, key: &str) -> DateTime<Utc>` — scheduler test, no database)
-  `retention_sweeper_lifecycle` function L216-574 — `()` — scheduler test, no database)
-  `retention_loop_ticks_until_break` function L581-597 — `()` — The in-process scheduler (KAIROS-A-0004 "in-process scheduled task"):

#### crates/kairos-db/tests/search.rs

-  `DEFAULT_DATABASE_URL` variable L54 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L56 — `: &str` — within the stated budget with exactly one task-hydration query
-  `admin_database_url` function L58-60 — `() -> String` — within the stated budget with exactly one task-hydration query
-  `with_database` function L63-68 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `insert_user` function L70-81 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — within the stated budget with exactly one task-hydration query
-  `board_id_by_slug` function L84-90 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `metadata_definition` function L94-115 — `(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid` — The tenant's metadata definition with this slug, creating it if the
-  `set_metadata` function L117-126 — `(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str)` — within the stated budget with exactly one task-hydration query
-  `run` function L130-135 — `(conn: &mut PgConnection, request: serde_json::Value) -> (SearchResults, SearchS...` — Parse a request from its JSON shape (the S-0005 wire format) and run it
-  `ids` function L137-139 — `(rows: &[T], id_of: impl Fn(&T) -> Uuid) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `task_ids` function L141-143 — `(results: &SearchResults) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `all_ids` function L145-153 — `(results: &SearchResults) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `returned_row_count` function L155-161 — `(results: &SearchResults) -> usize` — within the stated budget with exactly one task-hydration query
-  `unified_search_pipeline` function L164-929 — `()` — within the stated budget with exactly one task-hydration query

#### crates/kairos-db/tests/seed_demo.rs

-  `DEFAULT_DATABASE_URL` variable L29 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L31 — `: &str` — (`kairos_seed_demo_test`) on the shared compose server.
-  `admin_database_url` function L33-35 — `() -> String` — (`kairos_seed_demo_test`) on the shared compose server.
-  `with_database` function L38-43 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `CountRow` struct L46-49 — `{ count: i64 }` — (`kairos_seed_demo_test`) on the shared compose server.
-  `count` function L51-56 — `(conn: &mut PgConnection, query: &str) -> i64` — (`kairos_seed_demo_test`) on the shared compose server.
-  `TextRow` struct L59-62 — `{ value: String }` — (`kairos_seed_demo_test`) on the shared compose server.
-  `text_values` function L64-71 — `(conn: &mut PgConnection, query: &str) -> Vec<String>` — (`kairos_seed_demo_test`) on the shared compose server.
-  `seed_demo_fixture_lifecycle` function L74-404 — `()` — (`kairos_seed_demo_test`) on the shared compose server.

#### crates/kairos-db/tests/team_pages.rs

-  `DEFAULT_DATABASE_URL` variable L21 — `: &str` — the scratch database `kairos_team_pages_test`.
-  `SCRATCH_DB` variable L22 — `: &str` — the scratch database `kairos_team_pages_test`.
-  `admin_database_url` function L24-26 — `() -> String` — the scratch database `kairos_team_pages_test`.
-  `with_database` function L28-33 — `(url: &str, db_name: &str) -> String` — the scratch database `kairos_team_pages_test`.
-  `scaffold_shape` function L37-64 — `(conn: &mut PgConnection, team: Uuid) -> Vec<(String, String, String, bool)>` — All live scaffold nodes of a team as (parent slug or "", slug, kind,
-  `team_page_scaffold` function L67-262 — `()` — the scratch database `kairos_team_pages_test`.

#### crates/kairos-db/tests/tenant_provisioning.rs

-  `DEFAULT_DATABASE_URL` variable L33 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L35 — `: &str` — `system_board_defaults` rows are seeded.
-  `EXPECTED_TABLES` variable L40-71 — `: [&str; 30]` — The tenant tables (sorted): the 21 from the KAIROS-S-0004 DDL plus
-  `EXPECTED_VIEWS` variable L73 — `: [&str; 2]` — `system_board_defaults` rows are seeded.
-  `EXPECTED_SEQUENCES` variable L75-81 — `: [&str; 5]` — `system_board_defaults` rows are seeded.
-  `EXPECTED_INDEXES` variable L88-112 — `: [&str; 23]` — Every named index a freshly provisioned tenant carries: the S-0004
-  `admin_database_url` function L114-116 — `() -> String` — `system_board_defaults` rows are seeded.
-  `with_database` function L119-124 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `NameRow` struct L127-130 — `{ name: String }` — `system_board_defaults` rows are seeded.
-  `CountRow` struct L133-136 — `{ count: i64 }` — `system_board_defaults` rows are seeded.
-  `names` function L138-148 — `(conn: &mut PgConnection, query: &str, param: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_tables` function L150-158 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_views` function L160-166 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_sequences` function L168-175 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_indexes` function L177-183 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_exists` function L185-192 — `(conn: &mut PgConnection, schema: &str) -> bool` — `system_board_defaults` rows are seeded.
-  `count` function L194-199 — `(conn: &mut PgConnection, sql: &str) -> i64` — `system_board_defaults` rows are seeded.
-  `board_columns` function L202-214 — `(conn: &mut PgConnection, board_slug: &str) -> Vec<String>` — Board columns (ordered by position) for a board slug in `org_acme`.
-  `board_transitions` function L217-232 — `(conn: &mut PgConnection, board_slug: &str) -> BTreeSet<String>` — Transition pairs `"From -> To"` for a board slug in `org_acme`.
-  `transitions` function L234-236 — `(pairs: &[(&str, &str)]) -> BTreeSet<String>` — `system_board_defaults` rows are seeded.
-  `tenant_provisioning_lifecycle` function L239-655 — `()` — `system_board_defaults` rows are seeded.

#### crates/kairos-db/tests/write_path.rs

-  `DEFAULT_DATABASE_URL` variable L48 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L50 — `: &str` — records ONE activity row with the cascade count + short codes
-  `admin_database_url` function L52-54 — `() -> String` — records ONE activity row with the cascade count + short codes
-  `with_database` function L57-62 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `tenant_connection` function L66-72 — `(scratch_url: &str) -> PgConnection` — A fresh connection to the scratch database with the tenant search_path
-  `insert_user` function L74-85 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — records ONE activity row with the cascade count + short codes
-  `board_id_by_slug` function L88-94 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `first_column` function L97-104 — `(conn: &mut PgConnection, board: Uuid) -> Uuid` — The first column of a board (position order).
-  `history_of` function L108-119 — `(conn: &mut PgConnection, item: Uuid) -> Vec<(i32, String, String)>` — All history snapshots for an item as `(version, title, content)`, in
-  `activity_details` function L122-130 — `(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String>` — All `activity_log.details` values for (action, entity_id), in order.
-  `CountRow` struct L133-136 — `{ n: i64 }` — records ONE activity row with the cascade count + short codes
-  `view_count` function L146-154 — `(conn: &mut PgConnection, view: &str, id: Uuid) -> i64` — How many LIVE rows of `searchable_items` / `entity_directory` carry
-  `view_count_any` function L160-166 — `(conn: &mut PgConnection, view: &str, id: Uuid) -> i64` — How many rows of the view carry this id REGARDLESS of liveness — the
-  `code_number` function L169-174 — `(code: &str) -> i64` — The numeric part of a `{PREFIX}-{L}-{NNNN}` short code.
-  `write_path_lifecycle` function L177-830 — `()` — records ONE activity row with the cascade count + short codes
-  `WRITERS` variable L373 — `: usize` — records ONE activity row with the cascade count + short codes

### crates/kairos-server/examples

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/examples/e2e_golden_path.rs

-  `TENANT` variable L40 — `: &str` — The tenant `seed-demo` provisions.
-  `SEARCH_TOKEN` variable L45 — `: &str` — A token that appears ONLY in the items this run creates, so the search
-  `Step` struct L47-50 — `{ n: u32, name: &'static str }` — smoke suite.
-  `Step` type L52-70 — `= Step` — smoke suite.
-  `start` function L53-55 — `(n: u32, name: &'static str) -> Step` — smoke suite.
-  `ok` function L57-64 — `(&self, detail: impl AsRef<str>)` — smoke suite.
-  `fail` function L66-69 — `(&self, cause: impl std::fmt::Display) -> !` — smoke suite.
-  `or_fail` function L73-78 — `(step: &Step, result: Result<T, E>) -> T` — `expect`-like unwrap that attributes the failure to its step.
-  `require` function L80-84 — `(step: &Step, condition: bool, cause: &str)` — smoke suite.
-  `rpc_message` function L88-103 — `(step: &Step, body: &str) -> Value` — Extract the JSON-RPC message from a streamable-HTTP response body:
-  `mcp_post` function L107-133 — `( step: &Step, http: &reqwest::Client, base_url: &str, token: &str, session: Opt...` — One JSON-RPC POST to `/mcp` (streamable HTTP, X-Tenant header).
-  `McpSession` struct L136-141 — `{ http: &'a reqwest::Client, base_url: &'a str, token: &'a str, session_id: Stri...` — An established MCP session's wiring (client, endpoint, credentials).
-  `tool` function L146-182 — `(&self, step: &Step, id: i64, tool: &str, arguments: Value) -> String` — Call one MCP tool; returns the CallToolResult text, failing the
-  `main` function L186-513 — `() -> ExitCode` — smoke suite.

### crates/kairos-server/src/api

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/api/adrs.rs

- pub `router` function L34-42 — `() -> Router<AppState>` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `MANAGE` variable L32 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L45-59 — `(conn: &mut PgConnection, short_code: &str, liveness: Liveness) -> Result<Adr, A...` — Load the live ADR with this short code, or 404.
-  `list_adrs` function L76-112 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `get_adr` function L125-137 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `create_adr` function L152-192 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `update_adr` function L209-246 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `delete_adr` function L261-284 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `transition_adr` function L302-322 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).

#### crates/kairos-server/src/api/cascade.rs

- pub `router` function L32-37 — `() -> Router<AppState>` — stack.
-  `cascade_preview` function L54-74 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — stack.

#### crates/kairos-server/src/api/convert.rs

- pub `IntoDto` interface L21-23 — `{ fn into_dto() }` — Local conversion into a shared wire type (`model.into_dto()`).
- pub `repository_ref` function L140-148 — `(repo: &Repository) -> RepositoryRef` — The embedded repository ref (KAIROS-T-0104).
- pub `attach_repositories` function L155-189 — `( conn: &mut PgConnection, tasks: &mut [dto::Task], ) -> Result<(), diesel::resu...` — Fill `repository` on a batch of task DTOs with ONE query over the
- pub `attach_repository` function L192-200 — `( conn: &mut PgConnection, task: dto::Task, ) -> Result<dto::Task, diesel::resul...` — Single-task convenience over [`attach_repositories`].
-  `timestamp` function L27-29 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (stable wire format for
-  `Strategy` type L31-49 — `= Strategy` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L32-48 — `(self) -> dto::Strategy` — here; [`IntoDto`] is the local conversion trait instead.
-  `Initiative` type L51-71 — `= Initiative` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L52-70 — `(self) -> dto::Initiative` — here; [`IntoDto`] is the local conversion trait instead.
-  `Task` type L73-97 — `= Task` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L74-96 — `(self) -> dto::Task` — here; [`IntoDto`] is the local conversion trait instead.
-  `Document` type L99-116 — `= Document` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L100-115 — `(self) -> dto::Document` — here; [`IntoDto`] is the local conversion trait instead.
-  `Adr` type L118-137 — `= Adr` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L119-136 — `(self) -> dto::Adr` — here; [`IntoDto`] is the local conversion trait instead.

#### crates/kairos-server/src/api/convert_meta.rs

- pub `definition_dto` function L88-104 — `( definition: MetadataDefinition, enum_options: Vec<String>, entity_types: Vec<S...` — A [`MetadataDefinition`] plus its option values and entity-type
- pub `template_detail_dto` function L109-123 — `( template: Template, metadata: Vec<dto::TemplateMetadataField>, ) -> dto::Templ...` — A [`Template`] plus its hydrated metadata fields → the detail DTO
-  `timestamp` function L19-21 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (the same wire format as
-  `ItemRelationship` type L23-33 — `= ItemRelationship` — with microsecond precision.
-  `into_dto` function L24-32 — `(self) -> dto::Relationship` — with microsecond precision.
-  `Template` type L35-47 — `= Template` — with microsecond precision.
-  `into_dto` function L36-46 — `(self) -> dto::Template` — with microsecond precision.
-  `ItemHistory` type L49-57 — `= ItemHistory` — with microsecond precision.
-  `into_dto` function L50-56 — `(self) -> dto::HistoryVersion` — with microsecond precision.
-  `ItemHistory` type L59-69 — `= ItemHistory` — with microsecond precision.
-  `into_dto` function L60-68 — `(self) -> dto::HistorySnapshot` — with microsecond precision.
-  `ActivityLogEntry` type L71-83 — `= ActivityLogEntry` — with microsecond precision.
-  `into_dto` function L72-82 — `(self) -> dto::ActivityEntry` — with microsecond precision.

#### crates/kairos-server/src/api/convert_org.rs

- pub `team_to_dto` function L75-85 — `(team: Team, delivery_board_id: Option<uuid::Uuid>) -> dto::Team` — [`Team`] → DTO.
-  `timestamp` function L14-16 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (same as [`super::convert`]).
-  `Board` type L18-30 — `= Board` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L19-29 — `(self) -> dto::Board` — its own module so T-0018's `convert.rs` stays untouched.
-  `BoardColumn` type L32-47 — `= BoardColumn` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L33-46 — `(self) -> dto::BoardColumn` — its own module so T-0018's `convert.rs` stays untouched.
-  `BoardTransition` type L49-58 — `= BoardTransition` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L50-57 — `(self) -> dto::BoardTransition` — its own module so T-0018's `convert.rs` stays untouched.
-  `DeliveryStream` type L60-71 — `= DeliveryStream` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L61-70 — `(self) -> dto::DeliveryStream` — its own module so T-0018's `convert.rs` stays untouched.

#### crates/kairos-server/src/api/documents.rs

- pub `router` function L45-58 — `() -> Router<AppState>` — pre-checks make a link failure after create unreachable in practice.
-  `MANAGE` variable L43 — `: &str` — The A-0006 manage capability for this family.
-  `set_lifecycle` function L78-104 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.
-  `load` function L107-125 — `( conn: &mut PgConnection, short_code: &str, liveness: Liveness, ) -> Result<Doc...` — Load the live document with this short code, or 404.
-  `authorization_board` function L130-135 — `( conn: &mut PgConnection, document_id: uuid::Uuid, ) -> Result<Option<uuid::Uui...` — The board that authorizes writes to this document: its parent workflow
-  `list_documents` function L155-191 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — pre-checks make a link failure after create unreachable in practice.
-  `get_document` function L204-216 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — pre-checks make a link failure after create unreachable in practice.
-  `create_document` function L233-293 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.
-  `update_document` function L310-348 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.
-  `delete_document` function L363-387 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.

#### crates/kairos-server/src/api/initiatives.rs

- pub `router` function L30-46 — `() -> Router<AppState>` — T-0018 handler pattern.
-  `MANAGE` variable L28 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L49-67 — `( conn: &mut PgConnection, short_code: &str, liveness: Liveness, ) -> Result<Ini...` — Load the live initiative with this short code, or 404.
-  `list_initiatives` function L84-120 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — T-0018 handler pattern.
-  `get_initiative` function L133-145 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — T-0018 handler pattern.
-  `create_initiative` function L160-201 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `update_initiative` function L218-256 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `delete_initiative` function L272-295 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `transition_initiative` function L312-339 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.

#### crates/kairos-server/src/api/mod.rs

- pub `adrs` module L29 — `-` — initiatives, tasks, documents, and ADRs — list/get/create/PATCH/DELETE
- pub `cascade` module L31 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `convert` module L32 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `convert_meta` module L33 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `convert_org` module L34 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `documents` module L35 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `initiatives` module L36 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `meta` module L37 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `org` module L38 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `search` module L39 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `strategies` module L40 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `tasks` module L41 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `openapi` module L43 — `-` — aggregation endpoint is KAIROS-T-0023.
- pub `router` function L62-71 — `() -> Router<AppState>` — All five entity family routers, merged (mounted behind the full
- pub `clamp_pagination` function L83-90 — `(pagination: &dto::Pagination) -> (i64, i64)` — Clamp raw S-0005 pagination params to `(limit, offset)`.
- pub `clamp_list` function L98-109 — `(query: &dto::ListQuery) -> (i64, i64, Liveness)` — Clamp an entity-family list query to `(limit, offset, liveness)`.
- pub `parse_uuid` function L117-120 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — Parse a UUID body field (`422 VALIDATION` on malformed input — the DTO
- pub `parse_opt_uuid` function L123-125 — `(value: Option<&str>, field: &str) -> Result<Option<Uuid>, ApiError>` — [`parse_uuid`] over an optional field.
- pub `parse_enum` function L129-140 — `(value: &str, field: &str, allowed: &[T]) -> Result<T, ApiError>` — Parse a TEXT-backed enum body field (`task_type`, `complexity`,
- pub `require_capability` function L151-168 — `( conn: &mut PgConnection, slug: &str, board_id: Option<Uuid>, user_id: Uuid, ca...` — The A-0006 write gate: org admins bypass; otherwise the caller needs a
- pub `Liveness` enum L191-197 — `LiveOnly | IncludeArchived` — Whether a lookup may return work that has been archived.
- pub `resolve_short_code` function L209-242 — `( conn: &mut PgConnection, short_code: &str, liveness: Liveness, ) -> Result<Opt...` — Resolve a short code to `(id, entity_type)` across all five entity
- pub `resolve_item_type` function L250-264 — `(conn: &mut PgConnection, id: Uuid) -> Result<Option<ItemType>, ApiError>` — The type of a live item by id (documents included), via the same
- pub `short_code_not_found` function L272-274 — `(entity_type: &str, short_code: &str) -> ApiError` — The 404 for `/{short_code}` path segments that resolve to nothing.
- pub `map_abac_error` function L283-285 — `(e: AbacError) -> ApiError` — [`AbacError`] never carries a client mistake on the check path (grants
- pub `map_item_error` function L291-333 — `(e: ItemError) -> ApiError` — [`ItemError`] → HTTP.
- pub `map_board_error` function L339-414 — `(e: BoardError) -> ApiError` — [`BoardError`] → HTTP, for the transition endpoints: invalid moves are
- pub `map_graph_error` function L419-431 — `(e: GraphError) -> ApiError` — [`GraphError`] → HTTP, for the document-create `supports` edge: rule
-  `DEFAULT_LIMIT` variable L78 — `: i64` — Default page size when `?limit=` is omitted.
-  `MAX_LIMIT` variable L80 — `: i64` — Hard cap on `?limit=`.
-  `DirectoryRow` struct L175-180 — `{ id: Uuid, entity_type: String }` — aggregation endpoint is KAIROS-T-0023.

#### crates/kairos-server/src/api/openapi.rs

- pub `spec` function L214-216 — `() -> utoipa::openapi::OpenApi` — The aggregated OpenAPI document (also consumed by `tests/openapi.rs`,
- pub `router` function L226-233 — `(dev_ui: bool) -> Router<AppState>` — Build the module's routes.
-  `ApiDoc` struct L210 — `-` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `SPEC_JSON` variable L219-221 — `: LazyLock<String>` — The serialized spec, built once per process.
-  `openapi_json` function L246-251 — `() -> impl IntoResponse` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `whoami` function L279 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `spa_config` function L304 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `token_relay` function L328 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `swagger_ui` function L336-338 — `() -> Html<&'static str>` — The dev-only Swagger UI page (`KAIROS_DEV_UI=true`).
-  `SWAGGER_UI_HTML` variable L344-365 — `: &str` — Kept minimal on purpose: the page is behind the same auth → tenant

#### crates/kairos-server/src/api/search.rs

- pub `router` function L52-54 — `() -> Router<AppState>` — DTO carries `archived_at`, and it is non-null exactly for those rows.
-  `search` function L71-117 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Js...` — DTO carries `archived_at`, and it is non-null exactly for those rows.
-  `field_invalid` function L124-127 — `(field: &str, message: impl Into<String>) -> ApiError` — 400 `VALIDATION` naming the offending field in `details.field`.
-  `uuid_field` function L130-133 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — Parse a UUID-carrying field.
-  `timestamp_field` function L136-145 — `(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError>` — Parse an RFC 3339 timestamp field.
-  `enum_field` function L149-160 — `( value: &str, field: &str, allowed: &str, ) -> Result<T, ApiError>` — Parse a closed-vocabulary field through the core model's serde
-  `to_core` function L165-178 — `(request: &dto_search::SearchRequest) -> Result<core_search::SearchRequest, ApiE...` — Convert the wire request into the typed `kairos_core::search` request.
-  `filter_to_core` function L180-255 — `( filter: &dto_search::SearchFilter, ) -> Result<core_search::SearchFilter, ApiE...` — DTO carries `archived_at`, and it is non-null exactly for those rows.
-  `traverse_to_core` function L257-288 — `( traverse: &dto_search::SearchTraverse, ) -> Result<core_search::Traverse, ApiE...` — DTO carries `archived_at`, and it is non-null exactly for those rows.
-  `sort_to_core` function L290-295 — `(sort: &dto_search::SearchSort) -> Result<core_search::Sort, ApiError>` — DTO carries `archived_at`, and it is non-null exactly for those rows.
-  `map_validation_error` function L305-333 — `(e: SearchValidationError) -> ApiError` — [`SearchValidationError`] → 400 `VALIDATION`.
-  `map_search_error` function L336-344 — `(e: SearchError) -> ApiError` — [`SearchError`] → HTTP (module docs).
-  `into_response` function L352-377 — `(results: SearchResults) -> dto_search::SearchResponse` — Convert the pipeline's typed results into the S-0005 response shape

#### crates/kairos-server/src/api/strategies.rs

- pub `router` function L29-45 — `() -> Router<AppState>` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `MANAGE` variable L27 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L48-66 — `( conn: &mut PgConnection, short_code: &str, liveness: Liveness, ) -> Result<Str...` — Load the live strategy with this short code, or 404.
-  `list_strategies` function L83-119 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `get_strategy` function L132-144 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `create_strategy` function L158-188 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `update_strategy` function L205-242 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `delete_strategy` function L257-280 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `transition_strategy` function L297-324 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.

#### crates/kairos-server/src/api/tasks.rs

- pub `router` function L31-45 — `() -> Router<AppState>` — handler pattern.
-  `MANAGE` variable L29 — `: &str` — The A-0006 manage capability for this family.
-  `map_repository_error` function L51-57 — `(e: repositories::RepositoryError) -> ApiError` — [`RepositoryError`] → HTTP for the routing paths (422 for every
-  `resolve_routing` function L62-69 — `( conn: &mut PgConnection, board_id: Option<Uuid>, team_id: Option<Uuid>, reposi...` — The routing decision lives in `kairos_db::repositories::route_task`
-  `require_task_create_capability` function L79-111 — `( conn: &mut PgConnection, slug: &str, user: Uuid, route: &TaskRoute, column_id:...` — Authorize a task CREATE (KAIROS-T-0105, A-0019 §4).
-  `load` function L117-131 — `(conn: &mut PgConnection, short_code: &str, liveness: Liveness) -> Result<Task, ...` — Load the task with this short code, or 404.
-  `list_tasks` function L148-187 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — handler pattern.
-  `get_task` function L200-213 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — handler pattern.
-  `create_task` function L228-282 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `update_task` function L299-339 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `delete_task` function L354-377 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `set_work_class` function L395-416 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `set_repository` function L435-463 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `move_task` function L483-507 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `board_id_by_ref` function L510-525 — `(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError>` — A live board by slug or UUID; 404 otherwise.
-  `transition_task` function L542-562 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.

### crates/kairos-server/src/api/meta

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/api/meta/activity.rs

- pub `router` function L26-28 — `() -> Router<AppState>` — `VALIDATION`.
-  `get_activity` function L41-129 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `VALIDATION`.
-  `filtered` macro L87-104 — `-` — Apply the combinable S-0005 filters to any boxed

#### crates/kairos-server/src/api/meta/definitions.rs

- pub `router` function L42-54 — `() -> Router<AppState>` — which is what made KAIROS-T-0152 a trap.
-  `load` function L57-66 — `(conn: &mut PgConnection, id: Uuid) -> Result<MetadataDefinition, ApiError>` — Load a definition by id, or 404.
-  `ENTITY_TYPES` variable L70 — `: &[&str]` — The entity-type vocabulary for scope rows (KAIROS-T-0078) — matches
-  `scopes_of` function L74-83 — `(conn: &mut PgConnection, definition_id: Uuid) -> Result<Vec<String>, ApiError>` — Load a definition's scope rows in vocabulary order (empty = applies
-  `check_entity_types` function L86-102 — `(entity_types: &[String]) -> Result<(), ApiError>` — Validate an entity_types list: known values, no duplicates.
-  `replace_scopes` function L106-127 — `( conn: &mut PgConnection, definition_id: Uuid, entity_types: &[String], ) -> Re...` — Replace a definition's scope rows (delete + insert, caller's
-  `hydrate` function L130-137 — `( conn: &mut PgConnection, definition: MetadataDefinition, ) -> Result<dto::Meta...` — Hydrate a definition row with its option values and scopes.
-  `check_option_rules` function L141-151 — `(field_type: FieldType, options: &[String]) -> Result<(), ApiError>` — The option-list rules shared by create and update: enum definitions
-  `replace_options` function L155-180 — `( conn: &mut PgConnection, definition_id: Uuid, options: &[String], ) -> Result<...` — Replace a definition's option list (delete + insert, caller's
-  `map_write_error` function L184-194 — `(e: DieselError) -> ApiError` — Map the unique violations a definition write can hit (`slug` UNIQUE,
-  `NAMED_BLOCKER_LIMIT` variable L200 — `: i64` — How many blockers a `DEFINITION_IN_USE` refusal names before it falls
-  `Carrier` struct L204-207 — `{ short_code: String, archived: bool }` — One work item carrying a definition's value: its short code, and
-  `Carrier` type L209-221 — `= Carrier` — which is what made KAIROS-T-0152 a trap.
-  `label` function L214-220 — `(&self) -> String` — `ACME-T-0007`, or `ACME-T-0007 (archived)` — the marking is the
-  `carrying_items` function L232-271 — `( conn: &mut PgConnection, definition_id: Uuid, limit: i64, ) -> Result<Vec<Carr...` — The work items carrying `definition_id`'s values (at most `limit`),
-  `carriers_in` macro L241-262 — `-` — The carriers in one entity table, cheapest form: the id subquery
-  `carrying_templates` function L275-294 — `( conn: &mut PgConnection, definition_id: Uuid, limit: i64, ) -> Result<Vec<Stri...` — The slugs of the templates that collect `definition_id` (at most
-  `blocker_list` function L298-308 — `(labels: &[String], total: i64) -> String` — `[a, b, and 3 more]` — the named blockers plus however many were left
-  `DefinitionListQuery` struct L315-327 — `{ limit: Option<i64>, offset: Option<i64>, entity_type: Option<String> }` — Query of [`list_definitions`]: pagination plus the KAIROS-T-0078
-  `list_definitions` function L341-412 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — which is what made KAIROS-T-0152 a trap.
-  `create_definition` function L426-459 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Js...` — which is what made KAIROS-T-0152 a trap.
-  `get_definition` function L472-486 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — which is what made KAIROS-T-0152 a trap.
-  `update_definition` function L504-556 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — which is what made KAIROS-T-0152 a trap.
-  `delete_definition` function L574-650 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — which is what made KAIROS-T-0152 a trap.
-  `tests` module L653-672 — `-` — which is what made KAIROS-T-0152 a trap.
-  `blocker_list_names_what_it_can_and_counts_the_rest` function L662-671 — `()` — The cap is what makes naming blockers affordable, so the remainder

#### crates/kairos-server/src/api/meta/history.rs

- pub `router` function L27-29 — `() -> Router<AppState>` — birth.
-  `get_history` function L47-104 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — birth.

#### crates/kairos-server/src/api/meta/metadata.rs

- pub `router` function L33-38 — `() -> Router<AppState>` — history/activity rows.
-  `get_metadata` function L54-68 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — history/activity rows.
-  `update_metadata` function L90-198 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — history/activity rows.

#### crates/kairos-server/src/api/meta/mod.rs

- pub `activity` module L40 — `-` — item metadata, metadata definitions, templates, content history, and
- pub `definitions` module L41 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `history` module L42 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `metadata` module L43 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `relationships` module L44 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `restore` module L45 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `templates` module L46 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `router` function L64-73 — `() -> Router<AppState>` — All six T-0020 family routers, merged.
- pub `require_org_admin` function L78-89 — `(tenant: &TenantContext) -> Result<(), ApiError>` — The A-0006 gate for tenant-wide configuration (relationships, metadata
- pub `require_edge_capability` function L101-130 — `( conn: &mut PgConnection, tenant: &TenantContext, user: Uuid, relationship: &st...` — Authorize writing (or removing) one relationship edge (KAIROS-T-0111,
- pub `require_edge_capability_on` function L137-177 — `( conn: &mut PgConnection, tenant: &TenantContext, user: Uuid, relationship: &st...` — [`require_edge_capability`] for a target that may not exist yet (MCP
- pub `item_type_of_family` function L181-190 — `(family: &str) -> Option<ItemType>` — Map a plural `{entity_type}` path segment (the S-0005 family names, as
- pub `manage_capability` function L193-201 — `(item_type: ItemType) -> &'static str` — The A-0006 manage capability for an entity type.
- pub `resolve_family_item` function L212-227 — `( conn: &mut PgConnection, family: &str, short_code: &str, liveness: Liveness, )...` — Resolve an `{entity_type}/{short_code}` path pair to an item: the family
- pub `enum_option_values` function L231-242 — `( conn: &mut PgConnection, definition_id: Uuid, ) -> Result<Vec<String>, ApiErro...` — A metadata definition's option values, in display order (empty for
- pub `validate_metadata_value` function L248-277 — `( conn: &mut PgConnection, definition: &MetadataDefinition, value: &str, ) -> Re...` — The KAIROS-A-0003 value check: `string` passes through, `date` must
- pub `item_metadata_response` function L281-307 — `( conn: &mut PgConnection, item_id: Uuid, short_code: &str, ) -> Result<dto::Ite...` — An item's metadata values hydrated with their definitions, ordered by

#### crates/kairos-server/src/api/meta/relationships.rs

- pub `router` function L35-49 — `() -> Router<AppState>` — `VALIDATION`.
-  `get_children_progress` function L76-113 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `VALIDATION`.
-  `get_item_links` function L132-163 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `VALIDATION`.
-  `GraphQuery` struct L168-171 — `{ depth: Option<u32> }` — Query of [`get_item_graph`] (explicit struct — serde_urlencoded
-  `get_item_graph` function L200-247 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `VALIDATION`.
-  `map_link_error` function L251-266 — `(e: GraphError) -> ApiError` — [`GraphError`] → HTTP for the relationship write endpoints: the typed
-  `group_neighbors` function L275-303 — `( neighbors: Vec<Neighbor>, edge_ids: &HashMap<(RelationshipType, Uuid, bool), U...` — Fold one direction's neighbors (already ordered by relationship, then
-  `get_relationships` function L330-374 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `VALIDATION`.
-  `create_relationship` function L391-436 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `VALIDATION`.
-  `delete_relationship` function L452-497 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `VALIDATION`.

#### crates/kairos-server/src/api/meta/restore.rs

- pub `router` function L38-43 — `() -> Router<AppState>` — `live_board_item_codes` refusing a team delete.
-  `restore_item` function L61-98 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `live_board_item_codes` refusing a team delete.

#### crates/kairos-server/src/api/meta/templates.rs

- pub `router` function L33-42 — `() -> Router<AppState>` — DELETE SET NULL`, associations cascade).
-  `load` function L45-54 — `(conn: &mut PgConnection, id: Uuid) -> Result<Template, ApiError>` — Load a template by id, or 404.
-  `metadata_fields` function L58-87 — `( conn: &mut PgConnection, template_id: Uuid, ) -> Result<Vec<dto::TemplateMetad...` — The template's associated metadata fields, hydrated with their
-  `detail` function L90-93 — `(conn: &mut PgConnection, template: Template) -> Result<dto::TemplateDetail, Api...` — The detail payload (template + hydrated metadata fields).
-  `resolve_entries` function L99-132 — `( conn: &mut PgConnection, entries: &[dto::TemplateMetadataEntry], ) -> Result<V...` — Resolve + validate a template write's metadata entries (KAIROS-A-0003:
-  `replace_associations` function L135-160 — `( conn: &mut PgConnection, template_id: Uuid, entries: &[(Uuid, Option<String>, ...` — Replace a template's metadata associations (caller's transaction).
-  `map_write_error` function L164-174 — `(e: DieselError) -> ApiError` — Map the unique violation a template write can hit (`slug` UNIQUE) to
-  `list_templates` function L186-216 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — DELETE SET NULL`, associations cascade).
-  `get_template` function L230-244 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — DELETE SET NULL`, associations cascade).
-  `create_template` function L260-290 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Js...` — DELETE SET NULL`, associations cascade).
-  `update_template` function L308-349 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — DELETE SET NULL`, associations cascade).
-  `delete_template` function L365-384 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — DELETE SET NULL`, associations cascade).

### crates/kairos-server/src/api/org

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/api/org/admin.rs

- pub `router` function L47-58 — `(state: AppState) -> Router<AppState>` — The admin router, wrapped in its own auth layer (no tenant layer — see
-  `require_deployment_admin` function L62-77 — `(state: &AppState, auth: &AuthContext) -> Result<(), ApiError>` — The KAIROS-T-0019 deployment-admin gate: the caller's OIDC `sub` must be
-  `run_admin` function L81-93 — `(state: &AppState, f: F) -> Result<T, ApiError>` — Run `f` on a fresh sync connection off the async runtime (cross-tenant:
-  `map_tenant_error` function L98-120 — `(e: TenantError) -> ApiError` — [`TenantError`] → HTTP: bad slug → 422 `VALIDATION`; existing tenant →
-  `create_tenant` function L138-201 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Json(b...` — rare and cross-tenant, so the tenant-pinned blocking pool does not fit).
-  `list_tenants` function L215-244 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Query(...` — rare and cross-tenant, so the tenant-pinned blocking pool does not fit).
-  `ConfirmParams` struct L249-253 — `{ confirm: Option<bool> }` — `?confirm=true` — required by the T-0008 destructive-operation guard.
-  `delete_tenant` function L275-309 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Path(s...` — rare and cross-tenant, so the tenant-pinned blocking pool does not fit).

#### crates/kairos-server/src/api/org/boards.rs

- pub `router` function L45-77 — `() -> Router<AppState>` — capability writes require `manage_members`.
-  `CONFIGURE` variable L41 — `: &str` — The A-0006 capability for board configuration writes.
-  `MANAGE_MEMBERS` variable L43 — `: &str` — The A-0006 capability for board membership/capability administration.
-  `load_columns` function L87-89 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError>` — LIVE columns of a board in position order.
-  `load_columns_including_removed` function L98-115 — `( conn: &mut PgConnection, board_id: Uuid, include_removed: bool, ) -> Result<Ve...` — Columns of a board in position order, optionally including the removed
-  `load_transitions` function L124-144 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<Vec<BoardTransition>, Api...` — Transition edges of a board, between LIVE columns.
-  `board_detail` function L152-164 — `( conn: &mut PgConnection, board: Board, include_removed_columns: bool, ) -> Res...` — The board + full configuration as the `BoardDetail` DTO.
-  `load_column_of_board` function L168-183 — `( conn: &mut PgConnection, board_id: Uuid, column_id: Uuid, ) -> Result<BoardCol...` — A LIVE column of `board_id` by id, or 404 (also 404 when the column
-  `log_activity` function L186-205 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Uu...` — Insert one `activity_log` row (same shape as the kairos-db services).
-  `list_boards` function L222-254 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — capability writes require `manage_members`.
-  `BoardDetailQuery` struct L258-268 — `{ include_removed_columns: bool }` — Query of `GET /api/boards/{id}`.
-  `get_board` function L282-297 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `create_board` function L314-362 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `update_board` function L379-430 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `delete_board` function L447-498 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `BoardItemsQuery` struct L506-515 — `{ repository: Option<String>, include_deleted: bool }` — Query of `GET /api/boards/{id}/items` (KAIROS-T-0104).
-  `board_items` function L538-730 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `list_columns` function L747-764 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_column` function L781-802 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `update_column` function L822-871 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_column` function L892-916 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `list_transitions` function L933-950 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_transition` function L967-997 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_transition` function L1014-1049 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `capabilities_of` function L1056-1069 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, ) -> Result<Vec<String...` — The `capabilities` a user holds on a board, sorted.
-  `board_member_view` function L1072-1084 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, ) -> Result<dto::Board...` — One user's `BoardMember` view (joins `public.users` for identity).
-  `list_board_members` function L1098-1153 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_board_member` function L1171-1197 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `replace_capabilities` function L1218-1255 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_board_member` function L1274-1306 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.

#### crates/kairos-server/src/api/org/forge.rs

- pub `router` function L40-54 — `() -> Router<AppState>` — so the operator must update the forge either way.
-  `MANAGE` variable L38 — `: &str` — Tenant-wide configuration → org-admin-only writes (A-0006 fallback).
-  `map_error` function L57-74 — `(e: ForgeError) -> ApiError` — [`ForgeError`] → HTTP.
-  `connection_dto` function L76-83 — `(row: ConnectionWithRepo) -> dto::ForgeConnection` — so the operator must update the forge either way.
-  `signing_key` function L88-98 — `(state: &AppState) -> Result<String, ApiError>` — The deployment's webhook signing key, or a 501 explaining that the
-  `public_url` function L102-111 — `(state: &AppState) -> Result<String, ApiError>` — The deployment's externally reachable base URL, or the 501 that says it
-  `webhook_url` function L114-122 — `( state: &AppState, tenant: &str, forge: Forge, connection_id: &str, ) -> Result...` — The delivery URL an operator pastes into the forge.
-  `list_connections` function L133-145 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, ) ...` — so the operator must update the forge either way.
-  `get_connection` function L158-172 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — so the operator must update the forge either way.
-  `create_connection` function L192-243 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — so the operator must update the forge either way.
-  `delete_connection` function L257-277 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — so the operator must update the forge either way.
-  `rotate_connection` function L295-343 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — so the operator must update the forge either way.

#### crates/kairos-server/src/api/org/members.rs

- pub `router` function L41-48 — `() -> Router<AppState>` — update action).
-  `MANAGE` variable L39 — `: &str` — The pseudo-capability named in 403s for these org-admin-only writes.
-  `member_view` function L51-62 — `(member: &OrganizationMember, user: &User) -> dto::OrgMember` — One member's DTO view.
-  `load_membership` function L65-83 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> Result<Organization...` — The org's membership row for `user_id`, or 404.
-  `admin_count` function L86-94 — `(conn: &mut PgConnection, org_id: Uuid) -> Result<i64, ApiError>` — How many admins the org currently has.
-  `last_admin_error` function L97-103 — `() -> ApiError` — The 422 guard: an org must always retain at least one admin.
-  `log_membership_activity` function L107-125 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, user_id: Uuid...` — Insert one `activity_log` row for a membership mutation (tenant-schema
-  `list_members` function L139-176 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — update action).
-  `add_member` function L194-256 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — update action).
-  `update_member` function L273-328 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — update action).
-  `remove_member` function L344-384 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — update action).

#### crates/kairos-server/src/api/org/mod.rs

- pub `admin` module L21 — `-` — (KAIROS-S-0005): boards (+columns/transitions/items/members), teams
- pub `boards` module L22 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `forge` module L23 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `members` module L24 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `repositories` module L25 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `streams` module L26 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `team_pages` module L27 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `teams` module L28 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `router` function L44-53 — `() -> Router<AppState>` — The tenant-scoped T-0019 families, merged (mounted behind the full
- pub `validate_capabilities` function L73-88 — `(capabilities: &[String]) -> Result<(), ApiError>` — 422 `VALIDATION` unless every entry is in the A-0006 vocabulary (list
- pub `load_board` function L95-105 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError>` — Load a live board by id, or 404.
- pub `count_live_board_items` function L126-155 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ApiError>` — How many LIVE workflow items (strategies/initiatives/tasks/ADRs) sit on
- pub `live_board_item_codes` function L159-201 — `( conn: &mut PgConnection, board_id: Uuid, limit: i64, ) -> Result<Vec<String>, ...` — The short codes of the live cards on `board_id` (at most `limit`), for
- pub `run_in_transaction` function L207-225 — `(conn: &mut PgConnection, f: F) -> Result<T, ApiError>` — Run `f` inside ONE database transaction, keeping `ApiError` as the
- pub `is_unique_violation` function L230-235 — `(e: &diesel::result::Error) -> bool` — Whether a diesel error is a unique-constraint violation (mapped to 409
- pub `map_config_error` function L243-266 — `(e: BoardError) -> ApiError` — [`BoardError`] → HTTP for the BOARD CONFIGURATION endpoints (columns /
- pub `map_grant_error` function L305-324 — `(e: AbacError) -> ApiError` — [`AbacError`] → HTTP for the board-member GRANT/REVOKE endpoints, where
- pub `require_user_exists` function L328-340 — `( conn: &mut PgConnection, user_id: Uuid, ) -> Result<kairos_db::models::User, A...` — Load a `public.users` row by id; 422 `VALIDATION` when it does not exist
-  `capability_vocabulary` function L64-69 — `() -> impl Iterator<Item = &'static str>` — The fixed A-0006 capability vocabulary plus its glob forms — the values
-  `TxError` enum L211-214 — `Api | Db` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `TxError` type L215-219 — `= TxError` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `from` function L216-218 — `(e: diesel::result::Error) -> Self` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `map_column_rule_error` function L269-300 — `(e: ColumnRuleError) -> ApiError` — The T-0010 column/transition rule violations as typed 422s.

#### crates/kairos-server/src/api/org/repositories.rs

- pub `router` function L44-56 — `() -> Router<AppState>` — [`super::forge`]) that hangs off a repository.
-  `MANAGE` variable L42 — `: &str` — The A-0006 capability that lets a team manage its own repositories
-  `map_error` function L59-95 — `(e: RepositoryError) -> ApiError` — [`RepositoryError`] → HTTP.
-  `resolve_team` function L98-111 — `(conn: &mut PgConnection, reference: &str) -> Result<Team, ApiError>` — Resolve a team reference (UUID or slug) to its live row, or 422.
-  `delivery_board_of` function L117-123 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError>` — The team's ONE live delivery board, or `None` when it has none or
-  `require_manage_for_team` function L127-137 — `( conn: &mut PgConnection, slug: &str, user: Uuid, team_id: Uuid, ) -> Result<()...` — Org admin, or `manage_tasks` on the team's delivery board (team
-  `render` function L141-213 — `( conn: &mut PgConnection, rows: Vec<Repository>, ) -> Result<Vec<dto::Repositor...` — Render repositories with their team, delivery board and counts — two
-  `render_one` function L215-218 — `(conn: &mut PgConnection, repo: Repository) -> Result<dto::Repository, ApiError>` — [`super::forge`]) that hangs off a repository.
-  `ListRepositoriesQuery` struct L222-231 — `{ team: Option<String>, forge: Option<String>, name: Option<String> }` — Query of `GET /api/repositories`.
-  `list_repositories` function L245-274 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — [`super::forge`]) that hangs off a repository.
-  `get_repository` function L289-330 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — [`super::forge`]) that hangs off a repository.
-  `create_repository` function L347-385 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — [`super::forge`]) that hangs off a repository.
-  `update_repository` function L403-447 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — [`super::forge`]) that hangs off a repository.
-  `delete_repository` function L463-484 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — [`super::forge`]) that hangs off a repository.

#### crates/kairos-server/src/api/org/streams.rs

- pub `router` function L33-51 — `() -> Router<AppState>` — fallback); reads are open tenant-wide.
-  `MANAGE` variable L31 — `: &str` — The pseudo-capability named in 403s for these org-admin-only writes.
-  `load_stream` function L54-64 — `(conn: &mut PgConnection, stream_id: Uuid) -> Result<DeliveryStream, ApiError>` — Load the live stream with this id, or 404.
-  `log_stream_activity` function L67-85 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, stream_id: Uu...` — Insert one `activity_log` row for a stream mutation.
-  `list_streams` function L98-130 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — fallback); reads are open tenant-wide.
-  `get_stream` function L143-156 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — fallback); reads are open tenant-wide.
-  `create_stream` function L170-212 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — fallback); reads are open tenant-wide.
-  `update_stream` function L229-281 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — fallback); reads are open tenant-wide.
-  `delete_stream` function L296-332 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — fallback); reads are open tenant-wide.
-  `list_stream_teams` function L349-388 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — fallback); reads are open tenant-wide.
-  `add_stream_team` function L405-464 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — fallback); reads are open tenant-wide.
-  `remove_stream_team` function L481-523 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — fallback); reads are open tenant-wide.

#### crates/kairos-server/src/api/org/team_pages.rs

- pub `router` function L30-45 — `() -> Router<AppState>` — way" decision).
-  `require_team` function L48-60 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<(), ApiError>` — 404 unless a live team with this id exists.
-  `require_team_member_or_admin` function L64-88 — `( conn: &mut PgConnection, tenant: &TenantContext, team_id: Uuid, user: Uuid, ) ...` — Team-page writes require team membership or org admin (the
-  `log_page_activity` function L93-112 — `( conn: &mut PgConnection, actor_id: Uuid, action: kairos_db::models::enums::Act...` — Activity row for a page/announcement lifecycle event.
-  `map_page_error` function L117-166 — `(e: TeamPageError) -> ApiError` — [`TeamPageError`] → HTTP: version conflicts carry the current page in
-  `page_dto` function L168-183 — `(page: TeamPage) -> dto::TeamPage` — way" decision).
-  `announcement_dto` function L185-194 — `(row: TeamAnnouncement) -> dto::TeamAnnouncement` — way" decision).
-  `list_pages` function L207-222 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — way" decision).
-  `get_page` function L238-254 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — way" decision).
-  `create_page` function L270-313 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — way" decision).
-  `update_page` function L334-403 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — way" decision).
-  `delete_page` function L421-452 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — way" decision).
-  `list_announcements` function L465-480 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — way" decision).
-  `create_announcement` function L497-542 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — way" decision).
-  `delete_announcement` function L559-604 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — way" decision).

#### crates/kairos-server/src/api/org/teams.rs

- pub `router` function L41-59 — `() -> Router<AppState>` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `MANAGE` variable L39 — `: &str` — The pseudo-capability named in 403s for these org-admin-only writes
-  `load_team` function L62-72 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Team, ApiError>` — Load the live team with this id, or 404.
-  `delivery_board_of` function L76-83 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError>` — The team's ONE live delivery board, if any (exactly-one semantics,
-  `log_team_activity` function L86-104 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, team_id: Uuid...` — Insert one `activity_log` row for a team mutation.
-  `list_teams` function L117-154 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `get_team` function L167-182 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `get_team_by_slug` function L196-219 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `TeamLinksQuery` struct L224-230 — `{ state: Option<String>, limit: Option<i64> }` — Query of [`list_team_links`] (explicit struct — serde_urlencoded
-  `list_team_links` function L248-291 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `list_work_documents` function L307-334 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `create_team` function L350-424 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `update_team` function L442-499 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `delete_team` function L515-600 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `list_members` function L620-661 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `add_member` function L680-735 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `remove_member` function L754-796 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.

### crates/kairos-server/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/app.rs

- pub `AppState` struct L27-41 — `{ config: Arc<AppConfig>, pool: TenantPool, blocking: BlockingTenantPool, auth: ...` — Shared state behind every request: config, the tenant-pinning pool, and
- pub `BuildError` enum L45-52 — `Pool | Oidc` — Why [`build_state`] failed (startup-time, fail-fast).
- pub `build_state` function L60-71 — `(config: AppConfig) -> Result<AppState, BuildError>` — Build the shared state: connect the pool and resolve the OIDC issuer
- pub `state_with` function L77-86 — `(config: AppConfig, pool: TenantPool, auth: Arc<Authenticator>) -> AppState` — Build a state from an existing pool and a pre-built [`Authenticator`]
- pub `router` function L90-209 — `(state: AppState) -> Router` — The production router: `/healthz` open; everything under `/api` behind
- pub `WhoamiTeam` struct L213-220 — `{ id: Uuid, slug: String, name: String }` — A team the caller belongs to (from the tenant schema's `team_members`).
- pub `WhoamiBoardCapabilities` struct L230-238 — `{ board_id: Uuid, board_slug: String, grants: Vec<String> }` — One board on which the caller holds explicit capability grants
- pub `WhoamiResponse` struct L243-261 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — `GET /api/whoami` — the S-0006 whoami precursor: proves the full
- pub `WhoamiRepository` struct L265-272 — `{ id: Uuid, slug: String, forge: String, repo_full_name: String, team_slug: Stri...` — One repository of [`WhoamiResponse::repositories`].
- pub `WhoamiUser` struct L276-285 — `{ id: Uuid, external_id: String, email: String, display_name: String }` — The `user` object of [`WhoamiResponse`].
- pub `WhoamiOrganization` struct L289-296 — `{ id: Uuid, slug: String, role: &'static str }` — The `organization` object of [`WhoamiResponse`].
- pub `serve` function L404-418 — `(config: AppConfig) -> Result<(), String>` — Run the server: build state, bind `KAIROS_BIND_ADDR`, serve with
-  `POOL_SIZE` variable L56 — `: u32` — Pool size for the server.
-  `whoami` function L299-399 — `( Extension(auth): Extension<AuthContext>, Extension(tenant): Extension<TenantCo...` — The probe endpoint behind the full middleware stack (KAIROS-T-0017).
-  `shutdown_signal` function L421-427 — `()` — Resolves when ctrl-c (SIGINT) arrives.

#### crates/kairos-server/src/blocking.rs

- pub `BlockingTenantPool` struct L23-25 — `{ pool: Pool<ConnectionManager<PgConnection>> }` — The sync pool.
- pub `new` function L40-47 — `(database_url: &str, max_size: u32) -> Self` — Build a lazy pool of at most `max_size` connections against
- pub `run` function L54-79 — `(&self, slug: &str, f: F) -> Result<T, ApiError>` — Run `f` on a sync connection pinned to `slug`'s tenant schema
- pub `run_public` function L87-101 — `(&self, f: F) -> Result<T, ApiError>` — Run `f` on a sync connection pinned to `search_path = public` (no
- pub `pool_state` function L106-109 — `(&self) -> (u32, u32)` — Point-in-time `(total_connections, idle_connections)` for the r2d2
-  `BlockingTenantPool` type L27-33 — `= BlockingTenantPool` — reset after the closure is defense-in-depth).
-  `fmt` function L28-32 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — reset after the closure is defense-in-depth).
-  `BlockingTenantPool` type L35-110 — `= BlockingTenantPool` — reset after the closure is defense-in-depth).

#### crates/kairos-server/src/config.rs

- pub `DEFAULT_BIND_ADDR` variable L11 — `: &str` — Default bind address when `KAIROS_BIND_ADDR` is unset.
- pub `LogFormat` enum L15-21 — `Json | Pretty` — `KAIROS_LOG_FORMAT` — structured JSON by default (KAIROS-A-0013).
- pub `ApiBearer` enum L33-41 — `AccessToken | IdToken` — `KAIROS_API_BEARER` — which OIDC token the browser GUI (and CLI) present
- pub `as_str` function L46-51 — `(self) -> &'static str` — The wire name, as sent to the SPA in `/api/config` and used as the
- pub `ConfigError` enum L57-74 — `Missing | Invalid` — A configuration error worth failing startup over.
- pub `AppConfig` struct L78-143 — `{ database_url: String, bind_addr: SocketAddr, oidc_issuer_url: String, oidc_aud...` — Everything the server needs to run, resolved once at startup.
- pub `from_env` function L148-150 — `() -> Result<Self, ConfigError>` — Read configuration from the process environment, failing fast on
- pub `from_lookup` function L154-244 — `(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError>` — Testable core of [`Self::from_env`]: resolve from any lookup
-  `ApiBearer` type L43-52 — `= ApiBearer` — variants directly instead of mutating process environment.
-  `AppConfig` type L145-245 — `= AppConfig` — variants directly instead of mutating process environment.
-  `tests` module L248-361 — `-` — variants directly instead of mutating process environment.
-  `lookup` function L252-258 — `(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String>` — variants directly instead of mutating process environment.
-  `MINIMAL` variable L260-264 — `: &[(&str, &str)]` — variants directly instead of mutating process environment.
-  `minimal_config_applies_defaults` function L267-285 — `()` — variants directly instead of mutating process environment.
-  `missing_required_var_names_it` function L288-291 — `()` — variants directly instead of mutating process environment.
-  `empty_value_is_treated_as_unset` function L294-299 — `()` — variants directly instead of mutating process environment.
-  `invalid_bind_addr_and_log_format_are_rejected` function L302-322 — `()` — variants directly instead of mutating process environment.
-  `api_bearer_id_token_is_parsed` function L325-331 — `()` — variants directly instead of mutating process environment.
-  `optional_vars_are_carried_through` function L334-360 — `()` — variants directly instead of mutating process environment.

#### crates/kairos-server/src/error.rs

- pub `ApiError` struct L16-25 — `{ status: StatusCode, code: &'static str, message: String, details: Value }` — An API-surface error: an HTTP status plus the S-0005 envelope fields.
- pub `new` function L29-36 — `(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self` — Build an error with empty `details`.
- pub `with_details` function L40-43 — `(mut self, details: Value) -> Self` — Attach structured details.
- pub `unauthorized` function L46-48 — `(message: impl Into<String>) -> Self` — 401 `UNAUTHORIZED` — missing/invalid bearer token (KAIROS-A-0010).
- pub `membership_required` function L53-63 — `(slug: &str) -> Self` — 403 `MEMBERSHIP_REQUIRED` — authenticated but not a member of the
- pub `forbidden` function L66-68 — `(message: impl Into<String>) -> Self` — 403 `FORBIDDEN` — authenticated and a member, but not allowed.
- pub `tenant_not_found` function L72-74 — `(message: impl Into<String>) -> Self` — 404 `TENANT_NOT_FOUND` — no organization for the resolved slug (or
- pub `not_found` function L77-79 — `(message: impl Into<String>) -> Self` — 404 `NOT_FOUND` — generic missing resource.
- pub `conflict` function L84-86 — `(message: impl Into<String>) -> Self` — 409 `CONFLICT` — KAIROS-A-0004 optimistic-concurrency rejection.
- pub `unprocessable` function L90-92 — `(code: &'static str, message: impl Into<String>) -> Self` — 422 with a caller-chosen code — the semantic-rejection family
- pub `validation` function L97-99 — `(message: impl Into<String>) -> Self` — 422 `VALIDATION` — a body/reference the request names is malformed
- pub `capability_required` function L104-113 — `(capability: &str, board_id: Option<uuid::Uuid>) -> Self` — 403 `FORBIDDEN` naming the missing KAIROS-A-0006 capability in
- pub `internal` function L117-124 — `(context: impl std::fmt::Display) -> Self` — 500 `INTERNAL` — the message is logged; the envelope carries a
-  `ApiError` type L27-125 — `= ApiError` — middleware layer and handler in this crate (KAIROS-T-0017).
-  `ApiError` type L127-138 — `impl IntoResponse for ApiError` — middleware layer and handler in this crate (KAIROS-T-0017).
-  `into_response` function L128-137 — `(self) -> Response` — middleware layer and handler in this crate (KAIROS-T-0017).
-  `tests` module L141-167 — `-` — middleware layer and handler in this crate (KAIROS-T-0017).
-  `envelope_matches_s0005_shape` function L145-157 — `()` — middleware layer and handler in this crate (KAIROS-T-0017).
-  `membership_required_names_the_org_and_access_path` function L160-166 — `()` — middleware layer and handler in this crate (KAIROS-T-0017).

#### crates/kairos-server/src/lib.rs

- pub `api` module L16 — `-` — does apart from CLI parsing, exposed so integration tests can build the
- pub `app` module L17 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `blocking` module L18 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `config` module L19 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `error` module L20 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `forge` module L21 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `metrics` module L22 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `middleware` module L23 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `ws` module L24 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `mcp` module L26 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `scim` module L28 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `service_accounts` module L30 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `web` module L32 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `init_tracing` function L40-52 — `(config: &AppConfig)` — Initialize `tracing-subscriber` per KAIROS-A-0013: `KAIROS_LOG_LEVEL`

#### crates/kairos-server/src/main.rs

-  `connect_and_migrate_public` function L43-64 — `() -> Result<PgConnection, String>` — Establish the migration connection from `DATABASE_URL` and apply pending
-  `flag_value` function L67-75 — `(args: &[String], flag: &str) -> Result<Option<String>, String>` — Value of `--flag <value>` in `args`, if present.
-  `has_flag` function L77-79 — `(args: &[String], flag: &str) -> bool` — version and exits 0.
-  `create_tenant` function L81-100 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — version and exits 0.
-  `drop_tenant` function L102-112 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — version and exits 0.
-  `migrate_tenants` function L114-138 — `(conn: &mut PgConnection) -> Result<(), String>` — version and exits 0.
-  `list_tenants` function L140-156 — `(conn: &mut PgConnection) -> Result<(), String>` — version and exits 0.
-  `seed_demo` function L163-202 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — The `seed-demo` subcommand (KAIROS-T-0035, KAIROS-A-0012 fixtures):
-  `serve` function L207-216 — `() -> Result<(), String>` — The `serve` subcommand (KAIROS-T-0017): fail-fast config, tracing init
-  `run` function L218-243 — `() -> Result<bool, String>` — version and exits 0.
-  `main` function L245-257 — `() -> ExitCode` — version and exits 0.
-  `tests` module L260-295 — `-` — version and exits 0.
-  `smoke` function L264-266 — `()` — version and exits 0.
-  `args` function L268-270 — `(list: &[&str]) -> Vec<String>` — version and exits 0.
-  `flag_value_parses_pairs` function L273-281 — `()` — version and exits 0.
-  `flag_value_rejects_missing_value` function L284-287 — `()` — version and exits 0.
-  `has_flag_detects_presence` function L290-294 — `()` — version and exits 0.

#### crates/kairos-server/src/metrics.rs

- pub `Metrics` struct L86-89 — `{ http: Mutex<BTreeMap<HttpKey, Histogram>>, tenants: Mutex<BTreeMap<String, u64...` — A per-router metrics registry (held as `Arc<Metrics>` in
- pub `new` function L93-95 — `() -> Self` — Fresh, empty registry.
- pub `track_metrics` function L179-201 — `(State(state): State<AppState>, req: Request, next: Next) -> Response` — The outer HTTP-metrics layer (KAIROS-A-0013): times every request, then
- pub `metrics_handler` function L207-238 — `(State(state): State<AppState>) -> Response` — `GET /metrics` (KAIROS-A-0013): the Prometheus scrape endpoint.
- pub `readyz` function L244-282 — `(State(state): State<AppState>) -> Response` — `GET /readyz` (KAIROS-A-0013): readiness = database connectivity AND no
-  `BUCKETS` variable L50-52 — `: &[f64]` — Histogram bucket upper bounds (`le`), in seconds — the Prometheus client
-  `Histogram` struct L58-65 — `{ buckets: [u64; BUCKETS.len()], sum: f64, count: u64 }` — One HTTP histogram: per-bucket cumulative counts (each entry counts
-  `Histogram` type L67-77 — `= Histogram` — are non-`/api` so they are outside the S-0005 surface entirely.
-  `observe` function L68-76 — `(&mut self, value: f64)` — are non-`/api` so they are outside the S-0005 surface entirely.
-  `HttpKey` type L80 — `= (String, String, String)` — The label triple for an HTTP histogram series.
-  `Metrics` type L91-170 — `= Metrics` — are non-`/api` so they are outside the S-0005 surface entirely.
-  `record_http` function L98-106 — `(&self, method: &str, route: &str, status: u16, secs: f64)` — Record one completed HTTP request into the duration histogram.
-  `record_tenant` function L109-116 — `(&self, tenant: &str)` — Increment the per-tenant request counter.
-  `render_http_and_tenants` function L121-169 — `(&self, out: &mut String)` — Render the accumulated HTTP histograms and per-tenant counters into
-  `tests` module L285-324 — `-` — are non-`/api` so they are outside the S-0005 surface entirely.
-  `histogram_buckets_are_cumulative` function L289-303 — `()` — are non-`/api` so they are outside the S-0005 surface entirely.
-  `render_emits_expected_families` function L306-323 — `()` — are non-`/api` so they are outside the S-0005 surface entirely.

#### crates/kairos-server/src/web.rs

- pub `router` function L124-137 — `(state: &AppState) -> Router<AppState>` — The web routes: `/api/config` + `/api/auth/token`.
- pub `spa_fallback` function L289-307 — `(State(state): State<AppState>, method: Method, uri: Uri) -> Response` — The router fallback (mounted in [`crate::app::router`]): reserved
-  `RESERVED_PREFIXES` variable L59-68 — `: &[&str]` — Path prefixes that belong to API surfaces: the SPA fallback never
-  `IdpEndpoints` struct L73-76 — `{ authorization_endpoint: String, token_endpoint: String }` — Issuer endpoints the SPA flow needs, resolved lazily from OIDC
-  `WebAuth` struct L82-94 — `{ issuer: String, client_id: String, api_bearer: crate::config::ApiBearer, clien...` — Shared context for the two auth routes: issuer + client id from
-  `WebAuth` type L96-116 — `= WebAuth` — tooling).
-  `endpoints` function L100-115 — `(&self) -> Result<&IdpEndpoints, ApiError>` — Discovery document fetch (`{issuer}/.well-known/openid-configuration`),
-  `idp_unreachable` function L118-120 — `(message: String) -> ApiError` — tooling).
-  `SpaConfig` struct L141-148 — `{ issuer: String, client_id: String, authorization_endpoint: String, api_bearer:...` — `GET /api/config` response body (mirrored by `kairos-web::auth`).
-  `spa_config` function L152-162 — `( Extension(web_auth): Extension<Arc<WebAuth>>, ) -> Result<Json<SpaConfig>, Api...` — `GET /api/config` — see module docs.
-  `TokenRelayForm` struct L167-173 — `{ grant_type: String, code: Option<String>, redirect_uri: Option<String>, code_v...` — What the SPA may relay.
-  `relay_params` function L180-223 — `( client_id: &'a str, client_secret: Option<&'a str>, form: &'a TokenRelayForm, ...` — Assemble the whitelisted form parameters to relay to the issuer's token
-  `token_relay` function L230-257 — `( Extension(web_auth): Extension<Arc<WebAuth>>, form: Result<Form<TokenRelayForm...` — `POST /api/auth/token` — the same-origin token relay (module docs).
-  `missing` function L260-262 — `(field: &'static str) -> impl FnOnce() -> ApiError` — Builder for the "required form field is missing" error.
-  `is_reserved` function L269-276 — `(path: &str) -> bool` — Is this path owned by an API surface (never SPA-fallback material)?
-  `wants_index_fallback` function L281-285 — `(rel: &str) -> bool` — Should a missing file fall back to `index.html`? Yes for route-like
-  `serve_from_dir` function L310-331 — `(dist: &std::path::Path, rel: &str) -> Response` — Dev serving: files out of `KAIROS_WEB_DIST`, with the SPA fallback.
-  `serve_embedded_or_placeholder` function L335-350 — `(rel: &str) -> Response` — Release serving: assets embedded by the `embed-web` feature.
-  `Assets` struct L339 — `-` — tooling).
-  `serve_embedded_or_placeholder` function L355-377 — `(_rel: &str) -> Response` — No dist dir, no embedded assets: an honest placeholder so plain
-  `PLACEHOLDER` variable L356-367 — `: &str` — tooling).
-  `asset_response` function L382-398 — `(name: &str, bytes: Bytes) -> Response` — An asset body with content type + cache policy.
-  `tests` module L401-525 — `-` — tooling).
-  `auth_code_form` function L404-412 — `() -> TokenRelayForm` — tooling).
-  `relay_includes_client_secret_only_when_configured` function L417-431 — `()` — The relay appends `client_secret` iff configured (KAIROS-T-0056), and
-  `relay_secret_applies_to_refresh_grant` function L436-447 — `()` — The secret is applied to the refresh grant too (Google refreshes are
-  `relay_rejects_unknown_grant` function L451-460 — `()` — An unsupported grant is rejected even with a secret configured.
-  `reserved_prefixes_match_paths_not_strings` function L463-490 — `()` — tooling).
-  `index_fallback_only_for_route_like_paths` function L493-501 — `()` — tooling).
-  `asset_responses_carry_mime_and_cache_policy` function L504-524 — `()` — tooling).

#### crates/kairos-server/src/ws.rs

- pub `EventHub` struct L86-91 — `{ tx: broadcast::Sender<Arc<BroadcastEvent>>, reconnects: Arc<AtomicU64> }` — The per-process fan-out hub: LISTEN → broadcast → sockets.
- pub `router` function L106-124 — `(state: AppState) -> Router<AppState>` — Build the `/ws/events` route behind the standard auth → tenant stack
-  `EVENT_CHANNEL` variable L61 — `: &str` — The NOTIFY channel the db services emit on (`kairos_db::events`).
-  `BROADCAST_CAPACITY` variable L66 — `: usize` — Broadcast buffer per server process.
-  `BACKOFF_INITIAL` variable L69 — `: Duration` — Reconnect backoff bounds for the LISTEN connection.
-  `BACKOFF_MAX` variable L70 — `: Duration` — resource, and it ends when the socket does.
-  `BroadcastEvent` struct L75-82 — `{ tenant: String, board_id: Option<Uuid>, message: String }` — One parsed NOTIFY payload, ready for fan-out: the routing fields the
-  `EventHub` type L93-101 — `= EventHub` — resource, and it ends when the socket does.
-  `new` function L94-100 — `() -> Self` — resource, and it ends when the socket does.
-  `query_access_token` function L132-137 — `(query: &str) -> Option<&str>` — The `access_token` query parameter, if present and non-empty.
-  `promote_query_token` function L142-150 — `(mut req: Request, next: Next) -> Response` — Copy `?access_token=<jwt>` into the `Authorization` header when the
-  `ClientMessage` struct L160-162 — `{ subscribe: Option<SubscribeFilter> }` — The client subscribe message (`kairos_client::types_events`):
-  `SubscribeFilter` struct L165-167 — `{ board_id: Option<Uuid> }` — resource, and it ends when the socket does.
-  `ws_events` function L171-178 — `( ws: WebSocketUpgrade, Extension(tenant): Extension<TenantContext>, Extension(h...` — `GET /ws/events`: upgrade the (already authenticated, tenant-resolved)
-  `handle_socket` function L183-231 — `( mut socket: WebSocket, tenant: String, mut rx: broadcast::Receiver<Arc<Broadca...` — Forward this tenant's events to one socket until it closes.
-  `parse_payload` function L240-256 — `(payload: &str) -> Option<BroadcastEvent>` — Parse one NOTIFY payload (`kairos_db::events` shape): pull out the
-  `spawn_listener` function L263-285 — `(database_url: String, hub: EventHub)` — Start the per-process LISTEN task: connect, `LISTEN kairos_events`,
-  `run_listener` function L289-333 — `( database_url: &str, hub: &EventHub, backoff: &mut Duration, ) -> Result<(), to...` — One LISTEN connection lifetime: returns when the connection ends
-  `tests` module L336-386 — `-` — resource, and it ends when the socket does.
-  `payloads_parse_and_strip_tenant` function L340-356 — `()` — resource, and it ends when the socket does.
-  `off_board_and_malformed_payloads` function L359-374 — `()` — resource, and it ends when the socket does.
-  `access_token_query_extraction` function L377-385 — `()` — resource, and it ends when the socket does.

### crates/kairos-server/src/forge

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/forge/auth.rs

- pub `derive_secret` function L41-47 — `(signing_key: &str, connection_id: Uuid) -> String` — The webhook secret for one connection.
- pub `github_signature` function L51-56 — `(secret: &str, body: &[u8]) -> String` — GitHub's `X-Hub-Signature-256` value for a body under this secret:
- pub `secure_eq` function L60-66 — `(a: &str, b: &str) -> bool` — Constant-time string comparison — never `==` on a credential, so a
-  `HmacSha256` type L31 — `= Hmac<Sha256>` — any deployment-wide secret and is documented as such.
-  `hex` function L34-36 — `(bytes: &[u8]) -> String` — Lowercase hex.
-  `tests` module L69-104 — `-` — any deployment-wide secret and is documented as such.
-  `derivation_is_deterministic_and_key_dependent` function L73-85 — `()` — any deployment-wide secret and is documented as such.
-  `github_signature_matches_the_documented_shape` function L88-95 — `()` — any deployment-wide secret and is documented as such.
-  `secure_eq_matches_equality_semantics` function L98-103 — `()` — any deployment-wide secret and is documented as such.

#### crates/kairos-server/src/forge/mod.rs

- pub `auth` module L8 — `-` — and, from KAIROS-T-0099, the delivery endpoint itself.
- pub `webhook` module L9 — `-` — holds the parts that are NOT part of the authenticated `/api` surface.

#### crates/kairos-server/src/forge/webhook.rs

- pub `router` function L47-49 — `() -> Router<AppState>` — no-op.
-  `rejected` function L52-63 — `() -> Response` — The single response every authenticity failure produces.
-  `accepted` function L66-72 — `(detail: &str, linked: usize) -> Response` — Accepted — including the many "understood but nothing to do" cases.
-  `core_kind` function L74-79 — `(kind: CoreKind) -> LinkKind` — no-op.
-  `core_state` function L81-88 — `(state: CoreState) -> LinkState` — no-op.
-  `receive` function L91-284 — `( State(state): State<AppState>, Path((forge_segment, tenant, connection_id)): P...` — Ingest one delivery.

### crates/kairos-server/src/mcp

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/mcp/mod.rs

- pub `router` function L69-101 — `(state: AppState) -> Router<AppState>` — The `/mcp` endpoint (behind auth → tenant) plus the open RFC 9728
-  `service` module L43 — `-` — tool surface per KAIROS-S-0006): a remote streamable-HTTP MCP endpoint
-  `tools` module L44 — `-` — context via `whoami`/`my_boards` (A-0011 "Session context").
-  `WELL_KNOWN_MCP` variable L64 — `: &str` — The RFC 9728 well-known path for the `/mcp` protected resource.
-  `request_origin` function L106-113 — `(headers: &HeaderMap) -> Option<String>` — `scheme://host` of the request as seen by the client: `Host` header
-  `advertise_resource_metadata` function L117-129 — `(req: Request, next: Next) -> Response` — Attach the RFC 9728 `WWW-Authenticate` challenge to 401 responses from
-  `protected_resource_metadata` function L134-145 — `( State(state): State<AppState>, headers: HeaderMap, ) -> Json<Value>` — RFC 9728 protected-resource metadata: the `/mcp` resource identifier

#### crates/kairos-server/src/mcp/service.rs

- pub `KairosMcp` struct L23-26 — `{ state: AppState, tool_router: ToolRouter<Self> }` — One MCP service instance (rmcp builds one per session via the service
- pub `new` function L30-35 — `(state: AppState) -> Self` — Build a service instance over the shared state.
-  `KairosMcp` type L28-58 — `= KairosMcp` — in [`super::tools`].
-  `caller` function L42-57 — `( context: &RequestContext<RoleServer>, ) -> Result<(AuthContext, TenantContext)...` — The authenticated caller of the CURRENT tool call: the
-  `KairosMcp` type L64-82 — `impl ServerHandler for KairosMcp` — in [`super::tools`].
-  `get_info` function L65-81 — `(&self) -> ServerInfo` — in [`super::tools`].
-  `tool_error` function L89-96 — `(e: ApiError) -> CallToolResult` — Render an [`ApiError`] as the S-0006 REQ-1.1 typed tool-error text:
-  `tool_text` function L100-102 — `(text: impl Into<String>) -> CallToolResult` — A successful tool result: one compact markdown/text content block

#### crates/kairos-server/src/mcp/tools.rs

- pub `ListRepositoriesParams` struct L67-70 — `{ team: Option<String> }` — `activity_log` writes as the API path).
- pub `GetRepositoryParams` struct L74-77 — `{ repository: String }` — `activity_log` writes as the API path).
- pub `MyBoardsParams` struct L81-84 — `{ level: Option<String> }` — `activity_log` writes as the API path).
- pub `BoardItemsParams` struct L88-101 — `{ board: String, column: Option<String>, repository: Option<String>, include_del...` — `activity_log` writes as the API path).
- pub `GetItemParams` struct L105-108 — `{ short_code: String }` — `activity_log` writes as the API path).
- pub `GetHistoryParams` struct L112-119 — `{ short_code: String, limit: Option<i64>, version: Option<i32> }` — `activity_log` writes as the API path).
- pub `SearchParams` struct L123-136 — `{ q: Option<String>, filter: Option<SearchFilterParams>, traverse: Option<Search...` — `activity_log` writes as the API path).
- pub `SearchFilterParams` struct L140-169 — `{ entity_type: Option<Vec<String>>, board_id: Option<String>, column_id: Option<...` — `activity_log` writes as the API path).
- pub `SearchTraverseParams` struct L173-182 — `{ from: String, relationships: Vec<String>, direction: String, depth: Option<u32...` — `activity_log` writes as the API path).
- pub `SearchSortParams` struct L186-191 — `{ field: String, order: String }` — `activity_log` writes as the API path).
- pub `CreateItemParams` struct L195-229 — `{ item_type: String, title: String, board: Option<String>, parent: Option<String...` — `activity_log` writes as the API path).
- pub `UpdateItemParams` struct L233-243 — `{ short_code: String, title: Option<String>, content: String, version: i32 }` — `activity_log` writes as the API path).
- pub `EditItemParams` struct L247-258 — `{ short_code: String, search: String, replace: String, replace_all: bool }` — `activity_log` writes as the API path).
- pub `TransitionItemParams` struct L262-268 — `{ short_code: String, to_column: String }` — `activity_log` writes as the API path).
- pub `MoveItemParams` struct L272-278 — `{ short_code: String, to_board: String }` — `activity_log` writes as the API path).
- pub `LinkItemsParams` struct L282-289 — `{ source: String, target: String, relationship: String }` — `activity_log` writes as the API path).
- pub `UnlinkItemsParams` struct L293-300 — `{ source: String, target: String, relationship: String }` — `activity_log` writes as the API path).
- pub `SetMetadataParams` struct L304-311 — `{ short_code: String, values: BTreeMap<String, Option<String>> }` — `activity_log` writes as the API path).
- pub `RestoreItemParams` struct L315-318 — `{ short_code: String }` — `activity_log` writes as the API path).
- pub `DeleteItemParams` struct L322-328 — `{ short_code: String, confirm: bool }` — `activity_log` writes as the API path).
- pub `whoami` function L351-436 — `( &self, context: RequestContext<RoleServer>, ) -> Result<CallToolResult, ErrorD...` — `activity_log` writes as the API path).
- pub `list_repositories` function L441-484 — `( &self, Parameters(params): Parameters<ListRepositoriesParams>, context: Reques...` — `activity_log` writes as the API path).
- pub `get_repository` function L489-558 — `( &self, Parameters(params): Parameters<GetRepositoryParams>, context: RequestCo...` — `activity_log` writes as the API path).
- pub `my_boards` function L563-635 — `( &self, Parameters(params): Parameters<MyBoardsParams>, context: RequestContext...` — `activity_log` writes as the API path).
- pub `board_items` function L640-702 — `( &self, Parameters(params): Parameters<BoardItemsParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `get_item` function L707-831 — `( &self, Parameters(params): Parameters<GetItemParams>, context: RequestContext<...` — `activity_log` writes as the API path).
- pub `get_history` function L836-909 — `( &self, Parameters(params): Parameters<GetHistoryParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `search` function L914-953 — `( &self, Parameters(params): Parameters<SearchParams>, context: RequestContext<R...` — `activity_log` writes as the API path).
- pub `create_item` function L958-970 — `( &self, Parameters(params): Parameters<CreateItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `update_item` function L975-1000 — `( &self, Parameters(params): Parameters<UpdateItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `edit_item` function L1005-1061 — `( &self, Parameters(params): Parameters<EditItemParams>, context: RequestContext...` — `activity_log` writes as the API path).
- pub `move_item` function L1066-1122 — `( &self, Parameters(params): Parameters<MoveItemParams>, context: RequestContext...` — `activity_log` writes as the API path).
- pub `transition_item` function L1127-1188 — `( &self, Parameters(params): Parameters<TransitionItemParams>, context: RequestC...` — `activity_log` writes as the API path).
- pub `link_items` function L1193-1222 — `( &self, Parameters(params): Parameters<LinkItemsParams>, context: RequestContex...` — `activity_log` writes as the API path).
- pub `unlink_items` function L1227-1256 — `( &self, Parameters(params): Parameters<UnlinkItemsParams>, context: RequestCont...` — `activity_log` writes as the API path).
- pub `set_metadata` function L1261-1353 — `( &self, Parameters(params): Parameters<SetMetadataParams>, context: RequestCont...` — `activity_log` writes as the API path).
- pub `delete_item` function L1358-1390 — `( &self, Parameters(params): Parameters<DeleteItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `restore_item` function L1395-1440 — `( &self, Parameters(params): Parameters<RestoreItemParams>, context: RequestCont...` — `activity_log` writes as the API path).
-  `KairosMcp` type L335-1441 — `= KairosMcp` — `activity_log` writes as the API path).
-  `run_tool` function L338-346 — `(&self, tenant: &TenantContext, f: F) -> Result<CallToolResult, ErrorData>` — Run one closure on a tenant-pinned sync connection (the T-0018
-  `ItemView` struct L1448-1474 — `{ id: Uuid, item_type: ItemType, short_code: String, title: String, content: Str...` — A uniform projection of any live item, whatever its table.
-  `load_item` function L1483-1658 — `( conn: &mut PgConnection, short_code: &str, liveness: Liveness, ) -> Result<Ite...` — Resolve a short code and load its [`ItemView`]; 404 `NOT_FOUND`
-  `authorize_item_write` function L1663-1678 — `( conn: &mut PgConnection, slug: &str, user: Uuid, item: &ItemView, ) -> Result<...` — The A-0006 write gate for an item: `manage_<type>` on the item's
-  `require_capability_explained` function L1685-1728 — `( conn: &mut PgConnection, slug: &str, board_id: Option<Uuid>, user: Uuid, capab...` — `require_capability`, but when the caller is a cross-team filer — no
-  `board_by_ref` function L1731-1746 — `(conn: &mut PgConnection, reference: &str) -> Result<Board, ApiError>` — Resolve a board by UUID or slug; 404 `NOT_FOUND` otherwise.
-  `team_by_ref` function L1749-1768 — `( conn: &mut PgConnection, reference: &str, ) -> Result<kairos_db::models::teams...` — Resolve a team by UUID or slug; 422 otherwise (a filter value).
-  `board_by_id` function L1771-1778 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError>` — A board row by id (must exist — callers hold a FK to it).
-  `board_columns` function L1783-1785 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError>` — A board's LIVE columns in position order — what the board is now, so
-  `board_columns_including_removed` function L1794-1811 — `( conn: &mut PgConnection, board_id: Uuid, liveness: Liveness, ) -> Result<Vec<B...` — A board's columns in position order, removed ones included when the
-  `column_label` function L1819-1828 — `(conn: &mut PgConnection, column_id: Uuid) -> Result<String, ApiError>` — The name of ANY column, removed ones included — the audit answer, not
-  `resolve_column` function L1832-1849 — `(columns: &[BoardColumn], reference: &str) -> Result<Uuid, ApiError>` — Resolve a column reference (UUID or case-insensitive name) against a
-  `BoardItemRow` struct L1852-1864 — `{ column_id: Uuid, short_code: String, title: String, repository_id: Option<Uuid...` — One compact row of a board listing.
-  `board_item_rows` function L1875-2021 — `( conn: &mut PgConnection, board_id: Uuid, repository: Option<Uuid>, liveness: L...` — Every item placed on a board (strategies, initiatives, tasks, and
-  `column_item_counts` function L2026-2035 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<HashMap<Uuid, i64>, ApiEr...` — Per-column LIVE item counts for one board — what `list_boards` prints
-  `repo_slug_map` function L2039-2054 — `( conn: &mut PgConnection, ids: &[Uuid], ) -> Result<BTreeMap<Uuid, String>, Api...` — Slugs for a set of repository ids, one query (KAIROS-T-0111): what the
-  `repo_label` function L2057-2074 — `(conn: &mut PgConnection, repository_id: Option<Uuid>) -> Result<String, ApiErro...` — `slug (owner team)` for one task's repository, or `(none)`.
-  `require_live_typed` function L2078-2086 — `( conn: &mut PgConnection, short_code: &str, field: &str, ) -> Result<(Uuid, Ite...` — A live item by short code WITH its type (the edge-permission check needs
-  `metadata_lines` function L2090-2103 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError>` — An item's metadata values as compact `- slug: value` lines (ordered by
-  `ChainRow` struct L2106-2115 — `{ id: Uuid, short_code: String, title: String, deleted_at: Option<DateTime<Utc>>...` — `activity_log` writes as the API path).
-  `parent_chain` function L2128-2152 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ChainRow>, ApiError>` — The item's ancestors via incoming `parent` edges, nearest first
-  `relationship_lines` function L2163-2221 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError>` — Agent-oriented relationship lines for `get_item`: parent chain,
-  `line` function L2171-2178 — `(neighbor: &kairos_db::graph::Neighbor) -> String` — One neighbour line, tagged when the neighbour is archived.
-  `map_update_error` function L2232-2260 — `( conn: &mut PgConnection, item: &ItemView, e: items::ItemError, ) -> Result<Api...` — Map an [`items::ItemError`] from a content update to the S-0006 tool
-  `map_link_error` function L2270-2285 — `(e: GraphError) -> ApiError` — [`GraphError`] → the same codes the REST relationship endpoints emit:
-  `level_of` function L2292-2300 — `(item_type: ItemType) -> BoardLevel` — The board level whose boards host this item type.
-  `default_board_for` function L2304-2326 — `(conn: &mut PgConnection, level: BoardLevel) -> Result<Board, ApiError>` — The tenant's single live board of `level`, or a 422 asking the agent to
-  `resolve_template` function L2329-2360 — `(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError>` — Resolve a template reference (UUID, slug, or name) to its id.
-  `reject_field` function L2364-2376 — `( field: &str, value: Option<&String>, item_type: ItemType, applies_to: &str, ) ...` — Reject a type-specific field supplied for the wrong item type (agents
-  `create_item_impl` function L2381-2664 — `( conn: &mut PgConnection, tenant: &TenantContext, user: Uuid, params: &CreateIt...` — The create_item body: resolve the target board (or parent, for
-  `field_invalid` function L2672-2674 — `(field: &str, message: impl Into<String>) -> ApiError` — A field-level 422 `VALIDATION` for the search input (the tool-error
-  `uuid_field` function L2676-2679 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — `activity_log` writes as the API path).
-  `timestamp_field` function L2681-2690 — `(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError>` — `activity_log` writes as the API path).
-  `enum_field` function L2694-2705 — `( value: &str, field: &str, allowed: &str, ) -> Result<T, ApiError>` — Parse a closed-vocabulary value through the core model's serde
-  `search_to_core` function L2709-2839 — `(params: &SearchParams) -> Result<core_search::SearchRequest, ApiError>` — Convert the tool input into the typed `kairos_core::search` request and
-  `map_search_error` function L2843-2851 — `(e: SearchError) -> ApiError` — [`SearchError`] → tool error (validation was pre-checked, so this is
-  `archived_marker` function L2857-2863 — `(deleted_at: Option<DateTime<Utc>>) -> &'static str` — `" [archived]"` for a row that has been put away, empty otherwise
-  `render_search_results` function L2867-2939 — `(results: &SearchResults, repo_slugs: &BTreeMap<Uuid, String>) -> String` — Compact REQ-1.6 rendering: results grouped by type, one line per item

### crates/kairos-server/src/middleware

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/middleware/auth.rs

- pub `AuthContext` struct L53-62 — `{ user_id: Uuid, external_id: String, email: String, display_name: String }` — The authenticated caller, inserted as a request extension for every
- pub `TokenClaims` struct L67-74 — `{ sub: String, email: Option<String>, name: Option<String> }` — The token claims this crate consumes.
- pub `DiscoveryError` enum L78-87 — `Discovery` — Why building an [`Authenticator`] failed (startup-time, fail-fast).
- pub `VerifyError` enum L91-112 — `MissingToken | UnknownKey | Invalid | MissingEmail | KeyFetch` — Why a token was rejected (or could not be checked).
- pub `Authenticator` struct L143-156 — `{ issuer: String, audiences: Vec<String>, jwks_uri: Option<String>, http: reqwes...` — Validates bearer tokens against one OIDC issuer: JWKS cache keyed by
- pub `discover` function L183-227 — `(issuer: &str, audience: &str) -> Result<Self, DiscoveryError>` — Resolve the issuer's discovery document, prime the JWKS cache, and
- pub `with_static_keys` function L232-245 — `( issuer: &str, audience: &str, keys: impl IntoIterator<Item = (String, Decoding...` — Test constructor: fixed keys, no JWKS endpoint (refresh-on-unknown-
- pub `verify` function L310-325 — `(&self, token: &str) -> Result<TokenClaims, VerifyError>` — Validate `token` (RS256 signature, `iss`, `aud`, `exp`) and return
- pub `require_auth` function L397-425 — `( State(state): State<AppState>, mut req: Request, next: Next, ) -> Result<Respo...` — The auth layer: validate the bearer token, JIT-upsert the user, insert
-  `ApiError` type L114-121 — `= ApiError` — mode, and an effectively-empty value is a startup error.
-  `from` function L115-120 — `(err: VerifyError) -> Self` — mode, and an effectively-empty value is a startup error.
-  `DiscoveryDoc` struct L124-126 — `{ jwks_uri: String }` — mode, and an effectively-empty value is a startup error.
-  `JwksDoc` struct L129-131 — `{ keys: Vec<Jwk> }` — mode, and an effectively-empty value is a startup error.
-  `Jwk` struct L134-139 — `{ kty: String, kid: Option<String>, n: Option<String>, e: Option<String> }` — mode, and an effectively-empty value is a startup error.
-  `Authenticator` type L158-166 — `= Authenticator` — mode, and an effectively-empty value is a startup error.
-  `fmt` function L159-165 — `(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` — mode, and an effectively-empty value is a startup error.
-  `parse_audiences` function L171-177 — `(raw: &str) -> Vec<String>` — Parse `OIDC_AUDIENCE`: a comma-separated allow-list (KAIROS-T-0055).
-  `Authenticator` type L179-326 — `= Authenticator` — mode, and an effectively-empty value is a startup error.
-  `refresh_keys` function L248-287 — `(&self) -> Result<(), VerifyError>` — Fetch the JWKS and replace the cache with its RSA keys.
-  `key_for` function L291-306 — `(&self, kid: &str) -> Result<Option<DecodingKey>, VerifyError>` — The decoding key for `kid`, refreshing the JWKS once (behind the
-  `bearer_token` function L329-337 — `(req: &Request) -> Result<&str, VerifyError>` — `Authorization: Bearer <token>` or [`VerifyError::MissingToken`].
-  `jit_upsert_user` function L345-385 — `(pool: &TenantPool, claims: &TokenClaims) -> Result<User, ApiError>` — JIT user provisioning (A-0010): upsert `public.users` keyed on
-  `tests` module L428-678 — `-` — mode, and an effectively-empty value is a startup error.
-  `TEST_RSA_PRIVATE_PEM` variable L442-469 — `: &str` — Throwaway RSA keypair for minting test tokens.
-  `TEST_RSA_PUBLIC_PEM` variable L471-479 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `ISSUER` variable L481 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `AUDIENCE` variable L482 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `KID` variable L483 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `authenticator` function L485-488 — `() -> Authenticator` — mode, and an effectively-empty value is a startup error.
-  `MintClaims` struct L491-498 — `{ iss: &'a str, aud: &'a str, sub: &'a str, exp: i64, email: &'a str, name: &'a ...` — mode, and an effectively-empty value is a startup error.
-  `mint` function L500-513 — `(iss: &str, aud: &str, exp: i64, kid: Option<&str>) -> String` — mode, and an effectively-empty value is a startup error.
-  `future_exp` function L515-517 — `() -> i64` — mode, and an effectively-empty value is a startup error.
-  `chrono_now` function L519-524 — `() -> u64` — mode, and an effectively-empty value is a startup error.
-  `valid_token_yields_claims` function L527-535 — `()` — mode, and an effectively-empty value is a startup error.
-  `expired_token_is_rejected` function L538-549 — `()` — mode, and an effectively-empty value is a startup error.
-  `wrong_audience_is_rejected` function L552-562 — `()` — mode, and an effectively-empty value is a startup error.
-  `any_listed_audience_validates_and_unlisted_is_rejected` function L569-592 — `()` — KAIROS-T-0055: `OIDC_AUDIENCE` as a comma-separated list — a token
-  `audience_parsing_handles_lists_and_blanks` function L599-607 — `()` — KAIROS-T-0055: single-value backward compat is the default test
-  `wrong_issuer_is_rejected` function L610-620 — `()` — mode, and an effectively-empty value is a startup error.
-  `unknown_kid_is_rejected` function L623-629 — `()` — mode, and an effectively-empty value is a startup error.
-  `garbage_token_is_rejected` function L632-638 — `()` — mode, and an effectively-empty value is a startup error.
-  `google_shaped_id_token_validates_and_opaque_access_token_is_rejected` function L648-670 — `()` — KAIROS-T-0054: the middleware is issuer/token-kind agnostic — it
-  `GOOGLE_ISSUER` variable L649 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `GOOGLE_CLIENT_ID` variable L650 — `: &str` — mode, and an effectively-empty value is a startup error.
-  `verify_errors_map_to_401_envelope` function L673-677 — `()` — mode, and an effectively-empty value is a startup error.

#### crates/kairos-server/src/middleware/mod.rs

- pub `auth` module L6 — `-` — bearer tokens and JIT-provisions users (KAIROS-A-0010); [`tenant`]
- pub `tenant` module L7 — `-` — Auth runs first; tenant consumes its [`auth::AuthContext`].

#### crates/kairos-server/src/middleware/tenant.rs

- pub `TenantContext` struct L38-45 — `{ org_id: Uuid, slug: String, role: OrgRole }` — The resolved tenant, inserted as a request extension for every request
- pub `TenantDb` struct L50-53 — `{ pool: TenantPool, slug: String }` — A tenant-pinned pool handle: connections checked out through it have
- pub `conn` function L57-62 — `(&self) -> Result<TenantConnection, ApiError>` — Check out a connection pinned to this tenant's schema.
- pub `slug` function L65-67 — `(&self) -> &str` — The tenant slug this handle is pinned to.
- pub `resolve_slug` function L81-109 — `(config: &AppConfig, headers: &HeaderMap) -> Result<String, ApiError>` — Resolve the tenant slug for a request per the module-level order.
- pub `require_tenant` function L113-179 — `( State(state): State<AppState>, mut req: Request, next: Next, ) -> Result<Respo...` — The tenant layer: resolve the slug, load the organization, require
-  `TenantDb` type L55-68 — `= TenantDb` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `host_without_port` function L72-77 — `(host: &str) -> &str` — The host part of a `Host` header value (port stripped; IPv6 literals
-  `tests` module L182-264 — `-` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `config` function L186-205 — `(single_tenant: Option<&str>, base_domain: Option<&str>) -> AppConfig` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `headers` function L207-216 — `(pairs: &[(&str, &str)]) -> HeaderMap` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `single_tenant_mode_wins_over_everything` function L219-223 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `host_subdomain_resolves_against_base_domain` function L226-232 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `non_matching_hosts_fall_through_to_x_tenant` function L235-248 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `x_tenant_is_the_fallback_without_base_domain` function L251-255 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `unresolvable_requests_are_tenant_not_found` function L258-263 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.

### crates/kairos-server/src/scim

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/scim/auth.rs

- pub `TOKEN_PREFIX` variable L42 — `: &str` — The fixed token prefix.
- pub `SECRET_LEN` variable L45 — `: usize` — Length of the hex secret (32 random bytes → 64 hex chars).
- pub `ScimContext` struct L50-63 — `{ org_id: Uuid, slug: String, token_id: Uuid, token_name: String, actor_id: Uuid...` — The authenticated SCIM principal, inserted as a request extension for
- pub `generate_token` function L72-76 — `(slug: &str) -> String` — Mint a fresh token string for `slug`: `kairos_scim_<slug>_<64-hex>`
- pub `hash_token` function L80-82 — `(token: &str) -> String` — The value stored at rest and compared on every request: hex SHA-256 of
- pub `parse_token` function L87-95 — `(token: &str) -> Option<(&str, &str)>` — Split a presented token into `(slug, secret)`, or `None` if it does not
- pub `require_scim_token` function L107-169 — `( State(state): State<AppState>, mut req: Request, next: Next, ) -> Result<Respo...` — The SCIM auth layer: parse the token, resolve its embedded tenant, and
-  `hex` function L66-68 — `(bytes: &[u8]) -> String` — Hex-encode bytes (lowercase).
-  `bad_token` function L98-103 — `() -> ScimError` — The uniform 401 for every authentication failure mode.
-  `tests` module L172-219 — `-` — endpoint is not a tenant-enumeration oracle.
-  `token_round_trip_parses` function L176-182 — `()` — endpoint is not a tenant-enumeration oracle.
-  `slugs_with_underscores_split_at_the_last_separator` function L185-189 — `()` — endpoint is not a tenant-enumeration oracle.
-  `malformed_tokens_are_rejected` function L192-207 — `()` — endpoint is not a tenant-enumeration oracle.
-  `hash_is_stable_and_secret_free` function L210-218 — `()` — endpoint is not a tenant-enumeration oracle.

#### crates/kairos-server/src/scim/discovery.rs

-  `service_provider_config` function L15-40 — `() -> Response` — `GET /scim/v2/ServiceProviderConfig` (RFC 7643 §5).
-  `user_schema` function L43-79 — `() -> Value` — The User schema resource: the attribute subset this server round-trips.
-  `group_schema` function L82-108 — `() -> Value` — The Group schema resource.
-  `schemas` function L111-116 — `() -> Response` — `GET /scim/v2/Schemas` (RFC 7643 §7): ListResponse of the two schemas.
-  `resource_types` function L119-146 — `() -> Response` — `GET /scim/v2/ResourceTypes` (RFC 7643 §6).

#### crates/kairos-server/src/scim/error.rs

- pub `ERROR_URN` variable L13 — `: &str` — The RFC 7644 error message URN.
- pub `SCIM_CONTENT_TYPE` variable L18 — `: &str` — The SCIM media type every `/scim/v2` response (success or error) ships
- pub `ScimError` struct L22-29 — `{ status: StatusCode, scim_type: Option<&'static str>, detail: String }` — A SCIM-surface error (RFC 7644 §3.12).
- pub `unauthorized` function L43-45 — `(detail: impl Into<String>) -> Self` — 401 — missing/malformed/unknown/revoked SCIM bearer token.
- pub `not_found` function L48-50 — `(detail: impl Into<String>) -> Self` — 404 — no SCIM resource with that id in this tenant's resource set.
- pub `invalid_syntax` function L53-55 — `(detail: impl Into<String>) -> Self` — 400 `invalidSyntax` — unparseable body / missing message schema.
- pub `invalid_value` function L58-60 — `(detail: impl Into<String>) -> Self` — 400 `invalidValue` — a required value is missing or the wrong shape.
- pub `invalid_path` function L63-65 — `(detail: impl Into<String>) -> Self` — 400 `invalidPath` — a PATCH path outside the supported subset.
- pub `invalid_filter` function L68-70 — `(detail: impl Into<String>) -> Self` — 400 `invalidFilter` — a filter outside the supported subset.
- pub `mutability` function L78-80 — `(detail: impl Into<String>) -> Self` — 400 `mutability` — "the attempted modification is not compatible
- pub `uniqueness` function L84-86 — `(detail: impl Into<String>) -> Self` — 409 `uniqueness` — the resource already exists (duplicate user
- pub `internal` function L90-97 — `(context: impl std::fmt::Display) -> Self` — 500 — the context is logged; the envelope carries a generic detail
- pub `scim_response` function L101-108 — `(status: StatusCode, body: Value) -> Response` — Build a `/scim/v2` SUCCESS response: `application/scim+json` body.
-  `ScimError` type L31-98 — `= ScimError` — SCIM surface.
-  `new` function L32-38 — `(status: StatusCode, scim_type: Option<&'static str>, detail: impl Into<String>)...` — SCIM surface.
-  `ScimError` type L110-122 — `impl IntoResponse for ScimError` — SCIM surface.
-  `into_response` function L111-121 — `(self) -> Response` — SCIM surface.
-  `tests` module L125-139 — `-` — SCIM surface.
-  `envelope_matches_rfc7644_shape` function L129-138 — `()` — SCIM surface.

#### crates/kairos-server/src/scim/groups.rs

- pub `ADMINS_GROUP_NAME` variable L38 — `: &str` — The role-mapping group's displayName.
- pub `TEAM_GROUP_PREFIX` variable L40 — `: &str` — The team-group displayName prefix (`kairos-team-<slug>`).
-  `GroupTarget` enum L47-52 — `Admins | Team` — Which group a `/scim/v2/Groups/{id}` id names.
-  `resolve_group` function L54-72 — `( conn: &mut PgConnection, ctx: &ScimContext, id: Uuid, ) -> Result<GroupTarget,...` — `invalidValue`.
-  `team_group_name` function L74-76 — `(team: &Team) -> String` — `invalidValue`.
-  `admins_members` function L79-89 — `(conn: &mut PgConnection, org_id: Uuid) -> Result<Vec<User>, ScimError>` — The org's admins, ordered by email.
-  `team_member_users` function L93-106 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<User>, ScimError>` — A team's members, ordered by email (two queries: `team_members` and
-  `group_resource` function L108-123 — `(id: Uuid, display_name: &str, members: &[User]) -> Value` — `invalidValue`.
-  `render_target` function L125-142 — `( conn: &mut PgConnection, ctx: &ScimContext, target: &GroupTarget, ) -> Result<...` — `invalidValue`.
-  `parse_member_list` function L149-168 — `(value: &Value) -> Result<Vec<Uuid>, ScimError>` — Member entries are `{"value": "<user uuid>"}` objects (or bare strings).
-  `GroupChange` enum L171-179 — `Add | Remove | RemoveAll | Replace | DisplayName` — One parsed Group PATCH change.
-  `parse_member_value_path` function L182-185 — `(path: &str) -> Option<&str>` — `members[value eq "<uuid>"]` → the uuid.
-  `parse_group_patch` function L188-275 — `(body: &Value) -> Result<Vec<GroupChange>, ScimError>` — Parse the RFC 7644 §3.5.2 PatchOp subset for Groups (module docs).
-  `require_provisioned` function L282-296 — `( conn: &mut PgConnection, ctx: &ScimContext, user_id: Uuid, ) -> Result<User, S...` — A group member must already be a provisioned org member.
-  `set_org_role` function L298-322 — `( conn: &mut PgConnection, ctx: &ScimContext, user: &User, role: OrgRole, ) -> R...` — `invalidValue`.
-  `apply_admins_change` function L325-380 — `( conn: &mut PgConnection, ctx: &ScimContext, change: &GroupChange, ) -> Result<...` — Apply one admins-group member change set.
-  `apply_team_change` function L383-460 — `( conn: &mut PgConnection, ctx: &ScimContext, team: &Team, change: &GroupChange,...` — Apply one team-group member change set.
-  `apply_group_changes` function L464-490 — `( conn: &mut PgConnection, ctx: &ScimContext, target: &GroupTarget, changes: &[G...` — Apply a parsed change list to a resolved group.
-  `list_groups` function L498-556 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Query(p...` — `GET /scim/v2/Groups` — `kairos-admins` + one group per live team;
-  `get_group` function L559-571 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `GET /scim/v2/Groups/{id}`.
-  `create_group` function L577-677 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, body: B...` — `POST /scim/v2/Groups` — create a team-mapped group
-  `patch_group` function L680-697 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `PATCH /scim/v2/Groups/{id}` — member add/remove/replace (module docs).
-  `replace_group` function L701-726 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `PUT /scim/v2/Groups/{id}` — replace the member set (`displayName` must
-  `count_board_items` function L731-759 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ScimError>` — How many LIVE workflow items sit on `board_id` (the board-empty rule,
-  `delete_group` function L764-829 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `DELETE /scim/v2/Groups/{id}` — soft-delete the team + its delivery

#### crates/kairos-server/src/scim/mod.rs

- pub `auth` module L104 — `-` — KAIROS-A-0016): per-tenant `/scim/v2/Users` + `/scim/v2/Groups` so
- pub `discovery` module L105 — `-` — driven changes are attributable to the admin who issued the credential.
- pub `error` module L106 — `-` — driven changes are attributable to the admin who issued the credential.
- pub `groups` module L107 — `-` — driven changes are attributable to the admin who issued the credential.
- pub `tokens` module L108 — `-` — driven changes are attributable to the admin who issued the credential.
- pub `users` module L109 — `-` — driven changes are attributable to the admin who issued the credential.
- pub `LIST_URN` variable L131 — `: &str` — SCIM list-response message URN (RFC 7644 §3.4.2).
- pub `PATCH_URN` variable L133 — `: &str` — SCIM PATCH message URN (RFC 7644 §3.5.2).
- pub `USER_URN` variable L135 — `: &str` — Core User resource URN (RFC 7643 §4.1).
- pub `GROUP_URN` variable L137 — `: &str` — Core Group resource URN (RFC 7643 §4.2).
- pub `router` function L142-176 — `(state: AppState) -> Router<AppState>` — The `/scim/v2` router: discovery + Users + Groups, every route behind
- pub `tokens_router` function L180-182 — `() -> Router<AppState>` — The `/api/scim-tokens` router (org-admin token management).
-  `run_scim` function L194-216 — `(state: &AppState, slug: &str, f: F) -> Result<T, ScimError>` — Run `f` on a sync connection pinned to `slug`'s tenant schema, off the
-  `scim_transaction` function L220-238 — `(conn: &mut PgConnection, f: F) -> Result<T, ScimError>` — Run `f` inside ONE transaction keeping [`ScimError`] as the error type
-  `TxError` enum L224-227 — `Scim | Db` — driven changes are attributable to the admin who issued the credential.
-  `TxError` type L228-232 — `= TxError` — driven changes are attributable to the admin who issued the credential.
-  `from` function L229-231 — `(e: diesel::result::Error) -> Self` — driven changes are attributable to the admin who issued the credential.
-  `parse_json_body` function L244-250 — `(body: &Bytes) -> Result<Value, ScimError>` — Parse a request body as JSON (400 `invalidSyntax` otherwise).
-  `log_scim_activity` function L255-275 — `( conn: &mut PgConnection, ctx_actor: Uuid, token_name: &str, action: ActivityAc...` — One tenant `activity_log` row for a SCIM-driven mutation (see module
-  `membership_of` function L278-291 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> Result<Option<Organ...` — The org's membership row for `user_id`, if any.
-  `load_user` function L294-302 — `(conn: &mut PgConnection, user_id: Uuid) -> Result<Option<User>, ScimError>` — The `public.users` row by id, if any.
-  `admin_count` function L305-313 — `(conn: &mut PgConnection, org_id: Uuid) -> Result<i64, ScimError>` — How many admins the org currently has.
-  `last_admin_error` function L316-321 — `() -> ScimError` — The LAST_ADMIN guard as a SCIM error (module docs: 400 `mutability`).
-  `ListParams` struct L329-334 — `{ filter: Option<String>, start_index: Option<i64>, count: Option<i64> }` — `?filter=`, `?startIndex=`, `?count=` as IdPs send them.
-  `MAX_COUNT` variable L337 — `: i64` — Hard cap on `count` (also advertised in ServiceProviderConfig).
-  `ListParams` type L339-347 — `= ListParams` — driven changes are attributable to the admin who issued the credential.
-  `page` function L342-346 — `(&self) -> (i64, i64)` — `(start_index, count)` per RFC 7644 §3.4.2.4: 1-based start (values
-  `parse_eq_filter` function L352-373 — `(filter: &str) -> Result<(String, String), ScimError>` — Parse the supported filter subset: `attribute eq "value"` (`eq` is
-  `list_response` function L376-384 — `(total: i64, start_index: i64, resources: Vec<Value>) -> Value` — A SCIM ListResponse envelope over already-paged resources.
-  `parse_resource_id` function L388-390 — `(id: &str) -> Result<Uuid, ScimError>` — Parse a SCIM resource id path segment as a UUID; unknown shapes are 404
-  `no_content` function L393-396 — `() -> axum::response::Response` — 204 No Content (DELETE success, RFC 7644 §3.6).
-  `tests` module L399-440 — `-` — driven changes are attributable to the admin who issued the credential.
-  `eq_filters_parse_and_everything_else_is_rejected` function L403-421 — `()` — driven changes are attributable to the admin who issued the credential.
-  `pagination_defaults_and_clamps` function L424-439 — `()` — driven changes are attributable to the admin who issued the credential.

#### crates/kairos-server/src/scim/tokens.rs

-  `MANAGE` variable L29 — `: &str` — The pseudo-capability named in 403s (org-admin fallback, A-0006).
-  `router` function L31-35 — `() -> Router<AppState>` — `scim_tokens` table, and GET never returns secrets or hashes.
-  `CreateScimTokenRequest` struct L39-42 — `{ name: String }` — `POST /api/scim-tokens` body.
-  `ScimTokenCreatedResponse` struct L46-52 — `{ id: String, name: String, token: String, created_at: String }` — `POST /api/scim-tokens` response — the ONLY place the secret appears.
-  `ScimTokenView` struct L56-62 — `{ id: String, name: String, created_by: String, created_at: String, revoked_at: ...` — One row of `GET /api/scim-tokens` — never a secret, never a hash.
-  `ScimTokenRevokedResponse` struct L66-69 — `{ id: String, revoked: bool }` — `DELETE /api/scim-tokens/{id}` response.
-  `ScimTokenListResponse` struct L73-76 — `{ items: Vec<ScimTokenView>, total: i64 }` — `GET /api/scim-tokens` envelope.
-  `rfc3339` function L78-80 — `(ts: chrono::DateTime<chrono::Utc>) -> String` — `scim_tokens` table, and GET never returns secrets or hashes.
-  `token_view` function L82-90 — `(row: &ScimToken) -> ScimTokenView` — `scim_tokens` table, and GET never returns secrets or hashes.
-  `log_token_activity` function L94-113 — `( conn: &mut diesel::pg::PgConnection, actor_id: Uuid, action: ActivityAction, t...` — One tenant `activity_log` row for a token-management mutation (actor =
-  `create_token` function L128-174 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `scim_tokens` table, and GET never returns secrets or hashes.
-  `list_tokens` function L186-204 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `scim_tokens` table, and GET never returns secrets or hashes.
-  `revoke_token` function L220-256 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `scim_tokens` table, and GET never returns secrets or hashes.

#### crates/kairos-server/src/scim/users.rs

-  `user_resource` function L32-48 — `(user: &User, active: bool) -> Value` — Render one `public.users` row as a SCIM User resource.
-  `UserPayload` struct L55-64 — `{ user_name: String, external_id: Option<String>, display_name: Option<String>, ...` — The inbound attribute subset of a POST/PUT User payload.
-  `parse_scim_bool` function L67-74 — `(value: &Value) -> Option<bool>` — A SCIM boolean: JSON bool, or Entra's `"True"`/`"False"` strings.
-  `first_email` function L78-91 — `(body: &Value) -> Option<String>` — The first usable email value: primary first, then first entry; entries
-  `parse_user_payload` function L93-128 — `(body: &Value) -> Result<UserPayload, ScimError>` — the identity-join and deprovision contracts).
-  `resolve_or_create_user` function L136-206 — `( conn: &mut PgConnection, payload: &UserPayload, ) -> Result<User, ScimError>` — The identity-join contract (module docs): externalId → external_id,
-  `revoke_membership` function L211-237 — `( conn: &mut PgConnection, ctx: &ScimContext, membership: &OrganizationMember, u...` — Revoke the org membership (deprovision): LAST_ADMIN-guarded, activity-
-  `load_member` function L241-254 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> Result<(Organizatio...` — Load the member (membership + user) or 404 — the resource set is the
-  `create_user` function L262-300 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, body: B...` — `POST /scim/v2/Users` — provision: link-or-create the `public.users`
-  `list_users` function L304-354 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Query(p...` — `GET /scim/v2/Users` — list the org's members; supports
-  `get_user` function L357-369 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `GET /scim/v2/Users/{id}`.
-  `UserChange` enum L372-375 — `Active | DisplayName` — One parsed User PATCH change.
-  `parse_user_patch` function L378-453 — `(body: &Value) -> Result<Vec<UserChange>, ScimError>` — Parse the RFC 7644 §3.5.2 PatchOp subset for Users (module docs).
-  `patch_user` function L457-506 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `PATCH /scim/v2/Users/{id}` — `active: false` deprovisions immediately
-  `replace_user` function L511-570 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `PUT /scim/v2/Users/{id}` — replace the writable profile subset.
-  `delete_user` function L574-588 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `DELETE /scim/v2/Users/{id}` — revoke the membership (LAST_ADMIN-

### crates/kairos-server/src/service_accounts

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/service_accounts/auth.rs

- pub `KEY_PREFIX` variable L38 — `: &str` — The fixed API-key prefix (distinguishes a key from a JWT at the bearer
- pub `SECRET_LEN` variable L41 — `: usize` — Length of the hex secret (32 random bytes → 64 hex chars).
- pub `generate_key` function L50-54 — `(slug: &str) -> String` — Mint a fresh key string for `slug`: `kairos_sk_<slug>_<64-hex>` from 32
- pub `hash_key` function L58-60 — `(key: &str) -> String` — The value stored at rest and compared on every request: hex SHA-256 of the
- pub `display_prefix` function L64-69 — `(key: &str) -> String` — A display-only prefix for listing keys: the identifying head plus the first
- pub `parse_key` function L73-81 — `(key: &str) -> Option<(&str, &str)>` — Split a presented key into `(slug, secret)`, or `None` if it does not have
- pub `is_api_key` function L86-88 — `(token: &str) -> bool` — True if the bearer looks like an API key (so `require_auth` takes the
- pub `ApiKeyTenant` struct L102 — `-` — The tenant an API key resolves to, inserted by [`require_auth`] so
- pub `authenticate_api_key` function L108-168 — `( state: &AppState, token: &str, ) -> Result<(AuthContext, String), ApiError>` — Authenticate an API-key bearer: parse it, resolve its embedded tenant, hash
-  `hex` function L44-46 — `(bytes: &[u8]) -> String` — Lowercase hex-encode.
-  `bad_key` function L91-96 — `() -> ApiError` — The uniform 401 for every API-key authentication failure mode.
-  `tests` module L171-233 — `-` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).
-  `key_round_trip_parses` function L175-182 — `()` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).
-  `slug_with_underscores_splits_at_last_separator` function L185-189 — `()` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).
-  `malformed_keys_are_rejected` function L192-209 — `()` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).
-  `hash_is_stable_and_secret_free` function L212-219 — `()` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).
-  `display_prefix_hides_the_secret` function L222-232 — `()` — (the key's slug is pinned for `require_tenant` via [`ApiKeyTenant`]).

#### crates/kairos-server/src/service_accounts/mod.rs

- pub `auth` module L15 — `-` — A service account is a machine principal (a `public.users` row with
- pub `routes` module L16 — `-` — mint/list/revoke keys), KAIROS-T-0059.

#### crates/kairos-server/src/service_accounts/routes.rs

-  `MANAGE` variable L32 — `: &str` — The pseudo-capability named in 403s (org-admin fallback, A-0006).
-  `router` function L34-49 — `() -> Router<AppState>` — secret or hash.
-  `CreateServiceAccountRequest` struct L57-60 — `{ name: String }` — `POST /api/service-accounts` body.
-  `ServiceAccountView` struct L64-68 — `{ id: String, name: String, created_at: String }` — A service account (never a secret).
-  `ServiceAccountListResponse` struct L72-75 — `{ items: Vec<ServiceAccountView>, total: i64 }` — `GET /api/service-accounts` envelope.
-  `CreateApiKeyRequest` struct L79-85 — `{ name: String, expires_at: Option<String> }` — `POST /api/service-accounts/{id}/keys` body.
-  `ApiKeyCreatedResponse` struct L90-98 — `{ id: String, name: String, key: String, prefix: String, created_at: String, exp...` — `POST /api/service-accounts/{id}/keys` response — the ONLY place the secret
-  `ApiKeyView` struct L102-110 — `{ id: String, name: String, prefix: String, created_at: String, expires_at: Opti...` — One row of `GET /api/service-accounts/{id}/keys` — never a secret or hash.
-  `ApiKeyListResponse` struct L114-117 — `{ items: Vec<ApiKeyView>, total: i64 }` — `GET /api/service-accounts/{id}/keys` envelope.
-  `DeletedResponse` struct L121-124 — `{ id: String, deleted: bool }` — `DELETE` response for both service accounts and keys.
-  `rfc3339` function L130-132 — `(ts: chrono::DateTime<chrono::Utc>) -> String` — secret or hash.
-  `account_view` function L134-140 — `(u: &User) -> ServiceAccountView` — secret or hash.
-  `key_view` function L142-152 — `(k: &ApiKey) -> ApiKeyView` — secret or hash.
-  `log_activity` function L155-175 — `( conn: &mut diesel::pg::PgConnection, actor_id: Uuid, action: ActivityAction, e...` — One tenant `activity_log` row for a service-account/key mutation.
-  `create_service_account` function L194-225 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.
-  `list_service_accounts` function L237-257 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.
-  `delete_service_account` function L271-305 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.
-  `create_key` function L326-396 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.
-  `list_keys` function L410-434 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.
-  `revoke_key` function L453-498 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — secret or hash.

### crates/kairos-server/tests

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/tests/api_key_auth.rs

-  `common` module L10 — `-` — (KAIROS-A-0017 / KAIROS-T-0058): a `kairos_sk_<slug>_<secret>` bearer is
-  `SCRATCH_DB` variable L35 — `: &str` — with a static-key (empty) [`Authenticator`].
-  `whoami` function L37-39 — `(router: &Router, key: &str, headers: &[(&str, &str)]) -> (StatusCode, Value)` — with a static-key (empty) [`Authenticator`].
-  `make_service_account` function L43-70 — `( conn: &mut PgConnection, org_id: Uuid, name: &str, with_membership: bool, ) ->...` — Create a service-account `public.users` row (+ optional org membership) and
-  `mint` function L73-88 — `(conn: &mut PgConnection, user_id: Uuid, name: &str) -> String` — Mint a key for `user_id`, storing its hash, and return the raw key.
-  `api_key_authentication_end_to_end` function L91-183 — `()` — with a static-key (empty) [`Authenticator`].

#### crates/kairos-server/tests/archived_hidden.rs

-  `common` module L46 — `-` — listings hide archived work.** Rules 1 and 2 (archived work stays
-  `SCRATCH_DB` variable L73 — `: &str` — never touched (shared-services discipline).
-  `MARKER` variable L77 — `: &str` — The word every seeded item carries, so one `q` reaches all five
-  `user_id` function L79-85 — `(conn: &mut PgConnection, email: &str) -> Uuid` — never touched (shared-services discipline).
-  `board_id` function L87-94 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — never touched (shared-services discipline).
-  `uuid` function L96-98 — `(s: &str) -> Uuid` — never touched (shared-services discipline).
-  `board_short_codes` function L102-116 — `(client: &KairosClient, board: Uuid) -> Vec<String>` — Every short code on a board, across all four board-bound families and
-  `search_short_codes` function L119-134 — `(client: &KairosClient, request: SearchRequest) -> Vec<String>` — Every short code a search request returns, across the five groups.
-  `archived_work_is_absent_from_every_default_listing` function L137-550 — `()` — never touched (shared-services discipline).

#### crates/kairos-server/tests/cascade_preview.rs

-  `common` module L17 — `-` — (`GET /api/{entity_type}/{short_code}/cascade-preview`), through the
-  `SCRATCH_DB` variable L38 — `: &str` — never touched (shared-services discipline).
-  `user_id` function L40-46 — `(conn: &mut PgConnection, email: &str) -> Uuid` — never touched (shared-services discipline).
-  `board_id` function L48-55 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — never touched (shared-services discipline).
-  `uuid` function L58-60 — `(s: &str) -> Uuid` — Parse a DTO id string to a Uuid.
-  `cascade_preview_matches_actual_cascade` function L63-281 — `()` — never touched (shared-services discipline).

#### crates/kairos-server/tests/client_roundtrip.rs

-  `common` module L23 — `-` — `kairos_client::KairosClient` — every S-0005 rejection family is
-  `SCRATCH_DB` variable L48 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_id` function L51-57 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `rejection` function L60-65 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `CountingProvider` struct L69-72 — `{ token: String, calls: AtomicUsize }` — A counting token provider: proves the client draws a token from the
-  `CountingProvider` type L74-80 — `impl TokenProvider for CountingProvider` — discipline).
-  `bearer_token` function L75-79 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — discipline).
-  `NoCredentials` struct L84 — `-` — A provider with no credentials: the client surfaces `Error::Token`
-  `NoCredentials` type L86-90 — `impl TokenProvider for NoCredentials` — discipline).
-  `bearer_token` function L87-89 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — discipline).
-  `typed_error_mapping_roundtrip` function L93-415 — `()` — discipline).

#### crates/kairos-server/tests/entities.rs

-  `common` module L23 — `-` — (contracts per KAIROS-S-0005 / A-0004 / A-0006), through the typed
-  `SCRATCH_DB` variable L51 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_id` function L54-60 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id` function L64-71 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level (provisioned defaults; delivery is created
-  `columns_of` function L74-81 — `(conn: &mut PgConnection, board: Uuid) -> Vec<Uuid>` — Column ids of a board in position order.
-  `edges_of` function L84-93 — `(conn: &mut PgConnection, board: Uuid) -> Vec<(Uuid, Uuid)>` — Transition edges of a board.
-  `valid_target` function L96-102 — `(edges: &[(Uuid, Uuid)], from: Uuid) -> Uuid` — A column reachable from `from` per the board's transition graph.
-  `invalid_target` function L106-112 — `(columns: &[Uuid], edges: &[(Uuid, Uuid)], from: Uuid) -> Uuid` — A column NOT reachable from `from` (there is always one on the default
-  `rejection` function L115-120 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `get_any` function L123-131 — `(client: &KairosClient, kind: EntityKind, code: &str) -> Result<(), Error>` — `GET /api/{family}/{code}` through the typed client, family-generic.
-  `update_any` function L134-147 — `( client: &KairosClient, kind: EntityKind, code: &str, request: &UpdateContentRe...` — `PATCH /api/{family}/{code}` through the typed client, family-generic.
-  `delete_any` function L150-158 — `(client: &KairosClient, kind: EntityKind, code: &str) -> Result<(), Error>` — `DELETE /api/{family}/{code}` through the typed client, family-generic.
-  `transition_any` function L161-180 — `( client: &KairosClient, kind: EntityKind, code: &str, to_column_id: &str, ) -> ...` — `POST /api/{family}/{code}/transition` through the typed client.
-  `entity_endpoints_against_live_stack` function L183-1282 — `()` — admin API for grants is KAIROS-T-0019.

#### crates/kairos-server/tests/file_backlog.rs

-  `common` module L18 — `-` — (KAIROS-T-0105, A-0019 §4 amending A-0006): any tenant member may create
-  `SCRATCH_DB` variable L45 — `: &str` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `user_id` function L47-53 — `(conn: &mut PgConnection, email: &str) -> Uuid` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `org_id` function L55-61 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `add_member` function L63-72 — `(conn: &mut PgConnection, org: Uuid, user: Uuid, role: OrgRole)` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `rejection` function L74-79 — `(result: Result<T, Error>) -> Error` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `forbidden` function L81-84 — `(result: Result<T, Error>, what: &str)` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `McpSession` struct L87-93 — `{ http: reqwest::Client, url: String, token: String, session_id: String, next_id...` — Minimal MCP session over HTTP (initialize → initialized → tools/call).
-  `McpSession` type L95-182 — `= McpSession` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `open` function L96-141 — `(base_url: &str, token: &str) -> Self` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.
-  `call` function L144-181 — `(&mut self, tool: &str, arguments: Value) -> (bool, String)` — Call a tool; returns `(is_error, text)`.
-  `file_backlog_against_live_stack` function L185-757 — `()` — - `globex-alice` — alice in ANOTHER tenant: isolation unchanged.

#### crates/kairos-server/tests/forge_connections.rs

-  `common` module L13 — `-` — (design in KAIROS-I-0009), through the typed `kairos_client` against
-  `SCRATCH_DB` variable L36 — `: &str` — - `alice` — org member: reads open, writes 403.
-  `user_id` function L38-44 — `(conn: &mut PgConnection, email: &str) -> Uuid` — - `alice` — org member: reads open, writes 403.
-  `rejection` function L46-51 — `(result: Result<T, Error>) -> Error` — - `alice` — org member: reads open, writes 403.
-  `forge_connection_lifecycle_against_live_stack` function L54-372 — `()` — - `alice` — org member: reads open, writes 403.

#### crates/kairos-server/tests/forge_webhook.rs

-  `common` module L15 — `-` — KAIROS-I-0009): signed deliveries in, `item_links` out.
-  `SCRATCH_DB` variable L37 — `: &str` — successful no-ops, because a non-2xx makes forges retry forever.
-  `user_id` function L39-45 — `(conn: &mut PgConnection, email: &str) -> Uuid` — successful no-ops, because a non-2xx makes forges retry forever.
-  `link_state` function L48-57 — `(conn: &mut PgConnection, item_id: Uuid, external_id: &str) -> Option<String>` — One `item_links` row's state, by item + PR number.
-  `rejection` function L60-65 — `(result: Result<T, kairos_client::Error>) -> kairos_client::Error` — Unwrap an expected API rejection (panics on success).
-  `link_count` function L67-73 — `(conn: &mut PgConnection) -> i64` — successful no-ops, because a non-2xx makes forges retry forever.
-  `github_pr_n` function L76-98 — `(number: i64, code: &str, state: &str, merged: bool, updated_at: &str) -> String` — A GitHub `pull_request` payload for PR `number` naming `code`.
-  `github_pr` function L101-103 — `(code: &str, state: &str, merged: bool, updated_at: &str) -> String` — The common case: PR #42.
-  `forge_webhook_ingestion_against_live_stack` function L106-469 — `()` — successful no-ops, because a non-2xx makes forges retry forever.

#### crates/kairos-server/tests/mcp.rs

-  `common` module L37 — `-` — per KAIROS-A-0011 / KAIROS-S-0006): a real MCP session over streamable
-  `SCRATCH_DB` variable L63 — `: &str` — Uniquely named scratch database for this test binary.
-  `TENANT` variable L66 — `: &str` — The tenant slug.
-  `HOST_HEADER` variable L71 — `: &str` — Every request carries a real `Host` header and the tenant resolves from
-  `raw_request` function L79-115 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, session: Opti...` — One in-process request against `/mcp` (or any URI): returns status, the
-  `rpc_message` function L120-135 — `(body: &str) -> Value` — Extract the JSON-RPC message from a streamable-HTTP response body:
-  `McpSession` struct L139-144 — `{ router: &'a Router, token: String, session_id: Option<String>, next_id: i64 }` — An MCP session over the production router: POSTs JSON-RPC to `/mcp`
-  `connect` function L149-195 — `(router: &'a Router, token: &str) -> (McpSession<'a>, Value)` — Drive `initialize` + `notifications/initialized`; returns the
-  `request` function L198-221 — `(&mut self, method: &str, params: Value) -> Value` — One JSON-RPC request within the session; returns the `result`.
-  `call` function L224-234 — `(&mut self, tool: &str, arguments: Value) -> (bool, String)` — Call a tool; returns `(is_error, text)` from the CallToolResult.
-  `call_ok` function L237-241 — `(&mut self, tool: &str, arguments: Value) -> String` — Call a tool and require success, returning the text.
-  `call_err` function L244-248 — `(&mut self, tool: &str, arguments: Value) -> String` — Call a tool and require a tool error, returning the text.
-  `extract_code` function L252-260 — `(text: &str, prefix: &str) -> String` — The first short code with `prefix` in `text` (e.g.
-  `user_id` function L267-273 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id_of` function L276-283 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level.
-  `NameRow` struct L286-289 — `{ name: String }` — calls (asserted straight from the scratch database).
-  `column_names` function L292-300 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Column names of a board in position order.
-  `reachable_from_first` function L303-317 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Column names reachable from the FIRST column per `board_transitions`.
-  `CountRow` struct L320-323 — `{ n: i64 }` — calls (asserted straight from the scratch database).
-  `activity_count` function L326-336 — `(conn: &mut PgConnection, actor: Uuid, action: &str) -> i64` — `activity_log` rows in the scratch tenant for one action + actor.
-  `mcp_endpoint_against_live_stack` function L343-1137 — `()` — calls (asserted straight from the scratch database).

#### crates/kairos-server/tests/meta.rs

-  `common` module L20 — `-` — relationships, item metadata, metadata definitions, templates, content
-  `SCRATCH_DB` variable L53 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_id` function L56-62 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id` function L65-72 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level.
-  `rejection` function L75-80 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `rel_group` function L84-89 — `( groups: &'a [RelationshipGroup], relationship: &str, ) -> Option<&'a Relations...` — The group of `relationship` in one direction of a relationships
-  `metadata_value` function L93-95 — `(body: &'a ItemMetadataResponse, slug: &str) -> Option<&'a MetadataValue>` — The metadata value entry for `slug` in an item-metadata response, if
-  `metadata_patch` function L98-102 — `(slug: &str, value: Option<&str>) -> UpdateMetadataRequest` — A one-entry metadata PATCH body (`None` clears the slug).
-  `meta_endpoints_against_live_stack` function L105-1337 — `()` — - `bob`   — org member with NO grants (the 403 matrix; reads only).

#### crates/kairos-server/tests/metrics.rs

-  `common` module L17 — `-` — `/readyz` probe, exercised against the real production router.
-  `SCRATCH_DB` variable L41 — `: &str` — router (see `kairos_server::metrics` module docs).
-  `TENANT_HEADERS` variable L42 — `: &[(&str, &str)]` — router (see `kairos_server::metrics` module docs).
-  `raw_request` function L47-79 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&...` — A raw in-process GET returning `(status, content-type, body text)` — for
-  `parse_prometheus` function L84-113 — `(body: &str) -> BTreeMap<String, f64>` — Parse Prometheus text into `key -> value`, where `key` is the full
-  `metrics_and_readyz_against_live_stack` function L116-278 — `()` — router (see `kairos_server::metrics` module docs).
-  `FIRST_BATCH` variable L189 — `: usize` — router (see `kairos_server::metrics` module docs).
-  `SECOND_BATCH` variable L248 — `: usize` — router (see `kairos_server::metrics` module docs).

#### crates/kairos-server/tests/middleware.rs

-  `common` module L28 — `-` — contracts per KAIROS-A-0010 / KAIROS-A-0005 §2 / KAIROS-A-0013).
-  `SCRATCH_DB` variable L54 — `: &str` — Uniquely named scratch database (shared-services discipline: the
-  `call` function L57-63 — `( router: &Router, token: Option<&str>, headers: &[(&str, &str)], ) -> (StatusCo...` — One in-process `GET /api/whoami` against the production router.
-  `middleware_stack_against_live_dex` function L66-251 — `()` — live in `tests/common/mod.rs` (KAIROS-T-0018 refactor).

#### crates/kairos-server/tests/openapi.rs

-  `common` module L33 — `-` — Swagger UI (`/api/docs`, `KAIROS_DEV_UI`).
-  `SCRATCH_DB` variable L60 — `: &str` — Uniquely named scratch database for this test binary.
-  `TENANT_HEADERS` variable L63 — `: &[(&str, &str)]` — Every request resolves the tenant via the X-Tenant fallback.
-  `EXCLUDED_FROM_SPEC` variable L67-71 — `: &[(&str, &str, &str)]` — Routes that exist on (some configuration of) the router but are
-  `rust_sources` function L78-87 — `(dir: &FsPath, out: &mut Vec<PathBuf>)` — Collect every `.rs` file under `dir`, recursively.
-  `balanced_args` function L91-106 — `(text: &str) -> (&str, usize)` — The argument text of one `.route(` call: everything up to the paren
-  `methods_in` function L111-132 — `(args: &str) -> Vec<String>` — The HTTP methods named in a `.route(...)` argument list: `get(`,
-  `registered_api_routes` function L137-180 — `() -> BTreeSet<(String, String)>` — Scan the crate's `src/` for `.route("literal", ...)` registrations and
-  `documented_api_routes` function L183-200 — `(spec_json: &Value) -> BTreeSet<(String, String)>` — The `(METHOD, path)` pairs the aggregated spec documents.
-  `registered_routes_and_spec_paths_match_exactly` function L208-228 — `()` — THE completeness gate (module docs): registered routes == spec paths.
-  `spec_is_openapi_3_and_covers_expected_families` function L233-280 — `()` — The spec is 3.x and covers every expected path family (spot list —
-  `operation_ids_are_unique` function L284-296 — `()` — operationIds must be unique across the aggregated document.
-  `write_spec_artifact` function L302-310 — `()` — Generate the CI artifact: write the spec to `target/openapi.json`
-  `raw_get` function L318-346 — `(router: &Router, uri: &str, token: Option<&str>) -> (StatusCode, String, String...` — A raw in-process request for non-JSON responses (the Swagger UI page):
-  `probe_uri` function L351-363 — `(spec_path: &str) -> String` — Substitute every `{param}` in a spec path with a literal segment so it
-  `openapi_endpoint_against_live_stack` function L366-500 — `()` — services), uploaded by the workflow's "OpenAPI artifact" step.

#### crates/kairos-server/tests/org_endpoints.rs

-  `common` module L21 — `-` — families (S-0005 boards/teams/delivery-streams/board-members, the
-  `SCRATCH_DB` variable L47 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_row` function L50-56 — `(conn: &mut PgConnection, email: &str) -> (Uuid, String)` — `(public.users.id, external_id)` by email.
-  `rejection` function L59-64 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `column_by_name` function L67-72 — `(columns: &'a [BoardColumn], name: &str) -> &'a BoardColumn` — The column with this name from a board-detail column list.
-  `org_and_admin_endpoints_against_live_stack` function L75-1076 — `()` — - `bob`   — org member used for the capability grant/revoke lifecycle.

#### crates/kairos-server/tests/repositories_api.rs

-  `common` module L12 — `-` — KAIROS-I-0010 §D4, decision KAIROS-A-0019): the directory, self-serve
-  `SCRATCH_DB` variable L36 — `: &str` — `web` (so: not platform — cannot register platform's repos).
-  `user_id` function L38-44 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `web` (so: not platform — cannot register platform's repos).
-  `rejection` function L46-51 — `(result: Result<T, Error>) -> Error` — `web` (so: not platform — cannot register platform's repos).
-  `request` function L53-63 — `(slug: Option<&str>, name: &str, team: &str) -> CreateRepositoryRequest` — `web` (so: not platform — cannot register platform's repos).
-  `repository_api_against_live_stack` function L66-544 — `()` — `web` (so: not platform — cannot register platform's repos).

#### crates/kairos-server/tests/scim.rs

-  `common` module L20 — `-` — (contract per KAIROS-A-0016): token management (/api/scim-tokens), the
-  `SCRATCH_DB` variable L45 — `: &str` — Uniquely named scratch database for this test binary.
-  `ERROR_URN` variable L47 — `: &str` — - `dave`  — never logs in: exists only through SCIM (created user row).
-  `LIST_URN` variable L48 — `: &str` — - `dave`  — never logs in: exists only through SCIM (created user row).
-  `PATCH_URN` variable L49 — `: &str` — - `dave`  — never logs in: exists only through SCIM (created user row).
-  `TENANT_HEADERS` variable L52 — `: &[(&str, &str)]` — Every OIDC-authed tenant request resolves the tenant via X-Tenant.
-  `api` function L55-63 — `( router: &Router, method: Method, uri: &str, token: &str, body: Option<Value>, ...` — One tenant-scoped /api request (OIDC bearer + X-Tenant).
-  `scim` function L67-101 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, body: Option<...` — One SCIM request: bearer only — deliberately NO X-Tenant / Host tenancy
-  `assert_scim_error` function L104-112 — `(status: StatusCode, body: &Value, want: StatusCode, scim_type: Option<&str>)` — Assert an RFC 7644 §3.12 error envelope with this status (+ scimType).
-  `user_row` function L115-121 — `(conn: &mut PgConnection, email: &str) -> (Uuid, String)` — `(public.users.id, external_id)` by email.
-  `acme_role` function L124-133 — `(conn: &mut PgConnection, org_id: Uuid, user_id: Uuid) -> Option<String>` — The member's role in acme, if any.
-  `CountRow` struct L136-139 — `{ count: i64 }` — - `dave`  — never logs in: exists only through SCIM (created user row).
-  `activity_count` function L142-152 — `(conn: &mut PgConnection, entity_type: &str, details_like: &str) -> i64` — Count acme activity rows matching an entity type and details pattern.
-  `scim_provisioning_against_live_stack` function L155-995 — `()` — - `dave`  — never logs in: exists only through SCIM (created user row).

#### crates/kairos-server/tests/search_endpoint.rs

-  `common` module L35 — `-` — KAIROS-A-0007 / S-0005), through the typed `kairos_client::KairosClient`
-  `SCRATCH_DB` variable L70 — `: &str` — Uniquely named scratch database for this test binary.
-  `rejection` function L73-78 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `traverse_from` function L81-96 — `( short_code: &str, relationships: &[&str], direction: &str, depth: Option<u32>,...` — A traverse clause from a short code (the S-0005 examples' shape).
-  `assert_validation_400` function L100-110 — `(client: &KairosClient, request: SearchRequest, field: &str)` — Assert the request fails as 400 `VALIDATION` naming `field` in
-  `present_groups` function L115-134 — `(body: &SearchResponse) -> Vec<&'static str>` — The names of the NON-EMPTY groups of a typed search response, sorted.
-  `sorted_codes` function L137-141 — `(codes: impl IntoIterator<Item = String>) -> Vec<String>` — The `short_code` values of one typed result group, sorted.
-  `user_id` function L144-150 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id_by_slug` function L153-160 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `metadata_definition` function L164-185 — `(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid` — The tenant's metadata definition with this slug, creating it if the
-  `set_metadata` function L187-196 — `(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str)` — and every archived hit comes back marked with `archived_at`
-  `search_endpoint_against_live_stack` function L199-923 — `()` — and every archived hit comes back marked with `archived_at`

#### crates/kairos-server/tests/service_account_mgmt.rs

-  `common` module L10 — `-` — (KAIROS-A-0017 / KAIROS-T-0059), end to end with the auth branch
-  `SCRATCH_DB` variable L28 — `: &str` — and real Dex tokens (alice = admin, bob = non-member).
-  `get` function L30-37 — `( router: &Router, uri: &str, token: &str, headers: &[(&str, &str)], ) -> (Statu...` — and real Dex tokens (alice = admin, bob = non-member).
-  `post` function L39-47 — `( router: &Router, uri: &str, token: &str, headers: &[(&str, &str)], body: Value...` — and real Dex tokens (alice = admin, bob = non-member).
-  `service_account_management_end_to_end` function L50-222 — `()` — and real Dex tokens (alice = admin, bob = non-member).

#### crates/kairos-server/tests/task_move.rs

-  `common` module L16 — `-` — another delivery board (`POST /api/tasks/{code}/move`) and deleting a
-  `SCRATCH_DB` variable L39 — `: &str` — scratch database `kairos_task_move_i0012_test`.
-  `user_id` function L41-47 — `(conn: &mut PgConnection, email: &str) -> Uuid` — scratch database `kairos_task_move_i0012_test`.
-  `rejection` function L49-54 — `(result: Result<T, Error>) -> Error` — scratch database `kairos_task_move_i0012_test`.
-  `unprocessable` function L58-69 — `(err: &Error, expected_code: &str) -> String` — A typed 422 (`SAME_BOARD`, `NOT_DELIVERY_BOARD`, …): the client keeps
-  `task_move_and_team_deletion_against_live_stack` function L72-314 — `()` — scratch database `kairos_task_move_i0012_test`.
-  `alice_grant` function L317-327 — `(svc: &kairos_client::KairosClient, board_id: &str, alice_id: &str)` — Grant alice `manage_tasks` on one board (org admin does the granting).

#### crates/kairos-server/tests/task_repositories.rs

-  `common` module L15 — `-` — in KAIROS-I-0010 §D2, decision KAIROS-A-0019): create-time routing,
-  `SCRATCH_DB` variable L39 — `: &str` — KAIROS-T-0105's concern.
-  `user_id` function L41-47 — `(conn: &mut PgConnection, email: &str) -> Uuid` — KAIROS-T-0105's concern.
-  `rejection` function L49-54 — `(result: Result<T, Error>) -> Error` — KAIROS-T-0105's concern.
-  `validation_message` function L56-61 — `(err: &Error) -> String` — KAIROS-T-0105's concern.
-  `new_repo` function L63-75 — `(slug: &str, name: &str, team: Uuid, actor: Uuid) -> NewRepository` — KAIROS-T-0105's concern.
-  `task_repository_binding_against_live_stack` function L78-486 — `()` — KAIROS-T-0105's concern.

#### crates/kairos-server/tests/team_pages.rs

-  `common` module L14 — `-` — (design in KAIROS-I-0007), through the typed `kairos_client` against
-  `SCRATCH_DB` variable L42 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_id` function L45-51 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `initiative_board` function L54-61 — `(conn: &mut PgConnection) -> Uuid` — The tenant's provisioned initiative board.
-  `rejection` function L64-69 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `by_slug` function L72-77 — `(pages: &'a [TeamPage], slug: &str) -> &'a TeamPage` — The scaffold node with this slug (panics when absent).
-  `content_edit` function L80-90 — `(content: &str, version: i32) -> UpdateTeamPageRequest` — A content-only PATCH body.
-  `structure_edit` function L93-107 — `( slug: Option<&str>, parent_id: Option<&str>, move_to_root: bool, ) -> UpdateTe...` — A structure-only PATCH body (rename/move).
-  `team_pages_endpoints_against_live_stack` function L110-684 — `()` — - `bob`   — org member, NOT a team member (403s name team membership).

#### crates/kairos-server/tests/tenant_isolation.rs

-  `common` module L17 — `-` — through `kairos_client::KairosClient` instances (KAIROS-T-0016 covered
-  `SCRATCH_DB` variable L40 — `: &str` — Uniquely named scratch database for this test binary.
-  `SHARED_TITLE` variable L43 — `: &str` — The distinctive text seeded IDENTICALLY into both tenants.
-  `user_id` function L46-52 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `org_id` function L55-61 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — `public.organizations.id` by slug.
-  `rejection` function L64-69 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `seed_tenant` function L74-117 — `(admin: &KairosClient) -> (String, String, String)` — Seed one tenant's same-shaped fixture THROUGH THE API: a delivery
-  `http_level_two_tenant_isolation` function L120-314 — `()` — discipline).

#### crates/kairos-server/tests/web.rs

-  `common` module L10 — `-` — A-0015): the public `/api/config` discovery endpoint, the
-  `SCRATCH_DB` variable L27 — `: &str` — Uniquely named scratch database for this test binary.
-  `raw_request` function L32-65 — `( router: &Router, method: Method, uri: &str, form_body: Option<&str>, ) -> (Sta...` — One in-process request, returning status + content-type + raw body
-  `web_surfaces_against_live_stack` function L68-232 — `()` — integration suite here.

#### crates/kairos-server/tests/ws_events.rs

-  `common` module L29 — `-` — per KAIROS-A-0005 §5 / KAIROS-S-0005 "Event Push"): NOTIFY emission
-  `SCRATCH_DB` variable L65 — `: &str` — Uniquely named scratch database for this test binary.
-  `RECV_TIMEOUT` variable L69 — `: Duration` — Bound on every socket receive; generous because CI shares the compose
-  `WsClient` type L71 — `= WebSocketStream<MaybeTlsStream<TcpStream>>` — disconnect/reconnect — every await timeout-bounded.
-  `tenant_connection` function L75-81 — `(scratch_url: &str, schema: &str) -> PgConnection` — A fresh sync connection pinned to `schema` (same mechanism as the
-  `user_id` function L84-90 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_of_level` function L93-100 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level in the CURRENT search_path schema.
-  `transition_target` function L103-110 — `(conn: &mut PgConnection, board: Uuid, from: Uuid) -> Uuid` — A column reachable from `from` per the board's transition graph.
-  `rejection` function L113-118 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `ws_connect` function L125-145 — `( addr: std::net::SocketAddr, path: &str, auth: Option<&str>, tenant: Option<&st...` — Open a RAW WebSocket to the test server (protocol-level upgrade probes
-  `upgrade_status` function L148-155 — `(err: tokio_tungstenite::tungstenite::Error) -> StatusCode` — The HTTP status a rejected raw upgrade came back with.
-  `recv_event` function L159-167 — `(stream: &mut EventStream) -> (ThinEvent, Value)` — Next event from the typed helper stream, bounded by [`RECV_TIMEOUT`]:
-  `recv_raw_ws` function L171-186 — `(socket: &mut WsClient) -> Value` — Next Text frame from a RAW socket as JSON (the browser-fallback probe
-  `assert_event_shape` function L190-200 — `(event: &ThinEvent, raw: &Value, kind: &str, entity_type: &str)` — Assert the S-0005 thin-event shape (typed fields + the raw-frame
-  `ws_events_against_live_stack` function L203-688 — `()` — disconnect/reconnect — every await timeout-bounded.

### crates/kairos-server/tests/common

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/tests/common/mod.rs

- pub `DEFAULT_DATABASE_URL` variable L27 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
- pub `ISSUER` variable L30 — `: &str` — The live dev/test issuer (`.angreal/dex/config.yaml`).
- pub `AUDIENCE` variable L35 — `: &str` — The audience the server accepts = the client id user tokens are minted
- pub `admin_database_url` function L38-40 — `() -> String` — The compose Postgres admin URL (`DATABASE_URL` env or the default).
- pub `with_database` function L43-48 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
- pub `recreate_scratch_db` function L114-124 — `(scratch_db: &str) -> PgConnection` — Drop (if present) and recreate the uniquely named scratch database on
- pub `drop_scratch_db` function L128-132 — `(admin_conn: &mut PgConnection, scratch_db: &str)` — Drop the scratch database after the test (retries transient errors,
- pub `dex_token` function L135-167 — `( http: &reqwest::Client, client_id: &str, client_secret: Option<&str>, username...` — Obtain a real token from the live Dex via the password grant.
- pub `user_token` function L171-180 — `(http: &reqwest::Client, user: &str) -> String` — A user token from the seeded `kairos-cli` public client
- pub `base_config` function L184-210 — `(scratch_url: &str) -> AppConfig` — The standard test config: multi-tenant with `kairos.test` as the base
- pub `request` function L215-251 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&...` — One in-process request against the production router: any method/URI,
- pub `error_code` function L255-259 — `(body: &Value) -> &str` — The `error.code` of an S-0005 error envelope (panics on any other
- pub `TestServer` struct L265-270 — `{ addr: std::net::SocketAddr, base_url: String }` — A live instance of the production router on an ephemeral local port —
- pub `spawn_server` function L273-285 — `(router: Router) -> TestServer` — Serve `router` on an ephemeral 127.0.0.1 port.
- pub `client` function L290-292 — `(&self, token: &str, tenant: &str) -> KairosClient` — A typed client for this server: fixed bearer token, tenant via the
- pub `client_untenanted` function L296-298 — `(&self, token: &str) -> KairosClient` — A typed client WITHOUT tenant resolution (cross-tenant
-  `DB_SETUP_ATTEMPTS` variable L63 — `: u32` — Attempts for each transient scratch-DB lifecycle op against the SHARED
-  `DB_SETUP_BACKOFF_BASE` variable L67 — `: std::time::Duration` — Base backoff between scratch-DB retry attempts; doubles each attempt,
-  `DB_SETUP_BACKOFF_CAP` variable L70 — `: std::time::Duration` — Ceiling on the per-attempt backoff.
-  `retry_db` function L74-91 — `(what: &str, mut op: impl FnMut() -> Result<T, E>) -> T` — Run a transient shared-Postgres op with bounded exponential backoff,
-  `connect_admin` function L95-103 — `() -> PgConnection` — Connect to the shared compose Postgres admin database, retrying transient
-  `TestServer` type L287-299 — `= TestServer` — different subset, so unused-item lints are expected noise here.

### crates/kairos-soak/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-soak/src/auth.rs

- pub `PasswordToken` struct L28-35 — `{ http: reqwest::Client, issuer: String, client_id: String, username: String, pa...` — A self-refreshing password-grant token for one workforce identity.
- pub `new` function L38-53 — `( http: reqwest::Client, issuer: &str, client_id: &str, username: &str, password...` — Dex mints 24h tokens; refresh margin is generous anyway).
- pub `token` function L56-99 — `(&self) -> Result<String, Error>` — The current access token, minting/refreshing as needed.
- pub `SmallRng` struct L111 — `-` — A tiny deterministic xorshift64* RNG for op-mix selection and jitter —
- pub `new` function L114-116 — `(seed: u64) -> SmallRng` — Dex mints 24h tokens; refresh margin is generous anyway).
- pub `next_u64` function L118-125 — `(&mut self) -> u64` — Dex mints 24h tokens; refresh margin is generous anyway).
- pub `below` function L128-130 — `(&mut self, bound: u64) -> u64` — Uniform in `0..bound` (bound > 0).
- pub `unit` function L133-135 — `(&mut self) -> f64` — Uniform float in `[0, 1)`.
-  `REFRESH_MARGIN` variable L25 — `: Duration` — Refresh when the cached token is within this margin of expiry.
-  `PasswordToken` type L37-100 — `= PasswordToken` — Dex mints 24h tokens; refresh margin is generous anyway).
-  `PasswordToken` type L102-106 — `impl TokenProvider for PasswordToken` — Dex mints 24h tokens; refresh margin is generous anyway).
-  `bearer_token` function L103-105 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — Dex mints 24h tokens; refresh margin is generous anyway).
-  `SmallRng` type L113-136 — `= SmallRng` — Dex mints 24h tokens; refresh margin is generous anyway).
-  `tests` module L139-156 — `-` — Dex mints 24h tokens; refresh margin is generous anyway).
-  `rng_is_deterministic_and_bounded` function L143-155 — `()` — Dex mints 24h tokens; refresh margin is generous anyway).

#### crates/kairos-soak/src/config.rs

- pub `DEFAULT_P95_BUDGET_MS` variable L22 — `: f64` — Latency budget defaults (ms), calibrated against the DECIDED contract:
- pub `DEFAULT_WRITE_QUERY_P95_BUDGET_MS` variable L29 — `: f64` — Classes the vision's 50ms sentence does NOT cover — `create` (short
- pub `DEFAULT_MCP_P95_BUDGET_MS` variable L33 — `: f64` — Default MCP-session p95 budget: one `mcp` op is a whole session — five
- pub `parse_duration` function L36-62 — `(raw: &str) -> Result<Duration, String>` — Parse a human duration: `45s`, `10m`, `4h`, or bare seconds (`600`).
- pub `OpMix` struct L68-85 — `{ create: u32, edit: u32, conflict_edit: u32, transition: u32, read: u32, search...` — The weighted operation mix (weights are relative, not percentages).
- pub `total` function L104-113 — `(&self) -> u32` — Total weight (never 0: an all-zero mix is a config error).
- pub `Thresholds` struct L123-156 — `{ max_error_rate: f64, p95_budget_ms: BTreeMap<String, f64>, min_samples_for_lat...` — Continuous-assertion thresholds.
- pub `p95_budget` function L174-185 — `(&self, class: &str) -> f64` — The p95 budget for an op class (config override or the defaults).
- pub `ProfileFile` struct L191-205 — `{ url: Option<String>, issuer: Option<String>, tenant: Option<String>, bystander...` — The TOML profile file shape (everything optional; see [`SoakConfig`]).
- pub `SoakConfig` struct L209-240 — `{ url: String, issuer: String, tenant: String, bystander_tenant: String, duratio...` — The fully resolved run configuration.
- pub `apply_profile` function L264-305 — `(&mut self, profile: ProfileFile) -> Result<(), String>` — Layer a parsed TOML profile over `self`.
- pub `validate` function L308-325 — `(&self) -> Result<(), String>` — Validate the resolved configuration.
-  `OpMix` type L87-100 — `impl Default for OpMix` — standing runner) — identical assertions, longer exposure.
-  `default` function L88-99 — `() -> Self` — standing runner) — identical assertions, longer exposure.
-  `OpMix` type L102-114 — `= OpMix` — standing runner) — identical assertions, longer exposure.
-  `Thresholds` type L158-170 — `impl Default for Thresholds` — standing runner) — identical assertions, longer exposure.
-  `default` function L159-169 — `() -> Self` — standing runner) — identical assertions, longer exposure.
-  `Thresholds` type L172-186 — `= Thresholds` — standing runner) — identical assertions, longer exposure.
-  `SoakConfig` type L242-260 — `impl Default for SoakConfig` — standing runner) — identical assertions, longer exposure.
-  `default` function L243-259 — `() -> Self` — standing runner) — identical assertions, longer exposure.
-  `SoakConfig` type L262-326 — `= SoakConfig` — standing runner) — identical assertions, longer exposure.
-  `tests` module L329-429 — `-` — standing runner) — identical assertions, longer exposure.
-  `duration_parsing` function L333-342 — `()` — standing runner) — identical assertions, longer exposure.
-  `profile_layering_and_precedence` function L345-398 — `()` — standing runner) — identical assertions, longer exposure.
-  `unknown_profile_keys_are_rejected` function L401-407 — `()` — standing runner) — identical assertions, longer exposure.
-  `validation_rejects_nonsense` function L410-428 — `()` — standing runner) — identical assertions, longer exposure.

#### crates/kairos-soak/src/main.rs

-  `auth` module L22 — `-` — (KAIROS-T-0046).
-  `config` module L23 — `-` — `--duration 4h` (hours-scale).
-  `mcp` module L24 — `-` — `--duration 4h` (hours-scale).
-  `prom` module L25 — `-` — `--duration 4h` (hours-scale).
-  `report` module L26 — `-` — `--duration 4h` (hours-scale).
-  `stats` module L27 — `-` — `--duration 4h` (hours-scale).
-  `workforce` module L28 — `-` — `--duration 4h` (hours-scale).
-  `world` module L29 — `-` — `--duration 4h` (hours-scale).
-  `Cli` struct L51-85 — `{ url: Option<String>, issuer: Option<String>, tenant: Option<String>, bystander...` — KAIROS-A-0012 tier-5 workforce soak driver (KAIROS-T-0046).
-  `resolve_config` function L88-129 — `(cli: Cli) -> Result<SoakConfig, String>` — Merge defaults <- profile <- CLI into the final config.
-  `BreachLog` struct L136-140 — `{ printed: BTreeMap<String, u32>, all: Vec<Breach>, fatal: bool }` — The breach ledger: records everything, prints each (check, subject)
-  `BreachLog` type L142-170 — `= BreachLog` — `--duration 4h` (hours-scale).
-  `MAX_PRINTS_PER_KEY` variable L143 — `: u32` — `--duration 4h` (hours-scale).
-  `MAX_RECORDS` variable L144 — `: usize` — `--duration 4h` (hours-scale).
-  `record` function L146-169 — `(&mut self, breach: Breach)` — `--duration 4h` (hours-scale).
-  `TickerFindings` struct L173-177 — `{ metrics_available: Option<bool>, scrapes: usize, final_pool_gauges: BTreeMap<S...` — What the assertion loop hands back when the run ends.
-  `assertion_loop` function L184-306 — `( config: Arc<SoakConfig>, recorder: Arc<Recorder>, http: reqwest::Client, bysta...` — The continuous-assertion loop: every interval, check error rate and
-  `run` function L308-617 — `() -> Result<u8, String>` — `--duration 4h` (hours-scale).
-  `main` function L620-628 — `() -> ExitCode` — `--duration 4h` (hours-scale).

#### crates/kairos-soak/src/mcp.rs

- pub `run_session` function L105-194 — `( http: &reqwest::Client, base_url: &str, tenant: &str, token: &str, board_slug:...` — Run one complete MCP session: initialize → notifications/initialized →
-  `rpc_message` function L12-27 — `(body: &str) -> Result<Value, String>` — Extract the JSON-RPC message from a streamable-HTTP body (plain JSON
-  `post` function L30-62 — `( http: &reqwest::Client, base_url: &str, tenant: &str, token: &str, session: Op...` — One JSON-RPC POST to `/mcp`; returns `(status, session header, body)`.
-  `call_tool` function L66-101 — `( http: &reqwest::Client, base_url: &str, tenant: &str, token: &str, session: &s...` — Call one tool inside an open session; errors carry the tool name.

#### crates/kairos-soak/src/prom.rs

- pub `MetricSample` type L17 — `= BTreeMap<String, f64>` — One scrape: gauge/counter samples keyed by `name{labels}`.
- pub `parse_prometheus_text` function L22-55 — `(body: &str) -> MetricSample` — Parse Prometheus text exposition format: `name{labels} value [ts]`
- pub `is_pool_metric` function L60-63 — `(key: &str) -> bool` — The metric-key filter for pool health: connection-pool gauges (bb8
- pub `MIN_CLIMB_STEPS` variable L66 — `: usize` — Minimum consecutive rising steps before a gauge counts as "climbing".
- pub `CLIMB_GROWTH_FACTOR` variable L69 — `: f64` — Relative growth (last/first) above which a monotonic climb is a breach.
- pub `check_pool_stability` function L75-105 — `(samples: &[MetricSample]) -> Vec<Breach>` — Detect unbounded pool growth: a pool gauge that rose on EVERY
-  `tests` module L108-164 — `-` — `thresholds.require_metrics` is set, does not fail the run.
-  `parses_prometheus_text` function L112-129 — `()` — `thresholds.require_metrics` is set, does not fail the run.
-  `pool_metric_filter` function L132-136 — `()` — `thresholds.require_metrics` is set, does not fail the run.
-  `sample` function L138-142 — `(value: f64) -> MetricSample` — `thresholds.require_metrics` is set, does not fail the run.
-  `monotonic_climb_is_flagged_but_oscillation_is_not` function L145-163 — `()` — `thresholds.require_metrics` is set, does not fail the run.

#### crates/kairos-soak/src/report.rs

- pub `ConfigEcho` struct L15-23 — `{ duration_secs: u64, rate_ops_per_sec: f64, human_workers: usize, agent_workers...` — Echo of the load-shape configuration the run used.
- pub `Totals` struct L27-34 — `{ recorded_calls: u64, ok: u64, expected_conflicts: u64, errors: u64, error_rate...` — Aggregate outcome counters.
- pub `WsReport` struct L38-43 — `{ subscribers: usize, connects: u64, events_received: u64, reconnects: u64 }` — WS subscriber counters.
- pub `MetricsReport` struct L47-54 — `{ available: Option<bool>, scrapes: usize, final_pool_gauges: BTreeMap<String, f...` — `/metrics` scrape findings.
- pub `BystanderReport` struct L58-63 — `{ tenant: String, status: String, changed_sections: Vec<String> }` — Bystander-tenant isolation findings.
- pub `RunReport` struct L67-84 — `{ tool: String, target: String, tenant: String, started_at: String, finished_at:...` — The full run report.
- pub `outcome` function L87-95 — `(breaches: &[Breach]) -> &'static str` — Derive the run outcome from the breach list.
- pub `exit_code` function L98-104 — `(outcome: &str) -> u8` — The process exit code for an outcome (PASS=0, FAIL=1, FATAL=2).
- pub `totals` function L107-122 — `(summaries: &[ClassSummary]) -> Totals` — Compute [`Totals`] from the class summaries.
- pub `print_summary` function L126-208 — `(&self)` — The stdout summary (the JSON file carries the full detail).
-  `RunReport` type L124-209 — `= RunReport` — to a file and summarized on stdout.
-  `tests` module L212-263 — `-` — to a file and summarized on stdout.
-  `breach` function L215-217 — `(severity: Severity) -> Breach` — to a file and summarized on stdout.
-  `outcome_and_exit_code_policy` function L220-230 — `()` — to a file and summarized on stdout.
-  `totals_math` function L233-262 — `()` — to a file and summarized on stdout.

#### crates/kairos-soak/src/stats.rs

- pub `OpClass` enum L18-27 — `Create | Edit | ConflictEdit | Transition | Read | Search | Traverse | Mcp` — The operation classes of the A-0012 tier-5 mix.
- pub `name` function L31-42 — `(self) -> &'static str` — Stable name used in config keys, reports, and breach messages.
- pub `ALL` variable L46-55 — `: [OpClass; 8]` — Every class, for iteration (test assertions).
- pub `Outcome` enum L60-68 — `Ok | ExpectedConflict | Error` — How one operation ended.
- pub `ClassStats` struct L72-78 — `{ latencies_us: Vec<u64>, ok: u64, expected_conflicts: u64, errors: u64 }` — Aggregated numbers for one op class.
- pub `ClassSummary` struct L82-91 — `{ class: String, ok: u64, expected_conflicts: u64, errors: u64, p50_ms: f64, p95...` — A point-in-time percentile summary for one op class.
- pub `Severity` enum L97-100 — `Breach | Fatal` — Severity of an assertion breach.
- pub `Breach` struct L104-113 — `{ at: String, severity: Severity, check: String, detail: String }` — One attributed assertion breach.
- pub `now` function L116-123 — `(severity: Severity, check: &str, detail: String) -> Breach` — can only be pessimistic relative to the vision's 50ms p95 budget.
- pub `Recorder` struct L128-130 — `{ inner: Mutex<RecorderInner> }` — The shared recorder all workers write to.
- pub `record` function L144-165 — `(&self, class: OpClass, elapsed: Duration, outcome: Outcome)` — Record one completed operation.
- pub `summaries` function L169-189 — `(&self) -> Vec<ClassSummary>` — Snapshot every class summary (sorts copies; the hot path only
- pub `error_samples` function L192-198 — `(&self) -> Vec<String>` — The retained error samples.
- pub `percentile_us` function L202-208 — `(sorted: &[u64], pct: f64) -> u64` — Nearest-rank percentile over an ASCENDING-sorted slice (0 for empty).
- pub `error_rate` function L213-224 — `(summaries: &[ClassSummary]) -> f64` — The run-wide error rate: errors over ALL completed ops, with the
- pub `check_snapshot` function L228-270 — `( summaries: &[ClassSummary], thresholds: &crate::config::Thresholds, ) -> Vec<B...` — Apply the error-rate and per-class p95 checks to a snapshot; returns
-  `OpClass` type L29-56 — `= OpClass` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `Breach` type L115-124 — `= Breach` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `RecorderInner` struct L133-137 — `{ classes: BTreeMap<&'static str, ClassStats>, error_samples: Vec<String> }` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `MAX_ERROR_SAMPLES` variable L140 — `: usize` — Cap on retained error sample strings.
-  `Recorder` type L142-199 — `= Recorder` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `tests` module L273-368 — `-` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `summary` function L277-288 — `(class: &str, ok: u64, conflicts: u64, errors: u64, p95: f64) -> ClassSummary` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `percentile_nearest_rank` function L291-299 — `()` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `error_rate_excludes_deliberate_conflicts_from_numerator` function L302-307 — `()` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `snapshot_checks_flag_rate_and_p95` function L310-336 — `()` — can only be pessimistic relative to the vision's 50ms p95 budget.
-  `recorder_records_and_summarizes` function L339-367 — `()` — can only be pessimistic relative to the vision's 50ms p95 budget.

#### crates/kairos-soak/src/workforce.rs

- pub `WorkerSpec` struct L29-40 — `{ name: String, client: KairosClient, token: Arc<PasswordToken>, home_board: usi...` — One worker's identity and wiring.
- pub `worker_loop` function L69-100 — `( spec: WorkerSpec, world: Arc<World>, config: Arc<SoakConfig>, recorder: Arc<Re...` — The worker loop: pick from the weighted mix, run, pace, until `stop`.
- pub `WsCounters` struct L378-382 — `{ connects: AtomicU64, events: AtomicU64, reconnects: AtomicU64 }` — Shared counters for the standing event subscribers.
- pub `ws_subscriber` function L386-423 — `( client: KairosClient, counters: Arc<WsCounters>, mut stop: watch::Receiver<boo...` — A standing `/ws/events` subscriber: counts delivered thin events,
-  `POOL_CAP` variable L26 — `: usize` — Cap on each worker's created-item pool (edit/transition targets).
-  `record_call` function L44-65 — `( recorder: &Recorder, class: OpClass, conflict_expected: bool, fut: impl Future...` — Record one client call under `class`.
-  `choose_op` function L104-131 — `(config: &SoakConfig, rng: &mut SmallRng, pool_empty: bool) -> OpClass` — Weighted op selection; ops that need the worker's own pool degrade to
-  `run_op` function L134-370 — `( spec: &WorkerSpec, world: &World, config: &SoakConfig, recorder: &Recorder, ht...` — the whole session (documented; it gets a session-sized budget).
-  `tests` module L426-459 — `-` — the whole session (documented; it gets a session-sized budget).
-  `op_choice_follows_weights_and_degrades_without_a_pool` function L431-458 — `()` — the whole session (documented; it gets a session-sized budget).

#### crates/kairos-soak/src/world.rs

- pub `BoardInfo` struct L32-37 — `{ id: String, slug: String, targets: BTreeMap<String, Vec<String>> }` — A delivery board with its column set and transition adjacency.
- pub `World` struct L41-51 — `{ delivery_boards: Vec<BoardInfo>, collision_codes: Vec<String>, strategy_code: ...` — Everything the workers share.
- pub `board_by_id` function L54-56 — `(&self, id: &str) -> Option<&BoardInfo>` — version, and stays under a hard cap).
- pub `setup_world` function L66-275 — `( alice: &KairosClient, others: &[(&str, &KairosClient)], ) -> Result<World, Str...` — Enroll the workforce and create the shared fixtures.
- pub `Bystander` struct L282-286 — `{ client: KairosClient, tenant: String, baseline: BTreeMap<String, String> }` — The bystander tenant and its baseline snapshot.
- pub `BystanderUnavailable` enum L290-295 — `NotDeploymentAdmin | Failed` — Why the bystander check could not run (reported loudly either way).
- pub `setup_bystander` function L301-374 — `( admin: &KairosClient, tenant_client: KairosClient, slug: &str, ) -> Result<Bys...` — Provision (or adopt) the bystander tenant and take the baseline
- pub `snapshot` function L389-481 — `(client: &KairosClient) -> Result<BTreeMap<String, String>, String>` — The bystander's full API-visible state, section by section.
- pub `changed_sections` function L485-503 — `( baseline: &BTreeMap<String, String>, current: &BTreeMap<String, String>, ) -> ...` — Compare a fresh snapshot against the baseline; returns the changed
- pub `HistoryBound` struct L511-515 — `{ short_code: String, version: i32, history_rows: i64 }` — Per-item history summary for the report.
- pub `history_bound_check` function L520-578 — `( client: &KairosClient, codes: &[String], max_rows_per_item: i64, ) -> (Vec<His...` — Assert `item_history` rows per item == item version (history grows
-  `World` type L53-57 — `= World` — version, and stays under a hard cap).
-  `setup_err` function L59-61 — `(stage: &str, e: impl std::fmt::Display) -> String` — version, and stays under a hard cap).
-  `canonical` function L379-386 — `(items: &[T]) -> Result<String, String>` — Serialize a list of JSON-serializable items into one canonical string
-  `tests` module L581-607 — `-` — version, and stays under a hard cap).
-  `changed_sections_attributes_differences` function L585-606 — `()` — version, and stays under a hard cap).

### crates/kairos-web/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/api.rs

- pub `get_json` function L40-48 — `(auth: Auth, path: &str) -> Result<T, ApiError>` — `GET {path}` with the bearer token; JSON-decode the body.
- pub `post_json` function L52-69 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, ApiError>` — `POST {path}` with a JSON body and the bearer token; JSON-decode the
- pub `patch_json` function L73-90 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, ApiError>` — `PATCH {path}` with a JSON body and the bearer token; JSON-decode the
- pub `put_json` function L93-110 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, ApiError>` — `PUT {path}` with a JSON body and the bearer token (KAIROS-T-0109).
- pub `delete_json` function L113-121 — `(auth: Auth, path: &str) -> Result<T, ApiError>` — `DELETE {path}` with the bearer token; JSON-decode the response body.
- pub `whoami` function L187-189 — `(auth: Auth) -> Result<Whoami, ApiError>` — `GET /api/whoami`.
- pub `Whoami` struct L193-202 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — mirror of: `kairos_server::app::WhoamiResponse` (partial).
- pub `WhoamiBoardCapabilities` struct L206-209 — `{ board_slug: String, grants: Vec<String> }` — mirror of: `kairos_server::app::WhoamiBoardCapabilities` (partial).
- pub `WhoamiUser` struct L215-219 — `{ id: String, display_name: String, email: String }` — mirror of: `kairos_server::app::WhoamiUser` (partial).
- pub `WhoamiOrganization` struct L223-226 — `{ slug: String, role: String }` — mirror of: `kairos_server::app::WhoamiOrganization` (partial).
- pub `WhoamiTeam` struct L232-236 — `{ id: String, slug: String, name: String }` — mirror of: `kairos_server::app::WhoamiTeam` (partial).
-  `decode_response` function L133-153 — `( auth: Auth, sent_token: Option<String>, path: &str, response: gloo_net::http::...` — The shared response tail of every `*_json` helper: 401 clears the
-  `error_from_response` function L157-169 — `(status: u16, response: gloo_net::http::Response) -> ApiError` — Map a non-2xx response onto [`ApiError`] via the S-0005 error envelope
-  `ErrorEnvelope` struct L173-175 — `{ error: ErrorBody }` — mirror of: `kairos_client::types::ErrorEnvelope` (S-0005).
-  `ErrorBody` struct L179-182 — `{ code: String, message: String }` — mirror of: `kairos_client::types::ErrorBody` (S-0005).
-  `tests` module L239-272 — `-` — login redirect); components never handle 401 themselves.
-  `whoami_mirror_decodes_server_shape` function L244-271 — `()` — The mirror decodes a real WhoamiResponse body (field-name lock).

#### crates/kairos-web/src/app.rs

- pub `App` function L48-85 — `() -> impl IntoView` — The application root: provides auth, injects the Aurora stylesheet,
- pub `BrandMark` function L290-298 — `() -> impl IntoView` — The Kairos brand mark — app-supplied (aurora ships no branding), drawn
-  `Shell` function L95-135 — `() -> impl IntoView` — The protected shell: aurora `AppShell` with the Kairos header
-  `MyTeamsNav` function L142-164 — `(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView` — "My teams" (KAIROS-T-0068): the caller's own teams from whoami, each
-  `GuardFallback` function L172-188 — `() -> impl IntoView` — The unauthenticated fallback for the protected shell: while a boot-time
-  `RedirectToIssuer` function L193-231 — `() -> impl IntoView` — The unauthenticated fallback: kick off the PKCE redirect (remembering
-  `NavLink` function L237-247 — `(#[prop(into)] href: String, #[prop(into)] label: String) -> impl IntoView` — One left-nav entry.
-  `WhoamiBadge` function L253-269 — `(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView` — Who am I, which org, which role — the A-0015 whoami display.
-  `LogoutButton` function L274-285 — `() -> impl IntoView` — Drop the in-memory session; the shell guard (which sees the explicit

#### crates/kairos-web/src/auth.rs

- pub `TOKEN_RELAY_PATH` variable L47 — `: &str` — Same-origin path of the server's token relay (see `kairos-server::web`).
- pub `CONFIG_PATH` variable L49 — `: &str` — Same-origin path of the public SPA config endpoint.
- pub `ApiBearer` enum L72-78 — `AccessToken | IdToken` — Which token the SPA presents as the `/api` bearer (`api_bearer` in
- pub `AuthConfig` struct L82-95 — `{ issuer: String, client_id: String, authorization_endpoint: String, api_bearer:...` — What `GET /api/config` returns (mirror of `kairos-server::web`).
- pub `TokenResponse` struct L99-110 — `{ access_token: String, id_token: Option<String>, refresh_token: Option<String>,...` — A successful token response (authorization_code or refresh_token grant).
- pub `Session` struct L130-135 — `{ access_token: String, refresh_token: Option<String> }` — The in-memory session (A-0015: never persisted).
- pub `Auth` struct L140-153 — `{ session: RwSignal<Option<Session>>, config: RwSignal<Option<AuthConfig>>, sign...` — Reactive auth state, provided at the app root ([`provide_auth`]) and
- pub `provide_auth` function L160-177 — `() -> Auth` — Create the auth state and put it into context.
- pub `use_auth` function L196-198 — `() -> Auth` — The app-root [`Auth`] (panics outside the app tree — a bug by
- pub `is_authenticated` function L202-204 — `(&self) -> bool` — Reactive: is there a live session?
- pub `token` function L207-210 — `(&self) -> Option<String>` — Current bearer token (reactive).
- pub `signed_out` function L215-217 — `(&self) -> bool` — Reactive: did the user explicitly log out (vs.
- pub `restoring` function L221-223 — `(&self) -> bool` — Reactive: is the boot-time session restore (KAIROS-T-0071) still in
- pub `logout` function L229-234 — `(&self)` — Drop the in-memory session AND the stored refresh token (a logout
- pub `expire` function L241-245 — `(&self)` — Drop the session WITHOUT marking an explicit sign-out (refresh
- pub `fetch_auth_config` function L340-352 — `() -> Result<AuthConfig, String>` — `GET /api/config` — the SPA's discovery endpoint (decision documented
- pub `begin_login` function L357-389 — `(auth: Auth, return_to: &str) -> Result<(), String>` — Start the PKCE flow: stash verifier/state/return-path in
- pub `complete_login` function L393-438 — `(auth: Auth) -> Result<String, String>` — Handle `/callback`: verify `state`, exchange the code through the
-  `SCOPES` variable L53 — `: &str` — Scopes requested at login.
-  `REFRESH_MARGIN_SECS` variable L56 — `: f64` — Refresh the session this many seconds before the access token expires.
-  `KEY_VERIFIER` variable L60 — `: &str` — Workspace (KAIROS-T-0054).
-  `KEY_STATE` variable L61 — `: &str` — Workspace (KAIROS-T-0054).
-  `KEY_RETURN_TO` variable L62 — `: &str` — Workspace (KAIROS-T-0054).
-  `KEY_REFRESH` variable L65 — `: &str` — The refresh token (KAIROS-T-0071): per-tab reload survival.
-  `TokenResponse` type L112-126 — `= TokenResponse` — Workspace (KAIROS-T-0054).
-  `bearer_for` function L117-125 — `(&self, kind: ApiBearer) -> String` — The token to use as the `/api` bearer for the given selection.
-  `restore_session` function L184-192 — `(auth: Auth)` — Boot-time session restore (KAIROS-T-0071): run the refresh grant with
-  `Auth` type L200-336 — `= Auth` — Workspace (KAIROS-T-0054).
-  `install` function L253-289 — `(&self, tokens: TokenResponse)` — Install a token response and schedule the silent refresh.
-  `refresh` function L294-302 — `(self)` — The silent-refresh grant through the relay, from the live session's
-  `refresh_with` function L308-325 — `(self, refresh_token: &str)` — One refresh grant with an explicit token — shared by the in-session
-  `config_cached` function L328-335 — `(&self) -> Result<AuthConfig, String>` — `/api/config`, fetched once and cached in the signal.
-  `post_token` function L441-460 — `(body: &str) -> Result<TokenResponse, String>` — POST a form body to the token relay and parse the token response.
-  `window` function L464-466 — `() -> web_sys::Window` — Workspace (KAIROS-T-0054).
-  `origin` function L468-473 — `() -> Result<String, String>` — Workspace (KAIROS-T-0054).
-  `session_storage` function L475-481 — `() -> Result<web_sys::Storage, String>` — Workspace (KAIROS-T-0054).
-  `clear_stored_refresh` function L484-488 — `()` — Remove the stored refresh token (logout / dead token).
-  `url_encode` function L490-492 — `(value: &str) -> String` — Workspace (KAIROS-T-0054).
-  `random_urlsafe` function L496-504 — `(len: usize) -> Result<String, String>` — `len` random bytes from WebCrypto, base64url-encoded (RFC 7636 §4.1
-  `s256_base64url` function L507-520 — `(verifier: &str) -> Result<String, String>` — The S256 code challenge: `BASE64URL(SHA256(verifier))` (RFC 7636 §4.2).
-  `base64url` function L524-544 — `(bytes: &[u8]) -> String` — Base64url without padding (RFC 4648 §5).
-  `ALPHABET` variable L525 — `: &[u8; 64]` — Workspace (KAIROS-T-0054).
-  `form_encode` function L547-553 — `(pairs: &[(&str, &str)]) -> String` — `application/x-www-form-urlencoded` body from pairs.
-  `form_component` function L556-567 — `(value: &str) -> String` — Percent-encode one form value (conservative: everything but unreserved).
-  `tests` module L570-646 — `-` — Workspace (KAIROS-T-0054).
-  `tokens` function L573-580 — `(access: &str, id: Option<&str>) -> TokenResponse` — Workspace (KAIROS-T-0054).
-  `bearer_selection_follows_api_bearer` function L583-589 — `()` — Workspace (KAIROS-T-0054).
-  `id_token_mode_falls_back_when_absent` function L592-597 — `()` — Workspace (KAIROS-T-0054).
-  `api_bearer_defaults_to_access_token_when_absent` function L600-615 — `()` — Workspace (KAIROS-T-0054).
-  `base64url_matches_rfc4648_vectors` function L619-627 — `()` — RFC 4648 §10 test vectors, translated to base64url-no-padding.
-  `base64url_uses_urlsafe_alphabet` function L630-635 — `()` — Workspace (KAIROS-T-0054).
-  `form_encoding_escapes_reserved_characters` function L638-645 — `()` — Workspace (KAIROS-T-0054).

#### crates/kairos-web/src/lib.rs

- pub `api` module L17 — `-` — `kairos-server` at `/` (KAIROS-T-0039).
- pub `app` module L18 — `-` — - [`pages`] — one module per route; T-0040..T-0044 replace the stubs
- pub `auth` module L19 — `-` — - [`pages`] — one module per route; T-0040..T-0044 replace the stubs
- pub `pages` module L20 — `-` — - [`pages`] — one module per route; T-0040..T-0044 replace the stubs

#### crates/kairos-web/src/main.rs

-  `main` function L4-7 — `()` — app lives in the lib (`kairos_web::App`) so host builds typecheck it.

#### crates/kairos-web/src/pages.rs

- pub `LoginPage` function L26-63 — `() -> impl IntoView` — Explicit sign-in page: logout lands here; a button restarts PKCE.
- pub `CallbackPage` function L68-86 — `() -> impl IntoView` — PKCE redirect target: exchanges the code (via the server relay), then
- pub `admin` module L127 — `-` — later tasks should keep.
- pub `NotFoundPage` function L138-148 — `() -> impl IntoView` — Router fallback.
-  `copy_link` module L92 — `-` — later tasks should keep.
-  `boards` module L96 — `-` — later tasks should keep.
-  `item` module L101 — `-` — later tasks should keep.
-  `search` module L106 — `-` — later tasks should keep.
-  `editor` module L113 — `-` — later tasks should keep.
-  `teams` module L115 — `-` — later tasks should keep.
-  `repositories` module L122 — `-` — later tasks should keep.
-  `activity` module L133 — `-` — later tasks should keep.

### crates/kairos-web/src/pages

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/activity.rs

- pub `ListEnvelope` struct L66-71 — `{ items: Vec<T>, total: i64, limit: i64, offset: i64 }` — mirror of: `kairos_client::types::ListEnvelope<T>`.
- pub `HistoryVersion` struct L75-79 — `{ version: i32, edited_by: String, edited_at: String }` — mirror of: `kairos_client::types_meta::HistoryVersion`.
- pub `HistorySnapshot` struct L83-89 — `{ version: i32, title: String, content: String, edited_by: String, edited_at: St...` — mirror of: `kairos_client::types_meta::HistorySnapshot`.
- pub `ActivityEntry` struct L93-101 — `{ id: String, actor_id: String, action: String, entity_id: Option<String>, entit...` — mirror of: `kairos_client::types_meta::ActivityEntry`.
- pub `Member` struct L105-109 — `{ user_id: String, email: String, display_name: String }` — mirror of: `kairos_client::types_org::OrgMember` (partial).
- pub `ItemHead` struct L115-126 — `{ id: String, short_code: String, title: String, version: i32, archived_at: Opti...` — mirror of: the shared head of `kairos_client::types::{Strategy,
- pub `DiffLine` struct L208-211 — `{ sign: char, text: String }` — One rendered diff line: `+` inserted, `-` deleted, ` ` unchanged.
- pub `diff_lines` function L217-229 — `(old: &str, new: &str) -> Vec<DiffLine>` — Client-side line diff (KAIROS-T-0044 AC).
- pub `ActivityPage` function L354-572 — `() -> impl IntoView` — `/activity` — the filterable, paginated audit-trail feed.
- pub `ItemHistoryPage` function L701-989 — `() -> impl IntoView` — `/activity/history/:code` — version list, snapshot viewer, two-version
-  `PAGE_SIZE` variable L55 — `: i64` — Feed page size.
-  `ALL` variable L58 — `: &str` — The "no filter" option label shared by the actor/action selects.
-  `ACTIONS` variable L134-143 — `: &[&str]` — The `activity_log.action` vocabulary (KAIROS-A-0004; enforcement point
-  `family_of_short_code` function L147-163 — `(code: &str) -> Option<&'static str>` — Map a `{PREFIX}-{LETTER}-{NNNN}` short code (S-0004) onto its API
-  `encode_query` function L168-179 — `(value: &str) -> String` — Percent-encode one query-string value (conservative: everything but
-  `format_when` function L183-191 — `(rfc3339: &str) -> String` — `2026-07-14T23:10:11.123456Z` → `2026-07-14 23:10:11` (display only;
-  `action_color` function L195-204 — `(action: &str) -> &'static str` — Accent token for an activity action pill (data-driven color per the
-  `diff_line_style` function L232-242 — `(sign: char) -> String` — Style + color for one diff line (inline `var(--…)` per the token rule).
-  `actor_label` function L246-251 — `(members: &HashMap<String, String>, actor_id: &str) -> String` — Resolve an actor id to a display name via the members map, falling
-  `fetch_item_head` function L258-266 — `(auth: Auth, code: String) -> Result<(ItemHead, &'static str), ApiError>` — `GET /api/{family}/{code}` — current id/title/version head.
-  `fetch_versions` function L269-275 — `( auth: Auth, code: String, ) -> Result<ListEnvelope<HistoryVersion>, ApiError>` — `GET /api/{family}/{code}/history` — the version list, newest first.
-  `fetch_snapshot` function L278-289 — `( auth: Auth, code: String, version: i32, ) -> Result<HistorySnapshot, ApiError>` — `GET /api/{family}/{code}/history?version=N` — one full snapshot.
-  `fetch_members` function L293-296 — `(auth: Auth) -> Result<Vec<Member>, ApiError>` — `GET /api/members?limit=200` → actor_id → display name map (+ the raw
-  `fetch_directory` function L301-313 — `(auth: Auth) -> HashMap<String, String>` — Best-effort entity_id → short_code directory from the five family list
-  `FeedFilters` struct L317-323 — `{ entity_code: String, actor_id: String, action: String, since: String, offset: ...` — The applied activity-feed filters (what the resource fetches for).
-  `fetch_feed` function L327-346 — `( auth: Auth, filters: FeedFilters, ) -> Result<ListEnvelope<ActivityEntry>, Api...` — `GET /api/activity` with the S-0005 filters.
-  `member_option` function L575-577 — `(member: &Member) -> String` — The option label shown for one member in the actor filter.
-  `FeedTable` function L581-636 — `( page: ListEnvelope<ActivityEntry>, names: HashMap<String, String>, codes: Hash...` — The feed table (extracted so the async-state match stays readable).
-  `FeedPager` function L640-674 — `(page: ListEnvelope<ActivityEntry>, applied: RwSignal<FeedFilters>) -> impl Into...` — Offset pagination controls under the feed table.
-  `DiffView` struct L682-687 — `{ from: i32, to: i32, title_lines: Vec<DiffLine>, content_lines: Vec<DiffLine> }` — A computed two-version diff, ready to render.
-  `RollbackNotice` enum L691-695 — `Done | Conflict | Failed` — The outcome banner state after a rollback attempt.
-  `rollback` function L994-1004 — `(auth: Auth, code: String, version: i32) -> Result<i32, ApiError>` — The A-0004 copy-forward rollback: old snapshot → standard versioned
-  `VersionsTable` function L1008-1093 — `( page: ListEnvelope<HistoryVersion>, names: HashMap<String, String>, current: O...` — The version-list table: view / diff-select / rollback per row.
-  `DiffPanel` function L1097-1139 — `(view_model: DiffView) -> impl IntoView` — Rendered diff between the two selected versions.
-  `tests` module L1146-1325 — `-` — handled exactly like any other content edit.
-  `short_code_family_mapping` function L1152-1175 — `()` — Letter → family mapping covers all five S-0004 types and rejects
-  `query_encoding_escapes_reserved` function L1180-1190 — `()` — Query values are percent-encoded so RFC 3339 `+00:00` offsets and
-  `when_formatting` function L1194-1201 — `()` — Timestamps render as date + clock; non-timestamps pass through.
-  `action_colors_cover_the_vocabulary` function L1206-1211 — `()` — Every documented activity action gets a deliberate accent; unknown
-  `diff_lines_marks_changes` function L1216-1232 — `()` — The `similar` line diff marks inserts/deletes/context the way the
-  `history_mirrors_decode_server_shape` function L1237-1260 — `()` — mirror decode lock: history version list envelope (server shape from
-  `activity_and_head_mirrors_decode_server_shape` function L1265-1312 — `()` — mirror decode lock: activity entries (nullable entity fields) and
-  `actor_labels_resolve_or_shorten` function L1316-1324 — `()` — Actor labels prefer the members map and degrade to a shortened id.

#### crates/kairos-web/src/pages/admin.rs

- pub `AdminPage` function L66-93 — `() -> impl IntoView` — The admin section shell: whoami-probed role gate, section tabs, and the
- pub `AdminNavLink` function L125-152 — `() -> impl IntoView` — The left-nav "Admin" entry, rendered only when whoami says the caller
- pub `AdminHomePage` function L202-258 — `() -> impl IntoView` — `/admin` — the overview: one card per admin surface.
-  `gating` module L42 — `-` — gate is UX, the server is the authority (A-0006).
-  `api` module L44 — `-` — gate is UX, the server is the authority (A-0006).
-  `boards` module L45 — `-` — gate is UX, the server is the authority (A-0006).
-  `capabilities` module L46 — `-` — gate is UX, the server is the authority (A-0006).
-  `members` module L47 — `-` — gate is UX, the server is the authority (A-0006).
-  `metadata` module L48 — `-` — gate is UX, the server is the authority (A-0006).
-  `repositories` module L49 — `-` — gate is UX, the server is the authority (A-0006).
-  `streams` module L50 — `-` — gate is UX, the server is the authority (A-0006).
-  `teams` module L51 — `-` — gate is UX, the server is the authority (A-0006).
-  `templates` module L52 — `-` — gate is UX, the server is the authority (A-0006).
-  `NotAdminGate` function L100-119 — `(role: String) -> impl IntoView` — The graceful denied path: what a plain `member` with no board-config
-  `SectionTabs` function L160-195 — `(is_admin: bool) -> impl IntoView` — Horizontal section tabs for the admin area (reuses the nav-link styling
-  `MutationOutcome` type L267 — `= Option<Result<String, aurora_dark::tokens::ApiError>>` — Outcome of the latest mutation in a panel: `Ok(what happened)` or the
-  `MutationNotice` function L273-296 — `(outcome: RwSignal<MutationOutcome>) -> impl IntoView` — Renders the latest mutation outcome per the conventions: success as a
-  `run_mutation` function L302-326 — `( busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, reload: RwSignal<u32...` — Run one admin mutation: guard against double-submit with `busy`, record

#### crates/kairos-web/src/pages/boards.rs

- pub `BoardsPage` function L430-505 — `() -> impl IntoView` — `/boards` — boards in flight-level bands (strategy above initiatives
- pub `BoardPage` function L694-807 — `() -> impl IntoView` — `/boards/:board` — columns from the board config, items grouped, a
-  `data` module L24 — `-` — view (`/boards/:board` — slug or id) with live `/ws/events` updates.
-  `live` module L25 — `-` — attach to strategies/initiatives/tasks only).
-  `describe` function L45-54 — `(error: &ApiError) -> String` — A short human line for a failed mutation (page-level `Banner`; load
-  `level_color` function L57-65 — `(level: &str) -> &'static str` — The accent token for a board level / entity kind.
-  `kind_color` function L67-74 — `(kind: EntityKind) -> &'static str` — attach to strategies/initiatives/tasks only).
-  `DragData` struct L82-91 — `{ kind: EntityKind, short_code: String, targets: Vec<String>, source_column: Str...` — The in-flight card drag (KAIROS-T-0064): which card, where it started,
-  `LANE_SUPPORT` variable L98 — `: &str` — The two board lanes: the lane is a projection of `tasks.work_class`.
-  `LANE_PLANNED` variable L99 — `: &str` — attach to strategies/initiatives/tasks only).
-  `card_lane` function L104-109 — `(work_class: Option<&str>) -> &'static str` — Which lane a card renders in: tasks follow their `work_class`;
-  `DropEffect` struct L118-121 — `{ transition_to: Option<String>, set_work_class: Option<String> }` — What dropping the in-flight drag onto (column, lane) would do.
-  `drop_effect` function L123-145 — `(drag: &DragData, column_id: &str, lane: Option<&str>) -> Option<DropEffect>` — attach to strategies/initiatives/tasks only).
-  `BoardPowers` struct L156-163 — `{ transition: bool, create: bool, documents: bool }` — What the signed-in user may do on THIS board — mirrors the A-0006
-  `create_capability` function L166-173 — `(kind: EntityKind) -> &'static str` — The `manage_*` capability that creating this kind requires.
-  `grant_covers` function L178-184 — `(grant: &str, required: &str) -> bool` — Does a stored grant cover `required`? Client mirror of the A-0006
-  `team_implies` function L188-193 — `(required: &str) -> bool` — The KAIROS-T-0072 implied set (mirror of
-  `board_powers` function L198-226 — `( me: &crate::api::Whoami, board_slug: &str, board_team_id: Option<&str>, create...` — Compute [`BoardPowers`] from the whoami identity.
-  `holds_capability` function L241-265 — `( me: &crate::api::Whoami, board_slug: Option<&str>, board_team_id: Option<&str>...` — Does the signed-in user hold `required` where it counts for ONE item
-  `movable_delivery_boards` function L274-293 — `( me: &crate::api::Whoami, boards: &[data::Board], here_slug: &str, ) -> Vec<(St...` — The delivery boards a task may be moved TO from `here_slug`: every
-  `run_drop` function L301-324 — `( auth: crate::auth::Auth, kind: EntityKind, code: String, effect: DropEffect, o...` — Apply one [`DropEffect`] and report through the standard board
-  `LEVEL_BANDS` variable L332-337 — `: &[(&str, &str)]` — The flight-level band order for `/boards` (KAIROS-T-0069/T-0063:
-  `BoardGroup` type L342 — `= (Option<(String, String)>, Vec<data::Board>)` — One board-list group: `(Some((heading, /teams/:slug href)), boards)`
-  `BandModel` struct L346-350 — `{ level: String, label: String, groups: Vec<BoardGroup> }` — One rendered board-list band: level heading + its tiles, with the
-  `band_models` function L356-424 — `( boards: Vec<data::Board>, teams: &[crate::pages::teams::api::Team], ) -> Vec<B...` — Bucket boards into level bands (strategy → initiative → delivery →
-  `CardModel` struct L516-531 — `{ kind: EntityKind, short_code: String, title: String, meta: Vec<(String, &'stat...` — One card's owned view model with a content-fingerprint `key`
-  `ColumnModel` struct L537-543 — `{ id: String, name: String, targets: Vec<(String, String)>, cards: Vec<CardModel...` — One column's owned view model.
-  `column_models` function L547-662 — `(view: &data::BoardView) -> Vec<ColumnModel>` — Flatten the wire shape into owned, keyed view models (leptos children
-  `doc_parent_options` function L666-689 — `(view: &data::BoardView) -> Vec<(String, String)>` — `(short_code, title)` of the board's document-parent candidates
-  `BoardBody` function L818-1111 — `( /// The live board view model. ALWAYS `Some` while this component is /// mount...` — The loaded board: header (+ create actions) and the column row.
-  `LaneSection` function L1122-1159 — `( lane: &'static str, label: &'static str, caption: &'static str, color: &'stati...` — One horizontal lane on a delivery board: accent header with a live
-  `RepoLane` enum L1163-1170 — `Any | Slug | Unbound` — Which repository a lane shows (KAIROS-T-0109).
-  `RepoLane` type L1172-1180 — `= RepoLane` — attach to strategies/initiatives/tasks only).
-  `admits` function L1173-1179 — `(&self, card: &CardModel) -> bool` — attach to strategies/initiatives/tasks only).
-  `parse_repo_query` function L1189-1201 — `(query: Option<&str>) -> Vec<String>` — Parse `?repo=a,b` into the requested slugs: empty segments dropped,
-  `prune_repos` function L1206-1212 — `(requested: &[String], known: &[String]) -> Vec<String>` — The effective selection: the requested slugs that are actually on the
-  `lens_admits` function L1216-1223 — `(card: &CardModel, selected: &[String]) -> bool` — Does the lens admit this card? The filter narrows TASKS only — other
-  `LaneKey` type L1227 — `= Option<String>` — A group-by-repository lane's identity: `Some(slug)` or the unbound
-  `repo_lanes` function L1232-1240 — `(selected: &[String], known: &[String]) -> Vec<LaneKey>` — The group-by-repository lanes: one per EFFECTIVE selection (every
-  `RepoLaneSection` function L1246-1285 — `( repo: Option<String>, columns: Memo<Vec<ColumnModel>>, drag: RwSignal<Option<D...` — One repository lane on a delivery board (KAIROS-T-0109): the columns
-  `LaneColumns` function L1292-1409 — `( lane: Option<&'static str>, /// Repository narrowing for the group-by-repo vie...` — The column row (KAIROS-T-0040/T-0074 fine-grained rendering), filtered
-  `ItemCard` function L1421-1580 — `( kind: EntityKind, short_code: String, title: String, /// `(label, color-token)...` — One board card: short code (the detail link, KAIROS-T-0076) with its
-  `CreateItemModal` function L1591-1768 — `( open: RwSignal<bool>, kind: EntityKind, board_id: String, /// Delivery boards ...` — The global create flow (KAIROS-T-0062): the board level's entity type
-  `CreateDocumentModal` function L1774-1899 — `( open: RwSignal<bool>, /// `(short_code, title)` of this board's eligible paren...` — "New document" (board header): template picker + parent picker.
-  `tests` module L1902-2296 — `-` — attach to strategies/initiatives/tasks only).
-  `board` function L1906-1914 — `(id: &str, level: &str, team_id: Option<&str>) -> data::Board` — attach to strategies/initiatives/tasks only).
-  `team` function L1916-1924 — `(id: &str, slug: &str) -> Team` — attach to strategies/initiatives/tasks only).
-  `band_models_orders_levels_and_groups_delivery_by_team` function L1929-1960 — `()` — Bands come out in flight-level order, the delivery band grouped by
-  `me` function L1962-1974 — `(role: &str, team_ids: &[&str], grants: &[(&str, &[&str])]) -> crate::api::Whoam...` — attach to strategies/initiatives/tasks only).
-  `board_powers_mirror_team_implication` function L1979-1999 — `()` — KAIROS-T-0072 client mirror: team membership implies the delivery
-  `board_powers_mirror_grants_and_admin` function L2003-2019 — `()` — Explicit grants (incl.
-  `holds_capability_answers_per_item` function L2025-2070 — `()` — KAIROS-T-0164: the Restore affordance asks per ITEM, and an item
-  `movable_delivery_boards_are_the_other_manageable_ones` function L2077-2104 — `()` — KAIROS-I-0012: the move picker offers the OTHER delivery boards
-  `card_lane_projects_work_class` function L2109-2113 — `()` — KAIROS-T-0077: the lane is a pure projection of work_class;
-  `drop_effect_decides_column_and_lane_moves` function L2120-2164 — `()` — KAIROS-T-0077 drop semantics: same-column cross-lane = lane write
-  `card` function L2166-2178 — `(kind: EntityKind, repository: Option<&str>) -> CardModel` — attach to strategies/initiatives/tasks only).
-  `slugs` function L2180-2182 — `(list: &[&str]) -> Vec<String>` — attach to strategies/initiatives/tasks only).
-  `selected_repos_parse_and_prune_against_the_board` function L2189-2207 — `()` — KAIROS-T-0114: `?repo=` parses to a deduplicated slug list and the
-  `repo_lane_admits_by_binding` function L2212-2230 — `()` — KAIROS-T-0109: a repo lane admits exactly its tasks; the unbound
-  `lens_filter_narrows_tasks_only` function L2235-2253 — `()` — KAIROS-T-0109: the lens retain filter narrows TASKS only — an
-  `repo_lanes_follow_the_effective_selection` function L2259-2280 — `()` — KAIROS-T-0114: group-by lanes come from the EFFECTIVE selection
-  `band_models_keeps_unknown_team_boards_reachable` function L2285-2295 — `()` — A board whose team id names an unknown team lands in "No team"

#### crates/kairos-web/src/pages/copy_link.rs

-  `item_url` function L12-15 — `(code: &str) -> Option<String>` — The absolute detail URL for an item, from the current origin.
-  `clipboard` function L19-22 — `() -> Option<web_sys::Clipboard>` — The Clipboard handle — `None` outside secure contexts, where the
-  `CopyLinkButton` function L27-66 — `(#[prop(into)] code: String) -> impl IntoView` — Copies `/items/{code}` (absolute) to the clipboard; flashes ✓ on

#### crates/kairos-web/src/pages/editor.rs

- pub `SaveFuture` type L35 — `= Pin<Box<dyn Future<Output = Result<i32, SaveError>>>>` — One save attempt: `(title, content, based-on version)` → the new
- pub `Saver` type L37 — `= Rc<dyn Fn(String, String, i32) -> SaveFuture>` — The domain-specific save call (item content PATCH, team-page PATCH…).
- pub `MarkdownEditor` function L54-349 — `( #[prop(into)] initial_title: String, #[prop(into)] initial_content: String, in...` — The editable content panel.
-  `TOOLBAR` variable L41-48 — `: &[(&str, &str, &str, &str, &str)]` — The toolbar's insertions: `(label, tooltip, prefix, suffix,

#### crates/kairos-web/src/pages/item.rs

- pub `ItemPage` function L61-122 — `() -> impl IntoView` — `/items/:code` — parse the family from the short code and hand off.
-  `api` module L19 — `-` — entity families (the short code's type letter picks the family — see
-  `create_doc` module L20 — `-` — warning ([`delete`], A-0001).
-  `delete` module L21 — `-` — warning ([`delete`], A-0001).
-  `editor` module L22 — `-` — warning ([`delete`], A-0001).
-  `markdown` module L23 — `-` — warning ([`delete`], A-0001).
-  `metadata` module L24 — `-` — warning ([`delete`], A-0001).
-  `tab_hrefs` function L49-57 — `(code: &str) -> Option<(String, String)>` — The Details | Graph tab targets for a resolved short code — `None`
-  `ItemDetailView` function L127-173 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — The detail resource + the four async view states.
-  `ItemLoaded` function L178-326 — `( item: ItemDetail, family: Family, on_saved: Callback<i32>, on_moved: Callback<...` — The loaded page: header + actions, the editor column, and the facts /
-  `manage_capability` function L333-341 — `(family: Family) -> &'static str` — The capability that putting an item BACK asks for: the same
-  `restore_power` function L349-370 — `( family: Family, board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>...` — May the signed-in user restore THIS item? The shared whoami mirror
-  `put_away_when` function L375-383 — `(rfc3339: &str) -> String` — `2026-09-23T11:30:07.479107Z` → `2026-09-23 11:30 UTC` (display only;
-  `ArchivedBanner` function L398-451 — `( family: Family, #[prop(into)] code: String, /// The entity UUID — the activi...` — The unmistakable marker on an archived item (KAIROS-T-0164, ADR-20).
-  `RestoreControl` function L458-550 — `( family: Family, #[prop(into)] code: String, can_restore: Memo<bool>, on_restor...` — The Restore action (KAIROS-T-0160's endpoint): visible only to someone
-  `lifecycle_color` function L555-561 — `(state: &str) -> &'static str` — The badge color of a document lifecycle state (KAIROS-T-0078):
-  `LifecyclePanel` function L568-629 — `( #[prop(into)] code: String, current: String, /// The document is archived in t...` — The lifecycle control (KAIROS-T-0078, documents only): a
-  `ChildrenProgressBar` function L637-676 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — The children rollup under the header (KAIROS-T-0080): a segmented bar
-  `TypeFacts` function L680-719 — `(item: ItemDetail) -> impl IntoView` — The type-specific facts as pills (each family's extra columns).
-  `BoardPanel` function L727-830 — `( family: Family, #[prop(into)] code: String, /// The page's shared board read (...` — Board/column display: documents never sit on boards; ADRs may not; the
-  `board_power` function L837-858 — `( board_slug: String, team_id: Option<String>, kind: Option<boards::data::Entity...` — One board power as a memo over the shell's shared whoami identity
-  `RepositoryControl` function L866-1007 — `( code: String, /// The board's slug — for the client-side capability mirror. ...` — The task's repository binding (KAIROS-T-0109, A-0019): pick one of the
-  `THIS_BOARD` variable L1012 — `: &str` — The board picker's "stay put" option — the default, so a board move is
-  `MoveBoardControl` function L1026-1129 — `( code: String, /// The board the task sits on now — the source half of the ru...` — Move a task to another DELIVERY board (KAIROS-I-0012 D2): a team
-  `MoveControl` function L1138-1286 — `( family: Family, code: String, board: api::BoardInfo, column_id: Option<String>...` — The keyboard-accessible transition path (KAIROS-T-0075) plus the lane
-  `RelationshipsPanel` function L1291-1329 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — Relationships summary: both directions, grouped, every neighbor linked
-  `link_state_color` function L1334-1341 — `(state: &str) -> &'static str` — The accent for a forge link's state (KAIROS-T-0100).
-  `DevelopmentPanel` function L1347-1418 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — Branches and pull/merge requests for this item (KAIROS-T-0100).
-  `relationship_label` function L1425-1444 — `(relationship: &str, outgoing: bool) -> String` — The human name of one relationship group, read from THIS item's side —
-  `RelationshipGroupView` function L1448-1468 — `( group: RelationshipGroup, #[prop(into)] direction: String, ) -> impl IntoView` — One direction of one relationship type, neighbors linked.
-  `tests` module L1471-1524 — `-` — warning ([`delete`], A-0001).
-  `tab_hrefs_never_emit_an_empty_code` function L1478-1483 — `()` — KAIROS-T-0124 #2: the tab anchors always carry the code — an empty
-  `put_away_when_reads_as_a_moment` function L1489-1499 — `()` — The banner says WHEN, in something a person reads (KAIROS-T-0164):
-  `restore_asks_for_the_archive_capability` function L1504-1510 — `()` — Restoring asks for the same `manage_<family>` the archive asked
-  `relationship_labels_read_from_the_items_side` function L1515-1523 — `()` — The summary reads from THIS item's side: an outgoing parent edge

#### crates/kairos-web/src/pages/repositories.rs

-  `api` module L10 — `-` — layer for repository-scoped work.

#### crates/kairos-web/src/pages/search.rs

- pub `data` module L12 — `-` — one text query + a structured filter builder + an optional graph
- pub `graph` module L13 — `-` — component is standalone so the item-detail task can also embed it.
- pub `graph_layout` module L14 — `-` — component is standalone so the item-detail task can also embed it.
- pub `relationships` module L15 — `-` — component is standalone so the item-detail task can also embed it.
- pub `SearchPage` function L63-452 — `() -> impl IntoView` — `/search` — the unified search page.
-  `ENTITY_TYPES` variable L30 — `: [&str; 5]` — The entity-type vocabulary (S-0005 `filter.entity_type`).
-  `TASK_TYPES` variable L33 — `: [&str; 3]` — The task-type vocabulary (S-0005 `filter.task_type`).
-  `RELATIONSHIPS` variable L36 — `: [&str; 5]` — The relationship vocabulary (S-0005 `traverse.relationships`).
-  `ANY` variable L39 — `: &str` — The "no board/column selected" option.
-  `entity_color` function L42-51 — `(entity_type: &str) -> &'static str` — A data-driven accent per entity type (token constants only).
-  `MetaRow` struct L55-59 — `{ id: usize, key: RwSignal<String>, value: RwSignal<String> }` — One dynamic metadata `key = value` filter row.
-  `ResultGroup` function L457-515 — `(entity: &'static str, title: &'static str, hits: Vec<data::Hit>) -> impl IntoVi...` — One entity-type result group: a panel with linked rows (omitted when

#### crates/kairos-web/src/pages/teams.rs

- pub `TeamsPage` function L62-119 — `() -> impl IntoView` — `/teams` — every team: name, type, member count, linking to detail.
- pub `TeamPage` function L246-276 — `() -> impl IntoView` — `/teams/:slug` — the fixed v1 landing layout (KAIROS-T-0085):
-  `api` module L18 — `-` — directory and the `/teams/:slug` detail — the member-readable answer to
-  `doc` module L19 — `-` — endpoint; stream counts are small at org scale).
-  `lifecycle_color` function L38-45 — `(state: &str) -> &'static str` — Lifecycle chip accent — the same mapping as the item detail's
-  `DirectoryRow` struct L53-58 — `{ name: String, slug: String, team_type: String, member_count: usize }` — One directory row, fully resolved before rendering.
-  `TeamView` struct L128-147 — `{ team: api::Team, members: Vec<api::TeamMember>, delivery_board: Option<(String...` — The fully-resolved `/teams/:slug` view model (fetched as one unit so
-  `load_delivery_board` function L151-163 — `( auth: crate::auth::Auth, delivery_board_id: Option<&str>, ) -> Result<Option<(...` — The team's delivery board as `(name, slug)`, resolved through the
-  `load_streams` function L169-187 — `( auth: crate::auth::Auth, team_id: &str, ) -> Result<Vec<api::DeliveryStream>, ...` — Stream membership has no reverse endpoint: check each stream's team
-  `load_team_view` function L194-240 — `( auth: crate::auth::Auth, slug: &str, ) -> Result<TeamView, aurora_dark::tokens...` — Load everything the detail page shows, resolving the slug through
-  `TeamBody` function L280-513 — `(view_model: TeamView, on_changed: Callback<()>) -> impl IntoView` — The loaded team detail — panels in the fixed v1 order.
-  `link_state_color` function L517-524 — `(state: &str) -> &'static str` — The accent for a forge link's state — same mapping as the item
-  `AnnouncementsPanel` function L535-672 — `( team_id: String, announcements: Vec<api::Announcement>, on_changed: Callback<(...` — Pinned-first feed with a member/org-admin post box and an author-or-
-  `DocTree` function L682-692 — `(pages: Vec<api::TeamPageNode>, team_slug: String) -> impl IntoView` — The page-tree navigator: root nodes except the charter (it has its own
-  `render_tree_level` function L697-747 — `( pages: &[api::TeamPageNode], parent: Option<&str>, team_slug: &str, path_prefi...` — One nesting level: `parent`'s children in position-then-title order

### crates/kairos-web/src/pages/admin

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/admin/api.rs

- pub `ListEnvelope` struct L22-24 — `{ items: Vec<T> }` — mirror of: `kairos_client::types::ListEnvelope<T>` (partial — the admin
- pub `Board` struct L35-41 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — mirror of: `kairos_client::types_org::Board` (partial).
- pub `BoardColumn` struct L45-53 — `{ id: String, name: String, position: i32, is_done: bool }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `BoardTransition` struct L57-61 — `{ id: String, from_column_id: String, to_column_id: String }` — mirror of: `kairos_client::types_org::BoardTransition` (partial).
- pub `BoardDetail` struct L66-71 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — mirror of: `kairos_client::types_org::BoardDetail` (partial; the board
- pub `list_boards` function L74-77 — `(auth: Auth) -> Result<Vec<Board>, ApiError>` — `GET /api/boards` (first page; admin scale).
- pub `create_board` function L80-98 — `( auth: Auth, name: &str, slug: &str, board_level: &str, team_id: Option<&str>, ...` — `POST /api/boards` — seeded with the level's default columns/transitions.
- pub `delete_board` function L101-103 — `(auth: Auth, board_id: &str) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}` (422 `BOARD_NOT_EMPTY` while items reference it).
- pub `board_detail` function L106-108 — `(auth: Auth, board_id: &str) -> Result<BoardDetail, ApiError>` — `GET /api/boards/{id}` — board + columns + transitions.
- pub `add_column` function L112-124 — `( auth: Auth, board_id: &str, name: &str, position: i32, ) -> Result<Value, ApiE...` — `POST /api/boards/{id}/columns` (422 `DUPLICATE_COLUMN_NAME` /
- pub `update_column` function L128-142 — `( auth: Auth, board_id: &str, column_id: &str, name: Option<&str>, position: Opt...` — `PATCH /api/boards/{id}/columns/{col_id}` — rename, move, and/or set
- pub `remove_column` function L146-148 — `(auth: Auth, board_id: &str, column_id: &str) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/columns/{col_id}` (422 `COLUMN_NOT_EMPTY` while
- pub `add_transition` function L152-164 — `( auth: Auth, board_id: &str, from_column_id: &str, to_column_id: &str, ) -> Res...` — `POST /api/boards/{id}/transitions` (422 `DUPLICATE_TRANSITION` /
- pub `remove_transition` function L167-177 — `( auth: Auth, board_id: &str, transition_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/transitions/{transition_id}`.
- pub `BoardMember` struct L185-190 — `{ user_id: String, email: String, display_name: String, capabilities: Vec<String...` — mirror of: `kairos_client::types_org::BoardMember`.
- pub `board_members` function L193-195 — `(auth: Auth, board_id: &str) -> Result<Vec<BoardMember>, ApiError>` — `GET /api/boards/{id}/members`.
- pub `add_board_member` function L198-210 — `( auth: Auth, board_id: &str, user_id: &str, capabilities: &[String], ) -> Resul...` — `POST /api/boards/{id}/members` — first grant(s) for a user.
- pub `replace_capabilities` function L213-225 — `( auth: Auth, board_id: &str, user_id: &str, capabilities: &[String], ) -> Resul...` — `PATCH /api/boards/{id}/members/{user_id}` — replaces the full set.
- pub `remove_board_member` function L228-234 — `( auth: Auth, board_id: &str, user_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/members/{user_id}` — full revocation.
- pub `create_team` function L245-257 — `( auth: Auth, name: &str, slug: &str, team_type: &str, ) -> Result<Team, ApiErro...` — `POST /api/teams` — also creates the team's delivery board
- pub `update_team` function L260-273 — `( auth: Auth, team_id: &str, name: &str, slug: &str, team_type: &str, ) -> Resul...` — `PATCH /api/teams/{id}`.
- pub `delete_team` function L276-278 — `(auth: Auth, team_id: &str) -> Result<Value, ApiError>` — `DELETE /api/teams/{id}`.
- pub `add_team_member` function L281-288 — `(auth: Auth, team_id: &str, user_id: &str) -> Result<Value, ApiError>` — `POST /api/teams/{id}/members`.
- pub `remove_team_member` function L291-297 — `( auth: Auth, team_id: &str, user_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/teams/{id}/members/{user_id}`.
- pub `create_stream` function L306-318 — `( auth: Auth, name: &str, slug: &str, description: Option<&str>, ) -> Result<Val...` — `POST /api/delivery-streams`.
- pub `update_stream` function L321-334 — `( auth: Auth, stream_id: &str, name: &str, slug: &str, description: Option<&str>...` — `PATCH /api/delivery-streams/{id}`.
- pub `delete_stream` function L337-339 — `(auth: Auth, stream_id: &str) -> Result<Value, ApiError>` — `DELETE /api/delivery-streams/{id}`.
- pub `add_stream_team` function L342-353 — `( auth: Auth, stream_id: &str, team_id: &str, ) -> Result<Value, ApiError>` — `POST /api/delivery-streams/{id}/teams`.
- pub `remove_stream_team` function L356-366 — `( auth: Auth, stream_id: &str, team_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
- pub `OrgMember` struct L374-379 — `{ user_id: String, email: String, display_name: String, role: String }` — mirror of: `kairos_client::types_org::OrgMember` (partial).
- pub `list_org_members` function L382-385 — `(auth: Auth) -> Result<Vec<OrgMember>, ApiError>` — `GET /api/members`.
- pub `add_org_member` function L389-396 — `(auth: Auth, email: &str, role: &str) -> Result<Value, ApiError>` — `POST /api/members` — resolve by email (users are JIT-provisioned at
- pub `set_org_member_role` function L400-407 — `(auth: Auth, user_id: &str, role: &str) -> Result<Value, ApiError>` — `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN` when
- pub `remove_org_member` function L410-412 — `(auth: Auth, user_id: &str) -> Result<Value, ApiError>` — `DELETE /api/members/{user_id}` (422 `LAST_ADMIN` for the only admin).
- pub `Template` struct L420-425 — `{ id: String, name: String, slug: String, is_system_default: bool }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateMetadataField` struct L429-433 — `{ slug: String, default_value: Option<String>, required: bool }` — mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
- pub `TemplateDetail` struct L437-444 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
- pub `TemplateMetadataEntry` struct L449-453 — `{ definition_slug: String, default_value: Option<String>, required: bool }` — One template ↔ definition association in a template write (the
- pub `list_templates` function L469-473 — `(auth: Auth) -> Result<Vec<Template>, ApiError>` — `GET /api/templates`.
- pub `template_detail` function L476-478 — `(auth: Auth, template_id: &str) -> Result<TemplateDetail, ApiError>` — `GET /api/templates/{id}` — template + its metadata associations.
- pub `create_template` function L481-499 — `( auth: Auth, name: &str, slug: &str, content: &str, metadata: &[TemplateMetadat...` — `POST /api/templates`.
- pub `update_template` function L502-521 — `( auth: Auth, template_id: &str, name: &str, slug: &str, content: &str, metadata...` — `PATCH /api/templates/{id}` — `metadata` replaces the association list.
- pub `delete_template` function L524-526 — `(auth: Auth, template_id: &str) -> Result<Value, ApiError>` — `DELETE /api/templates/{id}` (hard delete).
- pub `MetadataDefinition` struct L534-541 — `{ id: String, name: String, slug: String, field_type: String, is_system_default:...` — mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
- pub `list_definitions` function L544-548 — `(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError>` — `GET /api/metadata-definitions`.
- pub `create_definition` function L552-570 — `( auth: Auth, name: &str, slug: &str, field_type: &str, enum_options: &[String],...` — `POST /api/metadata-definitions` — `enum_options` required non-empty for
- pub `update_definition` function L574-587 — `( auth: Auth, definition_id: &str, name: &str, slug: &str, enum_options: Option<...` — `PATCH /api/metadata-definitions/{id}` — `enum_options` replaces the
- pub `delete_definition` function L590-592 — `(auth: Auth, definition_id: &str) -> Result<Value, ApiError>` — `DELETE /api/metadata-definitions/{id}` (hard delete).
- pub `CreatedForgeConnection` struct L681-685 — `{ id: String, webhook_url: String, webhook_secret: String }` — mirror of: `kairos_client::types_forge::CreatedForgeConnection` (partial —
- pub `RepositoryDetail` struct L690-693 — `{ connection_id: Option<String> }` — mirror of: `kairos_client::types_repositories::RepositoryDetail`
- pub `create_repository` function L697-721 — `( auth: Auth, slug: Option<&str>, forge: &str, repo_full_name: &str, repo_url: &...` — `POST /api/repositories`.
- pub `update_repository` function L724-751 — `( auth: Auth, reference: &str, slug: Option<&str>, repo_url: Option<&str>, defau...` — `PATCH /api/repositories/{slug}` — every field optional; `team` re-homes.
- pub `delete_repository` function L754-756 — `(auth: Auth, reference: &str) -> Result<Value, ApiError>` — `DELETE /api/repositories/{slug}` (409 while referenced).
- pub `repository_detail` function L759-761 — `(auth: Auth, reference: &str) -> Result<RepositoryDetail, ApiError>` — `GET /api/repositories/{slug}` — for the connection id.
- pub `connect_webhook` function L764-774 — `( auth: Auth, repository: &str, ) -> Result<CreatedForgeConnection, ApiError>` — `POST /api/forge-connections` — the secret is shown ONCE.
- pub `disconnect_webhook` function L777-779 — `(auth: Auth, connection_id: &str) -> Result<Value, ApiError>` — `DELETE /api/forge-connections/{id}` — disconnect (the repo stays).
-  `PAGE` variable L27 — `: &str` — Big-enough page for admin lists (server clamps to its own max).
-  `metadata_entries_json` function L455-466 — `(entries: &[TemplateMetadataEntry]) -> Vec<Value>` — decoded as `serde_json::Value` — refetch is the source of truth.
-  `tests` module L595-670 — `-` — decoded as `serde_json::Value` — refetch is the source of truth.
-  `board_detail_mirror_decodes_server_shape` function L601-622 — `()` — `BoardDetail` decodes the wire shape: board fields flattened at the
-  `board_member_mirror_decodes_server_shape` function L626-635 — `()` — `BoardMember` decodes the grants list (capability editor input).
-  `definition_mirror_decodes_server_shape` function L642-651 — `()` — `MetadataDefinition` decodes enum options in order.
-  `template_detail_mirror_decodes_server_shape` function L655-669 — `()` — `TemplateDetail` decodes the metadata association rows.

#### crates/kairos-web/src/pages/admin/boards.rs

- pub `AdminBoardsPage` function L28-176 — `() -> impl IntoView` — `/admin/boards` — every live board, plus create/delete.
- pub `AdminBoardPage` function L182-225 — `() -> impl IntoView` — `/admin/boards/:board` — one board's configuration: columns (add /
-  `LEVELS` variable L24 — `: [&str; 4]` — message plus the `code:` line — components never inspect statuses.
-  `ColumnsPanel` function L233-390 — `( board_id: String, columns: Vec<api::BoardColumn>, transitions: Vec<api::BoardT...` — Columns: position-ordered rows with rename / move / remove / done
-  `TransitionsPanel` function L394-501 — `( board_id: String, columns: Vec<api::BoardColumn>, transitions: Vec<api::BoardT...` — Transition edges: which column-to-column moves the board allows.
-  `MembersPanel` function L507-689 — `( board_id: String, busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, re...` — Board members and their A-0006 capability grants: list with pills, an

#### crates/kairos-web/src/pages/admin/capabilities.rs

- pub `SelectionFlags` struct L50-56 — `{ full_access: bool, manage_all: bool, configure_all: bool, singles: Vec<String>...` — The editor's state as plain booleans — the pure, testable core.
- pub `from_capabilities` function L63-77 — `(capabilities: &[String]) -> Self` — Parse a stored grant set (what `GET .../members` returns) into
- pub `compose_selection` function L83-102 — `(flags: &SelectionFlags) -> Vec<String>` — Flags → the capability list the API receives: `*` alone wins; family
- pub `EditorState` struct L107-121 — `{ full_access: RwSignal<bool>, manage_all: RwSignal<bool>, configure_all: RwSign...` — Reactive editor state: one signal per toggle (signals are Copy, so the
- pub `new` function L131-133 — `() -> Self` — Fresh editor, everything off (whitelist model: no grant by default).
- pub `from_capabilities` function L136-154 — `(capabilities: &[String]) -> Self` — Editor prefilled from a member's stored grants.
- pub `flags` function L171-182 — `(&self) -> SelectionFlags` — Snapshot the toggles into pure flags (untracked — call at submit).
- pub `selection` function L186-188 — `(&self) -> Vec<String>` — The capability list to send (empty means "nothing selected" — the
- pub `CapabilityEditor` function L209-305 — `(state: EditorState) -> impl IntoView` — The grant editor (see module docs for the presentation design).
- pub `CapabilityPills` function L310-328 — `(capabilities: Vec<String>) -> impl IntoView` — A member's stored grants as pills: `*` gold, family globs violet,
-  `SINGLES` variable L35-46 — `: &[&str]` — Everything the editor can emit besides the globs.
-  `SelectionFlags` type L58-78 — `= SelectionFlags` — is host-unit-tested; the component is a thin binding over it.
-  `EditorState` type L123-127 — `impl Default for EditorState` — is host-unit-tested; the component is a thin binding over it.
-  `default` function L124-126 — `() -> Self` — is host-unit-tested; the component is a thin binding over it.
-  `EditorState` type L129-189 — `= EditorState` — is host-unit-tested; the component is a thin binding over it.
-  `single_signal` function L156-168 — `(&self, name: &str) -> RwSignal<bool>` — is host-unit-tested; the component is a thin binding over it.
-  `CapabilityRow` function L194-205 — `( checked: RwSignal<bool>, #[prop(into)] label: String, #[prop(into)] name: Stri...` — One capability toggle row: switch + human label + the raw capability
-  `tests` module L331-390 — `-` — is host-unit-tested; the component is a thin binding over it.
-  `caps` function L334-336 — `(list: &[&str]) -> Vec<String>` — is host-unit-tested; the component is a thin binding over it.
-  `full_access_wins` function L340-345 — `()` — Full access always composes to exactly `["*"]`.
-  `family_glob_swallows_covered_singles` function L350-366 — `()` — Family globs travel as globs and swallow their covered singles —
-  `singles_round_trip` function L370-375 — `()` — Singles round-trip: stored grants → flags → the same set out.
-  `transition_glob_normalizes` function L379-382 — `()` — A stored `transition_*` glob normalizes to its only member.
-  `empty_selection_is_empty` function L387-389 — `()` — Nothing selected composes to the empty list (callers block the

#### crates/kairos-web/src/pages/admin/gating.rs

-  `BOARD_CONFIG_CAPS` variable L19 — `: [&str; 2]` — Capabilities that unlock the per-board admin configuration surface
-  `capability_matches` function L26-31 — `(granted: &str, required: &str) -> bool` — Does the stored grant `granted` satisfy the concrete `required`
-  `glob_match` function L35-53 — `(pattern: &str, text: &str) -> bool` — LIKE-style match: `*` in `pattern` matches any (possibly empty) sequence;
-  `holds_any` function L57-60 — `(caps: &[WhoamiBoardCapabilities], required: &str) -> bool` — Does the caller hold `required` on ANY board (whitelist — no grant, no
-  `is_org_admin` function L64-66 — `(me: &Whoami) -> bool` — Whether the caller is an org admin (the A-0006 bypass — full access to
-  `has_board_config` function L71-75 — `(me: &Whoami) -> bool` — Whether the caller holds a per-board configuration capability
-  `can_access` function L79-81 — `(me: &Whoami) -> bool` — May the caller reach the `/admin` section at all? Org admins always;
-  `tests` module L84-156 — `-` — what the UI offers.
-  `me` function L88-108 — `(role: &str, grants: &[(&str, &[&str])]) -> Whoami` — what the UI offers.
-  `glob_semantics_mirror_a0006` function L111-120 — `()` — what the UI offers.
-  `org_admin_can_access_everything` function L123-127 — `()` — what the UI offers.
-  `plain_member_without_grants_is_denied` function L130-134 — `()` — what the UI offers.
-  `member_with_board_config_grant_can_access_but_is_not_admin` function L137-155 — `()` — what the UI offers.

#### crates/kairos-web/src/pages/admin/members.rs

- pub `AdminMembersPage` function L20-132 — `() -> impl IntoView` — `/admin/members`.

#### crates/kairos-web/src/pages/admin/metadata.rs

- pub `AdminMetadataPage` function L65-145 — `() -> impl IntoView` — `/admin/metadata`.
-  `FIELD_TYPES` variable L17 — `: [&str; 3]` — options for enums and forbids them otherwise — 422 `VALIDATION`).
-  `EnumOptionsEditor` function L21-61 — `(options: RwSignal<Vec<String>>) -> impl IntoView` — A reusable enum-option list editor over one `RwSignal<Vec<String>>`.
-  `DefinitionRow` function L150-239 — `( definition: api::MetadataDefinition, busy: RwSignal<bool>, outcome: RwSignal<M...` — One definition row with inline edit (name/slug, and the option list for

#### crates/kairos-web/src/pages/admin/repositories.rs

- pub `AdminRepositoriesPage` function L32-178 — `() -> impl IntoView` — `/admin/repositories`.
-  `Secret` struct L24-28 — `{ slug: String, webhook_url: String, webhook_secret: String }` — What the operator pastes into the forge — shown once, then gone.
-  `RepositoryRow` function L184-352 — `( repo: api::Repository, team_options: Vec<api::Team>, busy: RwSignal<bool>, out...` — One repository row: identity and counts, inline edit (incl.

#### crates/kairos-web/src/pages/admin/streams.rs

- pub `AdminStreamsPage` function L16-85 — `() -> impl IntoView` — `/admin/streams`.
-  `StreamRow` function L89-185 — `( stream: api::DeliveryStream, busy: RwSignal<bool>, outcome: RwSignal<MutationO...` — One stream row: identity, inline edit, delete, expandable teams panel.
-  `StreamTeamsPanel` function L189-302 — `( stream_id: String, busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, r...` — Teams feeding one stream: list, add (team picker), remove.

#### crates/kairos-web/src/pages/admin/teams.rs

- pub `AdminTeamsPage` function L25-128 — `() -> impl IntoView` — `/admin/teams`.
-  `TEAM_TYPES` variable L16-21 — `: [&str; 4]` — the success notice surfaces it with a link into board configuration.
-  `TeamRow` function L133-228 — `( team: api::Team, busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, rel...` — One team row: identity + delivery-board link, inline edit, delete, and
-  `TeamMembersPanel` function L232-343 — `( team_id: String, busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, rel...` — Members of one team: list, add (org-member picker), remove.

#### crates/kairos-web/src/pages/admin/templates.rs

- pub `AdminTemplatesPage` function L88-188 — `() -> impl IntoView` — `/admin/templates`.
-  `EntryRow` struct L20-24 — `{ slug: RwSignal<String>, default_value: RwSignal<String>, required: RwSignal<bo...` — One editable metadata-association row (definition slug + default +
-  `EntryRow` type L26-43 — `= EntryRow` — slugs; the association list is replaced wholesale on save).
-  `new` function L27-33 — `(slug: &str, default_value: &str, required: bool) -> Self` — slugs; the association list is replaced wholesale on save).
-  `to_entry` function L35-42 — `(self) -> api::TemplateMetadataEntry` — slugs; the association list is replaced wholesale on save).
-  `AssociationsEditor` function L47-84 — `( rows: RwSignal<Vec<EntryRow>>, definition_slugs: Vec<String>, ) -> impl IntoVi...` — The metadata-association editor: rows of definition-slug pickers.
-  `TemplateRow` function L193-319 — `( template: api::Template, busy: RwSignal<bool>, outcome: RwSignal<MutationOutco...` — One template row: identity, edit (loads the full detail on open),

### crates/kairos-web/src/pages/boards

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/boards/data.rs

- pub `Board` struct L17-25 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — mirror of: `kairos_client::types_org::Board` (partial).
- pub `BoardColumn` struct L29-33 — `{ id: String, name: String, position: i32 }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `BoardTransition` struct L37-40 — `{ from_column_id: String, to_column_id: String }` — mirror of: `kairos_client::types_org::BoardTransition` (partial).
- pub `BoardDetail` struct L45-50 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — mirror of: `kairos_client::types_org::BoardDetail` (the board's own
- pub `BoardListEnvelope` struct L54-56 — `{ items: Vec<Board> }` — mirror of: `kairos_client::types::ListEnvelope<Board>` (partial).
- pub `Strategy` struct L62-65 — `{ short_code: String, title: String }` — mirror of: `kairos_client::types::Strategy` (partial — card fields).
- pub `Initiative` struct L69-78 — `{ short_code: String, title: String, complexity: Option<String>, is_bucket: bool...` — mirror of: `kairos_client::types::Initiative` (partial — card fields).
- pub `Task` struct L82-93 — `{ short_code: String, title: String, task_type: String, work_class: String, repo...` — mirror of: `kairos_client::types::Task` (partial — card fields).
- pub `Adr` struct L97-102 — `{ short_code: String, title: String, decision_date: Option<String> }` — mirror of: `kairos_client::types::Adr` (partial — card fields).
- pub `BoardColumnItems` struct L106-112 — `{ column: BoardColumn, strategies: Vec<Strategy>, initiatives: Vec<Initiative>, ...` — mirror of: `kairos_client::types_org::BoardColumnItems`.
- pub `BoardItemsResponse` struct L116-127 — `{ board: Board, columns: Vec<BoardColumnItems>, children_progress: std::collecti...` — mirror of: `kairos_client::types_org::BoardItemsResponse`.
- pub `BlocksCounts` struct L131-134 — `{ blocked_by: i64, blocks: i64 }` — mirror of: `kairos_client::types_org::BlocksCounts` (KAIROS-T-0091).
- pub `ProgressCounts` struct L138-144 — `{ done: i64, total: i64, has_done: bool }` — mirror of: `kairos_client::types_org::ProgressCounts` (KAIROS-T-0080).
- pub `Template` struct L150-153 — `{ id: String, name: String }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateListEnvelope` struct L157-159 — `{ items: Vec<Template> }` — mirror of: `kairos_client::types::ListEnvelope<Template>` (partial).
- pub `ThinEvent` struct L173-175 — `{ event: String }` — mirror of: `kairos_client::types_events::ThinEvent` (partial — the view
- pub `BoardView` struct L182-185 — `{ detail: BoardDetail, items: BoardItemsResponse }` — Everything the board view renders: configuration (columns +
- pub `list_boards` function L188-191 — `(auth: Auth) -> Result<Vec<Board>, ApiError>` — The board list (one page is plenty for v1 — the demo tenant has 5).
- pub `load_board_view` function L195-209 — `(auth: Auth, param: &str) -> Result<BoardView, ApiError>` — Resolve a route param (board slug, or id as a fallback) against the
- pub `list_templates` function L212-215 — `(auth: Auth) -> Result<Vec<Template>, ApiError>` — Templates for the document create flow.
- pub `EntityKind` enum L221-226 — `Strategy | Initiative | Task | Adr` — The four board-item entity kinds (documents are off-board, S-0005).
- pub `api_family` function L230-237 — `(self) -> &'static str` — The S-0005 URL family (`/api/{family}/{short_code}/…`).
- pub `label` function L240-247 — `(self) -> &'static str` — The card label.
- pub `for_board_level` function L251-259 — `(level: &str) -> Option<EntityKind>` — The entity type a board level's create flow produces
- pub `transition` function L270-279 — `( auth: Auth, kind: EntityKind, short_code: &str, to_column_id: &str, ) -> Resul...` — `POST /api/{family}/{short_code}/transition`.
- pub `set_work_class` function L290-298 — `( auth: Auth, short_code: &str, work_class: &str, ) -> Result<(), ApiError>` — `POST /api/tasks/{short_code}/work-class` — move a task between the
- pub `NewItem` struct L303-325 — `{ title: String, content: String, hypothesis: Option<String>, complexity: Option...` — The create-from-column form data; [`create_item`] maps it onto the
- pub `create_item` function L381-452 — `( auth: Auth, kind: EntityKind, board_id: &str, column_id: &str, item: &NewItem,...` — `POST /api/{family}` — create a board item in the given column.
- pub `create_document` function L466-483 — `( auth: Auth, title: &str, template_id: Option<&str>, parent_short_code: &str, )...` — `POST /api/documents` — create a document attached to a board item.
-  `EntityKind` type L228-260 — `= EntityKind` — every mirror carries a `mirror of:` line and a decode test).
-  `TransitionRequest` struct L264-266 — `{ to_column_id: &'a str }` — mirror of: `kairos_client::types::TransitionRequest`.
-  `SetWorkClassRequest` struct L283-285 — `{ work_class: &'a str }` — mirror of: `kairos_client::types::SetWorkClassRequest`.
-  `CreateStrategyRequest` struct L329-336 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, hypot...` — mirror of: `kairos_client::types::CreateStrategyRequest`.
-  `CreateInitiativeRequest` struct L340-347 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, compl...` — mirror of: `kairos_client::types::CreateInitiativeRequest`.
-  `CreateTaskRequest` struct L351-365 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, task_...` — mirror of: `kairos_client::types::CreateTaskRequest`.
-  `CreateAdrRequest` struct L369-378 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, decis...` — mirror of: `kairos_client::types::CreateAdrRequest`.
-  `CreateDocumentRequest` struct L456-463 — `{ title: &'a str, template_id: Option<&'a str>, parent_short_code: &'a str }` — mirror of: `kairos_client::types::CreateDocumentRequest`.
-  `tests` module L486-694 — `-` — every mirror carries a `mirror of:` line and a decode test).
-  `board_detail_mirror_decodes_server_shape` function L493-520 — `()` — The board-view mirrors decode a realistic
-  `board_items_mirror_decodes_server_shape` function L524-579 — `()` — The grouped-items mirror decodes all four entity types.
-  `template_and_event_mirrors_decode` function L583-618 — `()` — The template + event mirrors decode their wire shapes.
-  `create_requests_serialize_wire_shape` function L623-673 — `()` — Create requests serialize with the exact S-0005 field names and
-  `entity_kind_per_board_level` function L677-693 — `()` — Board level → create-flow entity kind (A-0002 one-family-per-level).

#### crates/kairos-web/src/pages/boards/live.rs

- pub `LiveBoardGuard` struct L78-80 — `{ state: Rc<Live> }` — Dropping this closes the socket and stops the reconnect chain.
- pub `subscribe_board_events` function L101-107 — `( auth: Auth, board_id: String, refetch: impl Fn() + 'static, ) -> LiveBoardGuar...` — Subscribe the board view to `/ws/events`, filtered to `board_id`.
- pub `subscribe_all_events` function L112-117 — `( auth: Auth, on_event: impl Fn(Option<&str>) + 'static, ) -> LiveBoardGuard` — Subscribe to the WHOLE tenant stream (no board filter): the graph
-  `BACKOFF_INITIAL_MS` variable L38 — `: u64` — Reconnect backoff bounds (milliseconds).
-  `BACKOFF_MAX_MS` variable L39 — `: u64` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `Handler` type L42 — `= RefCell<Option<Closure<T>>>` — A stored wasm event-handler closure (present while a socket is live).
-  `Refetch` type L46 — `= Box<dyn Fn(Option<&str>)>` — The consumer's refetch hook: called with the event's `short_code` when
-  `Live` struct L50-74 — `{ auth: Auth, board_id: Option<String>, refetch: Refetch, closed: Cell<bool>, at...` — The per-connection state shared by the event handlers and the
-  `LiveBoardGuard` type L82-97 — `impl Drop for LiveBoardGuard` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `drop` function L83-96 — `(&mut self)` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `subscribe` function L119-138 — `( auth: Auth, board_id: Option<String>, refetch: impl Fn(Option<&str>) + 'static...` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `events_url` function L143-149 — `(token: &str) -> Option<String>` — The `ws(s)://…/ws/events?access_token=…` URL for the current origin.
-  `connect` function L152-234 — `(state: Rc<Live>)` — Open one connection attempt and register its handlers.
-  `schedule_reconnect` function L237-245 — `(state: Rc<Live>)` — Queue the next connection attempt with exponential backoff.
-  `tests` module L248-261 — `-` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `backoff_doubles_and_caps` function L252-260 — `()` — Backoff doubles from 1s and caps at 15s (host-testable math for

### crates/kairos-web/src/pages/item

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/item/api.rs

- pub `Family` enum L34-40 — `Strategy | Initiative | Task | Document | Adr` — The five entity families, resolved from a short code's type letter
- pub `of_short_code` function L46-63 — `(code: &str) -> Option<Family>` — Parse the family out of a short code.
- pub `entity_type` function L67-75 — `(self) -> &'static str` — The singular entity-type name (KAIROS-T-0078 metadata scoping;
- pub `api_family` function L78-86 — `(self) -> &'static str` — The plural family segment used by every `/api/{family}` route.
- pub `label` function L89-97 — `(self) -> &'static str` — Human label for the page header.
- pub `is_workflow` function L101-103 — `(self) -> bool` — Workflow items (strategy/initiative/task) can parent a document
- pub `ItemDetail` struct L114-167 — `{ short_code: String, title: String, content: String, version: i32, board_id: Op...` — mirror of: `kairos_client::types::{Strategy,Initiative,Task,Document,Adr}`
- pub `BoardInfo` struct L173-182 — `{ name: String, slug: String, team_id: Option<String>, columns: Vec<BoardColumnI...` — mirror of: `kairos_client::types_org::BoardDetail` (partial).
- pub `BoardColumnInfo` struct L186-194 — `{ id: String, name: String, removed_at: Option<String> }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `BoardTransitionInfo` struct L198-201 — `{ from_column_id: String, to_column_id: String }` — mirror of: `kairos_client::types_org::BoardTransition` (partial).
- pub `Page` struct L206-208 — `{ items: Vec<T> }` — mirror of: `kairos_client::types::ListEnvelope` (partial — the page
- pub `ItemMetadata` struct L212-214 — `{ values: Vec<MetadataValue> }` — mirror of: `kairos_client::types_meta::ItemMetadataResponse` (partial).
- pub `MetadataValue` struct L218-221 — `{ slug: String, value: String }` — mirror of: `kairos_client::types_meta::MetadataValue` (partial).
- pub `MetadataDefinition` struct L225-232 — `{ name: String, slug: String, field_type: String, enum_options: Vec<String> }` — mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
- pub `ChildrenProgress` struct L237-245 — `{ total: i64, done: i64, has_done_columns: bool, by_column: Vec<ChildColumnProgr...` — mirror of: `kairos_client::types_meta::ChildrenProgressResponse`
- pub `ChildColumnProgress` struct L249-253 — `{ column_name: String, is_done: bool, count: i64 }` — mirror of: `kairos_client::types_meta::ChildColumnProgress` (partial).
- pub `ItemRelationships` struct L258-263 — `{ outgoing: Vec<RelationshipGroup>, incoming: Vec<RelationshipGroup> }` — mirror of: `kairos_client::types_meta::ItemRelationshipsResponse`
- pub `RelationshipGroup` struct L267-270 — `{ relationship: String, items: Vec<RelatedItem> }` — mirror of: `kairos_client::types_meta::RelationshipGroup` (partial).
- pub `RelatedItem` struct L274-278 — `{ short_code: String, entity_type: String, title: String }` — mirror of: `kairos_client::types_meta::RelatedItem` (partial).
- pub `TemplateSummary` struct L282-285 — `{ id: String, name: String }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateDetail` struct L289-295 — `{ id: String, name: String, content: String, metadata: Vec<TemplateField> }` — mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
- pub `TemplateField` struct L299-309 — `{ slug: String, name: String, field_type: String, enum_options: Vec<String>, def...` — mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
- pub `DeleteOutcome` struct L314-319 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — mirror of: `kairos_client::types::DeleteResponse` (the A-0001 soft
- pub `CascadePreview` struct L326-331 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — mirror of: `kairos_client::types::CascadePreviewResponse` (KAIROS-T-0051
- pub `CurrentVersion` struct L340-344 — `{ version: i32, title: String, content: String }` — The server-current entity carried by a 409 in `details.current`
- pub `SaveError` enum L377-383 — `Conflict | Api` — Outcome of a content save: a version conflict is not a dead end — it
- pub `fetch_item` function L390-392 — `(auth: Auth, family: Family, code: String) -> Result<ItemDetail, ApiError>` — `GET /api/{family}/{short_code}` → the entity, whichever family.
- pub `fetch_board` function L404-410 — `(auth: Auth, board_id: String) -> Result<BoardInfo, ApiError>` — `GET /api/boards/{id}?include_removed_columns=true` → board name/slug +
- pub `fetch_definitions` function L416-429 — `( auth: Auth, family: Family, ) -> Result<Vec<MetadataDefinition>, ApiError>` — `GET /api/metadata-definitions?entity_type=…` — ONLY the definitions
- pub `set_lifecycle` function L434-442 — `(auth: Auth, code: &str, lifecycle: &str) -> Result<(), ApiError>` — `PATCH /api/documents/{short_code}/lifecycle` — set the editorial
- pub `fetch_metadata` function L445-456 — `( auth: Auth, family: Family, code: String, ) -> Result<Vec<MetadataValue>, ApiE...` — `GET /api/{family}/{short_code}/metadata` → the item's current values.
- pub `fetch_children_progress` function L460-470 — `( auth: Auth, family: Family, code: String, ) -> Result<ChildrenProgress, ApiErr...` — `GET /api/{family}/{short_code}/children-progress` → the direct
- pub `fetch_relationships` function L474-484 — `( auth: Auth, family: Family, code: String, ) -> Result<ItemRelationships, ApiEr...` — `GET /api/{family}/{short_code}/relationships` → both directions,
- pub `fetch_cascade_preview` function L490-500 — `( auth: Auth, family: Family, code: String, ) -> Result<CascadePreview, ApiError...` — `GET /api/{family}/{short_code}/cascade-preview` → the AUTHORITATIVE
- pub `fetch_archived_by` function L531-552 — `(auth: Auth, item_id: String) -> Option<String>` — Who archived this item, best-effort (KAIROS-T-0164).
- pub `fetch_templates` function L555-558 — `(auth: Auth) -> Result<Vec<TemplateSummary>, ApiError>` — `GET /api/templates` → the picker's list.
- pub `fetch_template_detail` function L561-563 — `(auth: Auth, id: String) -> Result<TemplateDetail, ApiError>` — `GET /api/templates/{id}` → content preview + declared metadata fields.
- pub `update_content` function L581-596 — `( auth: Auth, family: Family, code: &str, title: &str, content: &str, version: i...` — `PATCH /api/{family}/{short_code}` — the A-0004 optimistic-concurrency
- pub `update_metadata` function L641-655 — `( auth: Auth, family: Family, code: &str, values: BTreeMap<String, Option<String...` — `PATCH /api/{family}/{short_code}/metadata`: definition slug → value
- pub `CreateDocumentBody` struct L661-665 — `{ title: String, template_id: String, parent_short_code: String }` — Body of `POST /api/documents` (mirror of:
- pub `create_document` function L669-674 — `( auth: Auth, body: &CreateDocumentBody, ) -> Result<ItemDetail, ApiError>` — `POST /api/documents` — create-from-template, attached to a workflow
- pub `move_task` function L691-698 — `(auth: Auth, code: &str, board: &str) -> Result<ItemDetail, ApiError>` — `POST /api/tasks/{short_code}/move` — re-home a task onto another
- pub `RestoreOutcome` struct L704-709 — `{ short_code: String, still_archived_count: i64, still_archived_short_codes: Vec...` — mirror of: `kairos_client::types::RestoreResponse` (KAIROS-T-0160).
- pub `RestoreError` enum L714-722 — `Blocked | Api` — Outcome of a restore attempt.
- pub `restore_item` function L728-767 — `( auth: Auth, family: Family, code: &str, ) -> Result<RestoreOutcome, RestoreErr...` — `POST /api/{family}/{short_code}/restore` — put an archived item back
- pub `delete_item` function L771-778 — `( auth: Auth, family: Family, code: &str, ) -> Result<DeleteOutcome, ApiError>` — `DELETE /api/{family}/{short_code}` — A-0001 soft delete; the response
- pub `error_text` function L782-795 — `(error: &ApiError) -> String` — One-line text for a *write* failure (loads use `<ErrorState/>`; writes
- pub `ItemLink` struct L883-898 — `{ kind: String, external_id: String, title: String, url: String, state: String, ...` — mirror of: `kairos_client::types_forge::ItemLink` (partial — the
- pub `fetch_links` function L902-908 — `( auth: Auth, family: Family, code: String, ) -> Result<Vec<ItemLink>, ApiError>` — `GET /api/{family}/{code}/links` — server-ordered (PRs first, newest
-  `Family` type L42-104 — `= Family` — flattening the conflict into an `ApiError`.
-  `DetailedErrorEnvelope` struct L349-351 — `{ error: DetailedErrorBody }` — mirror of: `kairos_client::types::ErrorEnvelope` — with `details`, which
-  `DetailedErrorBody` struct L355-360 — `{ code: String, message: String, details: ErrorDetails }` — mirror of: `kairos_client::types::ErrorBody` (partial, + details).
-  `ErrorDetails` struct L365-372 — `{ current: Option<CurrentVersion>, missing: Vec<String> }` — The structured extras this page understands (`current` on 409,
-  `Body` struct L436-438 — `{ lifecycle: &'a str }` — flattening the conflict into an `ApiError`.
-  `ArchiveEvent` struct L510-512 — `{ actor_id: String }` — mirror of: `kairos_client::types_meta::ActivityEntry` (partial — the
-  `MemberName` struct L516-519 — `{ user_id: String, display_name: String }` — mirror of: `kairos_client::types_org::OrgMember` (partial).
-  `UpdateContentBody` struct L572-576 — `{ title: &'a str, content: &'a str, version: i32 }` — Body of the content PATCH (mirror of:
-  `patch_versioned` function L602-637 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, SaveError>` — The A-0004 versioned PATCH, generically: 409 parses `details.current`
-  `Body` struct L648-650 — `{ values: BTreeMap<String, Option<String>> }` — flattening the conflict into an `ApiError`.
-  `MoveTaskBody` struct L680-682 — `{ board: &'a str }` — Body of `POST /api/tasks/{short_code}/move` (mirror of:
-  `Verb` enum L806-810 — `Post | Patch | Delete` — The verbs this module drives directly.
-  `send` function L813-836 — `( auth: Auth, verb: Verb, path: &str, body: Option<&B>, ) -> Result<gloo_net::ht...` — Build + send one authenticated JSON request; no status handling yet.
-  `send_json` function L840-858 — `( auth: Auth, verb: Verb, path: &str, body: Option<&B>, ) -> Result<T, ApiError>` — One authenticated JSON round-trip with the standard status handling
-  `error_from` function L862-874 — `(status: u16, response: gloo_net::http::Response) -> ApiError` — Non-2xx → `ApiError` via the S-0005 envelope (the `api.rs` mapping,
-  `tests` module L911-1209 — `-` — flattening the conflict into an `ApiError`.
-  `item_link_mirror_decodes_server_shape` function L916-933 — `()` — `ItemLink` decodes the KAIROS-T-0100 wire shape.
-  `family_parses_from_short_codes` function L938-954 — `()` — Short-code → family across all five letters, multi-segment
-  `item_mirror_decodes_task_shape` function L958-981 — `()` — The union mirror decodes a full Task body (field-name lock).
-  `board_mirror_decodes_transitions_and_team` function L987-1008 — `()` — The board mirror decodes the `GET /api/boards/{id}` shape the move
-  `item_mirror_decodes_document_shape` function L1012-1030 — `()` — The union mirror decodes a Document body (no board fields at all).
-  `conflict_envelope_extracts_current` function L1035-1069 — `()` — The 409 envelope parse finds `details.current` whether it is the
-  `move_body_serializes_and_response_carries_new_placement` function L1075-1101 — `()` — The move body carries the target board under the exact wire name
-  `item_mirror_decodes_the_archived_state` function L1107-1128 — `()` — KAIROS-T-0154/T-0164: an ARCHIVED item is served by short code
-  `board_mirror_decodes_a_removed_column` function L1135-1156 — `()` — KAIROS-T-0161/T-0164: with `include_removed_columns=true` the board
-  `restore_response_and_refusal_decode` function L1162-1192 — `()` — The restore contract (KAIROS-T-0160): the 200 reports what stayed
-  `metadata_body_serializes_null_clears` function L1197-1208 — `()` — The metadata PATCH body serializes `None` as JSON null (the A-0003
-  `Body` struct L1199-1201 — `{ values: BTreeMap<String, Option<String>> }` — flattening the conflict into an `ApiError`.

#### crates/kairos-web/src/pages/item/create_doc.rs

- pub `CreateDocumentDialog` function L22-48 — `( /// Short code of the workflow item that will parent the document. #[prop(into...` — The "New document" dialog.
-  `TemplatePicker` function L53-76 — `(#[prop(into)] parent_code: String) -> impl IntoView` — Template list + preview + create form (own component so its resources
-  `TemplateForm` function L81-213 — `( templates: Vec<TemplateSummary>, #[prop(into)] parent_code: String, ) -> impl ...` — The picker itself: select a template, preview it, name the document,

#### crates/kairos-web/src/pages/item/delete.rs

- pub `DeleteDialog` function L19-43 — `( family: Family, #[prop(into)] code: String, #[prop(into)] title: String, open:...` — The delete confirm dialog.
-  `DeleteFlow` function L48-168 — `( family: Family, #[prop(into)] code: String, #[prop(into)] title: String, open:...` — Confirm → delete → cascade report (own component so the children

#### crates/kairos-web/src/pages/item/editor.rs

- pub `ContentEditor` function L19-56 — `( family: Family, #[prop(into)] code: String, #[prop(into)] initial_title: Strin...` — The editable content panel.

#### crates/kairos-web/src/pages/item/markdown.rs

- pub `to_html` function L20-34 — `(source: &str) -> String` — Render markdown to HTML, escaping raw HTML events (XSS-safe for
-  `tests` module L37-65 — `-` — tables, code fences, links — renders normally.
-  `renders_commonmark_structure` function L41-47 — `()` — tables, code fences, links — renders normally.
-  `renders_tables_and_task_lists` function L50-54 — `()` — tables, code fences, links — renders normally.
-  `escapes_raw_html` function L58-64 — `()` — Raw HTML (block and inline) is escaped, never emitted as markup.

#### crates/kairos-web/src/pages/item/metadata.rs

- pub `MetadataPanel` function L36-83 — `( family: Family, #[prop(into)] code: String, /// The item is archived (KAIROS-T...` — The metadata panel: definitions + values fetched together, typed
-  `FieldRow` struct L27-31 — `{ definition: MetadataDefinition, draft: RwSignal<String>, original: String }` — One field's editing state: its definition, the live draft, and the
-  `ADD_PLACEHOLDER` variable L86 — `: &str` — The "add a field" picker's no-choice option.
-  `MetadataForm` function L92-276 — `( family: Family, #[prop(into)] code: String, definitions: Vec<MetadataDefinitio...` — The editors + save button, built fresh per fetch (drafts start at the
-  `FieldEditor` function L282-348 — `(row: FieldRow, read_only: bool) -> impl IntoView` — One typed editor row: label + the editor its `field_type` calls for.

### crates/kairos-web/src/pages/repositories

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/repositories/api.rs

- pub `NO_REPOSITORY` variable L14 — `: &str` — The repository pickers' "no repository" option value (item page and
- pub `RepositoryRef` struct L20-25 — `{ id: String, slug: String, repo_full_name: String }` — mirror of: `kairos_client::types_repositories::RepositoryRef` (partial —
- pub `RepositoryTeam` struct L29-33 — `{ id: String, slug: String, name: String }` — mirror of: `kairos_client::types_repositories::RepositoryTeam`.
- pub `Repository` struct L37-53 — `{ id: String, slug: String, forge: String, repo_full_name: String, repo_url: Str...` — mirror of: `kairos_client::types_repositories::Repository`.
- pub `list_repositories` function L56-65 — `( auth: Auth, team: Option<&str>, ) -> Result<Vec<Repository>, ApiError>` — `GET /api/repositories[?team=]` — the directory, optionally one team's.
- pub `set_repository` function L74-83 — `( auth: Auth, short_code: &str, repository: Option<&str>, ) -> Result<(), ApiErr...` — `PUT /api/tasks/{code}/repository` — bind, re-home, or clear (`None`).
-  `SetTaskRepositoryRequest` struct L69-71 — `{ repository: Option<&'a str> }` — mirror of: `kairos_client::types_repositories::SetTaskRepositoryRequest`.
-  `tests` module L86-136 — `-` — a decode test).
-  `repository_mirrors_decode_server_shape` function L93-121 — `()` — The repository mirrors decode a realistic `GET /api/repositories`
-  `set_repository_request_serializes_wire_shape` function L126-135 — `()` — The bind request serializes `null` for a clear (the server reads

### crates/kairos-web/src/pages/search

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/search/data.rs

- pub `family_of` function L27-38 — `(short_code: &str) -> Option<&'static str>` — The API family (plural path segment) for a short code's type letter
- pub `SearchRequest` struct L54-65 — `{ q: Option<String>, filter: Option<SearchFilter>, traverse: Option<SearchTraver...` — mirror of: `kairos_client::types_search::SearchRequest` (partial —
- pub `SearchFilter` struct L71-86 — `{ entity_type: Option<Vec<String>>, board_id: Option<String>, column_id: Option<...` — mirror of: `kairos_client::types_search::SearchFilter` (partial — only
- pub `is_empty` function L92-94 — `(&self) -> bool` — Does this filter constrain anything? (An all-`None` filter is
- pub `SearchTraverse` struct L99-104 — `{ from: SearchTraverseFrom, relationships: Vec<String>, direction: String, depth...` — mirror of: `kairos_client::types_search::SearchTraverse`.
- pub `SearchTraverseFrom` struct L109-111 — `{ short_code: String }` — mirror of: `kairos_client::types_search::SearchTraverseFrom` (partial —
- pub `SearchResponse` struct L115-120 — `{ results: SearchResultGroups, total: i64, limit: i64, offset: i64 }` — mirror of: `kairos_client::types_search::SearchResponse`.
- pub `SearchResultGroups` struct L126-137 — `{ strategies: Vec<Hit>, initiatives: Vec<Hit>, tasks: Vec<Hit>, documents: Vec<H...` — mirror of: `kairos_client::types_search::SearchResultGroups` (partial —
- pub `Hit` struct L143-154 — `{ short_code: String, title: String, task_type: Option<String>, is_bucket: Optio...` — One result row.
- pub `search` function L157-159 — `(auth: Auth, request: &SearchRequest) -> Result<SearchResponse, ApiError>` — `POST /api/search`.
- pub `BoardList` struct L167-169 — `{ items: Vec<Board> }` — mirror of: `kairos_client::types::ListEnvelope<Board>` (partial).
- pub `Board` struct L173-177 — `{ id: String, slug: String, board_level: String }` — mirror of: `kairos_client::types_org::Board` (partial).
- pub `BoardColumns` struct L182-184 — `{ columns: Vec<BoardColumn> }` — mirror of: `kairos_client::types_org::BoardDetail` (partial — only the
- pub `BoardColumn` struct L188-191 — `{ id: String, name: String }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `boards` function L194-198 — `(auth: Auth) -> Result<Vec<Board>, ApiError>` — `GET /api/boards` (first page is plenty — a tenant has a handful).
- pub `board_columns` function L201-205 — `(auth: Auth, board_id: &str) -> Result<Vec<BoardColumn>, ApiError>` — `GET /api/boards/{id}` → its columns, in position order.
- pub `ItemRelationships` struct L213-222 — `{ outgoing: Vec<RelationshipGroup>, incoming: Vec<RelationshipGroup> }` — mirror of: `kairos_client::types_meta::ItemRelationshipsResponse`.
- pub `group` function L228-239 — `(&self, relationship: &str, outgoing: bool) -> Vec<RelatedItem>` — The neighbors of one relationship type in one direction.
- pub `RelationshipGroup` struct L244-247 — `{ relationship: String, items: Vec<RelatedItem> }` — mirror of: `kairos_client::types_meta::RelationshipGroup`.
- pub `RelatedItem` struct L252-257 — `{ relationship_id: String, short_code: String, entity_type: String, title: Strin...` — mirror of: `kairos_client::types_meta::RelatedItem` (partial — `id` is
- pub `CreateRelationship` struct L261-265 — `{ source_short_code: String, target_short_code: String, relationship: String }` — mirror of: `kairos_client::types_meta::CreateRelationshipRequest`.
- pub `CreatedRelationship` struct L269-271 — `{ id: String }` — mirror of: `kairos_client::types_meta::Relationship` (partial).
- pub `DeletedRelationship` struct L275-277 — `{ id: String }` — mirror of: `kairos_client::types_meta::DeletedResponse` (partial).
- pub `relationships` function L280-283 — `(auth: Auth, short_code: &str) -> Result<ItemRelationships, ApiError>` — `GET /api/{family}/{short_code}/relationships`.
- pub `create_relationship` function L287-292 — `( auth: Auth, request: &CreateRelationship, ) -> Result<CreatedRelationship, Api...` — `POST /api/relationships` (org admin; the typed 422s — `RELATIONSHIP_RULE`,
- pub `delete_relationship` function L295-300 — `( auth: Auth, relationship_id: &str, ) -> Result<DeletedRelationship, ApiError>` — `DELETE /api/relationships/{id}` (org admin).
- pub `GraphNode` struct L460-472 — `{ id: String, short_code: String, entity_type: String, title: String, status: St...` — mirror of: `kairos_client::types_graph::GraphNode`.
- pub `GraphEdge` struct L476-482 — `{ source_id: String, target_id: String, relationship: String, depth: i32 }` — mirror of: `kairos_client::types_graph::GraphEdge`.
- pub `GraphResponse` struct L486-491 — `{ focus: String, depth: u32, nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }` — mirror of: `kairos_client::types_graph::GraphResponse`.
- pub `item_graph` function L494-505 — `( auth: Auth, short_code: &str, depth: Option<u32>, ) -> Result<GraphResponse, A...` — `GET /api/{family}/{code}/graph?depth=N` — the focal subgraph.
-  `family_or_err` function L41-44 — `(short_code: &str) -> Result<&'static str, ApiError>` — [`family_of`] as an [`ApiError`] for fetchers that need a family.
-  `SearchFilter` type L88-95 — `= SearchFilter` — run on the host (`cargo test -p kairos-web`).
-  `ItemRelationships` type L224-240 — `= ItemRelationships` — run on the host (`cargo test -p kairos-web`).
-  `tests` module L307-452 — `-` — run on the host (`cargo test -p kairos-web`).
-  `family_of_maps_type_letters` function L312-321 — `()` — Type letters map to the S-0004 families; junk maps to none.
-  `search_request_serializes_s0005_field_names` function L326-378 — `()` — The request mirror serializes the exact S-0005 field names (the
-  `search_response_mirror_decodes_server_shape` function L383-421 — `()` — The response mirror decodes a realistic grouped body — typed extra
-  `relationships_mirror_decodes_server_shape` function L426-451 — `()` — The relationships mirror decodes the T-0020 grouped shape, and
-  `graph_tests` module L508-532 — `-` — run on the host (`cargo test -p kairos-web`).
-  `graph_response_mirror_decodes_server_shape` function L513-531 — `()` — `GraphResponse` decodes the KAIROS-T-0088 wire shape.

#### crates/kairos-web/src/pages/search/graph.rs

- pub `GraphView` function L65-493 — `(#[prop(into)] short_code: String) -> impl IntoView` — The focal graph, standalone: `short_code` is its only input (T-0090
-  `column_of` function L33-40 — `(entity_type: &str) -> Option<Column>` — The canvas column of an entity type; documents/ADRs return `None` and
-  `clip` function L43-50 — `(title: &str, max: usize) -> String` — Truncate a title for its node box (SVG text does not wrap).
-  `PanelRow` struct L54-60 — `{ direction: String, entity_type: String, short_code: String, title: String, sta...` — One row of the supporting-material side panel.
-  `ManagePanel` function L499-662 — `(#[prop(into)] short_code: String, on_changed: Callback<()>) -> impl IntoView` — Org-admin link/unlink (ported from the old explorer): the focus item's

#### crates/kairos-web/src/pages/search/graph_layout.rs

- pub `Column` enum L17-21 — `Strategy | Initiative | Task` — The three canvas columns, in fixed left-to-right order.
- pub `LayoutInputNode` struct L35-44 — `{ id: String, short_code: String, column: Column, hidden_neighbors: i64 }` — One node the layout places (the component maps wire mirrors to this).
- pub `LayoutInputEdge` struct L48-54 — `{ source_id: String, target_id: String, relationship: String }` — One edge the layout considers (already filtered to canvas nodes).
- pub `PlacedNode` struct L58-67 — `{ id: String, x: f64, y: f64, w: f64, h: f64, hidden_neighbors: i64 }` — A placed node box.
- pub `PlacedLane` struct L72-78 — `{ parent_id: String, x: f64, y: f64, w: f64, h: f64 }` — A containment lane: the band a visible parent draws around its
- pub `PlacedArrow` struct L82-86 — `{ source_id: String, target_id: String, path: String }` — A blocks arrow, as a ready-to-render SVG path between box edges.
- pub `GraphLayout` struct L90-98 — `{ width: f64, height: f64, headers: Vec<(&'static str, f64)>, lanes: Vec<PlacedL...` — The finished geometry.
- pub `NODE_W` variable L100 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
- pub `NODE_H` variable L101 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
- pub `layout` function L140-372 — `(nodes: &[LayoutInputNode], edges: &[LayoutInputEdge]) -> GraphLayout` — Lay out the subgraph.
-  `Column` type L23-31 — `= Column` — so positions build spatial memory and survive WS refetches unchanged.
-  `index` function L24-30 — `(self) -> usize` — so positions build spatial memory and survive WS refetches unchanged.
-  `COLUMN_GAP` variable L102 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `ROW_GAP` variable L103 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `LANE_PAD` variable L104 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `MARGIN_X` variable L105 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `HEADER_H` variable L106 — `: f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `GROUP_GAP` variable L108 — `: f64` — Extra vertical gap between lane groups so bands never touch.
-  `column_x` function L110-112 — `(column: Column) -> f64` — so positions build spatial memory and survive WS refetches unchanged.
-  `order_column` function L117-137 — `( mut ids: Vec<usize>, nodes: &[LayoutInputNode], parent_row: &std::collections:...` — Order a column: barycenter over the average placed-parent row, ties
-  `tests` module L375-467 — `-` — so positions build spatial memory and survive WS refetches unchanged.
-  `node` function L378-385 — `(id: &str, code: &str, column: Column, hidden_neighbors: i64) -> LayoutInputNode` — so positions build spatial memory and survive WS refetches unchanged.
-  `edge` function L387-393 — `(source: &str, target: &str, relationship: &str) -> LayoutInputEdge` — so positions build spatial memory and survive WS refetches unchanged.
-  `fixture` function L395-414 — `() -> (Vec<LayoutInputNode>, Vec<LayoutInputEdge>)` — so positions build spatial memory and survive WS refetches unchanged.
-  `layout_is_deterministic` function L419-435 — `()` — Identical input → identical geometry, twice over (the no-force
-  `columns_lanes_and_arrows` function L439-466 — `()` — Columns are fixed by type; lanes band children under their parent.

#### crates/kairos-web/src/pages/search/relationships.rs

- pub `RelationshipsPage` function L14-32 — `() -> impl IntoView` — The route wrapper; the view's question is the page subtitle.

### crates/kairos-web/src/pages/teams

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/teams/api.rs

- pub `ListEnvelope` struct L25-27 — `{ items: Vec<T> }` — mirror of: `kairos_client::types::ListEnvelope<T>` (partial — these
- pub `Team` struct L34-40 — `{ id: String, name: String, slug: String, team_type: String, delivery_board_id: ...` — mirror of: `kairos_client::types_org::Team` (partial).
- pub `TeamMember` struct L44-48 — `{ user_id: String, email: String, display_name: String }` — mirror of: `kairos_client::types_org::TeamMember` (partial).
- pub `DeliveryStream` struct L52-57 — `{ id: String, name: String, slug: String, description: Option<String> }` — mirror of: `kairos_client::types_org::DeliveryStream` (partial).
- pub `BoardRef` struct L62-66 — `{ id: String, name: String, slug: String }` — mirror of: `kairos_client::types_org::Board` (partial — enough to
- pub `list_teams` function L69-72 — `(auth: Auth) -> Result<Vec<Team>, ApiError>` — `GET /api/teams`.
- pub `team_members` function L75-77 — `(auth: Auth, team_id: &str) -> Result<Vec<TeamMember>, ApiError>` — `GET /api/teams/{id}/members`.
- pub `list_streams` function L80-84 — `(auth: Auth) -> Result<Vec<DeliveryStream>, ApiError>` — `GET /api/delivery-streams`.
- pub `stream_teams` function L87-89 — `(auth: Auth, stream_id: &str) -> Result<Vec<Team>, ApiError>` — `GET /api/delivery-streams/{id}/teams`.
- pub `list_board_refs` function L92-95 — `(auth: Auth) -> Result<Vec<BoardRef>, ApiError>` — `GET /api/boards` — id/name/slug refs (delivery-board link resolution).
- pub `team_by_slug` function L99-101 — `(auth: Auth, slug: &str) -> Result<Team, ApiError>` — `GET /api/teams/by-slug/{slug}` (KAIROS-T-0083) — kills the
- pub `TeamPageNode` struct L107-118 — `{ id: String, parent_id: Option<String>, kind: String, slug: String, title: Stri...` — mirror of: `kairos_client::types_team_pages::TeamPage` (partial — the
- pub `team_pages` function L121-123 — `(auth: Auth, team_id: &str) -> Result<Vec<TeamPageNode>, ApiError>` — `GET /api/teams/{id}/pages` — the flat page tree (nest by parent_id).
- pub `save_page_content` function L128-153 — `( auth: Auth, team_id: &str, page_id: &str, title: &str, content: &str, version:...` — `PATCH /api/teams/{id}/pages/{page_id}` — the A-0004 versioned
- pub `create_page` function L167-187 — `( auth: Auth, team_id: &str, parent_id: Option<&str>, kind: &str, slug: &str, ti...` — `POST /api/teams/{id}/pages` (team member or org admin).
- pub `rename_page` function L202-218 — `( auth: Auth, team_id: &str, page_id: &str, slug: &str, ) -> Result<TeamPageNode...` — `PATCH /api/teams/{id}/pages/{page_id}` — rename (new sibling slug).
- pub `move_page` function L222-238 — `( auth: Auth, team_id: &str, page_id: &str, new_parent: Option<&str>, ) -> Resul...` — `PATCH /api/teams/{id}/pages/{page_id}` — move under `new_parent`
- pub `delete_page` function L242-246 — `(auth: Auth, team_id: &str, page_id: &str) -> Result<(), ApiError>` — `DELETE /api/teams/{id}/pages/{page_id}` (soft; folders must be
- pub `Announcement` struct L250-256 — `{ id: String, body: String, pinned: bool, created_by: String, created_at: String...` — mirror of: `kairos_client::types_team_pages::TeamAnnouncement`.
- pub `team_announcements` function L259-261 — `(auth: Auth, team_id: &str) -> Result<Vec<Announcement>, ApiError>` — `GET /api/teams/{id}/announcements` (pinned first, newest first).
- pub `post_announcement` function L272-284 — `( auth: Auth, team_id: &str, body: &str, pinned: bool, ) -> Result<Announcement,...` — `POST /api/teams/{id}/announcements` (team member or org admin;
- pub `delete_announcement` function L288-299 — `( auth: Auth, team_id: &str, announcement_id: &str, ) -> Result<(), ApiError>` — `DELETE /api/teams/{id}/announcements/{announcement_id}` (author or
- pub `TeamLink` struct L303-320 — `{ kind: String, external_id: String, title: String, url: String, state: String, ...` — mirror of: `kairos_client::types_forge::TeamLink` (KAIROS-T-0101).
- pub `team_links` function L324-326 — `(auth: Auth, team_id: &str) -> Result<Vec<TeamLink>, ApiError>` — `GET /api/teams/{id}/links` — in-flight work (open + draft) across the
- pub `WorkDocument` struct L330-337 — `{ short_code: String, title: String, lifecycle: String, parent_short_code: Strin...` — mirror of: `kairos_client::types_team_pages::TeamWorkDocument`.
- pub `team_work_documents` function L341-343 — `(auth: Auth, team_id: &str) -> Result<Vec<WorkDocument>, ApiError>` — `GET /api/teams/{id}/work-documents` (KAIROS-T-0084) — live documents
- pub `team_type_color` function L347-356 — `(team_type: &str) -> &'static str` — The accent token for a team type pill (shared by directory, detail,
-  `PAGE` variable L30 — `: &str` — Big-enough page for org-scale lists (server clamps to its own max).
-  `Body` struct L137-141 — `{ title: &'a str, content: &'a str, version: i32 }` — a `mirror of:` line so drift stays greppable.
-  `CreatePageBody` struct L157-164 — `{ parent_id: Option<&'a str>, kind: &'a str, slug: &'a str, title: &'a str, cont...` — mirror of: `kairos_client::types_team_pages::CreateTeamPageRequest`.
-  `StructureBody` struct L192-199 — `{ slug: Option<&'a str>, parent_id: Option<&'a str>, move_to_root: bool }` — mirror of: `kairos_client::types_team_pages::UpdateTeamPageRequest`
-  `PostAnnouncementBody` struct L265-268 — `{ body: &'a str, pinned: bool }` — mirror of: `kairos_client::types_team_pages::CreateTeamAnnouncementRequest`.
-  `tests` module L359-477 — `-` — a `mirror of:` line so drift stays greppable.
-  `team_mirror_decodes_server_shape` function L365-375 — `()` — `Team` decodes the wire shape (field-name lock; same body the admin
-  `team_member_mirror_decodes_server_shape` function L379-388 — `()` — `TeamMember` decodes the roster row.
-  `board_ref_mirror_decodes_server_shape` function L392-400 — `()` — `BoardRef` decodes a board list element (link resolution only).
-  `team_page_node_mirror_decodes_server_shape` function L404-423 — `()` — `TeamPageNode` decodes the server's TeamPage shape.
-  `announcement_mirror_decodes_server_shape` function L427-439 — `()` — `Announcement` decodes the server's TeamAnnouncement shape.
-  `team_link_mirror_decodes_server_shape` function L443-460 — `()` — `TeamLink` decodes the in-flight rollup row.
-  `work_document_mirror_decodes_server_shape` function L464-476 — `()` — `WorkDocument` decodes the derived work-documents row.

#### crates/kairos-web/src/pages/teams/doc.rs

- pub `TeamDocPage` function L74-123 — `() -> impl IntoView` — `/teams/:slug/pages/{path…}`.
-  `DocView` struct L32-35 — `{ team: api::Team, pages: Vec<api::TeamPageNode> }` — The fetched unit: the team and its full (flat) page tree.
-  `resolve_path` function L39-53 — `( pages: &'a [api::TeamPageNode], segments: &[&str], ) -> Option<&'a api::TeamPa...` — Resolve `segments` against the tree: each segment is a slug under the
-  `path_of` function L56-70 — `(pages: &[api::TeamPageNode], node: &api::TeamPageNode) -> String` — The slug path of a node (ancestors walked through the flat list).
-  `DocBody` function L127-209 — `( doc_view: DocView, path: String, manage: bool, on_changed: Callback<()>, ) -> ...` — The resolved page/folder view with breadcrumbs and management.
-  `FolderIndex` function L213-255 — `( team: api::Team, pages: Vec<api::TeamPageNode>, folder: api::TeamPageNode, man...` — A folder: its children as links, plus the create surface.
-  `PageView` function L259-330 — `( team: api::Team, pages: Vec<api::TeamPageNode>, node: api::TeamPageNode, manag...` — A page: rendered markdown, or the generalized editor when editing.
-  `CreateForm` function L335-409 — `(team_id: String, parent_id: String, on_changed: Callback<()>) -> impl IntoView` — The create surface inside a folder (KAIROS-T-0086): kind + slug +
-  `StructurePanel` function L417-580 — `( team: api::Team, pages: Vec<api::TeamPageNode>, node: api::TeamPageNode, on_ch...` — Rename / move / delete for an unprotected node (KAIROS-T-0086).

### e2e/helpers

> *Semantic summary to be generated by AI agent.*

#### e2e/helpers/api.ts

- pub `BoardSnapshot` interface L17-22 — `{ boardId: : string, columnName: : Map<string, string>, transitions: : { from: s...`
- pub `loadPlatformDelivery` function L25-49 — `function loadPlatformDelivery( server: string, token: string, ): Promise<BoardSn...`
- pub `MovePick` interface L51-55 — `{ code: : string, toColumnId: : string, toColumnName: : string }`
- pub `pickMovableTask` function L63-86 — `function pickMovableTask( server: string, token: string, exclude: string[] = [],...`
- pub `transitionTask` function L89-103 — `function transitionTask( server: string, token: string, code: string, toColumnId...`
- pub `moveTask` function L111-126 — `function moveTask( server: string, token: string, code: string, board: string, )...`
- pub `TaskState` interface L128-132 — `{ version: : number, title: : string, content: : string }`
- pub `getTask` function L135-142 — `function getTask( server: string, token: string, code: string, ): Promise<TaskSt...`
- pub `patchTask` function L148-163 — `function patchTask( server: string, token: string, code: string, body: { title: ...`
- pub `TeamPageState` interface L167-174 — `{ id: : string, teamId: : string, slug: : string, title: : string, content: : st...`
- pub `getTeamPage` function L177-200 — `function getTeamPage( server: string, token: string, teamSlug: string, path: str...`
- pub `patchTeamPage` function L203-219 — `function patchTeamPage( server: string, token: string, teamId: string, pageId: s...`
- pub `ForgeConnection` interface L223-227 — `{ id: : string, webhookUrl: : string, webhookSecret: : string }`
- pub `Repository` interface L230-237 — `{ id: : string, slug: : string, teamSlug: : string, deliveryBoardId: : string | ...`
- pub `createRepository` function L254-275 — `function createRepository( server: string, token: string, opts: { slug?: string;...`
- pub `listRepositories` function L278-289 — `function listRepositories( server: string, token: string, team?: string, ): Prom...`
- pub `createTask` function L296-315 — `function createTask( server: string, token: string, opts: { title: string; board...`
- pub `tryTransitionTask` function L318-330 — `function tryTransitionTask( server: string, token: string, shortCode: string, to...`
- pub `createForgeConnection` function L336-355 — `function createForgeConnection( server: string, token: string, repository: strin...`
- pub `deliverGithubWebhook` function L365-390 — `function deliverGithubWebhook( server: string, connection: ForgeConnection, even...`
- pub `githubPullRequest` function L393-423 — `function githubPullRequest(opts: { number: number; code: string; repoFullName: s...`
- pub `tryCreateRelationship` function L430-445 — `function tryCreateRelationship( server: string, token: string, edge: { source: s...`
-  `bearer` function L9 — `const bearer = (token: string)`
-  `json` function L11-15 — `function json(server: string, token: string, path: string): Promise<any>`
-  `toRepository` function L239-248 — `function toRepository(body: any): Repository`

#### e2e/helpers/auth.ts

- pub `MintOptions` interface L18-24 — `{ issuer: : string, server: : string, clientId: : string, email: : string, passw...`
- pub `mintToken` function L27-124 — `function mintToken(opts: MintOptions = {}): Promise<string>`
-  `b64url` function L15-16 — `const b64url = (b: Buffer)`
-  `remember` function L40-47 — `const remember = (res: Response)`
-  `cookieHeader` function L48-49 — `const cookieHeader = ()`
-  `follow` function L50-58 — `const follow = (url: string, init: RequestInit = {})`

#### e2e/helpers/drag.ts

- pub `dragTo` function L14-32 — `function dragTo(page: Page, source: Locator, target: Locator): Promise<void>`

### e2e/tests

> *Semantic summary to be generated by AI agent.*

#### e2e/tests/archived.spec.ts

-  `bearer` function L32 — `const bearer = (token: string)`
-  `api` function L34-52 — `function api( token: string, method: string, path: string, body?: unknown, ): Pr...`
-  `deliveryBoard` function L55-60 — `function deliveryBoard(token: string): Promise<any>`
-  `createTask` function L62-74 — `function createTask( token: string, boardId: string, columnId: string, title: st...`
-  `login` function L76-85 — `function login(page: Page): Promise<void>`
-  `entryOf` function L170-175 — `const entryOf = (level: string)`

#### e2e/tests/drag.spec.ts

-  `column` function L24-29 — `const column = (page: Page, name: string): Locator`
-  `cardIn` function L31-32 — `const cardIn = (page: Page, columnName: string): Locator`

#### e2e/tests/forge.spec.ts

-  `panel` function L33-36 — `const panel = (page: Page, title: string)`

#### e2e/tests/graph.spec.ts

-  `canvas` function L28 — `const canvas = (page: Page)`
-  `columnIdOf` function L148-153 — `const columnIdOf = (name: string)`

#### e2e/tests/lanes.spec.ts

-  `Lane` type L28 — `= 'planned' | 'support'`
-  `column` function L30-35 — `const column = (page: Page, name: string, lane: Lane): Locator`
-  `cardIn` function L37-45 — `const cardIn = ( page: Page, columnName: string, lane: Lane, needle: string, ): ...`
-  `field` function L98-101 — `const field = (label: string)`

#### e2e/tests/metadata.spec.ts

-  `complexityField` function L46-49 — `const complexityField = ()`

#### e2e/tests/repositories.spec.ts

-  `panel` function L50-53 — `const panel = (page: Page, title: string)`
-  `login` function L55-64 — `function login(page: Page, email: string, password: string)`
-  `field` function L353-356 — `const field = (label: string)`

#### e2e/tests/smoke.spec.ts

-  `column` function L41-47 — `const column = (page: Page, name: string, lane?: 'planned' | 'support'): Locator`
-  `cardIn` function L49-50 — `const cardIn = (page: Page, columnName: string, needle: string): Locator`

#### e2e/tests/team-lens.spec.ts

-  `navbar` function L24 — `const navbar = (page: Page)`

#### e2e/tests/teampages.spec.ts

-  `panel` function L28-31 — `const panel = (page: Page, title: string)`

### plugin/hooks

> *Semantic summary to be generated by AI agent.*

#### plugin/hooks/session_start.py

- pub `read_frontmatter` function L39-56 — `def read_frontmatter(path)` — Parse the YAML frontmatter's simple `key: value` pairs (no external
- pub `probe` function L59-70 — `def probe(url)` — Unauthenticated reachability check; returns a short status note.
- pub `live_state_hint` function L73-102 — `def live_state_hint(values)` — The instruction that points the agent at live state over MCP.
- pub `build_context` function L105-116 — `def build_context(values, status)` — The full additionalContext text (pure; testable).
- pub `main` function L119-138 — `def main()`

#### plugin/hooks/test_session_start.py

- pub `write` function L20-24 — `def write(text)`
- pub `ReadFrontmatter` class L27-51 — `(unittest.TestCase) { test_reads_every_known_key_including_repository, test_empt...`
- pub `test_reads_every_known_key_including_repository` method L28-42 — `def test_reads_every_known_key_including_repository(self)`
- pub `test_empty_values_and_unknown_keys_are_dropped` method L44-51 — `def test_empty_values_and_unknown_keys_are_dropped(self)`
- pub `LiveStateHint` class L54-71 — `(unittest.TestCase) { test_repo_scoped_when_repository_is_set, test_board_scoped...`
- pub `test_repo_scoped_when_repository_is_set` method L55-64 — `def test_repo_scoped_when_repository_is_set(self)`
- pub `test_board_scoped_when_repository_is_unset` method L66-71 — `def test_board_scoped_when_repository_is_unset(self)`
- pub `BuildContext` class L74-92 — `(unittest.TestCase) { test_lists_every_key_and_adds_the_hint_when_reachable, tes...`
- pub `test_lists_every_key_and_adds_the_hint_when_reachable` method L75-84 — `def test_lists_every_key_and_adds_the_hint_when_reachable(self)`
- pub `test_no_hint_when_offline` method L86-92 — `def test_no_hint_when_offline(self)`

### uat/checks

> *Semantic summary to be generated by AI agent.*

#### uat/checks/zz-surface-coverage.check.ts

-  `allJourneyIds` function L33-39 — `function allJourneyIds(): string[]`

### uat/fixtures

> *Semantic summary to be generated by AI agent.*

#### uat/fixtures/team.ts

- pub `TeamFixture` interface L17-27 — `{ teamId: : string, teamSlug: : string, boardId: : string, boardSlug: : string, ...`
- pub `FixtureSteps` interface L29-42 — `{ createTeam(), useTeam(), addMember(), registerRepository(), createAgent(), fix...`
- pub `teamFixture` function L48-154 — `function teamFixture(alice: Persona, ledger: Ledger, suffix = 'mobile'): Fixture...`
- pub `setupTeamRepoAgent` function L162-168 — `function setupTeamRepoAgent(alice: Persona, ledger: Ledger, suffix = 'mobile'): ...`
- pub `setupRepoAgentOnTeam` function L175-181 — `function setupRepoAgentOnTeam(alice: Persona, ledger: Ledger, suffix = 'mobile')...`
-  `observeBoard` function L53-60 — `function observeBoard(teamSlug: string): Promise<Observed>`

### uat/journeys

> *Semantic summary to be generated by AI agent.*

#### uat/journeys/board-setup.journey.ts

-  `field` function L60-63 — `const field = (label: string)`

#### uat/journeys/decision-record.journey.ts

-  `adrColumn` function L26-30 — `function adrColumn(page: Page, name: string): Locator`
-  `relationshipGroup` function L33-35 — `function relationshipGroup(page: Page, label: string): Locator`
-  `columnId` function L46-50 — `const columnId = (name: string): string`
-  `state` function L237-240 — `const state = (code: string)`

#### uat/journeys/explorer.journey.ts

-  `chips` function L20-26 — `function chips(page: Page, heading: string): Locator`
-  `field` function L29-31 — `function field(page: Page, label: string): Locator`
-  `drawn` function L34-38 — `function drawn(page: Page): Promise<string[]>`
-  `node` function L41-45 — `function node(page: Page, code: string): Locator`
-  `codesIn` function L48-54 — `function codesIn(results: any): string[]`

#### uat/journeys/growing-team.journey.ts

-  `ledgerTask` function L42-54 — `function ledgerTask(code: string): Promise<void>`

#### uat/journeys/incident.journey.ts

-  `Lane` type L27 — `= 'planned' | 'support'`
-  `laneColumn` function L29-33 — `function laneColumn(page: Page, lane: Lane, name: string): Locator`
-  `laneCard` function L35-37 — `function laneCard(page: Page, lane: Lane, columnName: string, code: string): Loc...`
-  `dragInLane` function L41-59 — `function dragInLane(page: Page, code: string, lane: Lane, toColumn: string): Pro...`

#### uat/journeys/machine-access.journey.ts

-  `machine` function L66 — `const machine = ()`

#### uat/journeys/new-kind-of-work.journey.ts

-  `field` function L21-23 — `function field(scope: Locator, page: Page, label: string): Locator`

#### uat/journeys/operations.journey.ts

-  `sample` function L19-23 — `function sample(text: string, needle: string): number | undefined`
-  `probe` function L34 — `const probe = (path: string)`
-  `forget` function L110-115 — `const forget = (family: string, code: string)`
-  `search` function L205-206 — `const search = (extra: string[])`
-  `byText` function L219-220 — `const byText = (extra: string[])`

#### uat/journeys/planning.journey.ts

-  `createFromHeader` function L13-24 — `function createFromHeader(page: Page, kind: string, title: string): Promise<stri...`
-  `columnOfCard` function L192-196 — `function columnOfCard(page: Page, code: string): Promise<string>`

#### uat/journeys/quarterly-review.journey.ts

-  `readBadge` function L22-28 — `function readBadge(text: string): { done: number; total: number }`
-  `bandOf` function L45-46 — `const bandOf = (label: string)`

### uat/personas

> *Semantic summary to be generated by AI agent.*

#### uat/personas/credentials.ts

- pub `Human` type L5 — `= 'alice' | 'bob' | 'carol' | 'newhire'`
- pub `PersonaName` type L6 — `= Human | 'agent'`
- pub `Credentials` interface L8-11 — `{ email: : string, password: : string }`
- pub `credentialsFor` function L30-36 — `function credentialsFor(name: Human): Credentials | null`

#### uat/personas/index.ts

- pub `Persona` class L15-96 — `-`
- pub `constructor` method L23-29 — `constructor( readonly name: PersonaName, readonly role: string, private readonly...`
- pub `isHuman` method L31-33 — `isHuman(): boolean`
- pub `credentials` method L35-38 — `credentials(): Credentials`
- pub `token` method L41-50 — `token(): Promise<string>`
- pub `api` method L52-55 — `api(): Promise<Api>`
- pub `cli` method L57-64 — `cli(): Promise<Cli>`
- pub `mcp` method L66-69 — `mcp(): Promise<McpSession>`
- pub `gui` method L72-81 — `gui(): Promise<Page>`
- pub `openPage` method L84-86 — `openPage(): Page | undefined`
- pub `close` method L88-95 — `close(keepTraceAs?: string): Promise<void>`
- pub `Cast` class L98-159 — `-`
- pub `constructor` method L101 — `constructor(private readonly browser: Browser)`
- pub `human` method L104-112 — `human(name: Human): Persona`
- pub `hasHuman` method L115-117 — `hasHuman(name: Human): boolean`
- pub `agent` method L122-131 — `agent(name: string, apiKey: string): Persona`
- pub `prepare` method L137-139 — `prepare(humans: Human[]): Promise<void>`
- pub `all` method L141-143 — `all(): Persona[]`
- pub `closeAll` method L146-158 — `closeAll(failedJourneyId?: string): Promise<string[]>`

### uat/run

> *Semantic summary to be generated by AI agent.*

#### uat/run/context.ts

- pub `Mode` type L8 — `= 'compose' | 'server'`
- pub `RunContext` interface L10-26 — `{ server: : string, issuer: : string, tenant: : string, mode: : Mode, run: : str...`
- pub `runContext` function L34-56 — `function runContext(): RunContext`
- pub `named` function L59-61 — `function named(suffix: string): string`

#### uat/run/coverage.ts

- pub `SurfaceKind` type L19 — `= 'mcp' | 'cli'`
- pub `recordSurface` function L37-39 — `function recordSurface(kind: SurfaceKind, name: string): void`
- pub `recordJourney` function L42-44 — `function recordJourney(id: string): void`
- pub `SurfaceUsage` interface L46-50 — `{ mcp: : string[], cli: : string[], journeys: : string[] }`
- pub `surfaceUsage` function L53-79 — `function surfaceUsage(): SurfaceUsage`
-  `ledgerPath` function L21-23 — `function ledgerPath(): string`
-  `append` function L25-34 — `function append(record: Record<string, string>): void`

#### uat/run/ledger.ts

- pub `LedgerEntry` interface L12-16 — `{ kind: : string, label: : string, delete: : () => Promise<void> }`
- pub `TeardownFailure` interface L29-33 — `{ kind: : string, label: : string, error: : string }`
- pub `Ledger` class L35-60 — `-`
- pub `add` method L39-41 — `add(entry: LedgerEntry): void`
- pub `teardown` method L44-55 — `teardown(): Promise<TeardownFailure[]>`
- pub `size` method L57-59 — `size(): number`
-  `alreadyGone` function L25-27 — `function alreadyGone(err: unknown): boolean`

#### uat/run/narrate.ts

- pub `Observed` type L14 — `= Record<string, string | number | boolean | string[] | undefined>`
- pub `StepRecord` interface L16-26 — `{ index: : number, persona: : string, narration: : string, status: : 'passed' | ...`
- pub `JourneyRecord` interface L28-35 — `{ id: : string, title: : string, mode: : Mode, steps: : StepRecord[], teardownFa...`
- pub `JourneyScope` interface L47-52 — `{ cast: : Cast, ledger: : Ledger, mode: : Mode, run: : string }`
- pub `JourneyOptions` interface L54-57 — `{ humans: : Human[] }`
- pub `journey` function L64-102 — `function journey( id: string, title: string, options: JourneyOptions, body: (sco...`
- pub `step` function L145-176 — `function step( persona: Persona, narration: string, fn: () => Promise<T>, ): Pro...`
-  `Current` interface L39-43 — `{ record: : JourneyRecord, cast: : Cast, testInfo: : TestInfo }`
-  `plainError` function L108-116 — `function plainError(err: any): string`
-  `requireCurrent` function L118-121 — `function requireCurrent(): Current`
-  `screenshotAll` function L123-139 — `function screenshotAll(record: JourneyRecord, stepIndex: number): Promise<string...`

#### uat/run/reporter.ts

- pub `UatReporter` class L50-191 — `implements Reporter`
- pub `onBegin` method L55-57 — `onBegin(_config: FullConfig): void`
- pub `onTestEnd` method L59-82 — `onTestEnd(test: TestCase, result: TestResult): void`
- pub `onEnd` method L84-190 — `onEnd(result: FullResult): Promise<void>`
-  `JourneyResult` interface L11-16 — `{ record: : JourneyRecord, status: : TestResult['status'], durationMs: : number,...`
-  `CoverageResult` interface L20-28 — `{ mcp: : { offered: number; exercised: number }, cli: : { offered: number; exerc...`
-  `md` function L32-34 — `function md(value: unknown): string`
-  `observedText` function L36-48 — `function observedText(step: StepRecord): string`

### uat/surfaces

> *Semantic summary to be generated by AI agent.*

#### uat/surfaces/api.ts

- pub `ApiError` class L7-16 — `extends Error`
- pub `constructor` method L8-15 — `constructor( public readonly method: string, public readonly path: string, publi...`
- pub `Api` class L18-73 — `-`
- pub `constructor` method L19-23 — `constructor( private readonly token: string, private readonly server = runContex...`
- pub `raw` method L26-44 — `raw(method: string, path: string, body?: unknown): Promise<{ status: number; bod...`
- pub `ok` method L46-52 — `ok(method: string, path: string, body?: unknown): Promise<any>`
- pub `get` method L54 — `get(path: string)`
- pub `post` method L55 — `post(path: string, body?: unknown)`
- pub `put` method L56 — `put(path: string, body?: unknown)`
- pub `patch` method L57 — `patch(path: string, body?: unknown)`
- pub `delete` method L58 — `delete(path: string)`
- pub `whoami` method L60 — `whoami()`
- pub `boards` method L63-65 — `boards(): Promise<any[]>`
- pub `boardBySlug` method L66-70 — `boardBySlug(slug: string): Promise<any>`
- pub `teamBySlug` method L71 — `teamBySlug(slug: string)`
- pub `task` method L72 — `task(code: string)`

#### uat/surfaces/auth.ts

- pub `MintOptions` interface L15-21 — `{ issuer: : string, server: : string, email: : string, password: : string, clien...`
- pub `mintToken` function L23-110 — `function mintToken(opts: MintOptions): Promise<string>`
-  `b64url` function L12-13 — `const b64url = (b: Buffer)`
-  `remember` function L32-39 — `const remember = (res: Response)`
-  `cookieHeader` function L40 — `const cookieHeader = ()`
-  `follow` function L41-49 — `const follow = (url: string, init: RequestInit = {})`

#### uat/surfaces/cli.ts

- pub `CliResult` interface L18-22 — `{ code: : number, stdout: : string, stderr: : string }`
- pub `CliError` class L24-28 — `extends Error`
- pub `constructor` method L25-27 — `constructor(public readonly args: string[], public readonly result: CliResult)`
- pub `Cli` class L30-115 — `-`
- pub `constructor` method L33-36 — `constructor(private readonly personaName: string)`
- pub `login` method L39-59 — `login(token: string, opts: { refreshToken?: string } = {}): void`
- pub `run` method L62-77 — `run(args: string[]): Promise<CliResult>`
- pub `nouns` method L86-97 — `nouns(): Promise<string[]>`
- pub `ok` method L100-104 — `ok(args: string[]): Promise<string>`
- pub `json` method L107-114 — `json(args: string[]): Promise<any>`

#### uat/surfaces/forge.ts

- pub `ForgeConnection` interface L9-13 — `{ id: : string, webhookUrl: : string, webhookSecret: : string }`
- pub `createForgeConnection` function L16-19 — `function createForgeConnection(api: Api, repository: string): Promise<ForgeConne...`
- pub `deliverGithubWebhook` function L22-43 — `function deliverGithubWebhook( connection: ForgeConnection, event: string, paylo...`
- pub `githubPullRequest` function L46-77 — `function githubPullRequest(opts: { number: number; code: string; repoFullName: s...`

#### uat/surfaces/gui.ts

- pub `login` function L9-16 — `function login(page: Page, creds: Credentials): Promise<void>`
- pub `openBoard` function L18-21 — `function openBoard(page: Page, slug: string): Promise<void>`
- pub `openItem` function L23-26 — `function openItem(page: Page, code: string): Promise<void>`
- pub `openTeam` function L28-30 — `function openTeam(page: Page, slug: string): Promise<void>`
- pub `column` function L33-39 — `function column(page: Page, name: string): Locator`
- pub `card` function L42-44 — `function card(page: Page, code: string): Locator`
- pub `cardIn` function L47-49 — `function cardIn(page: Page, columnName: string, code: string): Locator`
- pub `dragCard` function L64-77 — `function dragCard(page: Page, code: string, toColumn: string): Promise<void>`
- pub `columnOf` function L94-97 — `function columnOf(page: Page, code: string): Promise<string>`
- pub `panel` function L100-102 — `function panel(page: Page, title: string): Locator`
- pub `newPage` function L105-107 — `function newPage(context: BrowserContext): Promise<Page>`
-  `visiblePoint` function L80-91 — `function visiblePoint(page: Page, target: Locator, what: string): Promise<{ x: n...`

#### uat/surfaces/mcp.ts

- pub `McpToolError` class L28-32 — `extends Error`
- pub `constructor` method L29-31 — `constructor(public readonly tool: string, public readonly text: string)`
- pub `McpSession` class L34-132 — `-`
- pub `constructor` method L38-43 — `constructor( private readonly token: string, private readonly clientName: string...`
- pub `post` method L45-58 — `post(body: unknown): Promise<{ status: number; session: string | null; text: str...`
- pub `initialize` method L60-81 — `initialize(): Promise<void>`
- pub `call` method L87-106 — `call(name: string, args: Record<string, unknown> = {}): Promise<string>`
- pub `listTools` method L113-120 — `listTools(): Promise<string[]>`
- pub `refused` method L123-131 — `refused(name: string, args: Record<string, unknown> = {}): Promise<string>`
- pub `shortCodes` function L135-137 — `function shortCodes(text: string): string[]`
- pub `field` function L140-143 — `function field(text: string, key: string): string | undefined`
-  `rpcMessage` function L8-26 — `function rpcMessage(body: string): any`

