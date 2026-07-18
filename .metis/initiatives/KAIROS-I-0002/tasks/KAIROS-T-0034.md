---
id: m3-bootstrap-skill-and
level: task
title: "M3: Bootstrap skill and SessionStart hook"
short_code: "KAIROS-T-0034"
created_at: 2026-07-10T09:09:19.450667+00:00
updated_at: 2026-07-11T01:18:50.414080+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0026, KAIROS-T-0027]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Bootstrap skill and SessionStart hook

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

/bootstrap and the SessionStart hook per KAIROS-A-0014: wire a repo/engineer to a Kairos deployment; inject board context each session.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] /bootstrap: prompts for deployment URL (and optional tenant slug for X-Tenant/dev setups); writes plugin .mcp.json config or guides `claude mcp add` for the /mcp endpoint; connects and calls whoami/my_boards to confirm auth + discover team/boards; writes `.claude/kairos.local.md` (YAML frontmatter: deployment URL, tenant, delivery stream, team board, default initiative board) and gitignores it; idempotent re-run updates in place
- [x] SessionStart hook (plugin/hooks/): reads kairos.local.md, calls my_boards/board_items for the engineer's delivery board, injects a compact summary (active/todo items, standing buckets); degrades gracefully offline (note, not failure); hook registered in plugin manifest per plugin-dev hook conventions
- [x] Verified against a live compose Kairos + Dex: bootstrap flow end-to-end, hook injects real context (evidence recorded)

## Implementation Notes

References A-0014 (decided; kairos.local.md contract), A-0011 (OAuth flow is client-driven). This task DOES need the live MCP endpoint — hence blocked by T-0026.

## Verification Gate (KAIROS-A-0012)

Plugin-validator and skill-reviewer passes recorded · every acceptance criterion demonstrated with evidence in Status Updates · router/README sync rule honored (A-0014) · full behavioral verification against a live Kairos happens in KAIROS-T-0035.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Read A-0014/A-0011/S-0006, plugin README/.mcp.json template, router skill, T-0026 mcp.rs transport, plugin-dev hook-development docs. Plan: (1) plugin/skills/meta/bootstrap/SKILL.md (user-invoked); (2) plugin/hooks/hooks.json + session_start.py (python3 stdlib); (3) plugin.json gains bootstrap skill entry + "hooks" pointer; (4) README "Still to land" + /kairos router entry. Hook-token design decision (v1): hooks have no OAuth broker (A-0014: tokens live with the MCP client), so the SessionStart hook injects the kairos.local.md context summary + an unauthenticated /healthz reachability probe as additionalContext and INSTRUCTS the agent to call my_boards/board_items over the authenticated MCP connection for live board state; offline degrades to a note. Live verification: scratch DB kairos_t0034_bootstrap, server on 127.0.0.1:8080 (KAIROS_BASE_DOMAIN=kairos.test, KAIROS_DEPLOYMENT_ADMINS=alice's sub), admin API create-tenant (seeds alice as initial org admin), RFC 9728 metadata curl, raw streamable-HTTP MCP session (initialize -> whoami/my_boards/board_items), sample kairos.local.md, hook run captured.
- 2026-07-10: DONE — all ACs demonstrated. Shipped: `plugin/skills/meta/bootstrap/SKILL.md` (user-invoked; URL + optional X-Tenant prompt with re-run recovery from existing .mcp.json/kairos.local.md; both config paths documented — project .mcp.json entry and `claude mcp add --transport http kairos <url>/mcp` [+ `--header "X-Tenant: <slug>"`]; healthz + RFC 9728 pre-write reachability gate; whoami/my_boards discovery; exact kairos.local.md frontmatter contract deployment_url/tenant/delivery_stream/team_board/initiative_board with empty-key rule; gitignore append; degraded auth/boardless path still writes config and notes what unblocks); `plugin/hooks/hooks.json` (plugin wrapper format, SessionStart, matcher *, timeout 10) + `plugin/hooks/session_start.py` (executable, python3 stdlib); `.claude-plugin/plugin.json` gained `"hooks": "./plugin/hooks/hooks.json"` + bootstrap skill entry; README "Still to land" replaced with shipped listing + hook design section; `/kairos` router gained the bootstrap entry (sync rule honored).
- 2026-07-10: DESIGN DECISION (hook auth, v1): hooks have no OAuth token broker and cannot drive the client's browser flow (A-0011); tokens live with the MCP client (A-0014). The SessionStart hook therefore holds NO credentials: it reads kairos.local.md (absent → silent exit 0), probes `<deployment_url>/healthz` unauthenticated (3s timeout), injects the wiring summary as `hookSpecificOutput.additionalContext`, and instructs the agent to call `my_boards`/`board_items` over the authenticated MCP connection for live active/todo items. Offline → note, not failure. Documented in hooks.json description, script docstring, and plugin/README.md.
- 2026-07-10: LIVE VERIFICATION EVIDENCE (scratch DB kairos_t0034_bootstrap on shared compose Postgres + live Dex; server `target/debug/kairos-server serve` on 127.0.0.1:8080 with OIDC_ISSUER_URL=http://localhost:5558/dex, OIDC_AUDIENCE=kairos-cli, KAIROS_BASE_DOMAIN=kairos.test, KAIROS_DEPLOYMENT_ADMINS=alice's sub; public migration 20260709000000 applied at boot): (1) real alice token via Dex password grant (kairos-cli client); (2) GET /healthz → ok; GET /.well-known/oauth-protected-resource/mcp → {"authorization_servers":["http://localhost:5558/dex"],"resource":"http://127.0.0.1:8080/mcp",...}; (3) admin API: GET /api/admin/tenants JIT-provisioned alice, POST create tenant t0034 → 3 tenant migrations, boards strategy/initiatives/adrs, alice initial admin; REST whoami confirmed; team `platform` created (auto delivery board platform-delivery), alice added, stream linked, task T0034-T-0001 created; (4) raw streamable-HTTP MCP session (T-0026 transport, X-Tenant: t0034): initialize → HTTP 200, Mcp-Session-Id issued, serverInfo {"name":"kairos","version":"0.1.0"}; notifications/initialized → 202; tools/call whoami → "# alice <alice@kairos.test> … platform — Platform (stream_aligned)"; my_boards → "platform-delivery — Platform Delivery [mine], columns: Backlog (1) | Todo (0) | Blocked (0) | Active (0) | Completed (0)"; board_items(platform-delivery) → "## Backlog (1) - T0034-T-0001 [task] Verify bootstrap flow"; (5) sample kairos.local.md written exactly per the skill contract + gitignore line; hook run against LIVE server (CLAUDE_PROJECT_DIR set, hook JSON on stdin) → exit 0, additionalContext with all five keys, "status: deployment reachable", and the live-state instruction naming platform-delivery; (6) hook degraded paths: no kairos.local.md → silent exit 0; unreachable URL → exit 0 with offline note. Claude-Code-interactive parts (`claude mcp add`, browser OAuth) documented, not executed. Cleanup: server stopped, scratch DB dropped; shared kairos DB/services untouched.
- 2026-07-10: REVIEWS: `claude plugin validate .` → "Validation passed" exit 0 (before and after review fixes). plugin-dev:skill-reviewer on bootstrap SKILL.md → PASS (0 blocking); both should-fixes applied (re-run config recovery in step 1; idempotency rule collapsed to one site) plus 3 nits (placeholder cue, step 2/4 phrasing, .mcp.json-vs-mcp-add proposal phrasing). plugin-dev:plugin-validator → PASS, 0 critical/0 warnings (informational: plugin/.mcp.json template intentionally unwired — already documented in README). Verification gate satisfied; behavioral end-to-end via Claude Code itself remains KAIROS-T-0035.