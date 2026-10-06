# `/scim/v2` — inbound SCIM 2.0 provisioning

This page describes Kairos 0.6.0.

OpenAPI (`GET /api/openapi.json`) specifies the rest of the HTTP
surface. But this endpoint speaks the RFC 7643/7644 wire protocol to identity
providers rather than the `/api` envelope. Thus this page describes it.
Implementation: `crates/kairos-server/src/scim/`.

## Scope

`/scim/v2` accepts inbound SCIM 2.0 for user and group lifecycle. It is
additive: JIT-on-first-login and `/api/members` add-by-email remain available
whether or not you configure SCIM.

To connect an issuer, see
[Provision users with SCIM](../how-to/provision-users-with-scim.md).

## Tokens

| Endpoint | Behaviour |
|---|---|
| `POST /api/scim-tokens` | Returns the bearer token **once**; only the SHA-256 of the whole token string is stored. |
| `GET /api/scim-tokens` | Metadata only; never secrets. |
| `DELETE /api/scim-tokens/{id}` | Revokes. 404 for an unknown id, **409** for one already revoked. |

All three are org-admin only. Unlike `/scim/v2`, they are ordinary `/api`
calls. Thus they need the tenant resolved the ordinary way: a host
subdomain, `X-Tenant`, or a single-tenant deployment.

These three are `/api` endpoints.
[Machine access](rest/machine-access.md#scim-tokens) specifies them with the
rest of that surface — request and response schemas, and every status. The
table above states only what is particular to them.

### Token format and tenant resolution

Tokens look like `kairos_scim_<org-slug>_<64-hex-secret>`. Kairos resolves
the tenant **from the token** (A-0016: "tenant-scoped by the token, not by
subdomain"). SCIM requests need no tenant subdomain or `X-Tenant`
header. The split is unambiguous. The secret is exactly 64 lowercase-hex
characters (hex never contains `_`). Thus the last `_` separates slug from
secret even for slugs containing underscores.

Every auth failure
(malformed, unknown tenant, wrong secret, revoked) returns the same SCIM
401 envelope.

## Identity join: the mapping contract

Inbound SCIM Users bind to `public.users` in this order:

| Priority | SCIM attribute | Kairos column |
|---|---|---|
| 1 | `externalId` | `users.external_id` (the OIDC `sub`) |
| 2 | `userName` | `users.user_name` |
| 3 | first `emails[].value` (primary preferred; or an email-shaped `userName`) | `users.email` |
| 4 | *no match* | new row created with `external_id = externalId ?? userName`, `user_name = userName` |

`userName` and `externalId` are **two columns**, because they are two things.
`externalId` is the OIDC subject logins join on. `userName` is the login
identifier the issuer uses, commonly an email. Earlier versions served both from
`users.external_id`. Existing rows got `user_name = external_id` as a backfill, so
a deployment that never sent a distinct `userName` sees no change.

SCIM never overwrites the login join key. Thus a user who has already logged in
keeps the `sub` on their row and the email fallback links them.

A SCIM-*created* user can have a stored `external_id` that is not the `sub` their
later login presents. This occurs with every issuer that does not send `externalId`.
At first login, Kairos matches that user on their email and **adopts** them. Kairos
then re-keys their `external_id` to the presented subject. They log in as the user
that SCIM provisioned for them, with the membership that SCIM granted.

**This requires a verified email.** The login-side fallback fires only when both
of these conditions are true:

- No user row has the presented `sub` as its `external_id`.
- The token asserts `email_verified` as true: the boolean `true`, or the string
  `"true"` in any letter case.

An issuer that omits the claim, or sends any other value, gets a second user row
instead. That row has no membership. That is the behaviour of earlier versions.

The trust boundary is deliberate, and
[KAIROS-A-0010](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0010.md)
records it. Kairos believes one claim and only when a single issuer asserts it. The
reason is the two costs. The cost of being too lax is one person binding to another's
identity. The cost of being too strict is a duplicate row.

Where several rows share the email, the **earliest-created** one wins, in both
directions.

Outbound, `userName` comes from `users.user_name` and `externalId` from
`users.external_id`.

`userName eq "…"` and `externalId eq "…"` are **different queries**, each
searching its own column. They were the same query in earlier versions. Thus
an issuer that filtered by login email against opaque subjects found nothing.
That included its own reconciliation sweeps, where a result of nothing means
"absent", which means re-create.

Configuring an issuer to satisfy this is
[Make the identity join work](../how-to/provision-users-with-scim.md#make-the-identity-join-work).

## Resource model

A tenant's SCIM `Users` **are its org memberships**: a resource exists
iff an `organization_members` row does. The `id` is the stable
`public.users.id` UUID. `POST` provisions (link-or-create user + create
membership, role `member`). `PATCH {"active": false}` and `DELETE`
revoke the membership immediately while **retaining the `users` row**
(audit integrity).

A deprovisioned user then answers 404 to `GET`, `PATCH`,
`PUT` and `DELETE` alike — the resource is gone, not inactive. Re-activation is
therefore a fresh `POST`. An issuer that reactivates by `PATCH {"active": true}`
against the id it remembers gets 404. It will then report the user as failing to
sync.

**Deprovision vs. live tokens (A-0010)**: access tokens validate locally
until their TTL, but Kairos checks org membership per request. A
deprovisioned user's still-valid OIDC token receives 403
`MEMBERSHIP_REQUIRED` on the next `/api` call.

### Groups

| Group `displayName` | `id` | Maps to |
|---|---|---|
| `kairos-admins` | the organization UUID | `organization_members.role`: add = promote to `admin`, remove = demote to `member` |
| `kairos-team-<slug>` | the team UUID | `team_members`; `POST` creates the team + its delivery board, `DELETE` soft-deletes both |

No other `displayName` is meaningful, and Kairos accepts no other `displayName`.
`POST` of one is **400 `invalidValue`**, not a silent skip. The slug after
`kairos-team-` must match `^[a-z][a-z0-9_-]{1,62}$`, so Kairos refuses
`kairos-team-Platform` and `kairos-team-x` too. The slug cannot have the form of
a UUID. This is the rule of a team [slug](glossary.md#slug) of the REST API.
`GET /scim/v2/Groups` lists every live team, not only the SCIM-created ones.

A group has no field for the short-code prefix of the delivery board. Kairos
makes the prefix from the letters and digits of the team slug, in capitals. The
prefix has 10 characters at most. `kairos-team-platform` gives `PLATFORM`. If a
live
delivery board has that prefix, Kairos adds a number (`PLATFORM2`).

Group members must already be provisioned Users of *this* tenant. A
`public.users` row is not enough. A member who is not one is 400
`invalidValue`. Push Users before group memberships; issuers do this naturally.

Kairos refuses to demote, deactivate or delete the **last admin**, with 400
`scimType: "mutability"` (`LAST_ADMIN`). Kairos refuses group renames the same
way. Rename teams via `/api/teams`.

### What group writes refuse

`DELETE` is not unconditional, and two of these are permanent:

| Condition | Response |
|---|---|
| `DELETE` of `kairos-admins` | 400 `mutability` — it is built in and always exists |
| `DELETE` of a team group whose delivery board still holds live items | 400 `mutability`, naming the board and the count. Move or delete the items through `/api` first; until then the issuer's delete will never succeed |
| `POST` of `kairos-admins` | 409 `uniqueness` — it always exists |
| `POST` of a team slug that already exists **and is live** | 409 `uniqueness` |
| `POST` where a live board has the slug `<slug>-delivery` | 409 `uniqueness`. The `detail` names the slug and the board that has it. Kairos creates no team and no board |
| `members` not an array, a member without `value`, or a `value` that is not a UUID | 400 `invalidValue` |
| A `remove` on `path: "members"` with no value | Accepted, and removes **every** member |

## Supported subset (RFC 7643/7644)

| Feature | Support |
|---|---|
| Discovery | `ServiceProviderConfig`, `Schemas`, `ResourceTypes` |
| Users | POST, GET (id/list), PATCH, PUT, DELETE |
| Groups | POST, GET (id/list), PATCH, PUT, DELETE |
| Filtering | `userName eq "…"`, `externalId eq "…"` (Users); `displayName eq "…"` (Groups) — anything else 400 `invalidFilter` |
| Pagination | `startIndex` (1-based) / `count`. `count` defaults to **100** and is clamped to 0–200; `startIndex` is clamped up to 1. Clamping is silent, and the echoed `startIndex` is the clamped value. `count=0` is a valid empty page, not an error |
| PATCH paths (Users) | `active` (bool or `"True"`/`"False"` strings), `displayName`, or a no-path value object (unknown attributes there are ignored). Attribute names are case-insensitive throughout, in the `path` form and inside a no-path value object alike (RFC 7643 §2.1) |
| PATCH paths (Groups) | `members` add/remove/replace incl. `members[value eq "…"]`, which is matched byte-exactly — lowercase `members`, single spaces, double quotes |
| Bulk / sorting / ETags / password | not supported, and advertised as such in `ServiceProviderConfig` |
| `/Me` | not implemented. RFC 7643 has no field for advertising that, so it is not advertised: it is simply unrouted |

A deleted team group can be re-created under the same name. `teams.slug` is
unique among **live** teams only. Thus an issuer can remove a group and add it
back. The soft-deleted row stays and keeps its audit history. The re-created
team is a new team with a new `id`. It does not get the members of the deleted
team.

### Errors

Errors from the handlers are the RFC 7644 §3.12 envelope
(`urn:ietf:params:scim:api:messages:2.0:Error`) with `status`, `detail`, and a
`scimType` for 400/409-class rejections. Responses are `application/scim+json`.
Requests may use it or `application/json` — Kairos does not check the content
type.

Kairos answers four classes of failure **before** a handler runs, and therefore
not in that envelope. An issuer's error handling has to tolerate them:

| Failure | Response |
|---|---|
| A path under `/scim/v2` that is not routed — `/scim/v2/Me`, the `location` URLs that `Schemas` and `ResourceTypes` advertise for individual resources, or a typo | 404 in the `/api` envelope (`{"error": {…}}`), unauthenticated |
| A non-integer `count` or `startIndex` | 400, plain text |
| A wrong method on a routed path, e.g. `POST /scim/v2/Users/{id}` | 405, empty body |
| A body over 2 MiB | 413, plain text |

Every authentication failure is one 401 with no `scimType`. Those failures are
a malformed token, an unknown tenant, a wrong secret, a revoked token, or a
missing `Bearer ` header.

The handler refusals, beyond the group ones tabled above:

| Condition | Response |
|---|---|
| `POST` Users for someone who is already a provisioned member | 409 `uniqueness` |
| `POST` Users whose new row collides on `external_id` | 409 `uniqueness` |
| `POST`/`PUT` Users with no derivable email — no `emails[].value` and an `userName` without `@` | 400 `invalidValue` |
| `POST`/`PUT` Users with a missing or blank `userName` | 400 `invalidValue` |
| `PUT` Users changing `externalId` | 400 `mutability`. It carries the OIDC subject logins join on, so changing it would lock the user out rather than rename them. The message names the stored and received values and points at `userName`. Changing `userName` is allowed; re-keying the identity itself means deprovision and re-provision |
| `PUT` Users changing `userName` to one another user holds | 409 `uniqueness` |
| `active` present but neither a bool nor `"true"`/`"false"` | 400 `invalidValue` |
| `op: "remove"` on a User | 400 `invalidPath` |
| A PATCH `op` that is not add, replace or remove | 400 `invalidValue` |
| A PATCH body missing the `PatchOp` schema URN or `Operations`, or an op missing `op` | 400 `invalidSyntax` |
| An empty or unparseable body on any POST, PUT or PATCH | 400 `invalidSyntax` |
| A no-path PATCH op whose `value` is not an object | 400 `invalidValue` |
| A resource id that is not a UUID | 404, no `scimType` |
| A filter Kairos does not support | 400 `invalidFilter` |
| An unsupported PATCH path | 400 `invalidPath` |

Every lifecycle mutation writes a tenant `activity_log` row. Kairos attributes
the row to the organization admin who created the SCIM token (`details` prefixed
`scim token:<name>`). A call that changes nothing — a `PUT` with no profile
delta, a promotion of an existing admin, a member add that was already true —
writes no row. Kairos logs token creation and revocation separately, under
`scim_token:<name>`, attributed to the admin who made the call.

## Related guides

- [Provision users with SCIM](../how-to/provision-users-with-scim.md) — wiring
  an issuer up, and the identity-join mapping to get right first
- [Configure an OIDC issuer](../how-to/configure-an-oidc-issuer.md) — the
  authentication half; SCIM does lifecycle only
- [Wind down a team](../how-to/wind-down-a-team.md) — what to do before an issuer
  can delete a team group

## Related reading

- [Machine access](rest/machine-access.md#scim-tokens) — the `/api/scim-tokens`
  endpoints in full
- [Errors](errors.md) — the `/api` envelope that SCIM does *not* use
- [Capabilities](capabilities.md) — what an organization admin can do that a member
  cannot
