# Code Index

> Generated: 2026-08-11T03:23:49Z | 190 files | Python, Rust, TypeScript

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
│   │       ├── types_meta.rs
│   │       ├── types_org.rs
│   │       ├── types_search.rs
│   │       ├── types_service_accounts.rs
│   │       └── ws.rs
│   ├── kairos-core/
│   │   └── src/
│   │       ├── abac.rs
│   │       ├── board.rs
│   │       ├── graph.rs
│   │       ├── items.rs
│   │       ├── lib.rs
│   │       ├── retention.rs
│   │       ├── search.rs
│   │       └── short_code.rs
│   ├── kairos-db/
│   │   ├── src/
│   │   │   ├── abac.rs
│   │   │   ├── api_keys.rs
│   │   │   ├── boards.rs
│   │   │   ├── events.rs
│   │   │   ├── graph.rs
│   │   │   ├── items.rs
│   │   │   ├── lib.rs
│   │   │   ├── migrations.rs
│   │   │   ├── models/
│   │   │   │   ├── boards.rs
│   │   │   │   ├── enums.rs
│   │   │   │   ├── graph.rs
│   │   │   │   ├── items.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── public.rs
│   │   │   │   ├── teams.rs
│   │   │   │   └── templates.rs
│   │   │   ├── pool.rs
│   │   │   ├── retention.rs
│   │   │   ├── schema.rs
│   │   │   ├── scim.rs
│   │   │   ├── search.rs
│   │   │   ├── seed.rs
│   │   │   ├── service_accounts.rs
│   │   │   └── tenant.rs
│   │   └── tests/
│   │       ├── abac.rs
│   │       ├── api_keys.rs
│   │       ├── board_rules.rs
│   │       ├── graph.rs
│   │       ├── isolation.rs
│   │       ├── models_roundtrip.rs
│   │       ├── public_migrations.rs
│   │       ├── retention.rs
│   │       ├── search.rs
│   │       ├── seed_demo.rs
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
│   │   │   │   │   └── templates.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── openapi.rs
│   │   │   │   ├── org/
│   │   │   │   │   ├── admin.rs
│   │   │   │   │   ├── boards.rs
│   │   │   │   │   ├── members.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── streams.rs
│   │   │   │   │   └── teams.rs
│   │   │   │   ├── search.rs
│   │   │   │   ├── strategies.rs
│   │   │   │   └── tasks.rs
│   │   │   ├── app.rs
│   │   │   ├── blocking.rs
│   │   │   ├── config.rs
│   │   │   ├── error.rs
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
│   │       ├── cascade_preview.rs
│   │       ├── client_roundtrip.rs
│   │       ├── common/
│   │       │   └── mod.rs
│   │       ├── entities.rs
│   │       ├── mcp.rs
│   │       ├── meta.rs
│   │       ├── metrics.rs
│   │       ├── middleware.rs
│   │       ├── openapi.rs
│   │       ├── org_endpoints.rs
│   │       ├── scim.rs
│   │       ├── search_endpoint.rs
│   │       ├── service_account_mgmt.rs
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
│           │   │   ├── streams.rs
│           │   │   ├── teams.rs
│           │   │   └── templates.rs
│           │   ├── admin.rs
│           │   ├── boards/
│           │   │   ├── data.rs
│           │   │   └── live.rs
│           │   ├── boards.rs
│           │   ├── item/
│           │   │   ├── api.rs
│           │   │   ├── create_doc.rs
│           │   │   ├── delete.rs
│           │   │   ├── editor.rs
│           │   │   ├── markdown.rs
│           │   │   └── metadata.rs
│           │   ├── item.rs
│           │   ├── search/
│           │   │   ├── data.rs
│           │   │   └── relationships.rs
│           │   ├── search.rs
│           │   ├── teams/
│           │   │   └── api.rs
│           │   └── teams.rs
│           └── pages.rs
├── e2e/
│   ├── helpers/
│   │   ├── api.ts
│   │   └── auth.ts
│   ├── playwright.config.ts
│   └── tests/
│       ├── drag.spec.ts
│       ├── smoke.spec.ts
│       └── team-lens.spec.ts
└── plugin/
    └── hooks/
        └── session_start.py
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
-  `print_board_items` function L71-108 — `(items: &BoardItemsResponse)` — The human board rendering: one section per column (in position order),

#### crates/kairos-cli/src/commands/entities.rs

- pub `ListArgs` struct L28-37 — `{ limit: Option<i64>, offset: Option<i64>, common: Common }` — `?limit=&offset=` pagination flags for the `list` verbs.
- pub `page` function L40-45 — `(&self) -> Pagination` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `GetArgs` struct L50-55 — `{ short_code: String, common: Common }` — Arguments of the `get` verbs.
- pub `EditArgs` struct L62-80 — `{ short_code: String, title: Option<String>, content: Option<String>, content_fi...` — Arguments of the `edit` verbs — the KAIROS-A-0004 optimistic-concurrency
- pub `build_request` function L84-113 — `( &self, current_version: i32, current_content: &str, ) -> Result<UpdateContentR...` — The PATCH body: flags merged over the fetched current entity.
- pub `TransitionArgs` struct L118-127 — `{ short_code: String, to_column: String, common: Common }` — Arguments of the `transition` verbs.
- pub `DeleteArgs` struct L131-139 — `{ short_code: String, confirm: bool, common: Common }` — Arguments of the `delete` verbs (soft delete, KAIROS-A-0001 cascade).
- pub `require_confirm` function L142-150 — `(confirm: bool, what: &str) -> Result<(), CliError>` — The client-side `--confirm` guard for destructive verbs.
- pub `EntityView` interface L159-174 — `{ fn short_code(), fn title(), fn version(), fn content(), fn column_id(), fn ta...` — The rendering surface the five entity DTOs share: identity, versioning,
- pub `emit_list` function L390-411 — `( common: &Common, envelope: &ListEnvelope<T>, ) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_get` function L413-424 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_created` function L426-438 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_edited` function L440-451 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_transitioned` function L453-464 — `(common: &Common, item: &T) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `emit_deleted` function L466-481 — `(common: &Common, response: &DeleteResponse) -> Result<(), CliError>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
- pub `StrategyCreateArgs` struct L489-507 — `{ board: String, column: Option<String>, title: String, content: String, hypothe...` — Arguments of `kairos strategies create`.
- pub `InitiativeCreateArgs` struct L523-544 — `{ board: String, column: Option<String>, title: String, content: String, complex...` — Arguments of `kairos initiatives create`.
- pub `TaskCreateArgs` struct L561-582 — `{ board: String, column: Option<String>, title: String, content: String, task_ty...` — Arguments of `kairos tasks create`.
- pub `DocumentCreateArgs` struct L599-616 — `{ title: String, parent: String, content: Option<String>, template: Option<Strin...` — Arguments of `kairos documents create`.
- pub `AdrCreateArgs` struct L631-652 — `{ title: String, board: Option<String>, column: Option<String>, content: String,...` — Arguments of `kairos adrs create`.
-  `ListArgs` type L39-46 — `= ListArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `EditArgs` type L82-114 — `= EditArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L161 — `: &'static str` — Human noun ("task", "initiative", ...).
-  `HEADERS` variable L163 — `: &'static [&'static str]` — Column headers of the `list` table.
-  `or_dash` function L176-178 — `(value: &Option<String>) -> String` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Strategy` type L180-218 — `impl EntityView for Strategy` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L181 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L182 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L184-186 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L187-189 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L190-192 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L193-195 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L196-198 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L199-206 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L207-217 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Initiative` type L220-262 — `impl EntityView for Initiative` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L221 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L222-223 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L225-227 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L228-230 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L231-233 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L234-236 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L237-239 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L240-249 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L250-261 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Task` type L264-304 — `impl EntityView for Task` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L265 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L266 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L268-270 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L271-273 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L274-276 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L277-279 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L280-282 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L283-291 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L292-303 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Document` type L306-342 — `impl EntityView for Document` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L307 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L308 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L310-312 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L313-315 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L316-318 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L319-321 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L322-324 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L325-332 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L333-341 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `Adr` type L344-384 — `impl EntityView for Adr` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `NOUN` variable L345 — `: &'static str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `HEADERS` variable L346 — `: &'static [&'static str]` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `short_code` function L348-350 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `title` function L351-353 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `version` function L354-356 — `(&self) -> i32` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `content` function L357-359 — `(&self) -> &str` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `column_id` function L360-362 — `(&self) -> Option<&str>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `table_row` function L363-371 — `(&self) -> Vec<String>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `fields` function L372-383 — `(&self) -> Vec<(&'static str, String)>` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `StrategyCreateArgs` type L509-519 — `= StrategyCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L510-518 — `(&self) -> CreateStrategyRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `InitiativeCreateArgs` type L546-557 — `= InitiativeCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L547-556 — `(&self) -> CreateInitiativeRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `TaskCreateArgs` type L584-595 — `= TaskCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L585-594 — `(&self) -> CreateTaskRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `DocumentCreateArgs` type L618-627 — `= DocumentCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L619-626 — `(&self) -> CreateDocumentRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `AdrCreateArgs` type L654-665 — `= AdrCreateArgs` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `request` function L655-664 — `(&self) -> CreateAdrRequest` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `entity_family_cli` macro L675-753 — `-` — Generate the per-family `Subcommand` enum and its `run` dispatcher over
-  `tests` module L815-872 — `-` — is flag plumbing, the fetch-then-patch edit flow, and rendering.
-  `edit_request_building` function L823-862 — `()` — The edit flow's request building: flags merge over the fetched
-  `delete_requires_confirm` function L866-871 — `()` — Deletes refuse to run without --confirm.

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
- pub `search` module L11 — `-` — and rendering only, no business logic.
- pub `service_accounts` module L12 — `-` — and rendering only, no business logic.
- pub `streams` module L13 — `-` — and rendering only, no business logic.
- pub `teams` module L14 — `-` — and rendering only, no business logic.

#### crates/kairos-cli/src/commands/orgs.rs

- pub `OrgsCommand` enum L12-19 — `Show` — Organization info for your credentials.
- pub `run` function L22-36 — `(self) -> Result<(), CliError>` — KAIROS-T-0037).
-  `OrgsCommand` type L21-37 — `= OrgsCommand` — KAIROS-T-0037).

#### crates/kairos-cli/src/commands/search.rs

- pub `SearchArgs` struct L21-93 — `{ query: Option<String>, entity_type: Vec<String>, board: Option<String>, column...` — Search and traverse all entity types (POST /api/search).
- pub `build_request` function L123-200 — `(&self) -> Result<SearchRequest, CliError>` — Compose the S-0005 request body from the flags (or take
- pub `run` function L259-268 — `(self) -> Result<(), CliError>` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `SearchArgs` type L95-269 — `= SearchArgs` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `has_flag_query` function L98-119 — `(&self) -> bool` — Whether any flag other than `--query-json` (and the pagination
-  `parse_metadata` function L203-217 — `(&self) -> Result<Option<BTreeMap<String, String>>, CliError>` — `--metadata k=v` pairs into the S-0005 metadata map.
-  `build_traverse` function L222-257 — `(&self) -> Result<Option<SearchTraverse>, CliError>` — The traverse clause: `--from`/`--from-id` anchor it; the companion
-  `non_empty` function L271-273 — `(values: &[String]) -> Option<Vec<String>>` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `print_results` function L276-306 — `(response: &SearchResponse)` — The human rendering: one CODE/TITLE/VER section per non-empty group.
-  `section` function L277-291 — `(label: &str, items: &[T])` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `tests` module L309-441 — `-` — (KAIROS-A-0007 / KAIROS-T-0037).
-  `flags_compose_the_s0005_body` function L315-352 — `()` — The filter flags compose the S-0005 body: metadata k=v parsing,
-  `traverse_flags` function L358-396 — `()` — Traversal flags: anchored by --from, direction defaults to
-  `query_json_escape_hatch` function L402-440 — `()` — --query-json is the verbatim escape hatch: parsed as the full

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
-  `Cli` struct L65-68 — `{ command: Command }` — 2 auth error.
-  `Command` enum L71-151 — `Login | Logout | Whoami | Orgs | Boards | Strategies | Initiatives | Tasks | Doc...` — 2 auth error.
-  `main` function L154-163 — `() -> ExitCode` — 2 auth error.
-  `run` function L165-191 — `(command: Command) -> Result<(), CliError>` — 2 auth error.
-  `login` function L195-287 — `( url: &str, issuer_override: Option<&str>, tenant: Option<String>, client_id: S...` — `kairos login` — discover the issuer, run the device grant, cache the
-  `load_store_for_login` function L291-302 — `(path: &std::path::Path) -> CredentialStore` — The store to merge a fresh login into: a corrupted cache is NOT fatal
-  `logout` function L305-329 — `(url: Option<&str>) -> Result<(), CliError>` — `kairos logout` — drop the deployment's entry from the cache.
-  `whoami` function L332-359 — `( url: Option<&str>, tenant_override: Option<String>, json: bool, ) -> Result<()...` — `kairos whoami` — the identity probe via `kairos-client`.
-  `print_identity` function L363-378 — `(identity: &WhoamiResponse)` — The human-readable `whoami` rendering: user, org, role, teams
-  `tests` module L381-654 — `-` — 2 auth error.
-  `smoke` function L385-387 — `()` — 2 auth error.
-  `cli_parses` function L392-453 — `()` — The clap surface parses per KAIROS-A-0015: login/logout/whoami with
-  `command_tree_parses` function L458-653 — `()` — The KAIROS-T-0037 command tree parses: every A-0015 noun with its

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
-  `cli_login_whoami_refresh_logout_live` function L152-431 — `()` — credentials to the form's URL, landing on `/device/callback`.

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
-  `cli_command_tree_golden_path_live` function L175-523 — `()` — members list → orgs show → teams list.

### crates/kairos-client/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-client/src/client.rs

- pub `TokenProvider` interface L54-58 — `{ fn bearer_token() }` — Supplies the bearer token for each request.
- pub `StaticToken` struct L62 — `-` — A fixed, never-refreshed token.
- pub `EntityKind` enum L74-80 — `Strategy | Initiative | Task | Document | Adr` — The five S-0005 entity families, for the endpoints that are generic
- pub `path_segment` function L84-92 — `(self) -> &'static str` — The URL path segment of the family (`/api/{segment}/...`).
- pub `KairosClient` struct L103-110 — `{ http: reqwest::Client, base_url: String, token: Arc<dyn TokenProvider>, tenant...` — The typed Kairos API client.
- pub `new` function L123-134 — `(base_url: impl Into<String>, token: Arc<dyn TokenProvider>) -> Self` — A client against `base_url` drawing tokens from `token`.
- pub `with_static_token` function L137-139 — `(base_url: impl Into<String>, token: impl Into<String>) -> Self` — A client with a fixed bearer token (integration tests).
- pub `with_tenant` function L144-147 — `(mut self, slug: impl Into<String>) -> Self` — Send `X-Tenant: {slug}` on every request (dev/test tenant
- pub `base_url` function L150-152 — `(&self) -> &str` — The configured base URL (no trailing slash).
- pub `tenant` function L155-157 — `(&self) -> Option<&str>` — The configured `X-Tenant` value, if any.
- pub `raw_request` function L275-297 — `( &self, method: Method, path_and_query: &str, body: Option<&Value>, ) -> Result...` — Protocol-level escape hatch: send an authorized request with an
- pub `list_boards` function L428-430 — `(&self, page: Pagination) -> Result<ListEnvelope<Board>, Error>` — `GET /api/boards`.
- pub `create_board` function L434-436 — `(&self, request: &CreateBoardRequest) -> Result<BoardDetail, Error>` — `POST /api/boards` — create a board seeded from the system defaults
- pub `get_board` function L439-441 — `(&self, board_id: &str) -> Result<BoardDetail, Error>` — `GET /api/boards/{id}` — board + columns + transition graph.
- pub `update_board` function L444-451 — `( &self, board_id: &str, request: &UpdateBoardRequest, ) -> Result<Board, Error>` — `PATCH /api/boards/{id}` — board settings.
- pub `delete_board` function L454-456 — `(&self, board_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/boards/{id}` (only when empty).
- pub `board_items` function L459-461 — `(&self, board_id: &str) -> Result<BoardItemsResponse, Error>` — `GET /api/boards/{id}/items` — every live item grouped by column.
- pub `list_columns` function L464-466 — `(&self, board_id: &str) -> Result<Vec<BoardColumn>, Error>` — `GET /api/boards/{id}/columns`.
- pub `add_column` function L469-476 — `( &self, board_id: &str, request: &CreateColumnRequest, ) -> Result<BoardColumn,...` — `POST /api/boards/{id}/columns`.
- pub `update_column` function L479-490 — `( &self, board_id: &str, column_id: &str, request: &UpdateColumnRequest, ) -> Re...` — `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
- pub `remove_column` function L493-500 — `( &self, board_id: &str, column_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/boards/{id}/columns/{col_id}` (only when empty).
- pub `list_transitions` function L503-506 — `(&self, board_id: &str) -> Result<Vec<BoardTransition>, Error>` — `GET /api/boards/{id}/transitions`.
- pub `add_transition` function L509-516 — `( &self, board_id: &str, request: &CreateTransitionRequest, ) -> Result<BoardTra...` — `POST /api/boards/{id}/transitions`.
- pub `remove_transition` function L519-528 — `( &self, board_id: &str, transition_id: &str, ) -> Result<OrgDeleteResponse, Err...` — `DELETE /api/boards/{id}/transitions/{transition_id}`.
- pub `list_board_members` function L531-533 — `(&self, board_id: &str) -> Result<Vec<BoardMember>, Error>` — `GET /api/boards/{id}/members` — members + capability grants.
- pub `add_board_member` function L537-544 — `( &self, board_id: &str, request: &AddBoardMemberRequest, ) -> Result<BoardMembe...` — `POST /api/boards/{id}/members` — grant capabilities
- pub `replace_capabilities` function L548-559 — `( &self, board_id: &str, user_id: &str, request: &ReplaceCapabilitiesRequest, ) ...` — `PATCH /api/boards/{id}/members/{user_id}` — replace the full
- pub `remove_board_member` function L562-569 — `( &self, board_id: &str, user_id: &str, ) -> Result<RemoveBoardMemberResponse, E...` — `DELETE /api/boards/{id}/members/{user_id}` — revoke everything.
- pub `list_teams` function L574-576 — `(&self, page: Pagination) -> Result<ListEnvelope<Team>, Error>` — `GET /api/teams`.
- pub `create_team` function L580-582 — `(&self, request: &CreateTeamRequest) -> Result<Team, Error>` — `POST /api/teams` — creates the team AND its delivery board
- pub `get_team` function L585-587 — `(&self, team_id: &str) -> Result<Team, Error>` — `GET /api/teams/{id}`.
- pub `update_team` function L590-596 — `( &self, team_id: &str, request: &UpdateTeamRequest, ) -> Result<Team, Error>` — `PATCH /api/teams/{id}`.
- pub `delete_team` function L599-601 — `(&self, team_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/teams/{id}`.
- pub `list_team_members` function L604-606 — `(&self, team_id: &str) -> Result<Vec<TeamMember>, Error>` — `GET /api/teams/{id}/members`.
- pub `add_team_member` function L609-616 — `( &self, team_id: &str, request: &AddTeamMemberRequest, ) -> Result<TeamMember, ...` — `POST /api/teams/{id}/members`.
- pub `remove_team_member` function L619-626 — `( &self, team_id: &str, user_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/teams/{id}/members/{user_id}`.
- pub `list_streams` function L631-636 — `( &self, page: Pagination, ) -> Result<ListEnvelope<DeliveryStream>, Error>` — `GET /api/delivery-streams`.
- pub `create_stream` function L639-644 — `( &self, request: &CreateStreamRequest, ) -> Result<DeliveryStream, Error>` — `POST /api/delivery-streams`.
- pub `get_stream` function L647-650 — `(&self, stream_id: &str) -> Result<DeliveryStream, Error>` — `GET /api/delivery-streams/{id}`.
- pub `update_stream` function L653-660 — `( &self, stream_id: &str, request: &UpdateStreamRequest, ) -> Result<DeliveryStr...` — `PATCH /api/delivery-streams/{id}`.
- pub `delete_stream` function L663-666 — `(&self, stream_id: &str) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/delivery-streams/{id}`.
- pub `list_stream_teams` function L669-672 — `(&self, stream_id: &str) -> Result<Vec<Team>, Error>` — `GET /api/delivery-streams/{id}/teams`.
- pub `add_stream_team` function L675-682 — `( &self, stream_id: &str, request: &AddStreamTeamRequest, ) -> Result<OrgDeleteR...` — `POST /api/delivery-streams/{id}/teams`.
- pub `remove_stream_team` function L685-694 — `( &self, stream_id: &str, team_id: &str, ) -> Result<OrgDeleteResponse, Error>` — `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
- pub `create_service_account` function L699-704 — `( &self, request: &crate::types_service_accounts::CreateServiceAccountRequest, )...` — `POST /api/service-accounts` — create a machine principal (org-admin).
- pub `list_service_accounts` function L707-711 — `( &self, ) -> Result<crate::types_service_accounts::ServiceAccountList, Error>` — `GET /api/service-accounts` — list the org's service accounts.
- pub `delete_service_account` function L714-719 — `( &self, id: &str, ) -> Result<crate::types_service_accounts::Deleted, Error>` — `DELETE /api/service-accounts/{id}` — delete a service account + its keys.
- pub `create_api_key` function L722-732 — `( &self, service_account_id: &str, request: &crate::types_service_accounts::Crea...` — `POST /api/service-accounts/{id}/keys` — mint a key (raw returned once).
- pub `list_api_keys` function L735-741 — `( &self, service_account_id: &str, ) -> Result<crate::types_service_accounts::Ap...` — `GET /api/service-accounts/{id}/keys` — list a service account's keys.
- pub `revoke_api_key` function L744-753 — `( &self, service_account_id: &str, key_id: &str, ) -> Result<crate::types_servic...` — `DELETE /api/service-accounts/{id}/keys/{key_id}` — revoke a key.
- pub `list_org_members` function L758-763 — `( &self, page: Pagination, ) -> Result<ListEnvelope<OrgMember>, Error>` — `GET /api/members`.
- pub `add_org_member` function L767-769 — `(&self, request: &AddOrgMemberRequest) -> Result<OrgMember, Error>` — `POST /api/members` — add by email (the user must have logged in
- pub `update_org_member` function L773-780 — `( &self, user_id: &str, request: &UpdateOrgMemberRequest, ) -> Result<OrgMember,...` — `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN`
- pub `remove_org_member` function L783-785 — `(&self, user_id: &str) -> Result<RemoveOrgMemberResponse, Error>` — `DELETE /api/members/{user_id}`.
- pub `list_tenants` function L791-796 — `( &self, page: Pagination, ) -> Result<ListEnvelope<TenantSummary>, Error>` — `GET /api/admin/tenants` (deployment-admin only; cross-tenant, no
- pub `create_tenant` function L799-804 — `( &self, request: &CreateTenantRequest, ) -> Result<TenantCreatedResponse, Error...` — `POST /api/admin/tenants` — provision a tenant (KAIROS-T-0008).
- pub `delete_tenant` function L809-820 — `( &self, slug: &str, confirm: Option<bool>, ) -> Result<TenantDeletedResponse, E...` — `DELETE /api/admin/tenants/{slug}` — destructive; requires
- pub `relationships` function L826-833 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<ItemRelationshipsRespon...` — `GET /api/{family}/{short_code}/relationships` — both directions,
- pub `create_relationship` function L836-841 — `( &self, request: &CreateRelationshipRequest, ) -> Result<Relationship, Error>` — `POST /api/relationships` (org admin, KAIROS-A-0006).
- pub `delete_relationship` function L844-850 — `( &self, relationship_id: &str, ) -> Result<DeletedResponse, Error>` — `DELETE /api/relationships/{id}` (org admin).
- pub `metadata` function L855-862 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<ItemMetadataResponse, E...` — `GET /api/{family}/{short_code}/metadata`.
- pub `update_metadata` function L866-874 — `( &self, kind: EntityKind, short_code: &str, request: &UpdateMetadataRequest, ) ...` — `PATCH /api/{family}/{short_code}/metadata` — typed upsert; `null`
- pub `list_metadata_definitions` function L879-884 — `( &self, page: Pagination, ) -> Result<ListEnvelope<MetadataDefinition>, Error>` — `GET /api/metadata-definitions`.
- pub `get_metadata_definition` function L887-889 — `(&self, id: &str) -> Result<MetadataDefinition, Error>` — `GET /api/metadata-definitions/{id}`.
- pub `create_metadata_definition` function L892-898 — `( &self, request: &CreateMetadataDefinitionRequest, ) -> Result<MetadataDefiniti...` — `POST /api/metadata-definitions` (org admin).
- pub `update_metadata_definition` function L901-908 — `( &self, id: &str, request: &UpdateMetadataDefinitionRequest, ) -> Result<Metada...` — `PATCH /api/metadata-definitions/{id}` (org admin).
- pub `delete_metadata_definition` function L912-915 — `(&self, id: &str) -> Result<DeletedResponse, Error>` — `DELETE /api/metadata-definitions/{id}` (org admin; 409
- pub `list_templates` function L920-922 — `(&self, page: Pagination) -> Result<ListEnvelope<Template>, Error>` — `GET /api/templates`.
- pub `get_template` function L925-927 — `(&self, id: &str) -> Result<TemplateDetail, Error>` — `GET /api/templates/{id}` — template + metadata associations.
- pub `create_template` function L930-935 — `( &self, request: &CreateTemplateRequest, ) -> Result<TemplateDetail, Error>` — `POST /api/templates` (org admin).
- pub `update_template` function L938-944 — `( &self, id: &str, request: &UpdateTemplateRequest, ) -> Result<TemplateDetail, ...` — `PATCH /api/templates/{id}` (org admin).
- pub `delete_template` function L947-949 — `(&self, id: &str) -> Result<DeletedResponse, Error>` — `DELETE /api/templates/{id}` (org admin; hard delete).
- pub `history` function L955-971 — `( &self, kind: EntityKind, short_code: &str, limit: Option<i64>, offset: Option<...` — `GET /api/{family}/{short_code}/history` — the version list,
- pub `history_snapshot` function L975-990 — `( &self, kind: EntityKind, short_code: &str, version: i32, ) -> Result<HistorySn...` — `GET /api/{family}/{short_code}/history?version=N` — one full
- pub `cascade_preview` function L998-1005 — `( &self, kind: EntityKind, short_code: &str, ) -> Result<CascadePreviewResponse,...` — `GET /api/{family}/{short_code}/cascade-preview` — the AUTHORITATIVE
- pub `activity` function L1010-1015 — `( &self, query: &ActivityQuery, ) -> Result<ListEnvelope<ActivityEntry>, Error>` — `GET /api/activity` with combinable filters (S-0005).
- pub `search` function L1020-1022 — `(&self, request: &SearchRequest) -> Result<SearchResponse, Error>` — `POST /api/search` — full-text + filter + traverse composition.
- pub `whoami` function L1027-1029 — `(&self) -> Result<WhoamiResponse, Error>` — `GET /api/whoami` — the resolved user/tenant/teams identity probe.
-  `StaticToken` type L64-69 — `impl TokenProvider for StaticToken` — typed surface deliberately cannot express.
-  `bearer_token` function L65-68 — `(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>` — typed surface deliberately cannot express.
-  `EntityKind` type L82-93 — `= EntityKind` — typed surface deliberately cannot express.
-  `EntityKind` type L95-99 — `= EntityKind` — typed surface deliberately cannot express.
-  `fmt` function L96-98 — `(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` — typed surface deliberately cannot express.
-  `KairosClient` type L112-119 — `= KairosClient` — typed surface deliberately cannot express.
-  `fmt` function L113-118 — `(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` — typed surface deliberately cannot express.
-  `KairosClient` type L121-298 — `= KairosClient` — typed surface deliberately cannot express.
-  `url` function L159-161 — `(&self, path: &str) -> String` — typed surface deliberately cannot express.
-  `authorize` function L164-174 — `( &self, builder: reqwest::RequestBuilder, ) -> Result<reqwest::RequestBuilder, ...` — Attach the bearer token and the optional X-Tenant header.
-  `execute` function L180-199 — `( &self, context: String, expect: u16, builder: reqwest::RequestBuilder, ) -> Re...` — Send an authorized request; decode the EXACT expected success
-  `get` function L201-204 — `(&self, path: &str) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `get_query` function L206-217 — `( &self, path: &str, query: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `post_created` function L220-231 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — POST expecting 201 Created (the S-0005 create family).
-  `post_ok` function L235-246 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — POST expecting 200 OK (actions on existing resources, e.g.
-  `patch` function L248-259 — `( &self, path: &str, body: &(impl Serialize + ?Sized), ) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `delete` function L261-268 — `(&self, path: &str) -> Result<T, Error>` — typed surface deliberately cannot express.
-  `entity_family` macro L301-341 — `-` — The five entity CRUD families (S-0005; KAIROS-T-0018 contracts).
-  `entity_transition` macro L344-362 — `-` — `POST /api/{family}/{short_code}/transition` for the on-board families.
-  `KairosClient` type L364-1030 — `= KairosClient` — typed surface deliberately cannot express.

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
- pub `types_org` module L14 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_search` module L15 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_service_accounts` module L16 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `ws` module L17 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_meta` module L19 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `types_events` module L21 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
- pub `CRATE_NAME` variable L28 — `: &str` — Placeholder marker kept for early consumers (KAIROS-I-0003).
-  `tests` module L31-59 — `-` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
-  `smoke` function L35-37 — `()` — to variants, and the [`ws`] `/ws/events` helper ([`EventStream`]).
-  `envelopes_round_trip` function L41-58 — `()` — The envelopes round-trip through serde with the S-0005 field names.

#### crates/kairos-client/src/types.rs

- pub `Strategy` struct L29-53 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — A strategy (Flight Level 3), as returned by `/api/strategies`.
- pub `Initiative` struct L57-85 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — An initiative (Flight Level 2), as returned by `/api/initiatives`.
- pub `Task` struct L89-115 — `{ id: String, short_code: String, title: String, content: String, board_id: Stri...` — A task/bug/tech-debt item (Flight Level 1), as returned by `/api/tasks`.
- pub `Document` struct L121-141 — `{ id: String, short_code: String, title: String, content: String, template_id: O...` — A supporting document, as returned by `/api/documents`.
- pub `Adr` struct L146-171 — `{ id: String, short_code: String, title: String, content: String, board_id: Opti...` — An Architecture Decision Record, as returned by `/api/adrs`.
- pub `CreateStrategyRequest` struct L179-191 — `{ board_id: String, column_id: Option<String>, title: String, content: String, h...` — Body of `POST /api/strategies`.
- pub `CreateInitiativeRequest` struct L195-212 — `{ board_id: String, column_id: Option<String>, title: String, content: String, c...` — Body of `POST /api/initiatives`.
- pub `CreateTaskRequest` struct L216-232 — `{ board_id: String, column_id: Option<String>, title: String, content: String, t...` — Body of `POST /api/tasks`.
- pub `CreateDocumentRequest` struct L239-252 — `{ title: String, content: Option<String>, template_id: Option<String>, parent_sh...` — Body of `POST /api/documents`.
- pub `CreateAdrRequest` struct L258-275 — `{ board_id: Option<String>, column_id: Option<String>, title: String, content: S...` — Body of `POST /api/adrs`.
- pub `UpdateContentRequest` struct L282-290 — `{ title: Option<String>, content: String, version: i32 }` — Body of `PATCH /api/{family}/{short_code}` — the KAIROS-A-0004
- pub `TransitionRequest` struct L294-299 — `{ to_column_id: String }` — Body of `POST /api/{family}/{short_code}/transition`.
- pub `ListEnvelope` struct L307-315 — `{ items: Vec<T>, total: i64, limit: i64, offset: i64 }` — The S-0005 list envelope: `{items, total, limit, offset}`.
- pub `Pagination` struct L321-328 — `{ limit: Option<i64>, offset: Option<i64> }` — `?limit=&offset=` pagination for list endpoints (S-0005: the only list
- pub `DeleteResponse` struct L333-340 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — Response of `DELETE /api/{family}/{short_code}` — the soft delete and
- pub `CascadePreviewResponse` struct L350-358 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — Response of `GET /api/{entity_type}/{short_code}/cascade-preview`
- pub `ErrorEnvelope` struct L362-364 — `{ error: ErrorBody }` — The S-0005 error envelope: `{"error": {"code", "message", "details"}}`.
- pub `ErrorBody` struct L368-379 — `{ code: String, message: String, details: serde_json::Value }` — The `error` object of [`ErrorEnvelope`].

#### crates/kairos-client/src/types_events.rs

- pub `ThinEvent` struct L19-37 — `{ event: String, entity_type: String, short_code: String, board_id: Option<Strin...` — One thin change event pushed by the server (S-0005 shape).
- pub `SubscribeRequest` struct L42-45 — `{ subscribe: SubscribeFilter }` — Client → server message: `{"subscribe": {"board_id": "uuid"}}` filters
- pub `SubscribeFilter` struct L49-54 — `{ board_id: Option<String> }` — The [`SubscribeRequest`] filter body.

#### crates/kairos-client/src/types_meta.rs

- pub `RelatedItem` struct L22-33 — `{ relationship_id: String, id: String, short_code: String, entity_type: String, ...` — One hydrated neighbor of an item in the relationship graph.
- pub `RelationshipGroup` struct L37-42 — `{ relationship: String, items: Vec<RelatedItem> }` — All of an item's neighbors under ONE relationship type in one direction.
- pub `ItemRelationshipsResponse` struct L47-56 — `{ short_code: String, outgoing: Vec<RelationshipGroup>, incoming: Vec<Relationsh...` — Response of `GET /api/{entity_type}/{short_code}/relationships`: both
- pub `CreateRelationshipRequest` struct L60-67 — `{ source_short_code: String, target_short_code: String, relationship: String }` — Body of `POST /api/relationships` (org admin only, KAIROS-A-0006).
- pub `Relationship` struct L71-82 — `{ id: String, source_id: String, target_id: String, relationship: String, create...` — A relationship edge, as returned by `POST /api/relationships`.
- pub `MetadataValue` struct L90-101 — `{ definition_id: String, slug: String, name: String, field_type: String, value: ...` — One typed metadata value on an item, hydrated with its definition.
- pub `ItemMetadataResponse` struct L105-110 — `{ short_code: String, values: Vec<MetadataValue> }` — Response of `GET/PATCH /api/{entity_type}/{short_code}/metadata`.
- pub `UpdateMetadataRequest` struct L117-120 — `{ values: BTreeMap<String, Option<String>> }` — Body of `PATCH /api/{entity_type}/{short_code}/metadata`: definition
- pub `MetadataDefinition` struct L128-143 — `{ id: String, name: String, slug: String, field_type: String, is_system_default:...` — A metadata field definition, as returned by `/api/metadata-definitions`.
- pub `CreateMetadataDefinitionRequest` struct L147-155 — `{ name: String, slug: String, field_type: String, enum_options: Vec<String> }` — Body of `POST /api/metadata-definitions` (org admin).
- pub `UpdateMetadataDefinitionRequest` struct L161-169 — `{ name: Option<String>, slug: Option<String>, enum_options: Option<Vec<String>> ...` — Body of `PATCH /api/metadata-definitions/{id}` (org admin).
- pub `Template` struct L177-190 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — A document template, as returned by `GET /api/templates`.
- pub `TemplateMetadataField` struct L195-209 — `{ definition_id: String, slug: String, name: String, field_type: String, enum_op...` — One metadata field a template carries (`template_metadata` hydrated
- pub `TemplateDetail` struct L214-228 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — Response of `GET /api/templates/{id}`: the template plus its associated
- pub `TemplateMetadataEntry` struct L232-240 — `{ definition_slug: String, default_value: Option<String>, required: bool }` — One template ↔ metadata-definition association in a template write.
- pub `CreateTemplateRequest` struct L244-253 — `{ name: String, slug: String, content: String, metadata: Vec<TemplateMetadataEnt...` — Body of `POST /api/templates` (org admin).
- pub `UpdateTemplateRequest` struct L258-268 — `{ name: Option<String>, slug: Option<String>, content: Option<String>, metadata:...` — Body of `PATCH /api/templates/{id}` (org admin).
- pub `DeletedResponse` struct L274-277 — `{ id: String }` — Response of the T-0020 hard-delete endpoints
- pub `HistoryVersion` struct L285-292 — `{ version: i32, edited_by: String, edited_at: String }` — One row of `GET /api/{entity_type}/{short_code}/history`.
- pub `HistorySnapshot` struct L297-307 — `{ version: i32, title: String, content: String, edited_by: String, edited_at: St...` — Response of `GET /api/{entity_type}/{short_code}/history?version=N`:
- pub `HistoryQuery` struct L312-322 — `{ version: Option<i32>, limit: Option<i64>, offset: Option<i64> }` — Query of `GET /api/{entity_type}/{short_code}/history`.
- pub `ActivityEntry` struct L330-346 — `{ id: String, actor_id: String, action: String, entity_id: Option<String>, entit...` — One `activity_log` row, as returned by `GET /api/activity`.
- pub `ActivityQuery` struct L351-370 — `{ entity_id: Option<String>, actor_id: Option<String>, action: Option<String>, s...` — Query of `GET /api/activity` (S-0005: all filters combinable).
-  `tests` module L373-413 — `-` — owns the model → DTO encoding.
-  `update_metadata_request_round_trips` function L379-386 — `()` — The metadata PATCH body round-trips: string values stay, `null`
-  `relationships_response_shape` function L390-412 — `()` — Grouped relationships serialize with the S-0005 field names.

#### crates/kairos-client/src/types_org.rs

- pub `Board` struct L22-35 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — A board (`/api/boards` list element).
- pub `BoardColumn` struct L39-51 — `{ id: String, board_id: String, name: String, position: i32, created_at: String,...` — A board column.
- pub `BoardTransition` struct L55-64 — `{ id: String, board_id: String, from_column_id: String, to_column_id: String }` — An allowed column-to-column transition edge.
- pub `BoardDetail` struct L69-76 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — Board detail: the board plus its full configuration
- pub `CreateBoardRequest` struct L81-89 — `{ name: String, slug: String, board_level: String, team_id: Option<String> }` — Body of `POST /api/boards`: creates a board seeded with the system
- pub `UpdateBoardRequest` struct L93-98 — `{ name: Option<String>, slug: Option<String> }` — Body of `PATCH /api/boards/{id}` (board settings).
- pub `BoardColumnItems` struct L103-109 — `{ column: BoardColumn, strategies: Vec<Strategy>, initiatives: Vec<Initiative>, ...` — One column's items in the `GET /api/boards/{id}/items` view: every live
- pub `BoardItemsResponse` struct L114-117 — `{ board: Board, columns: Vec<BoardColumnItems> }` — Response of `GET /api/boards/{id}/items`: all items on the board,
- pub `CreateColumnRequest` struct L121-126 — `{ name: String, position: i32 }` — Body of `POST /api/boards/{id}/columns`.
- pub `UpdateColumnRequest` struct L130-136 — `{ name: Option<String>, position: Option<i32> }` — Body of `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
- pub `CreateTransitionRequest` struct L140-145 — `{ from_column_id: String, to_column_id: String }` — Body of `POST /api/boards/{id}/transitions`.
- pub `BoardMember` struct L154-161 — `{ user_id: String, email: String, display_name: String, capabilities: Vec<String...` — A board member and their capability grants
- pub `AddBoardMemberRequest` struct L165-171 — `{ user_id: String, capabilities: Vec<String> }` — Body of `POST /api/boards/{id}/members`.
- pub `ReplaceCapabilitiesRequest` struct L176-180 — `{ capabilities: Vec<String> }` — Body of `PATCH /api/boards/{id}/members/{user_id}` — replaces the user's
- pub `RemoveBoardMemberResponse` struct L185-190 — `{ user_id: String, revoked_capabilities: Vec<String> }` — Response of `DELETE /api/boards/{id}/members/{user_id}` — full
- pub `OrgDeleteResponse` struct L195-199 — `{ id: String, deleted: bool }` — Generic delete acknowledgement for organizational resources (boards,
- pub `Team` struct L209-222 — `{ id: String, name: String, slug: String, team_type: String, delivery_board_id: ...` — A team (`/api/teams`).
- pub `CreateTeamRequest` struct L228-235 — `{ name: String, slug: String, team_type: Option<String> }` — Body of `POST /api/teams`.
- pub `UpdateTeamRequest` struct L239-246 — `{ name: Option<String>, slug: Option<String>, team_type: Option<String> }` — Body of `PATCH /api/teams/{id}`.
- pub `TeamMember` struct L250-257 — `{ user_id: String, email: String, display_name: String, joined_at: String }` — A team member (`GET /api/teams/{id}/members` element).
- pub `AddTeamMemberRequest` struct L261-264 — `{ user_id: String }` — Body of `POST /api/teams/{id}/members`.
- pub `DeliveryStream` struct L272-282 — `{ id: String, name: String, slug: String, description: Option<String>, created_a...` — A delivery stream (`/api/delivery-streams`).
- pub `CreateStreamRequest` struct L286-291 — `{ name: String, slug: String, description: Option<String> }` — Body of `POST /api/delivery-streams`.
- pub `UpdateStreamRequest` struct L295-302 — `{ name: Option<String>, slug: Option<String>, description: Option<String> }` — Body of `PATCH /api/delivery-streams/{id}`.
- pub `AddStreamTeamRequest` struct L306-309 — `{ team_id: String }` — Body of `POST /api/delivery-streams/{id}/teams`.
- pub `OrgMember` struct L317-328 — `{ user_id: String, external_id: String, email: String, display_name: String, rol...` — An organization member (`GET /api/members` element).
- pub `AddOrgMemberRequest` struct L334-339 — `{ email: String, role: Option<String> }` — Body of `POST /api/members`.
- pub `UpdateOrgMemberRequest` struct L344-347 — `{ role: String }` — Body of `PATCH /api/members/{user_id}` — role change.
- pub `RemoveOrgMemberResponse` struct L351-355 — `{ user_id: String, removed: bool }` — Response of `DELETE /api/members/{user_id}`.
- pub `WhoamiResponse` struct L364-377 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — Response of `GET /api/whoami`: everything the auth → tenant stack
- pub `WhoamiBoardCapabilities` struct L383-391 — `{ board_id: String, board_slug: String, grants: Vec<String> }` — One board on which the caller holds explicit capability grants
- pub `WhoamiUser` struct L395-402 — `{ id: String, external_id: String, email: String, display_name: String }` — The `user` object of [`WhoamiResponse`].
- pub `WhoamiOrganization` struct L406-412 — `{ id: String, slug: String, role: String }` — The `organization` object of [`WhoamiResponse`].
- pub `WhoamiTeam` struct L416-421 — `{ id: String, slug: String, name: String }` — One team of [`WhoamiResponse::teams`].
- pub `CreateTenantRequest` struct L430-440 — `{ slug: String, name: String, initial_admin_external_id: Option<String> }` — Body of `POST /api/admin/tenants` (deployment-admin only; see
- pub `TenantInitialAdmin` struct L444-452 — `{ user_id: String, external_id: String, email: String, role: String }` — The org-admin membership created with a new tenant.
- pub `TenantCreatedResponse` struct L457-469 — `{ slug: String, schema: String, migrations_applied: Vec<String>, boards_created:...` — Response of `POST /api/admin/tenants` — the T-0008 provisioning report
- pub `TenantSummary` struct L473-478 — `{ slug: String, name: String, schema_exists: bool }` — A provisioned tenant (`GET /api/admin/tenants` element).
- pub `TenantDeletedResponse` struct L482-485 — `{ slug: String, dropped: bool }` — Response of `DELETE /api/admin/tenants/{slug}`.

#### crates/kairos-client/src/types_search.rs

- pub `SearchRequest` struct L33-54 — `{ q: Option<String>, filter: Option<SearchFilter>, traverse: Option<SearchTraver...` — Body of `POST /api/search`.
- pub `SearchFilter` struct L60-98 — `{ entity_type: Option<Vec<String>>, board_id: Option<String>, column_id: Option<...` — The `filter` capability.
- pub `SearchTraverse` struct L104-118 — `{ from: SearchTraverseFrom, relationships: Vec<String>, direction: String, depth...` — The `traverse` capability: recursive walk of the relationship graph from
- pub `SearchTraverseFrom` struct L123-130 — `{ short_code: Option<String>, id: Option<String> }` — `traverse.from`: exactly one of `short_code`/`id`.
- pub `SearchSort` struct L136-141 — `{ field: String, order: String }` — The `sort` clause, applied to the combined cross-type result set before
- pub `SearchResponse` struct L151-161 — `{ results: SearchResultGroups, total: i64, limit: i64, offset: i64 }` — Response of `POST /api/search`: results grouped by entity type plus the
- pub `SearchResultGroups` struct L167-183 — `{ strategies: Vec<Strategy>, initiatives: Vec<Initiative>, tasks: Vec<Task>, doc...` — The `results` object: one fully-typed group per entity type, each group
-  `tests` module L186-255 — `-` — at the boundary, never silently ignored (matching the core model).
-  `s0005_request_round_trips` function L192-223 — `()` — The S-0005 Unified Search request example parses field for field and
-  `unknown_fields_are_rejected` function L227-235 — `()` — Unknown fields are rejected, mirroring the core model.
-  `empty_groups_are_omitted` function L240-254 — `()` — Empty groups vanish from the serialized response; present groups and

#### crates/kairos-client/src/types_service_accounts.rs

- pub `CreateServiceAccountRequest` struct L9-11 — `{ name: String }` — `POST /api/service-accounts` body.
- pub `ServiceAccount` struct L15-19 — `{ id: String, name: String, created_at: String }` — A service account (never carries a secret).
- pub `ServiceAccountList` struct L23-26 — `{ items: Vec<ServiceAccount>, total: i64 }` — `GET /api/service-accounts` envelope.
- pub `CreateApiKeyRequest` struct L30-35 — `{ name: String, expires_at: Option<String> }` — `POST /api/service-accounts/{id}/keys` body.
- pub `ApiKeyCreated` struct L40-47 — `{ id: String, name: String, key: String, prefix: String, created_at: String, exp...` — `POST /api/service-accounts/{id}/keys` response — the ONLY place the raw
- pub `ApiKey` struct L51-59 — `{ id: String, name: String, prefix: String, created_at: String, expires_at: Opti...` — One key row (`GET /api/service-accounts/{id}/keys`) — prefix only.
- pub `ApiKeyList` struct L63-66 — `{ items: Vec<ApiKey>, total: i64 }` — `GET /api/service-accounts/{id}/keys` envelope.
- pub `Deleted` struct L70-73 — `{ id: String, deleted: bool }` — The `DELETE` response for a service account or a key.

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
- pub `TenantConfigResource` enum L111-118 — `Templates | MetadataDefinitions | Relationships` — Tenant-wide configuration resources that live on no board.
- pub `org_admin_only` function L126-128 — `(self) -> bool` — Whether writes to this resource require `organization_members.role =
- pub `capability_matches` function L144-151 — `(granted: &str, required: &str) -> bool` — Does the stored grant `granted` satisfy the `required` capability?
- pub `is_authorized` function L179-183 — `(grants: &[String], required: &str) -> bool` — The A-0006 decision over a user's loaded grants for one board: allowed
-  `TenantConfigResource` type L120-129 — `= TenantConfigResource` — grant time by API-layer validation, not relied upon.
-  `glob_match` function L155-174 — `(pattern: &str, text: &str) -> bool` — LIKE-style match: `*` in `pattern` matches any (possibly empty) sequence;
-  `tests` module L186-374 — `-` — grant time by API-layer validation, not relied upon.
-  `exact_capability_matches_itself_only` function L192-204 — `()` — grant time by API-layer validation, not relied upon.
-  `bare_star_grants_every_capability` function L209-216 — `()` — grant time by API-layer validation, not relied upon.
-  `manage_glob_matches_manage_capabilities_only` function L221-241 — `()` — grant time by API-layer validation, not relied upon.
-  `configure_and_transition_globs` function L244-252 — `()` — grant time by API-layer validation, not relied upon.
-  `non_matching_prefixes_are_rejected` function L255-261 — `()` — grant time by API-layer validation, not relied upon.
-  `empty_strings_mirror_sql_like` function L266-276 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_percent_is_literal_not_wildcard` function L281-289 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_underscore_is_literal_not_single_char_wildcard` function L292-298 — `()` — grant time by API-layer validation, not relied upon.
-  `stored_backslash_is_literal` function L301-306 — `()` — grant time by API-layer validation, not relied upon.
-  `embedded_star_wildcards_like_the_sql_translation` function L309-322 — `()` — grant time by API-layer validation, not relied upon.
-  `is_authorized_is_a_whitelist_over_grants` function L327-339 — `()` — grant time by API-layer validation, not relied upon.
-  `tenant_config_resources_are_org_admin_only` function L344-352 — `()` — grant time by API-layer validation, not relied upon.
-  `team_implies_only_the_delivery_set` function L358-373 — `()` — KAIROS-T-0072: team membership implies exactly the day-to-day

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
-  `tests` module L79-176 — `-` — and persists the outcome (the same layering as `board`/`abac`).
-  `version_check_matches_a0004_contract` function L83-98 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `cascade_walks_chain` function L101-118 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `cascade_ignores_unrelated_edges_and_dedups_diamonds` function L121-156 — `()` — and persists the outcome (the same layering as `board`/`abac`).
-  `cascade_tolerates_cycles_and_excludes_root` function L159-175 — `()` — and persists the outcome (the same layering as `board`/`abac`).

#### crates/kairos-core/src/lib.rs

- pub `abac` module L7 — `-` — transitions, ABAC capability checks, and short-code generation.
- pub `board` module L8 — `-` — `kairos-db` and `kairos-server`.
- pub `graph` module L9 — `-` — `kairos-db` and `kairos-server`.
- pub `items` module L10 — `-` — `kairos-db` and `kairos-server`.
- pub `retention` module L11 — `-` — `kairos-db` and `kairos-server`.
- pub `search` module L12 — `-` — `kairos-db` and `kairos-server`.
- pub `short_code` module L13 — `-` — `kairos-db` and `kairos-server`.

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

- pub `MAX_TRAVERSE_DEPTH` variable L47 — `: u32` — Server cap on `traverse.depth` (A-0007: "server-capped to prevent
- pub `MAX_LIMIT` variable L50 — `: i64` — Server cap on `limit`.
- pub `DEFAULT_LIMIT` variable L54 — `: i64` — Default `limit` when the request omits it (the S-0005 examples' page
- pub `SearchRequest` struct L65-84 — `{ q: Option<String>, filter: Option<SearchFilter>, traverse: Option<Traverse>, s...` — The `POST /api/search` request body (KAIROS-A-0007 / S-0005).
- pub `effective_limit` function L88-90 — `(&self) -> i64` — The page size to use: `limit` or [`DEFAULT_LIMIT`].
- pub `effective_offset` function L93-95 — `(&self) -> i64` — The offset to use: `offset` or 0.
- pub `effective_sort` function L98-103 — `(&self) -> Sort` — The sort to use: `sort` or `created_at desc`.
- pub `SearchFilter` struct L110-147 — `{ entity_type: Option<Vec<SearchEntityType>>, board_id: Option<Uuid>, column_id:...` — The `filter` capability (A-0007: fields are AND with each other; array
- pub `is_constraining` function L153-163 — `(&self) -> bool` — Whether this filter narrows results at all.
- pub `SearchEntityType` enum L169-180 — `Strategy | Initiative | Task | Document | Adr` — The `filter.entity_type` vocabulary (the five S-0004 entity tables).
- pub `item_type` function L184-192 — `(self) -> ItemType` — The corresponding [`ItemType`].
- pub `SearchTaskType` enum L198-205 — `Task | Bug | TechDebt` — The `filter.task_type` vocabulary (`tasks.task_type` CHECK set).
- pub `as_str` function L209-215 — `(self) -> &'static str` — The TEXT value stored in `tasks.task_type`.
- pub `Traverse` struct L222-236 — `{ from: TraverseFrom, relationships: Vec<SearchRelationship>, direction: Directi...` — The `traverse` capability (A-0007: recursive walk of
- pub `TraverseFrom` struct L242-249 — `{ short_code: Option<String>, id: Option<Uuid> }` — `traverse.from`: exactly one of `short_code`/`id` (enforced by
- pub `SearchRelationship` enum L255-266 — `Parent | Supports | Informs | Supersedes | Blocks` — The `traverse.relationships` vocabulary (`item_relationships.
- pub `as_str` function L270-278 — `(self) -> &'static str` — The TEXT value stored in `item_relationships.relationship`.
- pub `Direction` enum L284-291 — `Outbound | Inbound | Both` — `traverse.direction` (A-0007).
- pub `Sort` struct L297-302 — `{ field: SortField, order: SortOrder }` — The `sort` clause; applies to the combined cross-type result set before
- pub `SortField` enum L308-315 — `CreatedAt | UpdatedAt | Title` — Sortable fields — attributes every entity type carries, so the combined
- pub `SortOrder` enum L320-325 — `Asc | Desc` — Sort direction.
- pub `SearchValidationError` enum L333-392 — `NoCapability | BlankQuery | EmptyEntityTypes | EmptyTaskTypes | BlankMetadataKey...` — A structurally invalid search request (HTTP 400 at the API layer).
- pub `validate` function L397-473 — `(request: &SearchRequest) -> Result<(), SearchValidationError>` — Validate a [`SearchRequest`] against the module-docs contract.
- pub `metadata_like_pattern` function L483-495 — `(value: &str) -> String` — Translate a metadata filter value into a SQL LIKE pattern (module docs):
-  `SearchRequest` type L86-104 — `= SearchRequest` — translates to a LIKE pattern equivalent to equality.
-  `SearchFilter` type L149-164 — `= SearchFilter` — translates to a LIKE pattern equivalent to equality.
-  `SearchEntityType` type L182-193 — `= SearchEntityType` — translates to a LIKE pattern equivalent to equality.
-  `SearchTaskType` type L207-216 — `= SearchTaskType` — translates to a LIKE pattern equivalent to equality.
-  `SearchRelationship` type L268-279 — `= SearchRelationship` — translates to a LIKE pattern equivalent to equality.
-  `tests` module L498-880 — `-` — translates to a LIKE pattern equivalent to equality.
-  `q` function L501-506 — `(text: &str) -> SearchRequest` — translates to a LIKE pattern equivalent to equality.
-  `traverse` function L508-518 — `(depth: Option<u32>) -> Traverse` — translates to a LIKE pattern equivalent to equality.
-  `empty_request_is_no_capability` function L523-528 — `()` — translates to a LIKE pattern equivalent to equality.
-  `non_constraining_filter_is_no_capability` function L531-551 — `()` — translates to a LIKE pattern equivalent to equality.
-  `each_capability_alone_is_valid` function L554-569 — `()` — translates to a LIKE pattern equivalent to equality.
-  `blank_q_is_rejected_even_alongside_other_capabilities` function L572-578 — `()` — translates to a LIKE pattern equivalent to equality.
-  `empty_or_lists_are_rejected` function L583-606 — `()` — translates to a LIKE pattern equivalent to equality.
-  `blank_metadata_keys_are_rejected` function L609-621 — `()` — translates to a LIKE pattern equivalent to equality.
-  `date_ranges_must_be_sane` function L624-661 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_depth_is_required_and_capped` function L666-685 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_from_names_exactly_one_reference` function L688-716 — `()` — translates to a LIKE pattern equivalent to equality.
-  `traverse_relationships_must_be_non_empty` function L719-731 — `()` — translates to a LIKE pattern equivalent to equality.
-  `limit_is_capped_and_offset_non_negative` function L736-765 — `()` — translates to a LIKE pattern equivalent to equality.
-  `effective_defaults` function L768-775 — `()` — translates to a LIKE pattern equivalent to equality.
-  `full_s0005_request_deserializes` function L780-828 — `()` — translates to a LIKE pattern equivalent to equality.
-  `unknown_vocabulary_is_rejected_by_serde` function L831-861 — `()` — translates to a LIKE pattern equivalent to equality.
-  `metadata_like_pattern_translates_globs_and_escapes_metacharacters` function L866-879 — `()` — translates to a LIKE pattern equivalent to equality.

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
- pub `authorize` function L154-165 — `( conn: &mut PgConnection, org_slug: &str, board_id: Uuid, user_id: Uuid, requir...` — The combined KAIROS-A-0006 write-authorization decision for a board
- pub `grant_capability` function L193-234 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, capability: &str, gran...` — Grant `capability` to `user_id` on `board_id`, writing the
- pub `revoke_capability` function L239-273 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, capability: &str, revo...` — Revoke `capability` from `user_id` on `board_id`, writing the
- pub `resolve_authorization_board` function L329-363 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Option<Uuid>, AbacError>` — Which board authorizes writes to `item_id` (KAIROS-A-0006 access-check
-  `BoolRow` struct L73-76 — `{ authorized: bool }` — mutation (KAIROS-A-0004/S-0004 action list).
-  `log_capability_activity` function L170-188 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, board_id: Uui...` — Insert one `activity_log` row (current tenant schema; same shape as
-  `board_of_workflow_item` function L278-312 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<Option<Uuid>, DieselError>` — The board a live workflow item (strategy/initiative/task/ADR) sits on.
-  `try_table` macro L284-296 — `-` — mutation (KAIROS-A-0004/S-0004 action list).

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

- pub `BoardError` enum L39-79 — `BoardNotFound | ColumnNotFound | ItemNotFound | ItemNotOnBoard | TransitionNotFo...` — Errors from board creation, item transitions, or board configuration.
- pub `create_board` function L172-239 — `( conn: &mut PgConnection, level: BoardLevel, name: &str, slug: &str, team_id: O...` — Create a board in the current tenant schema, seeding its columns and
- pub `transition_adr` function L345-402 — `( conn: &mut PgConnection, item_id: Uuid, to_column_id: Uuid, actor_id: Uuid, ) ...` — Move an ADR to another column of its board.
- pub `add_column` function L410-442 — `( conn: &mut PgConnection, board_id: Uuid, name: &str, position: i32, actor_id: ...` — Add a column to a board (unique name and position enforced by
- pub `rename_column` function L446-479 — `( conn: &mut PgConnection, column_id: Uuid, new_name: &str, actor_id: Uuid, ) ->...` — Rename a column (name uniqueness enforced by
- pub `remove_column` function L486-513 — `( conn: &mut PgConnection, column_id: Uuid, actor_id: Uuid, ) -> Result<(), Boar...` — Remove a column.
- pub `reorder_columns` function L518-559 — `( conn: &mut PgConnection, board_id: Uuid, new_order: &[Uuid], actor_id: Uuid, )...` — Reorder a board's columns.
- pub `add_transition` function L564-599 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Add a transition edge to a board
- pub `remove_transition` function L602-641 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Remove a transition edge from a board.
- pub `dead_end_columns` function L647-653 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<Vec<rules::ColumnRef>, Bo...` — Columns of `board_id` with no outbound transitions, in position order —
-  `log_activity` function L82-100 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Op...` — Insert one `activity_log` row (current tenant schema).
-  `load_board_rules` function L105-149 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<(Vec<rules::Column>, Vec<...` — Load a board's columns and transition edges as core rule inputs.
-  `column_name` function L151-157 — `(columns: &[rules::Column], id: Uuid) -> Result<String, BoardError>` — NULL, so system-provisioned boards write no activity row).
-  `validate_transition` function L247-259 — `( conn: &mut PgConnection, board_id: Uuid, from_column_id: Uuid, to_column_id: U...` — Validate a move against the board's transition graph and return the
-  `transition_item_fn` macro L266-321 — `-` — Generate `transition_<entity>` for an item table with NOT NULL
-  `column_board_id` function L656-664 — `(conn: &mut PgConnection, column_id: Uuid) -> Result<Uuid, BoardError>` — The `board_id` of a column, or [`BoardError::ColumnNotFound`].
-  `count_items_in_column` function L669-690 — `(conn: &mut PgConnection, column_id: Uuid) -> Result<u64, BoardError>` — How many workflow items reference `column_id` across every entity table.

#### crates/kairos-db/src/events.rs

- pub `EVENT_CHANNEL` variable L53 — `: &str` — The one NOTIFY channel every Kairos event travels on (A-0005 §5).
- pub `EventKind` enum L57-70 — `ItemCreated | ItemUpdated | ItemTransitioned | ItemDeleted | RelationshipChanged...` — The S-0005 event vocabulary.
- pub `as_str` function L74-83 — `(self) -> &'static str` — The wire name (`event` field).
- pub `ThinEvent` struct L88-101 — `{ event: EventKind, entity_type: String, short_code: String, board_id: Option<Uu...` — One thin change event (S-0005 shape, before tenant tagging).
- pub `emit_event` function L123-148 — `(conn: &mut PgConnection, event: &ThinEvent) -> Result<(), DieselError>` — Emit `event` on [`EVENT_CHANNEL`], tagged with the current connection's
- pub `ItemPlacement` type L151 — `= (String, Option<Uuid>, Option<Uuid>)` — [`item_placement`]'s row: `(short_code, board_id, column_id)`.
- pub `item_placement` function L157-178 — `( conn: &mut PgConnection, entity_type: &str, item_id: Uuid, ) -> Result<Option<...` — `(short_code, board_id, column_id)` of an item by id, straight from its
- pub `emit_item_event_by_id` function L183-204 — `( conn: &mut PgConnection, event: EventKind, entity_type: &str, item_id: Uuid, a...` — Convenience for call sites that only hold an item id: look up the
-  `EventKind` type L72-84 — `= EventKind` — short-code prefix.
-  `SchemaName` struct L104-107 — `{ name: String }` — short-code prefix.
-  `PlacementRow` struct L110-117 — `{ short_code: String, board_id: Option<Uuid>, column_id: Option<Uuid> }` — short-code prefix.

#### crates/kairos-db/src/graph.rs

- pub `GraphError` enum L53-92 — `ItemNotFound | SelfLink | Rule | CycleDetected | AlreadyLinked | NotLinked | Dat...` — Errors from the relationship-graph services.
- pub `link_items` function L203-273 — `( conn: &mut PgConnection, source_id: Uuid, target_id: Uuid, relationship: Relat...` — Create a `relationship` edge from `source_id` to `target_id`
- pub `unlink_items` function L281-338 — `( conn: &mut PgConnection, source_id: Uuid, target_id: Uuid, relationship: Relat...` — Remove the `relationship` edge from `source_id` to `target_id`, in ONE
- pub `Neighbor` struct L344-355 — `{ relationship: RelationshipType, id: Uuid, short_code: String, entity_type: Ite...` — One neighbor of an item in the relationship graph, hydrated through
- pub `ItemRelationships` struct L360-369 — `{ outgoing: Vec<Neighbor>, incoming: Vec<Neighbor> }` — Both directions of an item's relationships, each grouped by
- pub `relationships_for` function L428-436 — `( conn: &mut PgConnection, item_id: Uuid, ) -> Result<ItemRelationships, GraphEr...` — Every relationship touching `item_id`, in BOTH directions, grouped by
-  `core_relationship` function L96-104 — `(relationship: RelationshipType) -> rules::Relationship` — The pure mirror of a stored [`RelationshipType`] (kairos-core carries no
-  `parse_entity_type` function L108-121 — `(value: &str) -> Result<ItemType, GraphError>` — Parse an `entity_directory.entity_type` value.
-  `DirectoryRow` struct L124-129 — `{ entity_type: String, short_code: String }` — [`crate::abac::grant_capability`].
-  `resolve_entity` function L134-145 — `( conn: &mut PgConnection, id: Uuid, ) -> Result<Option<(ItemType, String)>, Gra...` — Resolve a UUID to its live entity type and short code via
-  `log_relationship_activity` function L149-167 — `( conn: &mut PgConnection, actor: Uuid, action: ActivityAction, relationship: Re...` — Insert one relationship `activity_log` row: `entity_id`/`entity_type`
-  `load_edges` function L171-187 — `( conn: &mut PgConnection, relationship: RelationshipType, ) -> Result<Vec<rules...` — All existing edges of ONE relationship type in the current tenant
-  `NeighborRow` struct L372-383 — `{ relationship: RelationshipType, id: Uuid, short_code: String, entity_type: Str...` — [`crate::abac::grant_capability`].
-  `NeighborRow` type L385-395 — `= NeighborRow` — [`crate::abac::grant_capability`].
-  `into_neighbor` function L386-394 — `(self) -> Result<Neighbor, GraphError>` — [`crate::abac::grant_capability`].
-  `neighbors_of` function L403-419 — `( conn: &mut PgConnection, item_id: Uuid, own_column: &str, other_column: &str, ...` — One direction of [`relationships_for`]: edges where `item_id` sits in

#### crates/kairos-db/src/items.rs

- pub `ItemError` enum L77-115 — `ItemNotFound | VersionConflict | HistoryNotFound | BoardNotFound | BoardHasNoCol...` — Errors from the item write path.
- pub `next_short_code` function L150-165 — `(conn: &mut PgConnection, item_type: ItemType) -> Result<String, ItemError>` — Allocate the next short code for `item_type` in the current tenant
- pub `ContentUpdate` struct L293-297 — `{ new_title: Option<&'a str>, new_content: &'a str, expected_version: i32 }` — A content edit for [`update_item_content`]: `new_title = None` keeps the
- pub `CreateStrategy` struct L474-481 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, hyp...` — Input for [`create_strategy`].
- pub `create_strategy` function L486-518 — `( conn: &mut PgConnection, input: CreateStrategy<'_>, actor: Uuid, ) -> Result<S...` — Create a strategy: assign the next `{PREFIX}-S-{NNNN}` short code,
- pub `CreateInitiative` struct L524-532 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, com...` — Input for [`create_initiative`].
- pub `create_initiative` function L535-569 — `( conn: &mut PgConnection, input: CreateInitiative<'_>, actor: Uuid, ) -> Result...` — Create an initiative (see [`create_strategy`] for the shared contract).
- pub `CreateTask` struct L573-581 — `{ board_id: Uuid, column_id: Option<Uuid>, title: &'a str, content: &'a str, tas...` — Input for [`create_task`].
- pub `create_task` function L585-618 — `( conn: &mut PgConnection, input: CreateTask<'_>, actor: Uuid, ) -> Result<Task,...` — Create a task/bug/tech-debt item (see [`create_strategy`] for the
- pub `CreateDocument` struct L622-631 — `{ title: &'a str, content: Option<&'a str>, template_id: Option<Uuid> }` — Input for [`create_document`].
- pub `create_document` function L637-708 — `( conn: &mut PgConnection, input: CreateDocument<'_>, actor: Uuid, ) -> Result<D...` — Create a document (documents do not live on boards).
- pub `CreateAdr` struct L716-723 — `{ board_id: Option<Uuid>, column_id: Option<Uuid>, title: &'a str, content: &'a ...` — Input for [`create_adr`].
- pub `create_adr` function L727-763 — `( conn: &mut PgConnection, input: CreateAdr<'_>, actor: Uuid, ) -> Result<Adr, I...` — Create an ADR (see [`create_strategy`] for the shared contract and
- pub `update_item_content` function L777-824 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, update: ContentUp...` — Apply a content edit with optimistic concurrency (KAIROS-A-0004), in ONE
- pub `rollback_item` function L833-872 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, to_version: i32, ...` — Roll an item back to a historical snapshot (KAIROS-A-0004 "rollback by
- pub `SoftDeleteOutcome` struct L880-886 — `{ root_short_code: String, cascaded_short_codes: Vec<String> }` — What [`soft_delete_item`] deleted.
- pub `CascadePreview` struct L893-900 — `{ root_short_code: String, cascaded_short_codes: Vec<String> }` — The AUTHORITATIVE pre-delete cascade set (KAIROS-T-0051): what a
- pub `preview_cascade` function L911-959 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Casca...` — Preview the KAIROS-A-0001 soft-delete cascade WITHOUT mutating anything
- pub `soft_delete_item` function L971-1051 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, actor: Uuid, ) ->...` — Soft-delete an item and cascade to its descendants (KAIROS-A-0001), in
-  `sequence_name` function L122-130 — `(item_type: ItemType) -> &'static str` — The S-0004 sequence backing each entity type's short-code numbers.
-  `SeqValue` struct L133-136 — `{ value: i64 }` — count and the cascaded short codes.
-  `SchemaName` struct L139-142 — `{ name: String }` — count and the cascaded short codes.
-  `log_activity` function L172-190 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Uu...` — Insert one `activity_log` row (current tenant schema).
-  `insert_history` function L193-211 — `( conn: &mut PgConnection, item_id: Uuid, version: i32, title: &str, content: &s...` — Append one `item_history` snapshot (append-only, KAIROS-A-0004).
-  `finish_create` function L217-242 — `( conn: &mut PgConnection, actor: Uuid, item_type: ItemType, item_id: Uuid, code...` — Create-time bookkeeping shared by all five create services: the v1
-  `resolve_column` function L247-283 — `( conn: &mut PgConnection, board_id: Uuid, column_id: Option<Uuid>, ) -> Result<...` — Resolve an item's column placement on `board_id`: an explicit
-  `content_table_ops` macro L302-398 — `-` — Generate the three per-table primitives the generic write path
-  `apply_content_update` function L437-451 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, update: &ContentU...` — Dispatch the atomic content UPDATE to the item's table.
-  `load_live_content` function L454-466 — `( conn: &mut PgConnection, item_type: ItemType, item_id: Uuid, ) -> Result<Optio...` — Dispatch the live-row load to the item's table.

#### crates/kairos-db/src/lib.rs

- pub `abac` module L9 — `-` — models, typed queries, embedded migrations (public + tenant trees), and
- pub `api_keys` module L10 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `boards` module L11 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `events` module L12 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `graph` module L13 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `items` module L14 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `migrations` module L15 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `models` module L16 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `pool` module L17 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `retention` module L18 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `schema` module L19 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `scim` module L20 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `search` module L21 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `seed` module L22 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `service_accounts` module L23 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.
- pub `tenant` module L24 — `-` — escape hatch for recursive-CTE traversals and the search pipeline.

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

- pub `SearchError` enum L84-98 — `Invalid | TraverseRootNotFound | Database` — Errors from the search pipeline.
- pub `SearchResults` struct L104-121 — `{ strategies: Vec<Strategy>, initiatives: Vec<Initiative>, tasks: Vec<Task>, doc...` — Search results grouped by entity type (A-0007 response shape), each
- pub `SearchStats` struct L127-132 — `{ hydration_queries: usize, total_queries: usize }` — Query-count instrumentation for one `execute_search` run — the proof
- pub `execute_search` function L136-141 — `( conn: &mut PgConnection, request: &SearchRequest, ) -> Result<SearchResults, S...` — Run the A-0007 search pipeline (module docs) for `request` in the
- pub `execute_search_with_stats` function L145-253 — `( conn: &mut PgConnection, request: &SearchRequest, ) -> Result<(SearchResults, ...` — [`execute_search`], also returning the [`SearchStats`] query counters
-  `IdRow` struct L260-263 — `{ id: Uuid }` — read-only, so no transaction is opened.
-  `resolve_root` function L266-293 — `( conn: &mut PgConnection, from: &TraverseFrom, stats: &mut SearchStats, ) -> Re...` — Resolve `traverse.from` to a live entity id via `entity_directory`.
-  `traverse_ids` function L299-348 — `( conn: &mut PgConnection, root: Uuid, traverse: &Traverse, stats: &mut SearchSt...` — Recursive-CTE walk from `root` (module docs, step 1).
-  `text_match_ids` function L351-363 — `( conn: &mut PgConnection, q: &str, stats: &mut SearchStats, ) -> Result<HashSet...` — Full-text match via the `searchable_items` view (module docs, step 2).
-  `metadata_match_ids` function L369-395 — `( conn: &mut PgConnection, metadata: &std::collections::BTreeMap<String, String>...` — Ids satisfying EVERY metadata entry (module docs, step 3): one query,
-  `TypedIdRow` struct L402-407 — `{ id: Uuid, entity_type: String }` — read-only, so no transaction is opened.
-  `parse_entity_type` function L412-425 — `(value: &str) -> Result<ItemType, SearchError>` — Parse an `entity_type` literal from type resolution.
-  `partition_by_type` function L431-467 — `( conn: &mut PgConnection, ids: &HashSet<Uuid>, include_deleted: bool, stats: &m...` — Resolve candidate ids to `(id, entity_type)` and partition by type
-  `applicable_types` function L473-500 — `(filter: Option<&SearchFilter>) -> Vec<ItemType>` — Which entity types can match the structural filter at all (module docs,
-  `model_task_type` function L509-515 — `(task_type: SearchTaskType) -> TaskType` — The stored counterpart of a [`SearchTaskType`].
-  `hydrate_strategies` function L517-551 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_initiatives` function L553-592 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_tasks` function L594-635 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_documents` function L637-665 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `hydrate_adrs` function L667-701 — `( conn: &mut PgConnection, ids: Option<Vec<Uuid>>, filter: Option<&SearchFilter>...` — read-only, so no transaction is opened.
-  `AnyItem` enum L708-714 — `Strategy | Initiative | Task | Document | Adr` — One hydrated row of any entity type, for the combined sort.
-  `AnyItem` type L716-756 — `= AnyItem` — read-only, so no transaction is opened.
-  `created_at` function L717-725 — `(&self) -> DateTime<Utc>` — read-only, so no transaction is opened.
-  `updated_at` function L727-735 — `(&self) -> DateTime<Utc>` — read-only, so no transaction is opened.
-  `title` function L737-745 — `(&self) -> &str` — read-only, so no transaction is opened.
-  `short_code` function L747-755 — `(&self) -> &str` — read-only, so no transaction is opened.
-  `sort_items` function L760-773 — `(items: &mut [AnyItem], sort: Sort)` — Sort the combined rows by the requested field/order, tie-broken by

#### crates/kairos-db/src/seed.rs

- pub `DEMO_SLUG` variable L67 — `: &str` — The demo tenant slug (schema `org_demo`, short-code prefix `DEMO`).
- pub `DEMO_NAME` variable L70 — `: &str` — The demo tenant display name.
- pub `DEMO_USERS` variable L80-102 — `: [(&str, &str, &str, OrgRole); 3]` — The demo users: `(external_id, email, display_name, org_role)`.
- pub `SeedError` enum L106-129 — `AlreadySeeded | Tenant | Item | Graph | Board | Database` — Errors from [`seed_demo`].
- pub `SeedDemoReport` struct L133-165 — `{ slug: String, schema: String, recreated: bool, users: usize, teams: usize, boa...` — What [`seed_demo`] created.
- pub `demo_tenant_exists` function L174-182 — `(conn: &mut PgConnection) -> Result<bool, SeedError>` — Whether the demo tenant exists (org row or schema).
- pub `seed_demo` function L310-709 — `(conn: &mut PgConnection, force: bool) -> Result<SeedDemoReport, SeedError>` — Seed the demo tenant (see module docs for the full inventory and the
-  `BoolRow` struct L168-171 — `{ present: bool }` — ever touched; other tenants and users are invisible to this module.
-  `column_id` function L185-192 — `(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Result<Uuid, SeedError>` — A board column's id by board + name (seeded default columns).
-  `board_id_of` function L196-203 — `(conn: &mut PgConnection, level: BoardLevel) -> Result<Uuid, SeedError>` — The tenant's board id for a level (unique for strategy/initiative/adr
-  `metadata_definition_id` function L207-213 — `(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError>` — The tenant's metadata definition id by slug (copied from system
-  `template_id` function L216-222 — `(conn: &mut PgConnection, slug: &str) -> Result<Uuid, SeedError>` — The tenant's template id by slug (copied from system defaults).
-  `upsert_demo_users` function L226-250 — `(conn: &mut PgConnection) -> Result<Vec<Uuid>, SeedError>` — Upsert the three demo users (keyed on `external_id`, exactly like JIT
-  `seed_team` function L254-289 — `( conn: &mut PgConnection, name: &str, slug: &str, team_type: TeamType, member_i...` — Create a team + its delivery board (the same shape as the API's team
-  `stamp_priority` function L292-306 — `( conn: &mut PgConnection, priority_def: Uuid, item_id: Uuid, value: &str, ) -> ...` — Stamp one `priority` metadata value on an item.
-  `SeedTask` struct L506-516 — `{ board: Uuid, column: &'a str, title: &'a str, content: &'a str, task_type: Tas...` — ever touched; other tenants and users are invisible to this module.

#### crates/kairos-db/src/service_accounts.rs

- pub `create_service_account` function L23-51 — `( conn: &mut PgConnection, org_id: Uuid, name: &str, ) -> QueryResult<User>` — Create a service account: a `users` row (`kind='service_account'`, synthetic
- pub `list_service_accounts` function L54-62 — `(conn: &mut PgConnection, org_id: Uuid) -> QueryResult<Vec<User>>` — All service accounts belonging to `org_id`, newest first.
- pub `find_service_account` function L67-80 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> QueryResult<Option<...` — The service account `user_id`, but ONLY if it is a service account AND a
- pub `delete_service_account` function L86-106 — `( conn: &mut PgConnection, org_id: Uuid, user_id: Uuid, ) -> QueryResult<()>` — Delete a service account and everything attached to it: its API keys and

#### crates/kairos-db/src/tenant.rs

- pub `TenantError` enum L43-69 — `InvalidSlug | AlreadyExists | NotFound | ConfirmationRequired | Migration | Boar...` — Errors from tenant provisioning, fleet migration, or teardown.
- pub `TenantProvisionReport` struct L73-87 — `{ slug: String, schema: String, migrations_applied: Vec<String>, boards_created:...` — What [`provision_tenant`] created.
- pub `TenantMigrationOutcome` struct L91-98 — `{ slug: String, schema: String, applied: Vec<String> }` — One tenant's outcome from [`migrate_all_tenants`].
- pub `TenantInfo` struct L102-109 — `{ slug: String, name: String, schema_exists: bool }` — A row from [`list_tenants`].
- pub `is_valid_slug` function L237-244 — `(slug: &str) -> bool` — Whether `slug` matches the KAIROS-S-0004 organization slug pattern
- pub `tenant_schema_name` function L255-257 — `(slug: &str) -> String` — The schema name for an organization slug: `org_{slug}`.
- pub `seed_system_defaults` function L271-274 — `(conn: &mut PgConnection) -> Result<(), TenantError>` — Idempotently seed the `public.system_*` default rows (see
- pub `provision_tenant` function L290-382 — `( conn: &mut PgConnection, slug: &str, name: &str, ) -> Result<TenantProvisionRe...` — Provision a new tenant (KAIROS-A-0001 application-level provisioning):
- pub `drop_tenant` function L389-406 — `(conn: &mut PgConnection, slug: &str, confirm: bool) -> Result<(), TenantError>` — Drop a tenant: remove the `org_{slug}` schema (CASCADE) and delete the
- pub `list_tenants` function L410-425 — `(conn: &mut PgConnection) -> Result<Vec<TenantInfo>, TenantError>` — List provisioned tenants from `public.organizations`, with a
- pub `migrate_all_tenants` function L432-468 — `( conn: &mut PgConnection, ) -> Result<Vec<TenantMigrationOutcome>, TenantError>` — Fleet operation: run pending tenant migrations in every provisioned
-  `PROVISION_BOARDS` variable L115-119 — `: [(BoardLevel, &str, &str); 3]` — The default boards created at provision time: `(board_level, name, slug)`.
-  `SEED_SYSTEM_DEFAULTS_SQL` variable L128-214 — `: &str` — Idempotent seed of the `public.system_*` default rows (KAIROS-A-0002
-  `BoolRow` struct L217-220 — `{ present: bool }` — (Interpretation recorded in KAIROS-T-0008.)
-  `TenantRow` struct L223-230 — `{ slug: String, name: String, schema_exists: bool }` — (Interpretation recorded in KAIROS-T-0008.)
-  `validated_slug` function L246-252 — `(slug: &str) -> Result<(), TenantError>` — (Interpretation recorded in KAIROS-T-0008.)
-  `schema_exists` function L259-265 — `(conn: &mut PgConnection, schema: &str) -> Result<bool, TenantError>` — (Interpretation recorded in KAIROS-T-0008.)
-  `tests` module L471-507 — `-` — (Interpretation recorded in KAIROS-T-0008.)
-  `slug_validation_matches_s0004_pattern` function L475-501 — `()` — (Interpretation recorded in KAIROS-T-0008.)
-  `tenant_schema_name_prefixes_org` function L504-506 — `()` — (Interpretation recorded in KAIROS-T-0008.)

### crates/kairos-db/src/models

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-db/src/models/boards.rs

- pub `Board` struct L23-33 — `{ id: Uuid, name: String, slug: String, board_level: BoardLevel, team_id: Option...` — A configurable board (`boards`).
- pub `NewBoard` struct L38-43 — `{ name: String, slug: String, board_level: BoardLevel, team_id: Option<Uuid> }` — Insert for [`Board`].
- pub `BoardChangeset` struct L48-55 — `{ name: Option<String>, slug: Option<String>, board_level: Option<BoardLevel>, t...` — Partial update for [`Board`].
- pub `BoardColumn` struct L66-74 — `{ id: Uuid, board_id: Uuid, name: String, position: i32, created_at: DateTime<Ut...` — A board column (`board_columns`).
- pub `NewBoardColumn` struct L79-83 — `{ board_id: Uuid, name: String, position: i32 }` — Insert for [`BoardColumn`].
- pub `BoardColumnChangeset` struct L88-92 — `{ name: Option<String>, position: Option<i32>, updated_at: Option<DateTime<Utc>>...` — Partial update for [`BoardColumn`].
- pub `BoardTransition` struct L103-108 — `{ id: Uuid, board_id: Uuid, from_column_id: Uuid, to_column_id: Uuid }` — An allowed column-to-column transition (`board_transitions`).
- pub `NewBoardTransition` struct L113-117 — `{ board_id: Uuid, from_column_id: Uuid, to_column_id: Uuid }` — Insert for [`BoardTransition`].
- pub `BoardTransitionChangeset` struct L122-125 — `{ from_column_id: Option<Uuid>, to_column_id: Option<Uuid> }` — Partial update for [`BoardTransition`] (rewiring an edge).
- pub `BoardMemberCapability` struct L140-146 — `{ board_id: Uuid, user_id: Uuid, capability: String, granted_at: DateTime<Utc>, ...` — A board-scoped capability grant (`board_member_capabilities`,
- pub `NewBoardMemberCapability` struct L151-156 — `{ board_id: Uuid, user_id: Uuid, capability: String, granted_by: Uuid }` — Insert for [`BoardMemberCapability`].
- pub `BoardMemberCapabilityChangeset` struct L162-165 — `{ granted_at: Option<DateTime<Utc>>, granted_by: Option<Uuid> }` — Partial update for [`BoardMemberCapability`] (re-attribution; grants are

#### crates/kairos-db/src/models/enums.rs

- pub `UnknownEnumValue` struct L21-26 — `{ enum_name: &'static str, value: String }` — A TEXT value read from the database that is not a member of the enum's
-  `text_enum` macro L30-92 — `-` — Declare a TEXT-backed enum: variants, their database strings, `FromStr`
-  `tests` module L192-289 — `-` — than defaulting.
-  `assert_text_enum` macro L197-220 — `-` — Round-trip every variant through its TEXT representation and reject
-  `org_role_round_trip_and_rejection` function L223-225 — `()` — than defaulting.
-  `field_type_round_trip_and_rejection` function L228-230 — `()` — than defaulting.
-  `team_type_round_trip_and_rejection` function L233-243 — `()` — than defaulting.
-  `board_level_round_trip_and_rejection` function L246-248 — `()` — than defaulting.
-  `task_type_round_trip_and_rejection` function L251-253 — `()` — than defaulting.
-  `complexity_round_trip_and_rejection` function L256-258 — `()` — than defaulting.
-  `bucket_type_round_trip_and_rejection` function L261-263 — `()` — than defaulting.
-  `relationship_type_round_trip_and_rejection` function L266-271 — `()` — than defaulting.
-  `activity_action_round_trip_and_rejection` function L274-288 — `()` — than defaulting.

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
- pub `Task` struct L147-162 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Uuid, ...` — A task/bug/tech-debt item (Flight Level 1, `tasks`).
- pub `NewTask` struct L167-177 — `{ short_code: String, title: String, content: String, board_id: Uuid, column_id:...` — Insert for [`Task`].
- pub `TaskChangeset` struct L182-193 — `{ title: Option<String>, content: Option<String>, board_id: Option<Uuid>, column...` — Partial update for [`Task`].
- pub `Document` struct L205-217 — `{ id: Uuid, short_code: String, title: String, content: String, template_id: Opt...` — A supporting document (`documents`); child of any entity via the
- pub `NewDocument` struct L222-229 — `{ short_code: String, title: String, content: String, template_id: Option<Uuid>,...` — Insert for [`Document`].
- pub `DocumentChangeset` struct L234-242 — `{ title: Option<String>, content: Option<String>, template_id: Option<Option<Uui...` — Partial update for [`Document`].
- pub `Adr` struct L255-270 — `{ id: Uuid, short_code: String, title: String, content: String, board_id: Option...` — An Architecture Decision Record (`adrs`).
- pub `NewAdr` struct L275-285 — `{ short_code: String, title: String, content: String, board_id: Option<Uuid>, co...` — Insert for [`Adr`].
- pub `AdrChangeset` struct L290-301 — `{ title: Option<String>, content: Option<String>, board_id: Option<Option<Uuid>>...` — Partial update for [`Adr`].

#### crates/kairos-db/src/models/mod.rs

- pub `boards` module L18 — `-` — table family:
- pub `enums` module L19 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `graph` module L20 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `items` module L21 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `public` module L22 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `teams` module L23 — `-` — (`team_delivery_streams`) deliberately have none.
- pub `templates` module L24 — `-` — (`team_delivery_streams`) deliberately have none.

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

- pub `Template` struct L23-31 — `{ id: Uuid, name: String, slug: String, content: String, is_system_default: bool...` — A tenant document template (`templates`), seeded from
- pub `NewTemplate` struct L36-41 — `{ name: String, slug: String, content: String, is_system_default: bool }` — Insert for [`Template`].
- pub `TemplateChangeset` struct L46-52 — `{ name: Option<String>, slug: Option<String>, content: Option<String>, is_system...` — Partial update for [`Template`].
- pub `MetadataDefinition` struct L62-70 — `{ id: Uuid, name: String, slug: String, field_type: FieldType, is_system_default...` — A tenant metadata field definition (`metadata_definitions`).
- pub `NewMetadataDefinition` struct L75-80 — `{ name: String, slug: String, field_type: FieldType, is_system_default: bool }` — Insert for [`MetadataDefinition`].
- pub `MetadataDefinitionChangeset` struct L85-91 — `{ name: Option<String>, slug: Option<String>, field_type: Option<FieldType>, is_...` — Partial update for [`MetadataDefinition`].
- pub `MetadataEnumOption` struct L102-107 — `{ id: Uuid, metadata_definition_id: Uuid, value: String, position: i32 }` — An enum option for a metadata definition (`metadata_enum_options`).
- pub `NewMetadataEnumOption` struct L112-116 — `{ metadata_definition_id: Uuid, value: String, position: i32 }` — Insert for [`MetadataEnumOption`].
- pub `MetadataEnumOptionChangeset` struct L121-124 — `{ value: Option<String>, position: Option<i32> }` — Partial update for [`MetadataEnumOption`].
- pub `TemplateMetadata` struct L136-142 — `{ id: Uuid, template_id: Uuid, metadata_definition_id: Uuid, default_value: Opti...` — Which metadata fields a template carries (`template_metadata`).
- pub `NewTemplateMetadata` struct L147-152 — `{ template_id: Uuid, metadata_definition_id: Uuid, default_value: Option<String>...` — Insert for [`TemplateMetadata`].
- pub `TemplateMetadataChangeset` struct L158-161 — `{ default_value: Option<Option<String>>, required: Option<bool> }` — Partial update for [`TemplateMetadata`].
- pub `ItemMetadata` struct L173-178 — `{ id: Uuid, item_id: Uuid, metadata_definition_id: Uuid, value: String }` — A metadata value on an entity (`item_metadata`).
- pub `NewItemMetadata` struct L183-187 — `{ item_id: Uuid, metadata_definition_id: Uuid, value: String }` — Insert for [`ItemMetadata`].
- pub `ItemMetadataChangeset` struct L192-194 — `{ value: Option<String> }` — Partial update for [`ItemMetadata`].

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
-  `team_membership_implies_delivery_capabilities` function L618-738 — `()` — KAIROS-T-0072 (A-0006 amendment): membership of a board's owning team
-  `TEAM_SCRATCH_DB` variable L621 — `: &str` — off-board ADRs, orphan documents, and unknown ids resolving to None

#### crates/kairos-db/tests/api_keys.rs

-  `DEFAULT_DATABASE_URL` variable L20 — `: &str` — without colliding.
-  `SLUG` variable L21 — `: &str` — without colliding.
-  `admin_url` function L23-25 — `() -> String` — without colliding.
-  `with_database` function L27-30 — `(url: &str, db: &str) -> String` — without colliding.
-  `setup` function L34-50 — `(db: &str) -> PgConnection` — Drop+recreate the named scratch DB, run public migrations, provision one
-  `teardown` function L52-55 — `(db: &str)` — without colliding.
-  `api_key_round_trip` function L58-123 — `()` — without colliding.
-  `expiry_is_honored` function L126-160 — `()` — without colliding.

#### crates/kairos-db/tests/board_rules.rs

-  `DEFAULT_DATABASE_URL` variable L40 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L42 — `: &str` — removed
-  `admin_database_url` function L44-46 — `() -> String` — removed
-  `with_database` function L49-54 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `board_id_by_slug` function L57-63 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `column_id_by_name` function L66-73 — `(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid` — The column named `name` on `board`.
-  `column_names` function L76-83 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Column names of `board` in position order.
-  `NameRow` struct L86-89 — `{ name: String }` — removed
-  `transition_pairs` function L92-106 — `(conn: &mut PgConnection, board: Uuid) -> BTreeSet<String>` — Transition pairs `"From -> To"` for a board (current search_path schema).
-  `pairs` function L108-110 — `(list: &[(&str, &str)]) -> BTreeSet<String>` — removed
-  `item_column` macro L113-121 — `-` — The `column_id` an item row currently occupies.
-  `activity_details` function L124-132 — `(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String>` — All `activity_log.details` values for (action, entity_id).
-  `dead_end_names` function L134-140 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — removed
-  `board_rules_lifecycle` function L143-612 — `()` — removed

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
-  `relationship_graph_service` function L170-729 — `()` — (`idx_item_relationships_source` / `idx_item_relationships_target`)

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
-  `directory_id_by_code` function L198-210 — `(conn: &mut PgConnection, short_code: &str) -> Option<Uuid>` — The id `entity_directory` resolves for `short_code` in the CURRENT tenant
-  `IdRow` struct L200-203 — `{ id: Uuid }` — by construction; each resolves only within its own tenant, never cross.
-  `Seed` struct L214-227 — `{ marker: String, strategy: Uuid, initiative: Uuid, task: Uuid, document: Uuid, ...` — Every seeded item in one tenant, with the short codes services assigned
-  `COLLIDE_CODE` variable L230 — `: &str` — The short code deliberately shared, byte-for-byte, across both tenants.
-  `seed` function L237-389 — `(conn: &mut PgConnection, slug: &str, user: Uuid) -> Seed` — Seed one tenant (connection already pinned to it).
-  `run_search` function L395-399 — `(conn: &mut PgConnection, request: serde_json::Value) -> SearchResults` — by construction; each resolves only within its own tenant, never cross.
-  `all_result_ids` function L401-409 — `(results: &SearchResults) -> Vec<Uuid>` — by construction; each resolves only within its own tenant, never cross.
-  `result_count` function L411-413 — `(results: &SearchResults) -> usize` — by construction; each resolves only within its own tenant, never cross.
-  `CHECKSUM_TABLES` variable L423-434 — `: [&str; 10]` — The tables whose contents must be byte-identical before and after an
-  `table_fingerprint` function L437-451 — `(conn: &mut PgConnection, table: &str) -> String` — `count:md5` fingerprint of one table in the CURRENT tenant schema.
-  `Fp` struct L439-442 — `{ fp: String }` — by construction; each resolves only within its own tenant, never cross.
-  `fingerprint_all` function L455-460 — `(conn: &mut PgConnection) -> Vec<(String, String)>` — Fingerprint every [`CHECKSUM_TABLES`] table (connection pinned to the
-  `cross_read_visibility_sweep` function L467-664 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L468 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `cross_write_battery` function L671-787 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L672 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `short_code_collision` function L794-861 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L795 — `: &str` — by construction; each resolves only within its own tenant, never cross.
-  `pool_stress` module L872-1030 — `-` — Barrier-synchronized concurrent rounds over ONE small pool, alternating
-  `ROUNDS` variable L887 — `: usize` — by construction; each resolves only within its own tenant, never cross.
-  `TitleRow` struct L890-893 — `{ title: String }` — by construction; each resolves only within its own tenant, never cross.
-  `TenantFixture` struct L898-901 — `{ board: Uuid, column: Uuid }` — Per-tenant board + first column, resolved once (sync) for the async
-  `pool_reuse_stress` function L904-1029 — `()` — by construction; each resolves only within its own tenant, never cross.
-  `DB` variable L905 — `: &str` — by construction; each resolves only within its own tenant, never cross.

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
-  `models_round_trip` function L146-525 — `()` — KAIROS-T-0016 adversarial suite.
-  `DB` variable L147 — `: &str` — KAIROS-T-0016 adversarial suite.
-  `pool_isolation_interleaved` function L528-666 — `()` — KAIROS-T-0016 adversarial suite.
-  `DB` variable L529 — `: &str` — KAIROS-T-0016 adversarial suite.
-  `team_names` function L551-558 — `(conn: &mut kairos_db::TenantConnection) -> Vec<String>` — KAIROS-T-0016 adversarial suite.

#### crates/kairos-db/tests/public_migrations.rs

-  `DEFAULT_DATABASE_URL` variable L22 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L24 — `: &str` — touches the dev `kairos` database or interferes with other tests.
-  `EXPECTED_TABLES` variable L27-36 — `: [&str; 8]` — The 8 public-schema tables defined by KAIROS-S-0004.
-  `admin_database_url` function L38-40 — `() -> String` — touches the dev `kairos` database or interferes with other tests.
-  `with_database` function L43-48 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `TableName` struct L51-54 — `{ table_name: String }` — touches the dev `kairos` database or interferes with other tests.
-  `public_base_tables` function L56-71 — `(conn: &mut PgConnection) -> Vec<String>` — touches the dev `kairos` database or interferes with other tests.
-  `assert_database_error_kind` function L74-83 — `( result: Result<T, DieselError>, kind: DatabaseErrorKind, context: &str, )` — Expect a database error of `kind` from `result`.
-  `public_migrations_from_empty_database` function L86-152 — `()` — touches the dev `kairos` database or interferes with other tests.

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

-  `DEFAULT_DATABASE_URL` variable L53 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L55 — `: &str` — within the stated budget with exactly one task-hydration query
-  `admin_database_url` function L57-59 — `() -> String` — within the stated budget with exactly one task-hydration query
-  `with_database` function L62-67 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `insert_user` function L69-80 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — within the stated budget with exactly one task-hydration query
-  `board_id_by_slug` function L83-89 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `metadata_definition` function L93-114 — `(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid` — The tenant's metadata definition with this slug, creating it if the
-  `set_metadata` function L116-125 — `(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str)` — within the stated budget with exactly one task-hydration query
-  `run` function L129-134 — `(conn: &mut PgConnection, request: serde_json::Value) -> (SearchResults, SearchS...` — Parse a request from its JSON shape (the S-0005 wire format) and run it
-  `ids` function L136-138 — `(rows: &[T], id_of: impl Fn(&T) -> Uuid) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `task_ids` function L140-142 — `(results: &SearchResults) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `all_ids` function L144-152 — `(results: &SearchResults) -> HashSet<Uuid>` — within the stated budget with exactly one task-hydration query
-  `returned_row_count` function L154-160 — `(results: &SearchResults) -> usize` — within the stated budget with exactly one task-hydration query
-  `unified_search_pipeline` function L163-830 — `()` — within the stated budget with exactly one task-hydration query

#### crates/kairos-db/tests/seed_demo.rs

-  `DEFAULT_DATABASE_URL` variable L29 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L31 — `: &str` — (`kairos_seed_demo_test`) on the shared compose server.
-  `admin_database_url` function L33-35 — `() -> String` — (`kairos_seed_demo_test`) on the shared compose server.
-  `with_database` function L38-43 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `CountRow` struct L46-49 — `{ count: i64 }` — (`kairos_seed_demo_test`) on the shared compose server.
-  `count` function L51-56 — `(conn: &mut PgConnection, query: &str) -> i64` — (`kairos_seed_demo_test`) on the shared compose server.
-  `TextRow` struct L59-62 — `{ value: String }` — (`kairos_seed_demo_test`) on the shared compose server.
-  `text_values` function L64-71 — `(conn: &mut PgConnection, query: &str) -> Vec<String>` — (`kairos_seed_demo_test`) on the shared compose server.
-  `seed_demo_fixture_lifecycle` function L74-314 — `()` — (`kairos_seed_demo_test`) on the shared compose server.

#### crates/kairos-db/tests/tenant_provisioning.rs

-  `DEFAULT_DATABASE_URL` variable L33 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L35 — `: &str` — `system_board_defaults` rows are seeded.
-  `EXPECTED_TABLES` variable L40-64 — `: [&str; 23]` — The tenant tables (sorted): the 21 from the KAIROS-S-0004 DDL plus
-  `EXPECTED_VIEWS` variable L66 — `: [&str; 2]` — `system_board_defaults` rows are seeded.
-  `EXPECTED_SEQUENCES` variable L68-74 — `: [&str; 5]` — `system_board_defaults` rows are seeded.
-  `EXPECTED_INDEXES` variable L77-99 — `: [&str; 21]` — Every named index in the S-0004 tenant DDL (partial + GIN included).
-  `admin_database_url` function L101-103 — `() -> String` — `system_board_defaults` rows are seeded.
-  `with_database` function L106-111 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `NameRow` struct L114-117 — `{ name: String }` — `system_board_defaults` rows are seeded.
-  `CountRow` struct L120-123 — `{ count: i64 }` — `system_board_defaults` rows are seeded.
-  `names` function L125-135 — `(conn: &mut PgConnection, query: &str, param: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_tables` function L137-145 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_views` function L147-153 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_sequences` function L155-162 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_indexes` function L164-170 — `(conn: &mut PgConnection, schema: &str) -> Vec<String>` — `system_board_defaults` rows are seeded.
-  `schema_exists` function L172-179 — `(conn: &mut PgConnection, schema: &str) -> bool` — `system_board_defaults` rows are seeded.
-  `count` function L181-186 — `(conn: &mut PgConnection, sql: &str) -> i64` — `system_board_defaults` rows are seeded.
-  `board_columns` function L189-201 — `(conn: &mut PgConnection, board_slug: &str) -> Vec<String>` — Board columns (ordered by position) for a board slug in `org_acme`.
-  `board_transitions` function L204-219 — `(conn: &mut PgConnection, board_slug: &str) -> BTreeSet<String>` — Transition pairs `"From -> To"` for a board slug in `org_acme`.
-  `transitions` function L221-223 — `(pairs: &[(&str, &str)]) -> BTreeSet<String>` — `system_board_defaults` rows are seeded.
-  `tenant_provisioning_lifecycle` function L226-526 — `()` — `system_board_defaults` rows are seeded.

#### crates/kairos-db/tests/write_path.rs

-  `DEFAULT_DATABASE_URL` variable L47 — `: &str` — Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
-  `SCRATCH_DB` variable L49 — `: &str` — with the cascade count + short codes
-  `admin_database_url` function L51-53 — `() -> String` — with the cascade count + short codes
-  `with_database` function L56-61 — `(url: &str, db_name: &str) -> String` — Replace the database name (final path segment) in a postgres URL.
-  `tenant_connection` function L65-71 — `(scratch_url: &str) -> PgConnection` — A fresh connection to the scratch database with the tenant search_path
-  `insert_user` function L73-84 — `(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid` — with the cascade count + short codes
-  `board_id_by_slug` function L87-93 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `first_column` function L96-103 — `(conn: &mut PgConnection, board: Uuid) -> Uuid` — The first column of a board (position order).
-  `history_of` function L107-118 — `(conn: &mut PgConnection, item: Uuid) -> Vec<(i32, String, String)>` — All history snapshots for an item as `(version, title, content)`, in
-  `activity_details` function L121-129 — `(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String>` — All `activity_log.details` values for (action, entity_id), in order.
-  `CountRow` struct L132-135 — `{ n: i64 }` — with the cascade count + short codes
-  `view_count` function L138-144 — `(conn: &mut PgConnection, view: &str, id: Uuid) -> i64` — How many rows of `searchable_items` / `entity_directory` carry this id.
-  `code_number` function L147-152 — `(code: &str) -> i64` — The numeric part of a `{PREFIX}-{L}-{NNNN}` short code.
-  `write_path_lifecycle` function L155-658 — `()` — with the cascade count + short codes
-  `WRITERS` variable L308 — `: usize` — with the cascade count + short codes

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
-  `main` function L186-511 — `() -> ExitCode` — smoke suite.

### crates/kairos-server/src/api

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/api/adrs.rs

- pub `router` function L34-42 — `() -> Router<AppState>` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `MANAGE` variable L32 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L45-55 — `(conn: &mut PgConnection, short_code: &str) -> Result<Adr, ApiError>` — Load the live ADR with this short code, or 404.
-  `list_adrs` function L68-100 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `get_adr` function L113-125 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `create_adr` function L140-180 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `update_adr` function L197-234 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `delete_adr` function L249-272 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).
-  `transition_adr` function L290-310 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `ITEM_NOT_ON_BOARD` (T-0010's typed error).

#### crates/kairos-server/src/api/cascade.rs

- pub `router` function L31-36 — `() -> Router<AppState>` — stack.
-  `cascade_preview` function L53-72 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — stack.

#### crates/kairos-server/src/api/convert.rs

- pub `IntoDto` interface L14-16 — `{ fn into_dto() }` — Local conversion into a shared wire type (`model.into_dto()`).
-  `timestamp` function L20-22 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (stable wire format for
-  `Strategy` type L24-41 — `= Strategy` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L25-40 — `(self) -> dto::Strategy` — here; [`IntoDto`] is the local conversion trait instead.
-  `Initiative` type L43-62 — `= Initiative` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L44-61 — `(self) -> dto::Initiative` — here; [`IntoDto`] is the local conversion trait instead.
-  `Task` type L64-82 — `= Task` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L65-81 — `(self) -> dto::Task` — here; [`IntoDto`] is the local conversion trait instead.
-  `Document` type L84-99 — `= Document` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L85-98 — `(self) -> dto::Document` — here; [`IntoDto`] is the local conversion trait instead.
-  `Adr` type L101-119 — `= Adr` — here; [`IntoDto`] is the local conversion trait instead.
-  `into_dto` function L102-118 — `(self) -> dto::Adr` — here; [`IntoDto`] is the local conversion trait instead.

#### crates/kairos-server/src/api/convert_meta.rs

- pub `definition_dto` function L85-99 — `( definition: MetadataDefinition, enum_options: Vec<String>, ) -> dto::MetadataD...` — A [`MetadataDefinition`] plus its option values (loaded separately —
- pub `template_detail_dto` function L104-118 — `( template: Template, metadata: Vec<dto::TemplateMetadataField>, ) -> dto::Templ...` — A [`Template`] plus its hydrated metadata fields → the detail DTO
-  `timestamp` function L17-19 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (the same wire format as
-  `ItemRelationship` type L21-31 — `= ItemRelationship` — with microsecond precision.
-  `into_dto` function L22-30 — `(self) -> dto::Relationship` — with microsecond precision.
-  `Template` type L33-45 — `= Template` — with microsecond precision.
-  `into_dto` function L34-44 — `(self) -> dto::Template` — with microsecond precision.
-  `ItemHistory` type L47-55 — `= ItemHistory` — with microsecond precision.
-  `into_dto` function L48-54 — `(self) -> dto::HistoryVersion` — with microsecond precision.
-  `ItemHistory` type L57-67 — `= ItemHistory` — with microsecond precision.
-  `into_dto` function L58-66 — `(self) -> dto::HistorySnapshot` — with microsecond precision.
-  `ActivityLogEntry` type L69-81 — `= ActivityLogEntry` — with microsecond precision.
-  `into_dto` function L70-80 — `(self) -> dto::ActivityEntry` — with microsecond precision.

#### crates/kairos-server/src/api/convert_org.rs

- pub `team_to_dto` function L71-81 — `(team: Team, delivery_board_id: Option<uuid::Uuid>) -> dto::Team` — [`Team`] → DTO.
-  `timestamp` function L14-16 — `(value: DateTime<Utc>) -> String` — RFC 3339 with microsecond precision (same as [`super::convert`]).
-  `Board` type L18-30 — `= Board` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L19-29 — `(self) -> dto::Board` — its own module so T-0018's `convert.rs` stays untouched.
-  `BoardColumn` type L32-43 — `= BoardColumn` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L33-42 — `(self) -> dto::BoardColumn` — its own module so T-0018's `convert.rs` stays untouched.
-  `BoardTransition` type L45-54 — `= BoardTransition` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L46-53 — `(self) -> dto::BoardTransition` — its own module so T-0018's `convert.rs` stays untouched.
-  `DeliveryStream` type L56-67 — `= DeliveryStream` — its own module so T-0018's `convert.rs` stays untouched.
-  `into_dto` function L57-66 — `(self) -> dto::DeliveryStream` — its own module so T-0018's `convert.rs` stays untouched.

#### crates/kairos-server/src/api/documents.rs

- pub `router` function L45-54 — `() -> Router<AppState>` — pre-checks make a link failure after create unreachable in practice.
-  `MANAGE` variable L43 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L57-67 — `(conn: &mut PgConnection, short_code: &str) -> Result<Document, ApiError>` — Load the live document with this short code, or 404.
-  `authorization_board` function L72-77 — `( conn: &mut PgConnection, document_id: uuid::Uuid, ) -> Result<Option<uuid::Uui...` — The board that authorizes writes to this document: its parent workflow
-  `list_documents` function L90-122 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — pre-checks make a link failure after create unreachable in practice.
-  `get_document` function L135-147 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — pre-checks make a link failure after create unreachable in practice.
-  `create_document` function L164-222 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.
-  `update_document` function L239-277 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.
-  `delete_document` function L292-316 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — pre-checks make a link failure after create unreachable in practice.

#### crates/kairos-server/src/api/initiatives.rs

- pub `router` function L30-46 — `() -> Router<AppState>` — T-0018 handler pattern.
-  `MANAGE` variable L28 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L49-59 — `(conn: &mut PgConnection, short_code: &str) -> Result<Initiative, ApiError>` — Load the live initiative with this short code, or 404.
-  `list_initiatives` function L72-104 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — T-0018 handler pattern.
-  `get_initiative` function L117-129 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — T-0018 handler pattern.
-  `create_initiative` function L144-185 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `update_initiative` function L202-240 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `delete_initiative` function L256-279 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.
-  `transition_initiative` function L296-323 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern.

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
- pub `parse_uuid` function L98-101 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — Parse a UUID body field (`422 VALIDATION` on malformed input — the DTO
- pub `parse_opt_uuid` function L104-106 — `(value: Option<&str>, field: &str) -> Result<Option<Uuid>, ApiError>` — [`parse_uuid`] over an optional field.
- pub `parse_enum` function L110-121 — `(value: &str, field: &str, allowed: &[T]) -> Result<T, ApiError>` — Parse a TEXT-backed enum body field (`task_type`, `complexity`,
- pub `require_capability` function L132-149 — `( conn: &mut PgConnection, slug: &str, board_id: Option<Uuid>, user_id: Uuid, ca...` — The A-0006 write gate: org admins bypass; otherwise the caller needs a
- pub `resolve_short_code` function L166-190 — `( conn: &mut PgConnection, short_code: &str, ) -> Result<Option<(Uuid, ItemType)...` — Resolve a short code to `(id, entity_type)` across all five entity
- pub `short_code_not_found` function L193-197 — `(entity_type: &str, short_code: &str) -> ApiError` — The 404 for `/{short_code}` path segments that resolve to nothing.
- pub `map_abac_error` function L206-208 — `(e: AbacError) -> ApiError` — [`AbacError`] never carries a client mistake on the check path (grants
- pub `map_item_error` function L214-253 — `(e: ItemError) -> ApiError` — [`ItemError`] → HTTP.
- pub `map_board_error` function L259-300 — `(e: BoardError) -> ApiError` — [`BoardError`] → HTTP, for the transition endpoints: invalid moves are
- pub `map_graph_error` function L305-317 — `(e: GraphError) -> ApiError` — [`GraphError`] → HTTP, for the document-create `supports` edge: rule
-  `DEFAULT_LIMIT` variable L78 — `: i64` — Default page size when `?limit=` is omitted.
-  `MAX_LIMIT` variable L80 — `: i64` — Hard cap on `?limit=`.
-  `DirectoryRow` struct L156-161 — `{ id: Uuid, entity_type: String }` — aggregation endpoint is KAIROS-T-0023.

#### crates/kairos-server/src/api/openapi.rs

- pub `spec` function L182-184 — `() -> utoipa::openapi::OpenApi` — The aggregated OpenAPI document (also consumed by `tests/openapi.rs`,
- pub `router` function L194-201 — `(dev_ui: bool) -> Router<AppState>` — Build the module's routes.
-  `ApiDoc` struct L178 — `-` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `SPEC_JSON` variable L187-189 — `: LazyLock<String>` — The serialized spec, built once per process.
-  `openapi_json` function L214-219 — `() -> impl IntoResponse` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `whoami` function L247 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `spa_config` function L272 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `token_relay` function L296 — `()` — [`crate::ws`] module docs; the spec's `info.description` points there.
-  `swagger_ui` function L304-306 — `() -> Html<&'static str>` — The dev-only Swagger UI page (`KAIROS_DEV_UI=true`).
-  `SWAGGER_UI_HTML` variable L312-333 — `: &str` — Kept minimal on purpose: the page is behind the same auth → tenant

#### crates/kairos-server/src/api/search.rs

- pub `router` function L42-44 — `() -> Router<AppState>` — - Everything else from the pipeline is a 500.
-  `search` function L61-90 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Js...` — - Everything else from the pipeline is a 500.
-  `field_invalid` function L97-100 — `(field: &str, message: impl Into<String>) -> ApiError` — 400 `VALIDATION` naming the offending field in `details.field`.
-  `uuid_field` function L103-106 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — Parse a UUID-carrying field.
-  `timestamp_field` function L109-118 — `(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError>` — Parse an RFC 3339 timestamp field.
-  `enum_field` function L122-133 — `( value: &str, field: &str, allowed: &str, ) -> Result<T, ApiError>` — Parse a closed-vocabulary field through the core model's serde
-  `to_core` function L138-151 — `(request: &dto_search::SearchRequest) -> Result<core_search::SearchRequest, ApiE...` — Convert the wire request into the typed `kairos_core::search` request.
-  `filter_to_core` function L153-214 — `( filter: &dto_search::SearchFilter, ) -> Result<core_search::SearchFilter, ApiE...` — - Everything else from the pipeline is a 500.
-  `traverse_to_core` function L216-247 — `( traverse: &dto_search::SearchTraverse, ) -> Result<core_search::Traverse, ApiE...` — - Everything else from the pipeline is a 500.
-  `sort_to_core` function L249-254 — `(sort: &dto_search::SearchSort) -> Result<core_search::Sort, ApiError>` — - Everything else from the pipeline is a 500.
-  `map_validation_error` function L264-291 — `(e: SearchValidationError) -> ApiError` — [`SearchValidationError`] → 400 `VALIDATION`.
-  `map_search_error` function L294-302 — `(e: SearchError) -> ApiError` — [`SearchError`] → HTTP (module docs).
-  `into_response` function L310-335 — `(results: SearchResults) -> dto_search::SearchResponse` — Convert the pipeline's typed results into the S-0005 response shape

#### crates/kairos-server/src/api/strategies.rs

- pub `router` function L29-45 — `() -> Router<AppState>` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `MANAGE` variable L27 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L48-58 — `(conn: &mut PgConnection, short_code: &str) -> Result<Strategy, ApiError>` — Load the live strategy with this short code, or 404.
-  `list_strategies` function L71-103 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `get_strategy` function L116-128 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `create_strategy` function L142-172 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `update_strategy` function L189-226 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `delete_strategy` function L241-264 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.
-  `transition_strategy` function L281-308 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — T-0018 handler pattern; see [`super`] for the shared conventions.

#### crates/kairos-server/src/api/tasks.rs

- pub `router` function L30-38 — `() -> Router<AppState>` — handler pattern.
-  `MANAGE` variable L28 — `: &str` — The A-0006 manage capability for this family.
-  `load` function L41-51 — `(conn: &mut PgConnection, short_code: &str) -> Result<Task, ApiError>` — Load the live task with this short code, or 404.
-  `list_tasks` function L64-96 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — handler pattern.
-  `get_task` function L109-121 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — handler pattern.
-  `create_task` function L136-174 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `update_task` function L191-228 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `delete_task` function L243-266 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.
-  `transition_task` function L283-303 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — handler pattern.

### crates/kairos-server/src/api/meta

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/api/meta/activity.rs

- pub `router` function L26-28 — `() -> Router<AppState>` — `VALIDATION`.
-  `get_activity` function L41-129 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `VALIDATION`.
-  `filtered` macro L87-104 — `-` — Apply the combinable S-0005 filters to any boxed

#### crates/kairos-server/src/api/meta/definitions.rs

- pub `router` function L33-45 — `() -> Router<AppState>` — enforcement (S-0005 "fails if in use").
-  `load` function L48-57 — `(conn: &mut PgConnection, id: Uuid) -> Result<MetadataDefinition, ApiError>` — Load a definition by id, or 404.
-  `hydrate` function L60-66 — `( conn: &mut PgConnection, definition: MetadataDefinition, ) -> Result<dto::Meta...` — Hydrate a definition row with its option values.
-  `check_option_rules` function L70-80 — `(field_type: FieldType, options: &[String]) -> Result<(), ApiError>` — The option-list rules shared by create and update: enum definitions
-  `replace_options` function L84-109 — `( conn: &mut PgConnection, definition_id: Uuid, options: &[String], ) -> Result<...` — Replace a definition's option list (delete + insert, caller's
-  `map_write_error` function L113-123 — `(e: DieselError) -> ApiError` — Map the unique violations a definition write can hit (`slug` UNIQUE,
-  `list_definitions` function L135-169 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — enforcement (S-0005 "fails if in use").
-  `create_definition` function L183-214 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Js...` — enforcement (S-0005 "fails if in use").
-  `get_definition` function L227-241 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — enforcement (S-0005 "fails if in use").
-  `update_definition` function L259-305 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — enforcement (S-0005 "fails if in use").
-  `delete_definition` function L322-367 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — enforcement (S-0005 "fails if in use").

#### crates/kairos-server/src/api/meta/history.rs

- pub `router` function L26-28 — `() -> Router<AppState>` — birth.
-  `get_history` function L46-102 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — birth.

#### crates/kairos-server/src/api/meta/metadata.rs

- pub `router` function L32-37 — `() -> Router<AppState>` — history/activity rows.
-  `get_metadata` function L53-66 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — history/activity rows.
-  `update_metadata` function L88-178 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — history/activity rows.

#### crates/kairos-server/src/api/meta/mod.rs

- pub `activity` module L37 — `-` — item metadata, metadata definitions, templates, content history, and
- pub `definitions` module L38 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `history` module L39 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `metadata` module L40 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `relationships` module L41 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `templates` module L42 — `-` — 404 otherwise — matching the per-family route behavior).
- pub `router` function L60-68 — `() -> Router<AppState>` — All six T-0020 family routers, merged.
- pub `require_org_admin` function L73-84 — `(tenant: &TenantContext) -> Result<(), ApiError>` — The A-0006 gate for tenant-wide configuration (relationships, metadata
- pub `item_type_of_family` function L88-97 — `(family: &str) -> Option<ItemType>` — Map a plural `{entity_type}` path segment (the S-0005 family names, as
- pub `manage_capability` function L100-108 — `(item_type: ItemType) -> &'static str` — The A-0006 manage capability for an entity type.
- pub `resolve_family_item` function L114-128 — `( conn: &mut PgConnection, family: &str, short_code: &str, ) -> Result<(Uuid, It...` — Resolve an `{entity_type}/{short_code}` path pair to a live item: the
- pub `enum_option_values` function L132-143 — `( conn: &mut PgConnection, definition_id: Uuid, ) -> Result<Vec<String>, ApiErro...` — A metadata definition's option values, in display order (empty for
- pub `validate_metadata_value` function L149-178 — `( conn: &mut PgConnection, definition: &MetadataDefinition, value: &str, ) -> Re...` — The KAIROS-A-0003 value check: `string` passes through, `date` must
- pub `item_metadata_response` function L182-208 — `( conn: &mut PgConnection, item_id: Uuid, short_code: &str, ) -> Result<dto::Ite...` — An item's metadata values hydrated with their definitions, ordered by

#### crates/kairos-server/src/api/meta/relationships.rs

- pub `router` function L34-42 — `() -> Router<AppState>` — `VALIDATION`.
-  `map_link_error` function L46-61 — `(e: GraphError) -> ApiError` — [`GraphError`] → HTTP for the relationship write endpoints: the typed
-  `group_neighbors` function L66-93 — `( neighbors: Vec<Neighbor>, edge_ids: &HashMap<(RelationshipType, Uuid, bool), U...` — Fold one direction's neighbors (already ordered by relationship, then
-  `get_relationships` function L111-154 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `VALIDATION`.
-  `create_relationship` function L171-203 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `VALIDATION`.
-  `delete_relationship` function L219-251 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `VALIDATION`.

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
-  `load_columns` function L84-92 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError>` — Columns of a board in position order.
-  `load_transitions` function L95-105 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<Vec<BoardTransition>, Api...` — Transition edges of a board.
-  `board_detail` function L108-116 — `(conn: &mut PgConnection, board: Board) -> Result<dto::BoardDetail, ApiError>` — The board + full configuration as the `BoardDetail` DTO.
-  `load_column_of_board` function L120-134 — `( conn: &mut PgConnection, board_id: Uuid, column_id: Uuid, ) -> Result<BoardCol...` — A column of `board_id` by id, or 404 (also 404 when the column belongs
-  `log_activity` function L137-156 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, entity_id: Uu...` — Insert one `activity_log` row (same shape as the kairos-db services).
-  `list_boards` function L173-205 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — capability writes require `manage_members`.
-  `get_board` function L219-233 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `create_board` function L250-298 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `update_board` function L315-366 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `delete_board` function L383-432 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `board_items` function L451-539 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `list_columns` function L556-573 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_column` function L590-611 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `update_column` function L631-676 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_column` function L695-719 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `list_transitions` function L736-753 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_transition` function L770-800 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_transition` function L817-852 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `capabilities_of` function L859-872 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, ) -> Result<Vec<String...` — The `capabilities` a user holds on a board, sorted.
-  `board_member_view` function L875-887 — `( conn: &mut PgConnection, board_id: Uuid, user_id: Uuid, ) -> Result<dto::Board...` — One user's `BoardMember` view (joins `public.users` for identity).
-  `list_board_members` function L901-956 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — capability writes require `manage_members`.
-  `add_board_member` function L974-1000 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `replace_capabilities` function L1021-1058 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.
-  `remove_board_member` function L1077-1109 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — capability writes require `manage_members`.

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
- pub `members` module L23 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `streams` module L24 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `teams` module L25 — `-` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
- pub `router` function L41-47 — `() -> Router<AppState>` — The tenant-scoped T-0019 families, merged (mounted behind the full
- pub `CAPABILITY_VOCABULARY` variable L56-71 — `: &[&str]` — The fixed A-0006 capability vocabulary plus its glob forms — the values
- pub `validate_capabilities` function L75-90 — `(capabilities: &[String]) -> Result<(), ApiError>` — 422 `VALIDATION` unless every entry is in the A-0006 vocabulary (list
- pub `load_board` function L97-107 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError>` — Load a live board by id, or 404.
- pub `count_board_items` function L113-138 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ApiError>` — How many workflow items (strategies/initiatives/tasks/ADRs) reference
- pub `run_in_transaction` function L144-162 — `(conn: &mut PgConnection, f: F) -> Result<T, ApiError>` — Run `f` inside ONE database transaction, keeping `ApiError` as the
- pub `is_unique_violation` function L167-172 — `(e: &diesel::result::Error) -> bool` — Whether a diesel error is a unique-constraint violation (mapped to 409
- pub `map_config_error` function L180-199 — `(e: BoardError) -> ApiError` — [`BoardError`] → HTTP for the BOARD CONFIGURATION endpoints (columns /
- pub `map_grant_error` function L238-257 — `(e: AbacError) -> ApiError` — [`AbacError`] → HTTP for the board-member GRANT/REVOKE endpoints, where
- pub `require_user_exists` function L261-273 — `( conn: &mut PgConnection, user_id: Uuid, ) -> Result<kairos_db::models::User, A...` — Load a `public.users` row by id; 422 `VALIDATION` when it does not exist
-  `TxError` enum L148-151 — `Api | Db` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `TxError` type L152-156 — `= TxError` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `from` function L153-155 — `(e: diesel::result::Error) -> Self` — the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.
-  `map_column_rule_error` function L202-233 — `(e: ColumnRuleError) -> ApiError` — The T-0010 column/transition rule violations as typed 422s.

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

#### crates/kairos-server/src/api/org/teams.rs

- pub `router` function L41-56 — `() -> Router<AppState>` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `MANAGE` variable L39 — `: &str` — The pseudo-capability named in 403s for these org-admin-only writes
-  `load_team` function L59-69 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Team, ApiError>` — Load the live team with this id, or 404.
-  `delivery_board_of` function L72-82 — `(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError>` — The team's live delivery board id, if any.
-  `log_team_activity` function L85-103 — `( conn: &mut PgConnection, actor_id: Uuid, action: ActivityAction, team_id: Uuid...` — Insert one `activity_log` row for a team mutation.
-  `list_teams` function L116-153 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Qu...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `get_team` function L166-181 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `create_team` function L197-266 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `update_team` function L284-341 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `delete_team` function L357-423 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `list_members` function L443-484 — `( State(state): State<AppState>, Extension(tenant): Extension<TenantContext>, Pa...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `add_member` function L503-558 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.
-  `remove_member` function L577-619 — `( State(state): State<AppState>, Extension(auth): Extension<AuthContext>, Extens...` — `BOARD_NOT_EMPTY`) and soft-deletes team + board together.

### crates/kairos-server/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-server/src/app.rs

- pub `AppState` struct L27-41 — `{ config: Arc<AppConfig>, pool: TenantPool, blocking: BlockingTenantPool, auth: ...` — Shared state behind every request: config, the tenant-pinning pool, and
- pub `BuildError` enum L45-52 — `Pool | Oidc` — Why [`build_state`] failed (startup-time, fail-fast).
- pub `build_state` function L60-71 — `(config: AppConfig) -> Result<AppState, BuildError>` — Build the shared state: connect the pool and resolve the OIDC issuer
- pub `state_with` function L77-86 — `(config: AppConfig, pool: TenantPool, auth: Arc<Authenticator>) -> AppState` — Build a state from an existing pool and a pre-built [`Authenticator`]
- pub `router` function L90-203 — `(state: AppState) -> Router` — The production router: `/healthz` open; everything under `/api` behind
- pub `WhoamiTeam` struct L207-214 — `{ id: Uuid, slug: String, name: String }` — A team the caller belongs to (from the tenant schema's `team_members`).
- pub `WhoamiBoardCapabilities` struct L224-232 — `{ board_id: Uuid, board_slug: String, grants: Vec<String> }` — One board on which the caller holds explicit capability grants
- pub `WhoamiResponse` struct L237-249 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — `GET /api/whoami` — the S-0006 whoami precursor: proves the full
- pub `WhoamiUser` struct L253-262 — `{ id: Uuid, external_id: String, email: String, display_name: String }` — The `user` object of [`WhoamiResponse`].
- pub `WhoamiOrganization` struct L266-273 — `{ id: Uuid, slug: String, role: &'static str }` — The `organization` object of [`WhoamiResponse`].
- pub `serve` function L350-364 — `(config: AppConfig) -> Result<(), String>` — Run the server: build state, bind `KAIROS_BIND_ADDR`, serve with
-  `POOL_SIZE` variable L56 — `: u32` — Pool size for the server.
-  `whoami` function L276-345 — `( Extension(auth): Extension<AuthContext>, Extension(tenant): Extension<TenantCo...` — The probe endpoint behind the full middleware stack (KAIROS-T-0017).
-  `shutdown_signal` function L367-373 — `()` — Resolves when ctrl-c (SIGINT) arrives.

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
- pub `AppConfig` struct L78-132 — `{ database_url: String, bind_addr: SocketAddr, oidc_issuer_url: String, oidc_aud...` — Everything the server needs to run, resolved once at startup.
- pub `from_env` function L137-139 — `() -> Result<Self, ConfigError>` — Read configuration from the process environment, failing fast on
- pub `from_lookup` function L143-231 — `(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError>` — Testable core of [`Self::from_env`]: resolve from any lookup
-  `ApiBearer` type L43-52 — `= ApiBearer` — variants directly instead of mutating process environment.
-  `AppConfig` type L134-232 — `= AppConfig` — variants directly instead of mutating process environment.
-  `tests` module L235-348 — `-` — variants directly instead of mutating process environment.
-  `lookup` function L239-245 — `(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String>` — variants directly instead of mutating process environment.
-  `MINIMAL` variable L247-251 — `: &[(&str, &str)]` — variants directly instead of mutating process environment.
-  `minimal_config_applies_defaults` function L254-272 — `()` — variants directly instead of mutating process environment.
-  `missing_required_var_names_it` function L275-278 — `()` — variants directly instead of mutating process environment.
-  `empty_value_is_treated_as_unset` function L281-286 — `()` — variants directly instead of mutating process environment.
-  `invalid_bind_addr_and_log_format_are_rejected` function L289-309 — `()` — variants directly instead of mutating process environment.
-  `api_bearer_id_token_is_parsed` function L312-318 — `()` — variants directly instead of mutating process environment.
-  `optional_vars_are_carried_through` function L321-347 — `()` — variants directly instead of mutating process environment.

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
- pub `metrics` module L21 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `middleware` module L22 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `ws` module L23 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `mcp` module L25 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `scim` module L27 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `service_accounts` module L29 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `web` module L31 — `-` — - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)
- pub `init_tracing` function L39-51 — `(config: &AppConfig)` — Initialize `tracing-subscriber` per KAIROS-A-0013: `KAIROS_LOG_LEVEL`

#### crates/kairos-server/src/main.rs

-  `connect_and_migrate_public` function L43-64 — `() -> Result<PgConnection, String>` — Establish the migration connection from `DATABASE_URL` and apply pending
-  `flag_value` function L67-75 — `(args: &[String], flag: &str) -> Result<Option<String>, String>` — Value of `--flag <value>` in `args`, if present.
-  `has_flag` function L77-79 — `(args: &[String], flag: &str) -> bool` — version and exits 0.
-  `create_tenant` function L81-100 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — version and exits 0.
-  `drop_tenant` function L102-112 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — version and exits 0.
-  `migrate_tenants` function L114-138 — `(conn: &mut PgConnection) -> Result<(), String>` — version and exits 0.
-  `list_tenants` function L140-156 — `(conn: &mut PgConnection) -> Result<(), String>` — version and exits 0.
-  `seed_demo` function L163-201 — `(conn: &mut PgConnection, args: &[String]) -> Result<(), String>` — The `seed-demo` subcommand (KAIROS-T-0035, KAIROS-A-0012 fixtures):
-  `serve` function L206-215 — `() -> Result<(), String>` — The `serve` subcommand (KAIROS-T-0017): fail-fast config, tracing init
-  `run` function L217-242 — `() -> Result<bool, String>` — version and exits 0.
-  `main` function L244-256 — `() -> ExitCode` — version and exits 0.
-  `tests` module L259-294 — `-` — version and exits 0.
-  `smoke` function L263-265 — `()` — version and exits 0.
-  `args` function L267-269 — `(list: &[&str]) -> Vec<String>` — version and exits 0.
-  `flag_value_parses_pairs` function L272-280 — `()` — version and exits 0.
-  `flag_value_rejects_missing_value` function L283-286 — `()` — version and exits 0.
-  `has_flag_detects_presence` function L289-293 — `()` — version and exits 0.

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

- pub `MyBoardsParams` struct L64-67 — `{ level: Option<String> }` — `activity_log` writes as the API path).
- pub `BoardItemsParams` struct L71-76 — `{ board: String, column: Option<String> }` — `activity_log` writes as the API path).
- pub `GetItemParams` struct L80-83 — `{ short_code: String }` — `activity_log` writes as the API path).
- pub `GetHistoryParams` struct L87-94 — `{ short_code: String, limit: Option<i64>, version: Option<i32> }` — `activity_log` writes as the API path).
- pub `SearchParams` struct L98-111 — `{ q: Option<String>, filter: Option<SearchFilterParams>, traverse: Option<Search...` — `activity_log` writes as the API path).
- pub `SearchFilterParams` struct L115-137 — `{ entity_type: Option<Vec<String>>, board_id: Option<String>, column_id: Option<...` — `activity_log` writes as the API path).
- pub `SearchTraverseParams` struct L141-150 — `{ from: String, relationships: Vec<String>, direction: String, depth: Option<u32...` — `activity_log` writes as the API path).
- pub `SearchSortParams` struct L154-159 — `{ field: String, order: String }` — `activity_log` writes as the API path).
- pub `CreateItemParams` struct L163-187 — `{ item_type: String, title: String, board: Option<String>, parent: Option<String...` — `activity_log` writes as the API path).
- pub `UpdateItemParams` struct L191-201 — `{ short_code: String, title: Option<String>, content: String, version: i32 }` — `activity_log` writes as the API path).
- pub `EditItemParams` struct L205-216 — `{ short_code: String, search: String, replace: String, replace_all: bool }` — `activity_log` writes as the API path).
- pub `TransitionItemParams` struct L220-226 — `{ short_code: String, to_column: String }` — `activity_log` writes as the API path).
- pub `LinkItemsParams` struct L230-237 — `{ source: String, target: String, relationship: String }` — `activity_log` writes as the API path).
- pub `UnlinkItemsParams` struct L241-248 — `{ source: String, target: String, relationship: String }` — `activity_log` writes as the API path).
- pub `SetMetadataParams` struct L252-259 — `{ short_code: String, values: BTreeMap<String, Option<String>> }` — `activity_log` writes as the API path).
- pub `DeleteItemParams` struct L263-269 — `{ short_code: String, confirm: bool }` — `activity_log` writes as the API path).
- pub `whoami` function L292-349 — `( &self, context: RequestContext<RoleServer>, ) -> Result<CallToolResult, ErrorD...` — `activity_log` writes as the API path).
- pub `my_boards` function L354-426 — `( &self, Parameters(params): Parameters<MyBoardsParams>, context: RequestContext...` — `activity_log` writes as the API path).
- pub `board_items` function L431-464 — `( &self, Parameters(params): Parameters<BoardItemsParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `get_item` function L469-546 — `( &self, Parameters(params): Parameters<GetItemParams>, context: RequestContext<...` — `activity_log` writes as the API path).
- pub `get_history` function L551-616 — `( &self, Parameters(params): Parameters<GetHistoryParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `search` function L621-636 — `( &self, Parameters(params): Parameters<SearchParams>, context: RequestContext<R...` — `activity_log` writes as the API path).
- pub `create_item` function L641-653 — `( &self, Parameters(params): Parameters<CreateItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `update_item` function L658-683 — `( &self, Parameters(params): Parameters<UpdateItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
- pub `edit_item` function L688-744 — `( &self, Parameters(params): Parameters<EditItemParams>, context: RequestContext...` — `activity_log` writes as the API path).
- pub `transition_item` function L749-803 — `( &self, Parameters(params): Parameters<TransitionItemParams>, context: RequestC...` — `activity_log` writes as the API path).
- pub `link_items` function L808-831 — `( &self, Parameters(params): Parameters<LinkItemsParams>, context: RequestContex...` — `activity_log` writes as the API path).
- pub `unlink_items` function L836-859 — `( &self, Parameters(params): Parameters<UnlinkItemsParams>, context: RequestCont...` — `activity_log` writes as the API path).
- pub `set_metadata` function L864-956 — `( &self, Parameters(params): Parameters<SetMetadataParams>, context: RequestCont...` — `activity_log` writes as the API path).
- pub `delete_item` function L961-993 — `( &self, Parameters(params): Parameters<DeleteItemParams>, context: RequestConte...` — `activity_log` writes as the API path).
-  `KairosMcp` type L276-994 — `= KairosMcp` — `activity_log` writes as the API path).
-  `run_tool` function L279-287 — `(&self, tenant: &TenantContext, f: F) -> Result<CallToolResult, ErrorData>` — Run one closure on a tenant-pinned sync connection (the T-0018
-  `ItemView` struct L1001-1018 — `{ id: Uuid, item_type: ItemType, short_code: String, title: String, content: Str...` — A uniform projection of any live item, whatever its table.
-  `load_item` function L1022-1173 — `(conn: &mut PgConnection, short_code: &str) -> Result<ItemView, ApiError>` — Resolve a short code to a live item and load its [`ItemView`]; 404
-  `authorize_item_write` function L1178-1186 — `( conn: &mut PgConnection, slug: &str, user: Uuid, item: &ItemView, ) -> Result<...` — The A-0006 write gate for an item: `manage_<type>` on the item's
-  `board_by_ref` function L1189-1204 — `(conn: &mut PgConnection, reference: &str) -> Result<Board, ApiError>` — Resolve a board by UUID or slug; 404 `NOT_FOUND` otherwise.
-  `board_by_id` function L1207-1214 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError>` — A board row by id (must exist — callers hold a FK to it).
-  `board_columns` function L1217-1225 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError>` — A board's columns in position order.
-  `resolve_column` function L1229-1246 — `(columns: &[BoardColumn], reference: &str) -> Result<Uuid, ApiError>` — Resolve a column reference (UUID or case-insensitive name) against a
-  `BoardItemRow` struct L1249-1256 — `{ column_id: Uuid, short_code: String, title: String, kind: String }` — One compact row of a board listing.
-  `board_item_rows` function L1260-1352 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardItemRow>, ApiError>` — Every live item placed on a board (strategies, initiatives, tasks, and
-  `column_item_counts` function L1355-1364 — `( conn: &mut PgConnection, board_id: Uuid, ) -> Result<HashMap<Uuid, i64>, ApiEr...` — Per-column live item counts for one board.
-  `require_live` function L1368-1374 — `(conn: &mut PgConnection, short_code: &str, field: &str) -> Result<Uuid, ApiErro...` — A live item's id by short code, with the offending field named on
-  `metadata_lines` function L1378-1391 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError>` — An item's metadata values as compact `- slug: value` lines (ordered by
-  `ChainRow` struct L1394-1401 — `{ id: Uuid, short_code: String, title: String }` — `activity_log` writes as the API path).
-  `parent_chain` function L1406-1430 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ChainRow>, ApiError>` — The item's ancestors via incoming `parent` edges, nearest first
-  `relationship_lines` function L1435-1476 — `(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError>` — Agent-oriented relationship lines for `get_item`: parent chain,
-  `map_update_error` function L1487-1515 — `( conn: &mut PgConnection, item: &ItemView, e: items::ItemError, ) -> Result<Api...` — Map an [`items::ItemError`] from a content update to the S-0006 tool
-  `map_link_error` function L1525-1540 — `(e: GraphError) -> ApiError` — [`GraphError`] → the same codes the REST relationship endpoints emit:
-  `level_of` function L1547-1555 — `(item_type: ItemType) -> BoardLevel` — The board level whose boards host this item type.
-  `default_board_for` function L1559-1581 — `(conn: &mut PgConnection, level: BoardLevel) -> Result<Board, ApiError>` — The tenant's single live board of `level`, or a 422 asking the agent to
-  `resolve_template` function L1584-1615 — `(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError>` — Resolve a template reference (UUID, slug, or name) to its id.
-  `reject_field` function L1619-1631 — `( field: &str, value: Option<&String>, item_type: ItemType, applies_to: &str, ) ...` — Reject a type-specific field supplied for the wrong item type (agents
-  `create_item_impl` function L1636-1844 — `( conn: &mut PgConnection, slug: &str, user: Uuid, params: &CreateItemParams, ) ...` — The create_item body: resolve the target board (or parent, for
-  `field_invalid` function L1852-1854 — `(field: &str, message: impl Into<String>) -> ApiError` — A field-level 422 `VALIDATION` for the search input (the tool-error
-  `uuid_field` function L1856-1859 — `(value: &str, field: &str) -> Result<Uuid, ApiError>` — `activity_log` writes as the API path).
-  `timestamp_field` function L1861-1870 — `(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError>` — `activity_log` writes as the API path).
-  `enum_field` function L1874-1885 — `( value: &str, field: &str, allowed: &str, ) -> Result<T, ApiError>` — Parse a closed-vocabulary value through the core model's serde
-  `search_to_core` function L1889-2002 — `(params: &SearchParams) -> Result<core_search::SearchRequest, ApiError>` — Convert the tool input into the typed `kairos_core::search` request and
-  `map_search_error` function L2006-2014 — `(e: SearchError) -> ApiError` — [`SearchError`] → tool error (validation was pre-checked, so this is
-  `render_search_results` function L2018-2063 — `(results: &SearchResults) -> String` — Compact REQ-1.6 rendering: results grouped by type, one line per item

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
-  `tests` module L182-262 — `-` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `config` function L186-203 — `(single_tenant: Option<&str>, base_domain: Option<&str>) -> AppConfig` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `headers` function L205-214 — `(pairs: &[(&str, &str)]) -> HeaderMap` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `single_tenant_mode_wins_over_everything` function L217-221 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `host_subdomain_resolves_against_base_domain` function L224-230 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `non_matching_hosts_fall_through_to_x_tenant` function L233-246 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `x_tenant_is_the_fallback_without_base_domain` function L249-253 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.
-  `unresolvable_requests_are_tenant_not_found` function L256-261 — `()` — [`TenantDb`] handle whose connections are pinned to the tenant schema.

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
-  `count_board_items` function L731-755 — `(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ScimError>` — How many workflow items still reference `board_id` (the T-0010
-  `delete_group` function L760-825 — `( State(state): State<AppState>, Extension(ctx): Extension<ScimContext>, Path(id...` — `DELETE /scim/v2/Groups/{id}` — soft-delete the team + its delivery

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

#### crates/kairos-server/tests/cascade_preview.rs

-  `common` module L17 — `-` — (`GET /api/{entity_type}/{short_code}/cascade-preview`), through the
-  `SCRATCH_DB` variable L38 — `: &str` — never touched (shared-services discipline).
-  `user_id` function L40-46 — `(conn: &mut PgConnection, email: &str) -> Uuid` — never touched (shared-services discipline).
-  `board_id` function L48-55 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — never touched (shared-services discipline).
-  `uuid` function L58-60 — `(s: &str) -> Uuid` — Parse a DTO id string to a Uuid.
-  `cascade_preview_matches_actual_cascade` function L63-262 — `()` — never touched (shared-services discipline).

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
-  `typed_error_mapping_roundtrip` function L93-409 — `()` — discipline).

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
-  `entity_endpoints_against_live_stack` function L183-1075 — `()` — admin API for grants is KAIROS-T-0019.

#### crates/kairos-server/tests/mcp.rs

-  `common` module L34 — `-` — per KAIROS-A-0011 / KAIROS-S-0006): a real MCP session over streamable
-  `SCRATCH_DB` variable L60 — `: &str` — Uniquely named scratch database for this test binary.
-  `TENANT` variable L63 — `: &str` — The tenant slug.
-  `HOST_HEADER` variable L68 — `: &str` — Every request carries a real `Host` header and the tenant resolves from
-  `raw_request` function L76-112 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, session: Opti...` — One in-process request against `/mcp` (or any URI): returns status, the
-  `rpc_message` function L117-132 — `(body: &str) -> Value` — Extract the JSON-RPC message from a streamable-HTTP response body:
-  `McpSession` struct L136-141 — `{ router: &'a Router, token: String, session_id: Option<String>, next_id: i64 }` — An MCP session over the production router: POSTs JSON-RPC to `/mcp`
-  `connect` function L146-192 — `(router: &'a Router, token: &str) -> (McpSession<'a>, Value)` — Drive `initialize` + `notifications/initialized`; returns the
-  `request` function L195-218 — `(&mut self, method: &str, params: Value) -> Value` — One JSON-RPC request within the session; returns the `result`.
-  `call` function L221-231 — `(&mut self, tool: &str, arguments: Value) -> (bool, String)` — Call a tool; returns `(is_error, text)` from the CallToolResult.
-  `call_ok` function L234-238 — `(&mut self, tool: &str, arguments: Value) -> String` — Call a tool and require success, returning the text.
-  `call_err` function L241-245 — `(&mut self, tool: &str, arguments: Value) -> String` — Call a tool and require a tool error, returning the text.
-  `extract_code` function L249-257 — `(text: &str, prefix: &str) -> String` — The first short code with `prefix` in `text` (e.g.
-  `user_id` function L264-270 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id_of` function L273-280 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level.
-  `NameRow` struct L283-286 — `{ name: String }` — calls (asserted straight from the scratch database).
-  `column_names` function L289-297 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Column names of a board in position order.
-  `reachable_from_first` function L300-314 — `(conn: &mut PgConnection, board: Uuid) -> Vec<String>` — Column names reachable from the FIRST column per `board_transitions`.
-  `CountRow` struct L317-320 — `{ n: i64 }` — calls (asserted straight from the scratch database).
-  `activity_count` function L323-333 — `(conn: &mut PgConnection, actor: Uuid, action: &str) -> i64` — `activity_log` rows in the scratch tenant for one action + actor.
-  `mcp_endpoint_against_live_stack` function L340-746 — `()` — calls (asserted straight from the scratch database).

#### crates/kairos-server/tests/meta.rs

-  `common` module L20 — `-` — relationships, item metadata, metadata definitions, templates, content
-  `SCRATCH_DB` variable L53 — `: &str` — Uniquely named scratch database for this test binary.
-  `user_id` function L56-62 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id` function L65-72 — `(conn: &mut PgConnection, level: BoardLevel) -> Uuid` — The tenant board of a level.
-  `rejection` function L75-80 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `rel_group` function L84-89 — `( groups: &'a [RelationshipGroup], relationship: &str, ) -> Option<&'a Relations...` — The group of `relationship` in one direction of a relationships
-  `metadata_value` function L93-95 — `(body: &'a ItemMetadataResponse, slug: &str) -> Option<&'a MetadataValue>` — The metadata value entry for `slug` in an item-metadata response, if
-  `metadata_patch` function L98-102 — `(slug: &str, value: Option<&str>) -> UpdateMetadataRequest` — A one-entry metadata PATCH body (`None` clears the slug).
-  `meta_endpoints_against_live_stack` function L105-982 — `()` — - `bob`   — org member with NO grants (the 403 matrix; reads only).

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
-  `org_and_admin_endpoints_against_live_stack` function L75-975 — `()` — - `bob`   — org member used for the capability grant/revoke lifecycle.

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

-  `common` module L31 — `-` — KAIROS-A-0007 / S-0005), through the typed `kairos_client::KairosClient`
-  `SCRATCH_DB` variable L66 — `: &str` — Uniquely named scratch database for this test binary.
-  `rejection` function L69-74 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `traverse_from` function L77-92 — `( short_code: &str, relationships: &[&str], direction: &str, depth: Option<u32>,...` — A traverse clause from a short code (the S-0005 examples' shape).
-  `assert_validation_400` function L96-106 — `(client: &KairosClient, request: SearchRequest, field: &str)` — Assert the request fails as 400 `VALIDATION` naming `field` in
-  `present_groups` function L111-130 — `(body: &SearchResponse) -> Vec<&'static str>` — The names of the NON-EMPTY groups of a typed search response, sorted.
-  `sorted_codes` function L133-137 — `(codes: impl IntoIterator<Item = String>) -> Vec<String>` — The `short_code` values of one typed result group, sorted.
-  `user_id` function L140-146 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `board_id_by_slug` function L149-156 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — The board with this slug in the current tenant schema.
-  `metadata_definition` function L160-181 — `(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid` — The tenant's metadata definition with this slug, creating it if the
-  `set_metadata` function L183-192 — `(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str)` — - unknown traverse root -> 404
-  `search_endpoint_against_live_stack` function L195-818 — `()` — - unknown traverse root -> 404

#### crates/kairos-server/tests/service_account_mgmt.rs

-  `common` module L10 — `-` — (KAIROS-A-0017 / KAIROS-T-0059), end to end with the auth branch
-  `SCRATCH_DB` variable L28 — `: &str` — and real Dex tokens (alice = admin, bob = non-member).
-  `get` function L30-37 — `( router: &Router, uri: &str, token: &str, headers: &[(&str, &str)], ) -> (Statu...` — and real Dex tokens (alice = admin, bob = non-member).
-  `post` function L39-47 — `( router: &Router, uri: &str, token: &str, headers: &[(&str, &str)], body: Value...` — and real Dex tokens (alice = admin, bob = non-member).
-  `service_account_management_end_to_end` function L50-222 — `()` — and real Dex tokens (alice = admin, bob = non-member).

#### crates/kairos-server/tests/tenant_isolation.rs

-  `common` module L17 — `-` — through `kairos_client::KairosClient` instances (KAIROS-T-0016 covered
-  `SCRATCH_DB` variable L40 — `: &str` — Uniquely named scratch database for this test binary.
-  `SHARED_TITLE` variable L43 — `: &str` — The distinctive text seeded IDENTICALLY into both tenants.
-  `user_id` function L46-52 — `(conn: &mut PgConnection, email: &str) -> Uuid` — `public.users.id` by email (JIT-provisioned by a first request).
-  `org_id` function L55-61 — `(conn: &mut PgConnection, slug: &str) -> Uuid` — `public.organizations.id` by slug.
-  `rejection` function L64-69 — `(result: Result<T, Error>) -> Error` — Unwrap an expected API rejection (panics on success).
-  `seed_tenant` function L74-115 — `(admin: &KairosClient) -> (String, String, String)` — Seed one tenant's same-shaped fixture THROUGH THE API: a delivery
-  `http_level_two_tenant_isolation` function L118-312 — `()` — discipline).

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
-  `ws_events_against_live_stack` function L203-625 — `()` — disconnect/reconnect — every await timeout-bounded.

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
- pub `base_config` function L184-205 — `(scratch_url: &str) -> AppConfig` — The standard test config: multi-tenant with `kairos.test` as the base
- pub `request` function L210-246 — `( router: &Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&...` — One in-process request against the production router: any method/URI,
- pub `error_code` function L250-254 — `(body: &Value) -> &str` — The `error.code` of an S-0005 error envelope (panics on any other
- pub `TestServer` struct L260-265 — `{ addr: std::net::SocketAddr, base_url: String }` — A live instance of the production router on an ephemeral local port —
- pub `spawn_server` function L268-280 — `(router: Router) -> TestServer` — Serve `router` on an ephemeral 127.0.0.1 port.
- pub `client` function L285-287 — `(&self, token: &str, tenant: &str) -> KairosClient` — A typed client for this server: fixed bearer token, tenant via the
- pub `client_untenanted` function L291-293 — `(&self, token: &str) -> KairosClient` — A typed client WITHOUT tenant resolution (cross-tenant
-  `DB_SETUP_ATTEMPTS` variable L63 — `: u32` — Attempts for each transient scratch-DB lifecycle op against the SHARED
-  `DB_SETUP_BACKOFF_BASE` variable L67 — `: std::time::Duration` — Base backoff between scratch-DB retry attempts; doubles each attempt,
-  `DB_SETUP_BACKOFF_CAP` variable L70 — `: std::time::Duration` — Ceiling on the per-attempt backoff.
-  `retry_db` function L74-91 — `(what: &str, mut op: impl FnMut() -> Result<T, E>) -> T` — Run a transient shared-Postgres op with bounded exponential backoff,
-  `connect_admin` function L95-103 — `() -> PgConnection` — Connect to the shared compose Postgres admin database, retrying transient
-  `TestServer` type L282-294 — `= TestServer` — different subset, so unused-item lints are expected noise here.

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
- pub `WsCounters` struct L376-380 — `{ connects: AtomicU64, events: AtomicU64, reconnects: AtomicU64 }` — Shared counters for the standing event subscribers.
- pub `ws_subscriber` function L384-421 — `( client: KairosClient, counters: Arc<WsCounters>, mut stop: watch::Receiver<boo...` — A standing `/ws/events` subscriber: counts delivered thin events,
-  `POOL_CAP` variable L26 — `: usize` — Cap on each worker's created-item pool (edit/transition targets).
-  `record_call` function L44-65 — `( recorder: &Recorder, class: OpClass, conflict_expected: bool, fut: impl Future...` — Record one client call under `class`.
-  `choose_op` function L104-131 — `(config: &SoakConfig, rng: &mut SmallRng, pool_empty: bool) -> OpClass` — Weighted op selection; ops that need the worker's own pool degrade to
-  `run_op` function L134-368 — `( spec: &WorkerSpec, world: &World, config: &SoakConfig, recorder: &Recorder, ht...` — the whole session (documented; it gets a session-sized budget).
-  `tests` module L424-457 — `-` — the whole session (documented; it gets a session-sized budget).
-  `op_choice_follows_weights_and_degrades_without_a_pool` function L429-456 — `()` — the whole session (documented; it gets a session-sized budget).

#### crates/kairos-soak/src/world.rs

- pub `BoardInfo` struct L32-37 — `{ id: String, slug: String, targets: BTreeMap<String, Vec<String>> }` — A delivery board with its column set and transition adjacency.
- pub `World` struct L41-51 — `{ delivery_boards: Vec<BoardInfo>, collision_codes: Vec<String>, strategy_code: ...` — Everything the workers share.
- pub `board_by_id` function L54-56 — `(&self, id: &str) -> Option<&BoardInfo>` — version, and stays under a hard cap).
- pub `setup_world` function L66-273 — `( alice: &KairosClient, others: &[(&str, &KairosClient)], ) -> Result<World, Str...` — Enroll the workforce and create the shared fixtures.
- pub `Bystander` struct L280-284 — `{ client: KairosClient, tenant: String, baseline: BTreeMap<String, String> }` — The bystander tenant and its baseline snapshot.
- pub `BystanderUnavailable` enum L288-293 — `NotDeploymentAdmin | Failed` — Why the bystander check could not run (reported loudly either way).
- pub `setup_bystander` function L299-372 — `( admin: &KairosClient, tenant_client: KairosClient, slug: &str, ) -> Result<Bys...` — Provision (or adopt) the bystander tenant and take the baseline
- pub `snapshot` function L387-479 — `(client: &KairosClient) -> Result<BTreeMap<String, String>, String>` — The bystander's full API-visible state, section by section.
- pub `changed_sections` function L483-501 — `( baseline: &BTreeMap<String, String>, current: &BTreeMap<String, String>, ) -> ...` — Compare a fresh snapshot against the baseline; returns the changed
- pub `HistoryBound` struct L509-513 — `{ short_code: String, version: i32, history_rows: i64 }` — Per-item history summary for the report.
- pub `history_bound_check` function L518-576 — `( client: &KairosClient, codes: &[String], max_rows_per_item: i64, ) -> (Vec<His...` — Assert `item_history` rows per item == item version (history grows
-  `World` type L53-57 — `= World` — version, and stays under a hard cap).
-  `setup_err` function L59-61 — `(stage: &str, e: impl std::fmt::Display) -> String` — version, and stays under a hard cap).
-  `canonical` function L377-384 — `(items: &[T]) -> Result<String, String>` — Serialize a list of JSON-serializable items into one canonical string
-  `tests` module L579-605 — `-` — version, and stays under a hard cap).
-  `changed_sections_attributes_differences` function L583-604 — `()` — version, and stays under a hard cap).

### crates/kairos-web/src

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/api.rs

- pub `get_json` function L40-48 — `(auth: Auth, path: &str) -> Result<T, ApiError>` — `GET {path}` with the bearer token; JSON-decode the body.
- pub `post_json` function L52-69 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, ApiError>` — `POST {path}` with a JSON body and the bearer token; JSON-decode the
- pub `patch_json` function L73-90 — `( auth: Auth, path: &str, body: &B, ) -> Result<T, ApiError>` — `PATCH {path}` with a JSON body and the bearer token; JSON-decode the
- pub `delete_json` function L93-101 — `(auth: Auth, path: &str) -> Result<T, ApiError>` — `DELETE {path}` with the bearer token; JSON-decode the response body.
- pub `whoami` function L167-169 — `(auth: Auth) -> Result<Whoami, ApiError>` — `GET /api/whoami`.
- pub `Whoami` struct L173-182 — `{ user: WhoamiUser, organization: WhoamiOrganization, teams: Vec<WhoamiTeam>, ca...` — mirror of: `kairos_server::app::WhoamiResponse` (partial).
- pub `WhoamiBoardCapabilities` struct L186-189 — `{ board_slug: String, grants: Vec<String> }` — mirror of: `kairos_server::app::WhoamiBoardCapabilities` (partial).
- pub `WhoamiUser` struct L193-196 — `{ display_name: String, email: String }` — mirror of: `kairos_server::app::WhoamiUser` (partial).
- pub `WhoamiOrganization` struct L200-203 — `{ slug: String, role: String }` — mirror of: `kairos_server::app::WhoamiOrganization` (partial).
- pub `WhoamiTeam` struct L209-213 — `{ id: String, slug: String, name: String }` — mirror of: `kairos_server::app::WhoamiTeam` (partial).
-  `decode_response` function L113-133 — `( auth: Auth, sent_token: Option<String>, path: &str, response: gloo_net::http::...` — The shared response tail of every `*_json` helper: 401 clears the
-  `error_from_response` function L137-149 — `(status: u16, response: gloo_net::http::Response) -> ApiError` — Map a non-2xx response onto [`ApiError`] via the S-0005 error envelope
-  `ErrorEnvelope` struct L153-155 — `{ error: ErrorBody }` — mirror of: `kairos_client::types::ErrorEnvelope` (S-0005).
-  `ErrorBody` struct L159-162 — `{ code: String, message: String }` — mirror of: `kairos_client::types::ErrorBody` (S-0005).
-  `tests` module L216-249 — `-` — login redirect); components never handle 401 themselves.
-  `whoami_mirror_decodes_server_shape` function L221-248 — `()` — The mirror decodes a real WhoamiResponse body (field-name lock).

#### crates/kairos-web/src/app.rs

- pub `App` function L48-83 — `() -> impl IntoView` — The application root: provides auth, injects the Aurora stylesheet,
- pub `BrandMark` function L288-296 — `() -> impl IntoView` — The Kairos brand mark — app-supplied (aurora ships no branding), drawn
-  `Shell` function L93-133 — `() -> impl IntoView` — The protected shell: aurora `AppShell` with the Kairos header
-  `MyTeamsNav` function L140-162 — `(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView` — "My teams" (KAIROS-T-0068): the caller's own teams from whoami, each
-  `GuardFallback` function L170-186 — `() -> impl IntoView` — The unauthenticated fallback for the protected shell: while a boot-time
-  `RedirectToIssuer` function L191-229 — `() -> impl IntoView` — The unauthenticated fallback: kick off the PKCE redirect (remembering
-  `NavLink` function L235-245 — `(#[prop(into)] href: String, #[prop(into)] label: String) -> impl IntoView` — One left-nav entry.
-  `WhoamiBadge` function L251-267 — `(whoami: LocalResource<Result<api::Whoami, ApiError>>) -> impl IntoView` — Who am I, which org, which role — the A-0015 whoami display.
-  `LogoutButton` function L272-283 — `() -> impl IntoView` — Drop the in-memory session; the shell guard (which sees the explicit

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
- pub `admin` module L115 — `-` — later tasks should keep.
- pub `NotFoundPage` function L126-136 — `() -> impl IntoView` — Router fallback.
-  `boards` module L92 — `-` — later tasks should keep.
-  `item` module L97 — `-` — later tasks should keep.
-  `search` module L102 — `-` — later tasks should keep.
-  `teams` module L109 — `-` — later tasks should keep.
-  `activity` module L121 — `-` — later tasks should keep.

### crates/kairos-web/src/pages

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/activity.rs

- pub `ListEnvelope` struct L66-71 — `{ items: Vec<T>, total: i64, limit: i64, offset: i64 }` — mirror of: `kairos_client::types::ListEnvelope<T>`.
- pub `HistoryVersion` struct L75-79 — `{ version: i32, edited_by: String, edited_at: String }` — mirror of: `kairos_client::types_meta::HistoryVersion`.
- pub `HistorySnapshot` struct L83-89 — `{ version: i32, title: String, content: String, edited_by: String, edited_at: St...` — mirror of: `kairos_client::types_meta::HistorySnapshot`.
- pub `ActivityEntry` struct L93-101 — `{ id: String, actor_id: String, action: String, entity_id: Option<String>, entit...` — mirror of: `kairos_client::types_meta::ActivityEntry`.
- pub `Member` struct L105-109 — `{ user_id: String, email: String, display_name: String }` — mirror of: `kairos_client::types_org::OrgMember` (partial).
- pub `ItemHead` struct L115-120 — `{ id: String, short_code: String, title: String, version: i32 }` — mirror of: the shared head of `kairos_client::types::{Strategy,
- pub `DiffLine` struct L202-205 — `{ sign: char, text: String }` — One rendered diff line: `+` inserted, `-` deleted, ` ` unchanged.
- pub `diff_lines` function L211-223 — `(old: &str, new: &str) -> Vec<DiffLine>` — Client-side line diff (KAIROS-T-0044 AC).
- pub `ActivityPage` function L348-564 — `() -> impl IntoView` — `/activity` — the filterable, paginated audit-trail feed.
- pub `ItemHistoryPage` function L693-932 — `() -> impl IntoView` — `/activity/history/:code` — version list, snapshot viewer, two-version
-  `PAGE_SIZE` variable L55 — `: i64` — Feed page size.
-  `ALL` variable L58 — `: &str` — The "no filter" option label shared by the actor/action selects.
-  `ACTIONS` variable L128-137 — `: &[&str]` — The `activity_log.action` vocabulary (KAIROS-A-0004; enforcement point
-  `family_of_short_code` function L141-157 — `(code: &str) -> Option<&'static str>` — Map a `{PREFIX}-{LETTER}-{NNNN}` short code (S-0004) onto its API
-  `encode_query` function L162-173 — `(value: &str) -> String` — Percent-encode one query-string value (conservative: everything but
-  `format_when` function L177-185 — `(rfc3339: &str) -> String` — `2026-07-14T23:10:11.123456Z` → `2026-07-14 23:10:11` (display only;
-  `action_color` function L189-198 — `(action: &str) -> &'static str` — Accent token for an activity action pill (data-driven color per the
-  `diff_line_style` function L226-236 — `(sign: char) -> String` — Style + color for one diff line (inline `var(--…)` per the token rule).
-  `actor_label` function L240-245 — `(members: &HashMap<String, String>, actor_id: &str) -> String` — Resolve an actor id to a display name via the members map, falling
-  `fetch_item_head` function L252-260 — `(auth: Auth, code: String) -> Result<(ItemHead, &'static str), ApiError>` — `GET /api/{family}/{code}` — current id/title/version head.
-  `fetch_versions` function L263-269 — `( auth: Auth, code: String, ) -> Result<ListEnvelope<HistoryVersion>, ApiError>` — `GET /api/{family}/{code}/history` — the version list, newest first.
-  `fetch_snapshot` function L272-283 — `( auth: Auth, code: String, version: i32, ) -> Result<HistorySnapshot, ApiError>` — `GET /api/{family}/{code}/history?version=N` — one full snapshot.
-  `fetch_members` function L287-290 — `(auth: Auth) -> Result<Vec<Member>, ApiError>` — `GET /api/members?limit=200` → actor_id → display name map (+ the raw
-  `fetch_directory` function L295-307 — `(auth: Auth) -> HashMap<String, String>` — Best-effort entity_id → short_code directory from the five family list
-  `FeedFilters` struct L311-317 — `{ entity_code: String, actor_id: String, action: String, since: String, offset: ...` — The applied activity-feed filters (what the resource fetches for).
-  `fetch_feed` function L321-340 — `( auth: Auth, filters: FeedFilters, ) -> Result<ListEnvelope<ActivityEntry>, Api...` — `GET /api/activity` with the S-0005 filters.
-  `member_option` function L567-569 — `(member: &Member) -> String` — The option label shown for one member in the actor filter.
-  `FeedTable` function L573-628 — `( page: ListEnvelope<ActivityEntry>, names: HashMap<String, String>, codes: Hash...` — The feed table (extracted so the async-state match stays readable).
-  `FeedPager` function L632-666 — `(page: ListEnvelope<ActivityEntry>, applied: RwSignal<FeedFilters>) -> impl Into...` — Offset pagination controls under the feed table.
-  `DiffView` struct L674-679 — `{ from: i32, to: i32, title_lines: Vec<DiffLine>, content_lines: Vec<DiffLine> }` — A computed two-version diff, ready to render.
-  `RollbackNotice` enum L683-687 — `Done | Conflict | Failed` — The outcome banner state after a rollback attempt.
-  `rollback` function L937-947 — `(auth: Auth, code: String, version: i32) -> Result<i32, ApiError>` — The A-0004 copy-forward rollback: old snapshot → standard versioned
-  `VersionsTable` function L951-1034 — `( page: ListEnvelope<HistoryVersion>, names: HashMap<String, String>, current: O...` — The version-list table: view / diff-select / rollback per row.
-  `DiffPanel` function L1038-1080 — `(view_model: DiffView) -> impl IntoView` — Rendered diff between the two selected versions.
-  `tests` module L1087-1266 — `-` — handled exactly like any other content edit.
-  `short_code_family_mapping` function L1093-1116 — `()` — Letter → family mapping covers all five S-0004 types and rejects
-  `query_encoding_escapes_reserved` function L1121-1131 — `()` — Query values are percent-encoded so RFC 3339 `+00:00` offsets and
-  `when_formatting` function L1135-1142 — `()` — Timestamps render as date + clock; non-timestamps pass through.
-  `action_colors_cover_the_vocabulary` function L1147-1152 — `()` — Every documented activity action gets a deliberate accent; unknown
-  `diff_lines_marks_changes` function L1157-1173 — `()` — The `similar` line diff marks inserts/deletes/context the way the
-  `history_mirrors_decode_server_shape` function L1178-1201 — `()` — mirror decode lock: history version list envelope (server shape from
-  `activity_and_head_mirrors_decode_server_shape` function L1206-1253 — `()` — mirror decode lock: activity entries (nullable entity fields) and
-  `actor_labels_resolve_or_shorten` function L1257-1265 — `()` — Actor labels prefer the members map and degrade to a shortened id.

#### crates/kairos-web/src/pages/admin.rs

- pub `AdminPage` function L64-91 — `() -> impl IntoView` — The admin section shell: whoami-probed role gate, section tabs, and the
- pub `AdminNavLink` function L123-150 — `() -> impl IntoView` — The left-nav "Admin" entry, rendered only when whoami says the caller
- pub `AdminHomePage` function L199-252 — `() -> impl IntoView` — `/admin` — the overview: one card per admin surface.
-  `gating` module L42 — `-` — gate is UX, the server is the authority (A-0006).
-  `api` module L44 — `-` — gate is UX, the server is the authority (A-0006).
-  `boards` module L45 — `-` — gate is UX, the server is the authority (A-0006).
-  `capabilities` module L46 — `-` — gate is UX, the server is the authority (A-0006).
-  `members` module L47 — `-` — gate is UX, the server is the authority (A-0006).
-  `metadata` module L48 — `-` — gate is UX, the server is the authority (A-0006).
-  `streams` module L49 — `-` — gate is UX, the server is the authority (A-0006).
-  `teams` module L50 — `-` — gate is UX, the server is the authority (A-0006).
-  `templates` module L51 — `-` — gate is UX, the server is the authority (A-0006).
-  `NotAdminGate` function L98-117 — `(role: String) -> impl IntoView` — The graceful denied path: what a plain `member` with no board-config
-  `SectionTabs` function L158-192 — `(is_admin: bool) -> impl IntoView` — Horizontal section tabs for the admin area (reuses the nav-link styling
-  `MutationOutcome` type L261 — `= Option<Result<String, aurora_dark::tokens::ApiError>>` — Outcome of the latest mutation in a panel: `Ok(what happened)` or the
-  `MutationNotice` function L267-290 — `(outcome: RwSignal<MutationOutcome>) -> impl IntoView` — Renders the latest mutation outcome per the conventions: success as a
-  `run_mutation` function L296-320 — `( busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, reload: RwSignal<u32...` — Run one admin mutation: guard against double-submit with `busy`, record

#### crates/kairos-web/src/pages/boards.rs

- pub `BoardsPage` function L281-356 — `() -> impl IntoView` — `/boards` — boards in flight-level bands (strategy above initiatives
- pub `BoardPage` function L365-444 — `() -> impl IntoView` — `/boards/:board` — columns from the board config, items grouped, a
-  `data` module L23 — `-` — view (`/boards/:board` — slug or id) with live `/ws/events` updates.
-  `live` module L24 — `-` — attach to strategies/initiatives/tasks only).
-  `describe` function L40-49 — `(error: &ApiError) -> String` — A short human line for a failed mutation (page-level `Banner`; load
-  `level_color` function L52-60 — `(level: &str) -> &'static str` — The accent token for a board level / entity kind.
-  `kind_color` function L62-69 — `(kind: EntityKind) -> &'static str` — attach to strategies/initiatives/tasks only).
-  `DragData` struct L76-80 — `{ kind: EntityKind, short_code: String, targets: Vec<String> }` — The in-flight card drag (KAIROS-T-0064): which card, and the column ids
-  `BoardPowers` struct L91-98 — `{ transition: bool, create: bool, documents: bool }` — What the signed-in user may do on THIS board — mirrors the A-0006
-  `create_capability` function L101-108 — `(kind: EntityKind) -> &'static str` — The `manage_*` capability that creating this kind requires.
-  `grant_covers` function L113-119 — `(grant: &str, required: &str) -> bool` — Does a stored grant cover `required`? Client mirror of the A-0006
-  `team_implies` function L123-128 — `(required: &str) -> bool` — The KAIROS-T-0072 implied set (mirror of
-  `board_powers` function L131-160 — `( me: &crate::api::Whoami, board_slug: &str, board_team_id: Option<&str>, create...` — Compute [`BoardPowers`] from the whoami identity.
-  `run_transition` function L164-178 — `( auth: crate::auth::Auth, kind: EntityKind, code: String, column_id: String, on...` — Run one transition and report through the standard board callbacks —
-  `LEVEL_BANDS` variable L186-191 — `: &[(&str, &str)]` — The flight-level band order for `/boards` (KAIROS-T-0069/T-0063:
-  `BandModel` struct L195-201 — `{ level: String, label: String, groups: Vec<(Option<(String, String)>, Vec<data:...` — One rendered board-list band: level heading + its tiles, with the
-  `band_models` function L207-275 — `( boards: Vec<data::Board>, teams: &[crate::pages::teams::api::Team], ) -> Vec<B...` — Bucket boards into level bands (strategy → initiative → delivery →
-  `BoardBody` function L448-704 — `( view: data::BoardView, /// What the user may do here (KAIROS-T-0072) — gates...` — The loaded board: header (+ document create) and the column row.
-  `CardModel` struct L537-542 — `{ kind: EntityKind, short_code: String, title: String, meta: Vec<(String, &'stat...` — attach to strategies/initiatives/tasks only).
-  `ColumnModel` struct L543-548 — `{ id: String, name: String, targets: Vec<(String, String)>, cards: Vec<CardModel...` — attach to strategies/initiatives/tasks only).
-  `ItemCard` function L715-830 — `( kind: EntityKind, short_code: String, title: String, /// `(label, color-token)...` — One board card: short code, title, type, key metadata, open link,
-  `CreateItemModal` function L841-966 — `( open: RwSignal<bool>, kind: EntityKind, board_id: String, /// Delivery boards ...` — The global create flow (KAIROS-T-0062): the board level's entity type
-  `CreateDocumentModal` function L972-1101 — `( open: RwSignal<bool>, /// `(short_code, title)` of this board's eligible paren...` — "New document" (board header): template picker + parent picker.
-  `tests` module L1104-1232 — `-` — attach to strategies/initiatives/tasks only).
-  `board` function L1108-1116 — `(id: &str, level: &str, team_id: Option<&str>) -> data::Board` — attach to strategies/initiatives/tasks only).
-  `team` function L1118-1126 — `(id: &str, slug: &str) -> Team` — attach to strategies/initiatives/tasks only).
-  `band_models_orders_levels_and_groups_delivery_by_team` function L1131-1162 — `()` — Bands come out in flight-level order, the delivery band grouped by
-  `me` function L1164-1176 — `(role: &str, team_ids: &[&str], grants: &[(&str, &[&str])]) -> crate::api::Whoam...` — attach to strategies/initiatives/tasks only).
-  `board_powers_mirror_team_implication` function L1181-1196 — `()` — KAIROS-T-0072 client mirror: team membership implies the delivery
-  `board_powers_mirror_grants_and_admin` function L1200-1216 — `()` — Explicit grants (incl.
-  `band_models_keeps_unknown_team_boards_reachable` function L1221-1231 — `()` — A board whose team id names an unknown team lands in "No team"

#### crates/kairos-web/src/pages/item.rs

- pub `ItemPage` function L43-61 — `() -> impl IntoView` — `/items/:code` — parse the family from the short code and hand off.
-  `api` module L19 — `-` — entity families (the short code's type letter picks the family — see
-  `create_doc` module L20 — `-` — warning ([`delete`], A-0001).
-  `delete` module L21 — `-` — warning ([`delete`], A-0001).
-  `editor` module L22 — `-` — warning ([`delete`], A-0001).
-  `markdown` module L23 — `-` — warning ([`delete`], A-0001).
-  `metadata` module L24 — `-` — warning ([`delete`], A-0001).
-  `ItemDetailView` function L66-106 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — The detail resource + the four async view states.
-  `ItemLoaded` function L111-171 — `(item: ItemDetail, family: Family, on_saved: Callback<i32>) -> impl IntoView` — The loaded page: header + actions, the editor column, and the facts /
-  `TypeFacts` function L175-205 — `(item: ItemDetail) -> impl IntoView` — The type-specific facts as pills (each family's extra columns).
-  `BoardPanel` function L211-258 — `( family: Family, board_id: Option<String>, column_id: Option<String>, ) -> impl...` — Board/column display: documents never sit on boards; ADRs may not; the
-  `RelationshipsPanel` function L263-301 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — Relationships summary: both directions, grouped, every neighbor linked
-  `RelationshipGroupView` function L305-330 — `( group: RelationshipGroup, #[prop(into)] direction: String, ) -> impl IntoView` — One direction of one relationship type, neighbors linked.

#### crates/kairos-web/src/pages/search.rs

- pub `data` module L12 — `-` — one text query + a structured filter builder + an optional graph
- pub `relationships` module L13 — `-` — component is standalone so the item-detail task can also embed it.
- pub `SearchPage` function L61-448 — `() -> impl IntoView` — `/search` — the unified search page.
-  `ENTITY_TYPES` variable L28 — `: [&str; 5]` — The entity-type vocabulary (S-0005 `filter.entity_type`).
-  `TASK_TYPES` variable L31 — `: [&str; 3]` — The task-type vocabulary (S-0005 `filter.task_type`).
-  `RELATIONSHIPS` variable L34 — `: [&str; 5]` — The relationship vocabulary (S-0005 `traverse.relationships`).
-  `ANY` variable L37 — `: &str` — The "no board/column selected" option.
-  `entity_color` function L40-49 — `(entity_type: &str) -> &'static str` — A data-driven accent per entity type (token constants only).
-  `MetaRow` struct L53-57 — `{ id: usize, key: RwSignal<String>, value: RwSignal<String> }` — One dynamic metadata `key = value` filter row.
-  `ResultGroup` function L453-511 — `(entity: &'static str, title: &'static str, hits: Vec<data::Hit>) -> impl IntoVi...` — One entity-type result group: a panel with linked rows (omitted when

#### crates/kairos-web/src/pages/teams.rs

- pub `TeamsPage` function L44-101 — `() -> impl IntoView` — `/teams` — every team: name, type, member count, linking to detail.
- pub `TeamPage` function L166-183 — `() -> impl IntoView` — `/teams/:slug` — roster, delivery board, and stream membership.
-  `api` module L18 — `-` — directory and the `/teams/:slug` detail — the member-readable answer to
-  `DirectoryRow` struct L35-40 — `{ name: String, slug: String, team_type: String, member_count: usize }` — One directory row, fully resolved before rendering.
-  `TeamView` struct L110-117 — `{ team: api::Team, members: Vec<api::TeamMember>, delivery_board: Option<(String...` — The fully-resolved `/teams/:slug` view model (fetched as one unit so
-  `load_team_view` function L121-162 — `( auth: crate::auth::Auth, slug: &str, ) -> Result<TeamView, aurora_dark::tokens...` — Load everything the detail page shows.
-  `TeamBody` function L187-255 — `(view_model: TeamView) -> impl IntoView` — The loaded team detail.

### crates/kairos-web/src/pages/admin

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/admin/api.rs

- pub `ListEnvelope` struct L22-24 — `{ items: Vec<T> }` — mirror of: `kairos_client::types::ListEnvelope<T>` (partial — the admin
- pub `Board` struct L35-41 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — mirror of: `kairos_client::types_org::Board` (partial).
- pub `BoardColumn` struct L45-49 — `{ id: String, name: String, position: i32 }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `BoardTransition` struct L53-57 — `{ id: String, from_column_id: String, to_column_id: String }` — mirror of: `kairos_client::types_org::BoardTransition` (partial).
- pub `BoardDetail` struct L62-67 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — mirror of: `kairos_client::types_org::BoardDetail` (partial; the board
- pub `list_boards` function L70-73 — `(auth: Auth) -> Result<Vec<Board>, ApiError>` — `GET /api/boards` (first page; admin scale).
- pub `create_board` function L76-94 — `( auth: Auth, name: &str, slug: &str, board_level: &str, team_id: Option<&str>, ...` — `POST /api/boards` — seeded with the level's default columns/transitions.
- pub `delete_board` function L97-99 — `(auth: Auth, board_id: &str) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}` (422 `BOARD_NOT_EMPTY` while items reference it).
- pub `board_detail` function L102-104 — `(auth: Auth, board_id: &str) -> Result<BoardDetail, ApiError>` — `GET /api/boards/{id}` — board + columns + transitions.
- pub `add_column` function L108-120 — `( auth: Auth, board_id: &str, name: &str, position: i32, ) -> Result<Value, ApiE...` — `POST /api/boards/{id}/columns` (422 `DUPLICATE_COLUMN_NAME` /
- pub `update_column` function L123-136 — `( auth: Auth, board_id: &str, column_id: &str, name: Option<&str>, position: Opt...` — `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
- pub `remove_column` function L140-142 — `(auth: Auth, board_id: &str, column_id: &str) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/columns/{col_id}` (422 `COLUMN_NOT_EMPTY` while
- pub `add_transition` function L146-158 — `( auth: Auth, board_id: &str, from_column_id: &str, to_column_id: &str, ) -> Res...` — `POST /api/boards/{id}/transitions` (422 `DUPLICATE_TRANSITION` /
- pub `remove_transition` function L161-171 — `( auth: Auth, board_id: &str, transition_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/transitions/{transition_id}`.
- pub `BoardMember` struct L179-184 — `{ user_id: String, email: String, display_name: String, capabilities: Vec<String...` — mirror of: `kairos_client::types_org::BoardMember`.
- pub `board_members` function L187-189 — `(auth: Auth, board_id: &str) -> Result<Vec<BoardMember>, ApiError>` — `GET /api/boards/{id}/members`.
- pub `add_board_member` function L192-204 — `( auth: Auth, board_id: &str, user_id: &str, capabilities: &[String], ) -> Resul...` — `POST /api/boards/{id}/members` — first grant(s) for a user.
- pub `replace_capabilities` function L207-219 — `( auth: Auth, board_id: &str, user_id: &str, capabilities: &[String], ) -> Resul...` — `PATCH /api/boards/{id}/members/{user_id}` — replaces the full set.
- pub `remove_board_member` function L222-228 — `( auth: Auth, board_id: &str, user_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/boards/{id}/members/{user_id}` — full revocation.
- pub `create_team` function L239-251 — `( auth: Auth, name: &str, slug: &str, team_type: &str, ) -> Result<Team, ApiErro...` — `POST /api/teams` — also creates the team's delivery board
- pub `update_team` function L254-267 — `( auth: Auth, team_id: &str, name: &str, slug: &str, team_type: &str, ) -> Resul...` — `PATCH /api/teams/{id}`.
- pub `delete_team` function L270-272 — `(auth: Auth, team_id: &str) -> Result<Value, ApiError>` — `DELETE /api/teams/{id}`.
- pub `add_team_member` function L275-282 — `(auth: Auth, team_id: &str, user_id: &str) -> Result<Value, ApiError>` — `POST /api/teams/{id}/members`.
- pub `remove_team_member` function L285-291 — `( auth: Auth, team_id: &str, user_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/teams/{id}/members/{user_id}`.
- pub `create_stream` function L300-312 — `( auth: Auth, name: &str, slug: &str, description: Option<&str>, ) -> Result<Val...` — `POST /api/delivery-streams`.
- pub `update_stream` function L315-328 — `( auth: Auth, stream_id: &str, name: &str, slug: &str, description: Option<&str>...` — `PATCH /api/delivery-streams/{id}`.
- pub `delete_stream` function L331-333 — `(auth: Auth, stream_id: &str) -> Result<Value, ApiError>` — `DELETE /api/delivery-streams/{id}`.
- pub `add_stream_team` function L336-347 — `( auth: Auth, stream_id: &str, team_id: &str, ) -> Result<Value, ApiError>` — `POST /api/delivery-streams/{id}/teams`.
- pub `remove_stream_team` function L350-360 — `( auth: Auth, stream_id: &str, team_id: &str, ) -> Result<Value, ApiError>` — `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
- pub `OrgMember` struct L368-373 — `{ user_id: String, email: String, display_name: String, role: String }` — mirror of: `kairos_client::types_org::OrgMember` (partial).
- pub `list_org_members` function L376-379 — `(auth: Auth) -> Result<Vec<OrgMember>, ApiError>` — `GET /api/members`.
- pub `add_org_member` function L383-390 — `(auth: Auth, email: &str, role: &str) -> Result<Value, ApiError>` — `POST /api/members` — resolve by email (users are JIT-provisioned at
- pub `set_org_member_role` function L394-401 — `(auth: Auth, user_id: &str, role: &str) -> Result<Value, ApiError>` — `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN` when
- pub `remove_org_member` function L404-406 — `(auth: Auth, user_id: &str) -> Result<Value, ApiError>` — `DELETE /api/members/{user_id}` (422 `LAST_ADMIN` for the only admin).
- pub `Template` struct L414-419 — `{ id: String, name: String, slug: String, is_system_default: bool }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateMetadataField` struct L423-427 — `{ slug: String, default_value: Option<String>, required: bool }` — mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
- pub `TemplateDetail` struct L431-438 — `{ id: String, name: String, slug: String, content: String, is_system_default: bo...` — mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
- pub `TemplateMetadataEntry` struct L443-447 — `{ definition_slug: String, default_value: Option<String>, required: bool }` — One template ↔ definition association in a template write (the
- pub `list_templates` function L463-467 — `(auth: Auth) -> Result<Vec<Template>, ApiError>` — `GET /api/templates`.
- pub `template_detail` function L470-472 — `(auth: Auth, template_id: &str) -> Result<TemplateDetail, ApiError>` — `GET /api/templates/{id}` — template + its metadata associations.
- pub `create_template` function L475-493 — `( auth: Auth, name: &str, slug: &str, content: &str, metadata: &[TemplateMetadat...` — `POST /api/templates`.
- pub `update_template` function L496-515 — `( auth: Auth, template_id: &str, name: &str, slug: &str, content: &str, metadata...` — `PATCH /api/templates/{id}` — `metadata` replaces the association list.
- pub `delete_template` function L518-520 — `(auth: Auth, template_id: &str) -> Result<Value, ApiError>` — `DELETE /api/templates/{id}` (hard delete).
- pub `MetadataDefinition` struct L528-535 — `{ id: String, name: String, slug: String, field_type: String, is_system_default:...` — mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
- pub `list_definitions` function L538-542 — `(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError>` — `GET /api/metadata-definitions`.
- pub `create_definition` function L546-564 — `( auth: Auth, name: &str, slug: &str, field_type: &str, enum_options: &[String],...` — `POST /api/metadata-definitions` — `enum_options` required non-empty for
- pub `update_definition` function L568-581 — `( auth: Auth, definition_id: &str, name: &str, slug: &str, enum_options: Option<...` — `PATCH /api/metadata-definitions/{id}` — `enum_options` replaces the
- pub `delete_definition` function L584-586 — `(auth: Auth, definition_id: &str) -> Result<Value, ApiError>` — `DELETE /api/metadata-definitions/{id}` (hard delete).
-  `PAGE` variable L27 — `: &str` — Big-enough page for admin lists (server clamps to its own max).
-  `metadata_entries_json` function L449-460 — `(entries: &[TemplateMetadataEntry]) -> Vec<Value>` — decoded as `serde_json::Value` — refetch is the source of truth.
-  `tests` module L589-664 — `-` — decoded as `serde_json::Value` — refetch is the source of truth.
-  `board_detail_mirror_decodes_server_shape` function L595-616 — `()` — `BoardDetail` decodes the wire shape: board fields flattened at the
-  `board_member_mirror_decodes_server_shape` function L620-629 — `()` — `BoardMember` decodes the grants list (capability editor input).
-  `definition_mirror_decodes_server_shape` function L636-645 — `()` — `MetadataDefinition` decodes enum options in order.
-  `template_detail_mirror_decodes_server_shape` function L649-663 — `()` — `TemplateDetail` decodes the metadata association rows.

#### crates/kairos-web/src/pages/admin/boards.rs

- pub `AdminBoardsPage` function L28-176 — `() -> impl IntoView` — `/admin/boards` — every live board, plus create/delete.
- pub `AdminBoardPage` function L182-225 — `() -> impl IntoView` — `/admin/boards/:board` — one board's configuration: columns (add /
-  `LEVELS` variable L24 — `: [&str; 4]` — message plus the `code:` line — components never inspect statuses.
-  `ColumnsPanel` function L231-354 — `( board_id: String, columns: Vec<api::BoardColumn>, busy: RwSignal<bool>, outcom...` — Columns: position-ordered rows with rename / move / remove, plus an
-  `TransitionsPanel` function L358-465 — `( board_id: String, columns: Vec<api::BoardColumn>, transitions: Vec<api::BoardT...` — Transition edges: which column-to-column moves the board allows.
-  `MembersPanel` function L471-653 — `( board_id: String, busy: RwSignal<bool>, outcome: RwSignal<MutationOutcome>, re...` — Board members and their A-0006 capability grants: list with pills, an

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
-  `tests` module L84-155 — `-` — what the UI offers.
-  `me` function L88-107 — `(role: &str, grants: &[(&str, &[&str])]) -> Whoami` — what the UI offers.
-  `glob_semantics_mirror_a0006` function L110-119 — `()` — what the UI offers.
-  `org_admin_can_access_everything` function L122-126 — `()` — what the UI offers.
-  `plain_member_without_grants_is_denied` function L129-133 — `()` — what the UI offers.
-  `member_with_board_config_grant_can_access_but_is_not_admin` function L136-154 — `()` — what the UI offers.

#### crates/kairos-web/src/pages/admin/members.rs

- pub `AdminMembersPage` function L20-132 — `() -> impl IntoView` — `/admin/members`.

#### crates/kairos-web/src/pages/admin/metadata.rs

- pub `AdminMetadataPage` function L65-145 — `() -> impl IntoView` — `/admin/metadata`.
-  `FIELD_TYPES` variable L17 — `: [&str; 3]` — options for enums and forbids them otherwise — 422 `VALIDATION`).
-  `EnumOptionsEditor` function L21-61 — `(options: RwSignal<Vec<String>>) -> impl IntoView` — A reusable enum-option list editor over one `RwSignal<Vec<String>>`.
-  `DefinitionRow` function L150-239 — `( definition: api::MetadataDefinition, busy: RwSignal<bool>, outcome: RwSignal<M...` — One definition row with inline edit (name/slug, and the option list for

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

- pub `Board` struct L16-24 — `{ id: String, name: String, slug: String, board_level: String, team_id: Option<S...` — mirror of: `kairos_client::types_org::Board` (partial).
- pub `BoardColumn` struct L28-32 — `{ id: String, name: String, position: i32 }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `BoardTransition` struct L36-39 — `{ from_column_id: String, to_column_id: String }` — mirror of: `kairos_client::types_org::BoardTransition` (partial).
- pub `BoardDetail` struct L44-49 — `{ board: Board, columns: Vec<BoardColumn>, transitions: Vec<BoardTransition> }` — mirror of: `kairos_client::types_org::BoardDetail` (the board's own
- pub `BoardListEnvelope` struct L53-55 — `{ items: Vec<Board> }` — mirror of: `kairos_client::types::ListEnvelope<Board>` (partial).
- pub `Strategy` struct L61-64 — `{ short_code: String, title: String }` — mirror of: `kairos_client::types::Strategy` (partial — card fields).
- pub `Initiative` struct L68-77 — `{ short_code: String, title: String, complexity: Option<String>, is_bucket: bool...` — mirror of: `kairos_client::types::Initiative` (partial — card fields).
- pub `Task` struct L81-86 — `{ short_code: String, title: String, task_type: String }` — mirror of: `kairos_client::types::Task` (partial — card fields).
- pub `Adr` struct L90-95 — `{ short_code: String, title: String, decision_date: Option<String> }` — mirror of: `kairos_client::types::Adr` (partial — card fields).
- pub `BoardColumnItems` struct L99-105 — `{ column: BoardColumn, strategies: Vec<Strategy>, initiatives: Vec<Initiative>, ...` — mirror of: `kairos_client::types_org::BoardColumnItems`.
- pub `BoardItemsResponse` struct L109-112 — `{ board: Board, columns: Vec<BoardColumnItems> }` — mirror of: `kairos_client::types_org::BoardItemsResponse`.
- pub `Template` struct L118-121 — `{ id: String, name: String }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateListEnvelope` struct L125-127 — `{ items: Vec<Template> }` — mirror of: `kairos_client::types::ListEnvelope<Template>` (partial).
- pub `ThinEvent` struct L133-135 — `{ event: String }` — mirror of: `kairos_client::types_events::ThinEvent` (partial — the view
- pub `BoardView` struct L142-145 — `{ detail: BoardDetail, items: BoardItemsResponse }` — Everything the board view renders: configuration (columns +
- pub `list_boards` function L148-151 — `(auth: Auth) -> Result<Vec<Board>, ApiError>` — The board list (one page is plenty for v1 — the demo tenant has 5).
- pub `load_board_view` function L155-169 — `(auth: Auth, param: &str) -> Result<BoardView, ApiError>` — Resolve a route param (board slug, or id as a fallback) against the
- pub `list_templates` function L172-175 — `(auth: Auth) -> Result<Vec<Template>, ApiError>` — Templates for the document create flow.
- pub `EntityKind` enum L181-186 — `Strategy | Initiative | Task | Adr` — The four board-item entity kinds (documents are off-board, S-0005).
- pub `api_family` function L190-197 — `(self) -> &'static str` — The S-0005 URL family (`/api/{family}/{short_code}/…`).
- pub `label` function L200-207 — `(self) -> &'static str` — The card label.
- pub `for_board_level` function L211-219 — `(level: &str) -> Option<EntityKind>` — The entity type a board level's create flow produces
- pub `transition` function L230-239 — `( auth: Auth, kind: EntityKind, short_code: &str, to_column_id: &str, ) -> Resul...` — `POST /api/{family}/{short_code}/transition`.
- pub `NewItem` struct L244-259 — `{ title: String, content: String, hypothesis: Option<String>, complexity: Option...` — The create-from-column form data; [`create_item`] maps it onto the
- pub `create_item` function L310-379 — `( auth: Auth, kind: EntityKind, board_id: &str, column_id: &str, item: &NewItem,...` — `POST /api/{family}` — create a board item in the given column.
- pub `create_document` function L393-410 — `( auth: Auth, title: &str, template_id: Option<&str>, parent_short_code: &str, )...` — `POST /api/documents` — create a document attached to a board item.
-  `EntityKind` type L188-220 — `= EntityKind` — every mirror carries a `mirror of:` line and a decode test).
-  `TransitionRequest` struct L224-226 — `{ to_column_id: &'a str }` — mirror of: `kairos_client::types::TransitionRequest`.
-  `CreateStrategyRequest` struct L263-270 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, hypot...` — mirror of: `kairos_client::types::CreateStrategyRequest`.
-  `CreateInitiativeRequest` struct L274-281 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, compl...` — mirror of: `kairos_client::types::CreateInitiativeRequest`.
-  `CreateTaskRequest` struct L285-294 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, task_...` — mirror of: `kairos_client::types::CreateTaskRequest`.
-  `CreateAdrRequest` struct L298-307 — `{ board_id: &'a str, column_id: &'a str, title: &'a str, content: &'a str, decis...` — mirror of: `kairos_client::types::CreateAdrRequest`.
-  `CreateDocumentRequest` struct L383-390 — `{ title: &'a str, template_id: Option<&'a str>, parent_short_code: &'a str }` — mirror of: `kairos_client::types::CreateDocumentRequest`.
-  `tests` module L413-560 — `-` — every mirror carries a `mirror of:` line and a decode test).
-  `board_detail_mirror_decodes_server_shape` function L420-447 — `()` — The board-view mirrors decode a realistic
-  `board_items_mirror_decodes_server_shape` function L451-487 — `()` — The grouped-items mirror decodes all four entity types.
-  `template_and_event_mirrors_decode` function L491-507 — `()` — The template + event mirrors decode their wire shapes.
-  `create_requests_serialize_wire_shape` function L512-539 — `()` — Create requests serialize with the exact S-0005 field names and
-  `entity_kind_per_board_level` function L543-559 — `()` — Board level → create-flow entity kind (A-0002 one-family-per-level).

#### crates/kairos-web/src/pages/boards/live.rs

- pub `LiveBoardGuard` struct L63-65 — `{ state: Rc<Live> }` — Dropping this closes the socket and stops the reconnect chain.
- pub `subscribe_board_events` function L86-105 — `( auth: Auth, board_id: String, refetch: impl Fn() + 'static, ) -> LiveBoardGuar...` — Subscribe the board view to `/ws/events`, filtered to `board_id`.
-  `BACKOFF_INITIAL_MS` variable L33 — `: u64` — Reconnect backoff bounds (milliseconds).
-  `BACKOFF_MAX_MS` variable L34 — `: u64` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `Handler` type L37 — `= RefCell<Option<Closure<T>>>` — A stored wasm event-handler closure (present while a socket is live).
-  `Live` struct L41-59 — `{ auth: Auth, board_id: String, refetch: Box<dyn Fn()>, closed: Cell<bool>, atte...` — The per-connection state shared by the event handlers and the
-  `LiveBoardGuard` type L67-82 — `impl Drop for LiveBoardGuard` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `drop` function L68-81 — `(&mut self)` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `events_url` function L110-116 — `(token: &str) -> Option<String>` — The `ws(s)://…/ws/events?access_token=…` URL for the current origin.
-  `connect` function L119-187 — `(state: Rc<Live>)` — Open one connection attempt and register its handlers.
-  `schedule_reconnect` function L190-198 — `(state: Rc<Live>)` — Queue the next connection attempt with exponential backoff.
-  `tests` module L201-214 — `-` — backoff timer chain, and break the closure ↔ state `Rc` cycle.
-  `backoff_doubles_and_caps` function L205-213 — `()` — Backoff doubles from 1s and caps at 15s (host-testable math for

### crates/kairos-web/src/pages/item

> *Semantic summary to be generated by AI agent.*

#### crates/kairos-web/src/pages/item/api.rs

- pub `Family` enum L34-40 — `Strategy | Initiative | Task | Document | Adr` — The five entity families, resolved from a short code's type letter
- pub `of_short_code` function L46-63 — `(code: &str) -> Option<Family>` — Parse the family out of a short code.
- pub `api_family` function L66-74 — `(self) -> &'static str` — The plural family segment used by every `/api/{family}` route.
- pub `label` function L77-85 — `(self) -> &'static str` — Human label for the page header.
- pub `is_workflow` function L89-91 — `(self) -> bool` — Workflow items (strategy/initiative/task) can parent a document
- pub `ItemDetail` struct L102-129 — `{ short_code: String, title: String, content: String, version: i32, board_id: Op...` — mirror of: `kairos_client::types::{Strategy,Initiative,Task,Document,Adr}`
- pub `BoardInfo` struct L133-138 — `{ name: String, slug: String, columns: Vec<BoardColumnInfo> }` — mirror of: `kairos_client::types_org::BoardDetail` (partial).
- pub `BoardColumnInfo` struct L142-145 — `{ id: String, name: String }` — mirror of: `kairos_client::types_org::BoardColumn` (partial).
- pub `Page` struct L150-152 — `{ items: Vec<T> }` — mirror of: `kairos_client::types::ListEnvelope` (partial — the page
- pub `ItemMetadata` struct L156-158 — `{ values: Vec<MetadataValue> }` — mirror of: `kairos_client::types_meta::ItemMetadataResponse` (partial).
- pub `MetadataValue` struct L162-165 — `{ slug: String, value: String }` — mirror of: `kairos_client::types_meta::MetadataValue` (partial).
- pub `MetadataDefinition` struct L169-176 — `{ name: String, slug: String, field_type: String, enum_options: Vec<String> }` — mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
- pub `ItemRelationships` struct L181-186 — `{ outgoing: Vec<RelationshipGroup>, incoming: Vec<RelationshipGroup> }` — mirror of: `kairos_client::types_meta::ItemRelationshipsResponse`
- pub `RelationshipGroup` struct L190-193 — `{ relationship: String, items: Vec<RelatedItem> }` — mirror of: `kairos_client::types_meta::RelationshipGroup` (partial).
- pub `RelatedItem` struct L197-201 — `{ short_code: String, entity_type: String, title: String }` — mirror of: `kairos_client::types_meta::RelatedItem` (partial).
- pub `TemplateSummary` struct L205-208 — `{ id: String, name: String }` — mirror of: `kairos_client::types_meta::Template` (partial).
- pub `TemplateDetail` struct L212-218 — `{ id: String, name: String, content: String, metadata: Vec<TemplateField> }` — mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
- pub `TemplateField` struct L222-232 — `{ slug: String, name: String, field_type: String, enum_options: Vec<String>, def...` — mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
- pub `DeleteOutcome` struct L237-242 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — mirror of: `kairos_client::types::DeleteResponse` (the A-0001 soft
- pub `CascadePreview` struct L249-254 — `{ short_code: String, cascade_count: i64, cascaded_short_codes: Vec<String> }` — mirror of: `kairos_client::types::CascadePreviewResponse` (KAIROS-T-0051
- pub `CurrentVersion` struct L263-267 — `{ version: i32, title: String, content: String }` — The server-current entity carried by a 409 in `details.current`
- pub `SaveError` enum L295-301 — `Conflict | Api` — Outcome of a content save: a version conflict is not a dead end — it
- pub `fetch_item` function L308-310 — `(auth: Auth, family: Family, code: String) -> Result<ItemDetail, ApiError>` — `GET /api/{family}/{short_code}` → the entity, whichever family.
- pub `fetch_board` function L314-316 — `(auth: Auth, board_id: String) -> Result<BoardInfo, ApiError>` — `GET /api/boards/{id}` → board name/slug + columns (for the
- pub `fetch_definitions` function L320-324 — `(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError>` — `GET /api/metadata-definitions` (all of them — the typed editors render
- pub `fetch_metadata` function L327-338 — `( auth: Auth, family: Family, code: String, ) -> Result<Vec<MetadataValue>, ApiE...` — `GET /api/{family}/{short_code}/metadata` → the item's current values.
- pub `fetch_relationships` function L342-352 — `( auth: Auth, family: Family, code: String, ) -> Result<ItemRelationships, ApiEr...` — `GET /api/{family}/{short_code}/relationships` → both directions,
- pub `fetch_cascade_preview` function L358-368 — `( auth: Auth, family: Family, code: String, ) -> Result<CascadePreview, ApiError...` — `GET /api/{family}/{short_code}/cascade-preview` → the AUTHORITATIVE
- pub `fetch_templates` function L371-374 — `(auth: Auth) -> Result<Vec<TemplateSummary>, ApiError>` — `GET /api/templates` → the picker's list.
- pub `fetch_template_detail` function L377-379 — `(auth: Auth, id: String) -> Result<TemplateDetail, ApiError>` — `GET /api/templates/{id}` → content preview + declared metadata fields.
- pub `update_content` function L397-441 — `( auth: Auth, family: Family, code: &str, title: &str, content: &str, version: i...` — `PATCH /api/{family}/{short_code}` — the A-0004 optimistic-concurrency
- pub `update_metadata` function L445-459 — `( auth: Auth, family: Family, code: &str, values: BTreeMap<String, Option<String...` — `PATCH /api/{family}/{short_code}/metadata`: definition slug → value
- pub `CreateDocumentBody` struct L465-469 — `{ title: String, template_id: String, parent_short_code: String }` — Body of `POST /api/documents` (mirror of:
- pub `create_document` function L473-478 — `( auth: Auth, body: &CreateDocumentBody, ) -> Result<ItemDetail, ApiError>` — `POST /api/documents` — create-from-template, attached to a workflow
- pub `delete_item` function L482-489 — `( auth: Auth, family: Family, code: &str, ) -> Result<DeleteOutcome, ApiError>` — `DELETE /api/{family}/{short_code}` — A-0001 soft delete; the response
- pub `error_text` function L493-506 — `(error: &ApiError) -> String` — One-line text for a *write* failure (loads use `<ErrorState/>`; writes
-  `Family` type L42-92 — `= Family` — flattening the conflict into an `ApiError`.
-  `DetailedErrorEnvelope` struct L272-274 — `{ error: DetailedErrorBody }` — mirror of: `kairos_client::types::ErrorEnvelope` — with `details`, which
-  `DetailedErrorBody` struct L278-283 — `{ code: String, message: String, details: ErrorDetails }` — mirror of: `kairos_client::types::ErrorBody` (partial, + details).
-  `ErrorDetails` struct L287-290 — `{ current: Option<CurrentVersion> }` — The structured extras this page understands (`current` on 409).
-  `UpdateContentBody` struct L388-392 — `{ title: &'a str, content: &'a str, version: i32 }` — Body of the content PATCH (mirror of:
-  `Body` struct L452-454 — `{ values: BTreeMap<String, Option<String>> }` — flattening the conflict into an `ApiError`.
-  `Verb` enum L514-517 — `Patch | Delete` — The two verbs the shared `api.rs` does not provide yet.
-  `send` function L520-542 — `( auth: Auth, verb: Verb, path: &str, body: Option<&B>, ) -> Result<gloo_net::ht...` — Build + send one authenticated JSON request; no status handling yet.
-  `send_json` function L546-564 — `( auth: Auth, verb: Verb, path: &str, body: Option<&B>, ) -> Result<T, ApiError>` — One authenticated JSON round-trip with the standard status handling
-  `error_from` function L568-580 — `(status: u16, response: gloo_net::http::Response) -> ApiError` — Non-2xx → `ApiError` via the S-0005 envelope (the `api.rs` mapping,
-  `tests` module L583-706 — `-` — flattening the conflict into an `ApiError`.
-  `family_parses_from_short_codes` function L589-605 — `()` — Short-code → family across all five letters, multi-segment
-  `item_mirror_decodes_task_shape` function L609-630 — `()` — The union mirror decodes a full Task body (field-name lock).
-  `item_mirror_decodes_document_shape` function L634-650 — `()` — The union mirror decodes a Document body (no board fields at all).
-  `conflict_envelope_extracts_current` function L655-689 — `()` — The 409 envelope parse finds `details.current` whether it is the
-  `metadata_body_serializes_null_clears` function L694-705 — `()` — The metadata PATCH body serializes `None` as JSON null (the A-0003
-  `Body` struct L696-698 — `{ values: BTreeMap<String, Option<String>> }` — flattening the conflict into an `ApiError`.

#### crates/kairos-web/src/pages/item/create_doc.rs

- pub `CreateDocumentDialog` function L22-48 — `( /// Short code of the workflow item that will parent the document. #[prop(into...` — The "New document" dialog.
-  `TemplatePicker` function L53-76 — `(#[prop(into)] parent_code: String) -> impl IntoView` — Template list + preview + create form (own component so its resources
-  `TemplateForm` function L81-213 — `( templates: Vec<TemplateSummary>, #[prop(into)] parent_code: String, ) -> impl ...` — The picker itself: select a template, preview it, name the document,

#### crates/kairos-web/src/pages/item/delete.rs

- pub `DeleteDialog` function L19-43 — `( family: Family, #[prop(into)] code: String, #[prop(into)] title: String, open:...` — The delete confirm dialog.
-  `DeleteFlow` function L48-168 — `( family: Family, #[prop(into)] code: String, #[prop(into)] title: String, open:...` — Confirm → delete → cascade report (own component so the children

#### crates/kairos-web/src/pages/item/editor.rs

- pub `ContentEditor` function L30-250 — `( family: Family, #[prop(into)] code: String, #[prop(into)] initial_title: Strin...` — The editable content panel.

#### crates/kairos-web/src/pages/item/markdown.rs

- pub `to_html` function L20-34 — `(source: &str) -> String` — Render markdown to HTML, escaping raw HTML events (XSS-safe for
-  `tests` module L37-65 — `-` — tables, code fences, links — renders normally.
-  `renders_commonmark_structure` function L41-47 — `()` — tables, code fences, links — renders normally.
-  `renders_tables_and_task_lists` function L50-54 — `()` — tables, code fences, links — renders normally.
-  `escapes_raw_html` function L58-64 — `()` — Raw HTML (block and inline) is escaped, never emitted as markup.

#### crates/kairos-web/src/pages/item/metadata.rs

- pub `MetadataPanel` function L34-70 — `(family: Family, #[prop(into)] code: String) -> impl IntoView` — The metadata panel: definitions + values fetched together, typed
-  `FieldRow` struct L25-29 — `{ definition: MetadataDefinition, draft: RwSignal<String>, original: String }` — One field's editing state: its definition, the live draft, and the
-  `ADD_PLACEHOLDER` variable L73 — `: &str` — The "add a field" picker's no-choice option.
-  `MetadataForm` function L79-209 — `( family: Family, #[prop(into)] code: String, definitions: Vec<MetadataDefinitio...` — The editors + save button, built fresh per fetch (drafts start at the
-  `FieldEditor` function L213-276 — `(row: FieldRow) -> impl IntoView` — One typed editor row: label + the editor its `field_type` calls for.

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
- pub `group` function L226-237 — `(&self, relationship: &str, outgoing: bool) -> Vec<RelatedItem>` — The neighbors of one relationship type in one direction.
- pub `RelationshipGroup` struct L242-245 — `{ relationship: String, items: Vec<RelatedItem> }` — mirror of: `kairos_client::types_meta::RelationshipGroup`.
- pub `RelatedItem` struct L250-255 — `{ relationship_id: String, short_code: String, entity_type: String, title: Strin...` — mirror of: `kairos_client::types_meta::RelatedItem` (partial — `id` is
- pub `CreateRelationship` struct L259-263 — `{ source_short_code: String, target_short_code: String, relationship: String }` — mirror of: `kairos_client::types_meta::CreateRelationshipRequest`.
- pub `CreatedRelationship` struct L267-269 — `{ id: String }` — mirror of: `kairos_client::types_meta::Relationship` (partial).
- pub `DeletedRelationship` struct L273-275 — `{ id: String }` — mirror of: `kairos_client::types_meta::DeletedResponse` (partial).
- pub `relationships` function L278-281 — `(auth: Auth, short_code: &str) -> Result<ItemRelationships, ApiError>` — `GET /api/{family}/{short_code}/relationships`.
- pub `create_relationship` function L285-290 — `( auth: Auth, request: &CreateRelationship, ) -> Result<CreatedRelationship, Api...` — `POST /api/relationships` (org admin; the typed 422s — `RELATIONSHIP_RULE`,
- pub `delete_relationship` function L293-298 — `( auth: Auth, relationship_id: &str, ) -> Result<DeletedRelationship, ApiError>` — `DELETE /api/relationships/{id}` (org admin).
- pub `ItemSummary` struct L307-310 — `{ short_code: String, title: String }` — mirror of: `kairos_client::types::{Strategy, Initiative, Task, Document,
- pub `item_summary` function L313-316 — `(auth: Auth, short_code: &str) -> Result<ItemSummary, ApiError>` — `GET /api/{family}/{short_code}` — just the header fields.
- pub `Crumb` struct L320-324 — `{ short_code: String, entity_type: String, title: String }` — One breadcrumb ancestor.
- pub `parent_chain` function L330-353 — `(auth: Auth, short_code: &str) -> Result<Vec<Crumb>, ApiError>` — Walk `parent` edges upward (each item's parent is the source of its
-  `family_or_err` function L41-44 — `(short_code: &str) -> Result<&'static str, ApiError>` — [`family_of`] as an [`ApiError`] for fetchers that need a family.
-  `SearchFilter` type L88-95 — `= SearchFilter` — run on the host (`cargo test -p kairos-web`).
-  `ItemRelationships` type L224-238 — `= ItemRelationships` — run on the host (`cargo test -p kairos-web`).
-  `tests` module L356-501 — `-` — run on the host (`cargo test -p kairos-web`).
-  `family_of_maps_type_letters` function L361-370 — `()` — Type letters map to the S-0004 families; junk maps to none.
-  `search_request_serializes_s0005_field_names` function L375-427 — `()` — The request mirror serializes the exact S-0005 field names (the
-  `search_response_mirror_decodes_server_shape` function L432-470 — `()` — The response mirror decodes a realistic grouped body — typed extra
-  `relationships_mirror_decodes_server_shape` function L475-500 — `()` — The relationships mirror decodes the T-0020 grouped shape, and

#### crates/kairos-web/src/pages/search/relationships.rs

- pub `RelationshipsPage` function L29-41 — `() -> impl IntoView` — `/search/relationships/:code` — route wrapper; re-mounts the view when
- pub `RelationshipsView` function L47-350 — `(#[prop(into)] short_code: String) -> impl IntoView` — One item's relationships, explorable.

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
- pub `team_type_color` function L99-108 — `(team_type: &str) -> &'static str` — The accent token for a team type pill (shared by directory, detail,
-  `PAGE` variable L30 — `: &str` — Big-enough page for org-scale lists (server clamps to its own max).
-  `tests` module L111-153 — `-` — a `mirror of:` line so drift stays greppable.
-  `team_mirror_decodes_server_shape` function L117-127 — `()` — `Team` decodes the wire shape (field-name lock; same body the admin
-  `team_member_mirror_decodes_server_shape` function L131-140 — `()` — `TeamMember` decodes the roster row.
-  `board_ref_mirror_decodes_server_shape` function L144-152 — `()` — `BoardRef` decodes a board list element (link resolution only).

### e2e/helpers

> *Semantic summary to be generated by AI agent.*

#### e2e/helpers/api.ts

- pub `BoardSnapshot` interface L15-20 — `{ boardId: : string, columnName: : Map<string, string>, transitions: : { from: s...`
- pub `loadPlatformDelivery` function L23-47 — `function loadPlatformDelivery( server: string, token: string, ): Promise<BoardSn...`
- pub `MovePick` interface L49-53 — `{ code: : string, toColumnId: : string, toColumnName: : string }`
- pub `pickMovableTask` function L61-84 — `function pickMovableTask( server: string, token: string, exclude: string[] = [],...`
- pub `transitionTask` function L87-101 — `function transitionTask( server: string, token: string, code: string, toColumnId...`
- pub `TaskState` interface L103-107 — `{ version: : number, title: : string, content: : string }`
- pub `getTask` function L110-117 — `function getTask( server: string, token: string, code: string, ): Promise<TaskSt...`
- pub `patchTask` function L123-138 — `function patchTask( server: string, token: string, code: string, body: { title: ...`
-  `bearer` function L7 — `const bearer = (token: string)`
-  `json` function L9-13 — `function json(server: string, token: string, path: string): Promise<any>`

#### e2e/helpers/auth.ts

- pub `MintOptions` interface L18-24 — `{ issuer: : string, server: : string, clientId: : string, email: : string, passw...`
- pub `mintToken` function L27-124 — `function mintToken(opts: MintOptions = {}): Promise<string>`
-  `b64url` function L15-16 — `const b64url = (b: Buffer)`
-  `remember` function L40-47 — `const remember = (res: Response)`
-  `cookieHeader` function L48-49 — `const cookieHeader = ()`
-  `follow` function L50-58 — `const follow = (url: string, init: RequestInit = {})`

### e2e/tests

> *Semantic summary to be generated by AI agent.*

#### e2e/tests/drag.spec.ts

-  `column` function L20-23 — `const column = (page: Page, name: string): Locator`
-  `cardIn` function L25-26 — `const cardIn = (page: Page, columnName: string): Locator`

#### e2e/tests/smoke.spec.ts

-  `column` function L34-37 — `const column = (page: Page, name: string): Locator`
-  `cardIn` function L39-40 — `const cardIn = (page: Page, columnName: string, needle: string): Locator`

#### e2e/tests/team-lens.spec.ts

-  `navbar` function L23 — `const navbar = (page: Page)`

### plugin/hooks

> *Semantic summary to be generated by AI agent.*

#### plugin/hooks/session_start.py

- pub `read_frontmatter` function L34-51 — `def read_frontmatter(path)` — Parse the YAML frontmatter's simple `key: value` pairs (no external
- pub `probe` function L54-65 — `def probe(url)` — Unauthenticated reachability check; returns a short status note.
- pub `main` function L68-103 — `def main()`

