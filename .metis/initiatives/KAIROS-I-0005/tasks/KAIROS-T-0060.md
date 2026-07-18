---
id: service-account-api-keys-cli
level: task
title: "Service-account API keys: CLI commands"
short_code: "KAIROS-T-0060"
created_at: 2026-07-17T22:31:37.363535+00:00
updated_at: 2026-07-18T03:50:19.884677+00:00
parent: KAIROS-I-0005
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0005
---

# Service-account API keys: CLI commands

## Parent Initiative

[[KAIROS-I-0005]] — implements [[KAIROS-A-0017]].

## Objective **[REQUIRED]**

CLI ergonomics for managing service accounts and keys, mirroring
`commands/members.rs`: `kairos service-accounts create|list|delete` and
`kairos keys create|list|revoke`, as thin veneers over new `kairos-client`
methods. Depends on [[KAIROS-T-0059]].

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] `kairos-client` gains typed methods for the T-0059 endpoints
      (create/list/delete service account; mint/list/revoke key) + response
      types, following the existing client pattern.
- [ ] New `commands/service_accounts.rs` (`ServiceAccountsCommand`) and
      `commands/keys.rs` (`KeysCommand`) subcommand enums, registered in
      `commands/mod.rs` + `main.rs` (`Command` variants + dispatch), following
      `members.rs`.
- [ ] `kairos service-accounts create --name … [--capability board:cap …]`
      prints the created account; `list`/`delete` behave like members.
- [ ] `kairos keys create --service-account <id> [--expires …]` prints the RAW
      key once with a clear "store this now, it won't be shown again" notice;
      `list` shows prefixes only; `revoke <key-id>` revokes.
- [ ] `cargo test -p kairos-cli` clap-parse tests cover the new command groups;
      full gate green (the live CLI suite exercises at least list/create paths).

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Follow `crates/kairos-cli/src/commands/members.rs` exactly (enum + `Common`
  flatten + `client(&common)?` + `impl … async fn run`).
- Print the raw key to stdout distinctly (and only once), matching how sensitive
  one-time values should be surfaced; never cache it in credentials.json.

### Dependencies
- [[KAIROS-T-0059]] (the endpoints + client methods it exposes).

### Risk Considerations
- Don't persist the raw key anywhere; it's for the operator to copy.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Complete; full gate green

- `kairos-client`: `types_service_accounts` (request/response types) + 6 methods
  (`create/list/delete_service_account`, `create/list/revoke_api_key`) over the
  existing get/post_created/delete helpers.
- CLI: `commands/service_accounts.rs` (`ServiceAccountsCommand`:
  create/list/delete) and `commands/keys.rs` (`KeysCommand`:
  create/list/revoke), registered in `commands/mod.rs` + `main.rs`
  (variants + dispatch). Human tables + `--json`; `delete`/`revoke` require
  `--confirm`. `keys create` prints the raw key once with a "store it now"
  notice and never caches it.
- Tests: clap-parse coverage for all new subcommands in `command_tree_parses`.

**Gate:** fmt/clippy(client+cli all-targets)/web-lint clean; unit all crates
green (cli 23); **integration all targets pass** (incl. live cli suite).

Unblocks [[KAIROS-T-0061]] (docs can reference the real CLI surface).