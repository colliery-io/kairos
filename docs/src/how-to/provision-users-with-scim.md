# Provision users with SCIM

Connect your identity provider, the issuer, to Kairos. Then a user that you
create, deactivate or group in the issuer gets the same change in Kairos. In
particular, a deactivation in the issuer revokes the Kairos access of that
person. Nobody has to remember to do it.

You need: org-admin access to the Kairos tenant, and admin access to the issuer's
SCIM application. The tenant must also be reachable from the issuer.

## Mint a SCIM token

```sh
curl -X POST https://<kairos-host>/api/scim-tokens \
  -H "Authorization: Bearer $YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "okta-prod"}'
```

This is an ordinary `/api` call. Thus, unlike the SCIM endpoints, it needs the
tenant resolved: use the tenant's own host, or add `-H "X-Tenant: <slug>"`.

**Copy the token out of the response now.** Kairos returns it once, and stores
only its SHA-256. If you lose it, revoke it and mint another —
`GET /api/scim-tokens` lists metadata but never secrets.

Name it after the issuer application it is for. You will want to know which token
to revoke when you rotate.

## Point the IdP at Kairos

In the issuer's SCIM application:

- **Base URL**: `https://<kairos-host>/scim/v2`
- **Authentication**: HTTP header / bearer token
- **Token**: the one you just copied

No tenant subdomain and no `X-Tenant` header. The token itself carries the
tenant — see [the SCIM reference](../reference/scim.md) for how Kairos parses it.

## Make the identity join work

This is the step that goes wrong, and it goes wrong quietly. Kairos binds an
inbound SCIM user to an existing user by `externalId`, then `userName`, then
email. A login binds by the OIDC `sub`. **If SCIM creates a user whose stored
key is not the `sub` their later login presents, they get a second user row
without the org membership** — they can log in and appear to have no access.

So configure the issuer to send **the same value as `externalId` that it puts in
the OIDC `sub` claim**.

If your issuer is **Okta**: `userName` defaults to the login email. Map the SCIM
app's `externalId` to whatever the OIDC app emits as `sub`. Alternatively set
the OIDC app's subject to Okta's `userName`, which lets the email-shaped
`userName` match instead.

If your issuer is **Entra ID**: the default mapping of `externalId` is from
`objectId`. Thus make the OIDC app emit `oid` as `sub`. Otherwise adjust the SCIM attribute
mapping to match whatever `sub` you do emit.

If your issuer is something else: find what it sends as `sub`, and send the same
thing as `externalId`.

Users who have already logged in are safe either way. Their existing row keeps
its `sub`, and SCIM never overwrites the login join key. It is
SCIM-created-then-first-login that breaks.

## Push users before groups

Group membership can only reference users Kairos already knows. Most issuers push
users first on their own; if yours lets you order the operations, do users
first.

Two group names are meaningful, and **Kairos accepts nothing else**. Push only
these two shapes. Kairos refuses any other `displayName` with `400 invalidValue`,
and does not skip it. Thus an issuer configured to push its whole group catalogue
reports a failure per group.

- `kairos-admins` — membership promotes to organization admin, removal demotes to member
- `kairos-team-<slug>` — membership is team membership, and creating the group
  creates the team and its delivery board. The slug must match
  `^[a-z][a-z0-9_-]{1,62}$`

Pick team slugs you will not want back. A team group's `DELETE` soft-deletes
the team, and the slug stays taken. Thus Kairos permanently refuses a
re-creation of the same group name, with `409 uniqueness`.

## Verify

Deactivate a test user in the issuer, and let it sync. Then have them call any
`/api` endpoint with a token they already hold. They should get **403
`MEMBERSHIP_REQUIRED`** rather than a successful response. Access tokens
validate locally until they expire, but Kairos checks membership per request.
Thus revocation takes effect immediately rather than at token expiry.

Then reactivate them and confirm access returns. Reactivation is a fresh
provision: the issuer must `POST` the user again. If yours reactivates by sending
`PATCH {"active": true}` to the id it stored, it gets a 404. The reason is that
the deprovision removed the resource and did not mark it inactive.

## Limits to expect

- **You cannot remove the last admin.** Kairos refuses to demote, deactivate or
  delete them, with `400 scimType: "mutability"`, so plan a second admin before
  you need one.
- **Kairos refuses renames.** Rename a team through `/api/teams`, not by renaming
  the SCIM group.
- **A team group will not delete while its board has work on it.** The refusal
  is `400 mutability`, and it will keep failing until someone moves or deletes
  the cards through `/api`. Wind the team's board down first — see
  [Wind down a team](wind-down-a-team.md).
- **`kairos-admins` cannot be deleted.** `400 mutability`; it is built in.
- **Deprovisioning keeps the user row** and removes the membership, for audit.
  Re-activation is a fresh provision, not an undelete.
- **Bulk, sorting, ETags and password operations are not supported**, and
  Kairos advertises that in `ServiceProviderConfig`, so a conforming issuer will
  not attempt them. Kairos also does not implement `/Me`. But SCIM has no way to
  advertise that, so an issuer that calls it gets a plain 404.
- **Not every refusal is a SCIM error envelope.** Kairos answers an unrouted
  path, a bad `count`, a wrong method or an oversized body before the SCIM
  layer — see [the SCIM reference](../reference/scim.md#errors).

## Related reading

- [SCIM reference](../reference/scim.md) — the mapping table, the supported
  subset, filters and the error envelope
- [Configure an OIDC issuer](configure-an-oidc-issuer.md) — the authentication
  half; SCIM handles lifecycle only
- [Capabilities](../reference/capabilities.md) — what an organization admin can do that
  a member cannot
