# Connect over MCP

Get an agent's MCP client talking to a Kairos deployment: one streamable-HTTP session
against `/mcp`, authenticated, resolved to the right tenant, with the eighteen
tools available.

**Before you start:** you need a bearer token for a principal that is a **member
of the tenant**. For an unattended client that means an API key — see
[Give an agent machine access](give-an-agent-machine-access.md). A human's OIDC
access token works identically for interactive use.

If your client speaks MCP streamable HTTP already, point it at
`https://<host>/mcp` with that bearer and skip to
[Tenant resolution](#tenant-resolution). The rest of this page is for wiring a
client by hand.

## The handshake

Three requests. All are `POST /mcp`; the transport is JSON-RPC over HTTP with
SSE-framed responses.

Send these headers on every request:

```text
Authorization: Bearer <token>
Accept: application/json, text/event-stream
Content-Type: application/json
```

Add `X-Tenant: <slug>` to that set now if this deployment resolves tenants by
header — check [Tenant resolution](#tenant-resolution) before you send
`initialize`, because getting it wrong fails at the first request rather than at
the first tool call.

### 1. `initialize`

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{
  "protocolVersion":"2025-06-18",
  "capabilities":{},
  "clientInfo":{"name":"my-agent","version":"1.0.0"}}}
```

The 200 response carries an **`Mcp-Session-Id` header**. Keep it and send it on
every subsequent request; without it the server has no session. The result's
`serverInfo.name` is `kairos` and `serverInfo.version` is the deployment's
version — a useful assertion that you are talking to what you think you are.

Responses may arrive as plain JSON **or** as SSE framing (`data:` lines, with
priming events that carry no JSON-RPC payload). Parse both; do not assume one.

### 2. `notifications/initialized`

```json
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

This answers **202**, not 200, and has no body. A client that insists on 200
will stall here.

### 3. `tools/call`

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
  "name":"whoami","arguments":{}}}
```

Tool output is **text**, in `result.content[]` entries of type `text` — short
codes, `key: value` lines, compact listings. It is not JSON to deserialize.
`whoami` returning your principal is the check that the whole chain works.

`tools/list` returns exactly eighteen tools, frozen by name and shape. Each one's
arguments, defaults and refusals are in [MCP tools](../reference/mcp-tools.md).

`uat/surfaces/mcp.ts` in the repository is a complete working client in about a
hundred lines — session handling, SSE parsing and tool-error handling included.
Copy from it rather than from this page if you are writing the transport
yourself.

## Tenant resolution

**There is no tenant argument on any tool.** The tenant comes from the
connection, and which mechanism applies depends on how the deployment was
configured ([Configuration → Tenant
resolution](../reference/configuration.md#tenant-resolution)):

- **An API key carries its own tenant.** Send nothing; it resolves itself. This
  is the case for most agents, and the reason a key is simpler than a token.
- **Subdomain tenancy** (`KAIROS_BASE_DOMAIN`): connect to
  `https://acme.<base-domain>/mcp` and the `Host` header resolves the tenant.
- **Single-tenant** (`KAIROS_SINGLE_TENANT`): the tenant is pinned and both
  subdomain and header resolution are skipped. Send nothing; an `X-Tenant`
  header is ignored.
- **Neither variable set**: send `X-Tenant: acme` alongside the bearer. This is
  the only case where the header does anything.

## Errors the client must handle

Two levels, and they are easy to conflate:

**Transport and auth errors** are HTTP statuses on the POST:

| Status | Meaning |
|---|---|
| `401` | No token, or an invalid one. The response carries an RFC 9728 `WWW-Authenticate` challenge whose `resource_metadata=` points at `/.well-known/oauth-protected-resource/mcp`, which an OAuth-capable client can follow to discover the issuer. |
| `403` | Authenticated, but not a member of the resolved tenant — `MEMBERSHIP_REQUIRED`. Authenticating is not the same as belonging; add the principal to the organization. |

`/mcp` sits behind the same auth and tenant middleware as `/api`, so these are
the same failures with the same causes.

**Tool refusals** come back as `200` with `result.isError` set and the reason in
the text content. Treat those as data, not as transport failures — the text
carries the same stable codes the REST API uses (`FORBIDDEN` naming the missing
capability, `INVALID_TRANSITION` enumerating the allowed columns,
`REPOSITORY_OWNER_MISMATCH`, `RESTORE_BLOCKED`, and the rest). An agent that
retries a `FORBIDDEN` forever is the failure mode to design against; the
refusals are listed in [MCP tools → Refusal
codes](../reference/mcp-tools.md#refusal-codes) and
[Errors](../reference/errors.md).

Expect a live agent to meet `FORBIDDEN` on work it can see: a capability lost
to a reorg leaves the queue query working and every write refused
([Move work between boards](move-work-between-boards.md#agents-scoped-to-the-repository)).

## Open each session with `whoami`

Call `whoami` and `my_boards` at the start of every session, and do not cache
the result across runs — capabilities change underneath a long-lived agent. Then
scope the work with `list_repositories` and
`board_items {board, repository}`, which is the queue for one checkout
([Repositories as execution
scope](../explanation/repositories-as-execution-scope.md#the-agents-frame-is-the-checkout)).

## Related

- [MCP tools](../reference/mcp-tools.md) — all eighteen, with arguments and
  refusals
- [Give an agent machine access](give-an-agent-machine-access.md)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Capabilities](../reference/capabilities.md)
