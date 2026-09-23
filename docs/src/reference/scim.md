# `/scim/v2` — inbound SCIM 2.0 provisioning

> **Why this page exists**: the Kairos REST surface is specified by
> OpenAPI (`GET /api/openapi.json`), but the SCIM endpoint speaks the
> RFC 7643/7644 wire protocol to identity providers, not the S-0005
> envelope, so it is documented here (like the WebSocket channel).
> Contract: KAIROS-A-0016; implementation: KAIROS-T-0025
> (`crates/kairos-server/src/scim/`).

## Scope

`/scim/v2` accepts inbound SCIM 2.0 for user and group lifecycle. It is
additive: JIT-on-first-login and `/api/members` add-by-email remain available
whether or not SCIM is configured.

To connect an IdP, see
[Provision users with SCIM](../how-to/provision-users-with-scim.md).

## Tokens

| Endpoint | Behaviour |
|---|---|
| `POST /api/scim-tokens` | Org-admin, normal API auth. Returns the bearer token **once**; only its SHA-256 is stored. |
| `GET /api/scim-tokens` | Metadata only; never secrets. |
| `DELETE /api/scim-tokens/{id}` | Revokes. |

### Token format and tenant resolution

Tokens look like `kairos_scim_<org-slug>_<64-hex-secret>`. The tenant is
resolved **from the token** (A-0016: "tenant-scoped by the token, not by
subdomain") — SCIM requests need no tenant subdomain or `X-Tenant`
header. The split is unambiguous: the secret is exactly 64 lowercase-hex
characters (hex never contains `_`), so the last `_` separates slug from
secret even for slugs containing underscores. Every auth failure
(malformed, unknown tenant, wrong secret, revoked) returns the same SCIM
401 envelope.

## Identity join: the mapping contract

Inbound SCIM Users bind to `public.users` in this order:

| Priority | SCIM attribute | Kairos column |
|---|---|---|
| 1 | `externalId` | `users.external_id` (the OIDC `sub`) |
| 2 | `userName` | `users.external_id` |
| 3 | first `emails[].value` (primary preferred; or an email-shaped `userName`) | `users.email` |
| 4 | *no match* | new row created with `external_id = externalId ?? userName` |

SCIM never overwrites the login join key, so a user who has already logged in
keeps the `sub` on their row and the email fallback links them. A
SCIM-*created* user whose stored `external_id` is not the `sub` their later
login presents is JIT-provisioned as a **second** user row, without the
membership.

Outbound, both `userName` and `externalId` are served from
`users.external_id`.

Configuring an IdP to satisfy this is
[Make the identity join work](../how-to/provision-users-with-scim.md#make-the-identity-join-work).

## Resource model

A tenant's SCIM `Users` **are its org memberships**: a resource exists
iff an `organization_members` row does; `id` is the stable
`public.users.id` UUID. `POST` provisions (link-or-create user + create
membership, role `member`); `PATCH {"active": false}` and `DELETE`
revoke the membership immediately while **retaining the `users` row**
(audit integrity). A deprovisioned user then GETs 404; re-activation is
a fresh POST (no "inactive membership" state is kept).

**Deprovision vs. live tokens (A-0010)**: access tokens validate locally
until their TTL, but org membership is checked per request — a
deprovisioned user's still-valid OIDC token receives 403
`MEMBERSHIP_REQUIRED` on the next `/api` call.

### Groups

| Group `displayName` | `id` | Maps to |
|---|---|---|
| `kairos-admins` | the organization UUID | `organization_members.role`: add = promote to `admin`, remove = demote to `member` |
| `kairos-team-<slug>` | the team UUID | `team_members`; `POST` creates the team + its delivery board, `DELETE` soft-deletes both |

Group members must already be provisioned Users (push Users before group
memberships — IdPs do this naturally). Demoting/deactivating/deleting
the **last admin** is refused with 400 `scimType: "mutability"`
(`LAST_ADMIN`). Group renames are refused the same way; rename teams via
`/api/teams`.

## Supported subset (RFC 7643/7644)

| Feature | Support |
|---|---|
| Discovery | `ServiceProviderConfig`, `Schemas`, `ResourceTypes` |
| Users | POST, GET (id/list), PATCH, PUT, DELETE |
| Groups | POST, GET (id/list), PATCH, PUT, DELETE |
| Filtering | `userName eq "…"`, `externalId eq "…"` (Users); `displayName eq "…"` (Groups) — anything else 400 `invalidFilter` |
| Pagination | `startIndex` (1-based) / `count` (≤ 200) |
| PATCH paths (Users) | `active` (bool or `"True"`/`"False"` strings), `displayName`, or a no-path value object (unknown attributes there are ignored) |
| PATCH paths (Groups) | `members` add/remove/replace incl. `members[value eq "…"]` |
| Bulk / sorting / ETags / `/Me` / password | not supported (advertised as such) |

Errors are always the RFC 7644 §3.12 envelope
(`urn:ietf:params:scim:api:messages:2.0:Error`) with `status`, `detail`,
and a `scimType` for 400/409-class rejections; responses are
`application/scim+json` (requests may use it or `application/json`).

Every lifecycle mutation writes a tenant `activity_log` row attributed
to the org admin who created the SCIM token (`details` prefixed
`scim token:<name>`).
