---
id: this-repository-is-wired-to-its
level: task
title: "This repository is wired to its own tailnet deployment"
short_code: "KAIROS-T-0210"
created_at: 2026-09-26T22:37:17.013549+00:00
updated_at: 2026-09-26T22:42:59.798271+00:00
parent: 
blocked_by: [KAIROS-T-0209]
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# This repository is wired to its own tailnet deployment

## Objective

Kairos runs its own work: the `colliery-io/kairos` checkout is bootstrapped
against the deployment [[KAIROS-T-0209]] put on the tailnet, the `kairos`
Claude Code plugin is installed from this repository's marketplace, and the
tenant has the team, repository and agent principal the plugin expects.

Done by hand through the REST API rather than `/kairos:bootstrap`, because
two of the bootstrap's assumptions do not hold here: the CLI logs in only by
OAuth device grant, and the deployment has no issuer (local accounts only), so
neither `kairos login` nor the MCP client's OAuth flow can run. The agent
principal is therefore a service account with an API key, exactly as
"Give an agent machine access" prescribes.

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification
- **User Value**: Dogfood. Every kairos skill now has a live board to work against from this checkout.
- **Business Value**: First real tenant; surfaced that the bootstrap skill and CLI login have no local-accounts path (filed below as follow-ups, not fixed here).
- **Effort Estimate**: S

## Acceptance Criteria

- [x] Tenant `colliery` has team `colliery-io` (delivery board `colliery-io-delivery`), with Dylan and the `claude-code` service account as members. Dylan first asked for `engineering`, then chose `colliery-io`; the first team and its repository were deleted and recreated.
- [x] Repository `kairos` registered: github `colliery-io/kairos`, owned by `colliery-io`, with a "how to work here" description.
- [x] Service account `claude-code` holds `manage_*` + `transition_items` on `colliery-io-delivery`, `manage_initiatives` + `manage_documents` + `transition_items` on `initiatives`, `manage_adrs` + `manage_documents` + `transition_items` on `adrs`, nothing on `strategy`. Key `dstorey-personal-mac`, no expiry. Verified: `/api/whoami` names the account; `POST /mcp initialize` answers 200 with an `Mcp-Session-Id` and `serverInfo.version` 0.4.0.
- [x] `.mcp.json` at the repo root points at `https://kairos.<tailnet>.ts.net/mcp` with `Authorization: Bearer ${KAIROS_MCP_KEY}` — the key itself never enters the repo.
- [x] `.claude/kairos.local.md` written with the six frontmatter keys; gitignored already.
- [x] Marketplace `colliery-io/kairos` added from GitHub and `kairos@kairos` 0.1.0 installed at user scope.
- [x] `KAIROS_MCP_KEY` set in the user's Claude Code settings `env`. The auto-mode classifier refused the first attempt; Dylan approved the write out of auto mode.
- [x] Tailscale client on this Mac: 1.102.4 installed and signed in as node `dstorey-personal`. From this Mac, `https://kairos.<tailnet>.ts.net/healthz` is 200 and an MCP `initialize` with the configured key succeeds.

## Implementation Notes

### Follow-ups worth their own tasks (not created here)
- The bootstrap skill assumes OAuth; on a local-accounts deployment it should offer the service-account path and say the CLI cannot log in.
- `kairos login` has no password grant; a local-accounts deployment has no CLI.

## Status Updates

- 2026-09-26: all API work verified over the tailnet from skadi's node (this Mac had no Tailscale client). Plugin installed. Two items left for Dylan: the key into settings, and the Tailscale sign-in.