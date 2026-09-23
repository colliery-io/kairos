# Provision users with SCIM

Connect your identity provider to Kairos so that creating, deactivating and
grouping users in the IdP does the same in Kairos — in particular so that
deactivating someone revokes their Kairos access without anyone remembering to.

You need: org-admin access to the Kairos tenant, admin access to the IdP's SCIM
application, and the tenant reachable from the IdP.

## Mint a SCIM token

```sh
curl -X POST https://<kairos-host>/api/scim-tokens \
  -H "Authorization: Bearer $YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "okta-prod"}'
```

**Copy the token out of the response now.** It is returned once; Kairos stores
only its SHA-256. If you lose it, revoke it and mint another —
`GET /api/scim-tokens` lists metadata but never secrets.

Name it after the IdP application it is for. You will want to know which token
to revoke when you rotate.

## Point the IdP at Kairos

In the IdP's SCIM application:

- **Base URL**: `https://<kairos-host>/scim/v2`
- **Authentication**: HTTP header / bearer token
- **Token**: the one you just copied

No tenant subdomain and no `X-Tenant` header. The token itself carries the
tenant — see [the SCIM reference](../reference/scim.md) for how it is parsed.

## Make the identity join work

This is the step that goes wrong, and it goes wrong quietly. Kairos binds an
inbound SCIM user to an existing user by `externalId`, then `userName`, then
email. A login binds by the OIDC `sub`. **If SCIM creates a user whose stored
key is not the `sub` their later login presents, they get a second user row
without the org membership** — they can log in and appear to have no access.

So configure the IdP to send **the same value as `externalId` that it puts in
the OIDC `sub` claim**.

If your IdP is **Okta**: `userName` defaults to the login email. Map the SCIM
app's `externalId` to whatever the OIDC app emits as `sub`. Alternatively set
the OIDC app's subject to Okta's `userName`, which lets the email-shaped
`userName` match instead.

If your IdP is **Entra ID**: `externalId` is mapped from `objectId` by default,
so make the OIDC app emit `oid` as `sub`. Otherwise adjust the SCIM attribute
mapping to match whatever `sub` you do emit.

If your IdP is something else: find what it sends as `sub`, and send the same
thing as `externalId`.

Users who have already logged in are safe either way — their existing row keeps
its `sub`, and SCIM never overwrites the login join key. It is
SCIM-created-then-first-login that breaks.

## Push users before groups

Group membership can only reference users Kairos already knows. Most IdPs push
users first on their own; if yours lets you order the operations, do users
first.

Two group names are meaningful, and the rest are ignored:

- `kairos-admins` — membership promotes to org admin, removal demotes to member
- `kairos-team-<slug>` — membership is team membership, and creating the group
  creates the team and its delivery board

## Verify

Deactivate a test user in the IdP, let it sync, then have them call any `/api`
endpoint with a token they already hold. They should get **403
`MEMBERSHIP_REQUIRED`** rather than a successful response: access tokens
validate locally until they expire, but membership is checked per request, so
revocation takes effect immediately rather than at token expiry.

Then reactivate them and confirm access returns.

## Limits to expect

- **The last admin cannot be removed.** Demoting, deactivating or deleting them
  is refused with `400 scimType: "mutability"`, so plan a second admin before
  you need one.
- **Renames are refused.** Rename a team through `/api/teams`, not by renaming
  the SCIM group.
- **Deprovisioning keeps the user row** and removes the membership, for audit.
  Re-activation is a fresh provision, not an undelete.
- **Bulk, sorting, ETags, `/Me` and password operations are not supported**,
  and Kairos advertises that in `ServiceProviderConfig`, so a conforming IdP
  will not attempt them.

## Related reading

- [SCIM reference](../reference/scim.md) — the mapping table, the supported
  subset, filters and the error envelope
- [Configure an OIDC issuer](configure-an-oidc-issuer.md) — the authentication
  half; SCIM handles lifecycle only
- [Capabilities](../reference/capabilities.md) — what an org admin can do that
  a member cannot
