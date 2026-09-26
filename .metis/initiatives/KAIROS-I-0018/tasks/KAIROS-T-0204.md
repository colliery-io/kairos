---
id: admin-surfaces-and-the-break-glass
level: task
title: "Admin surfaces and the break-glass CLI, including the first-boot admin"
short_code: "KAIROS-T-0204"
created_at: 2026-09-26T12:44:50.779267+00:00
updated_at: 2026-09-26T16:39:43.239152+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0203]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0018
---

## Parent Initiative

[[KAIROS-I-0018]]

## Objective

The ways a local account comes into existence and gets recovered: an admin creating
one, an admin resetting a password, an operator doing it from the CLI when locked out
of the GUI, and the first-boot admin that solves the empty-deployment problem.

## Dependencies

[[KAIROS-T-0203]].

## Implementation Notes

### The first-boot admin, and its trap

A fresh deployment with local auth and no users cannot be logged into, so there has
to be a bootstrap. The obvious shape — `KAIROS_BOOTSTRAP_ADMIN` +
`KAIROS_BOOTSTRAP_PASSWORD` — has a trap worth designing around rather than
discovering: **an env var in a Helm values file is a permanent credential in the
deployment manifest and in your release history.**

So it must be single-use: consumed on first boot **only when no users exist**, and
inert afterwards. Log loudly that it was consumed and that the variable should be
removed. A bootstrap that silently keeps working is a backdoor with a documented
name.

Consider accepting a pre-hashed value so the plaintext never has to be in the
manifest at all — the CLI can produce it.

### The break-glass CLI

`kairos users set-password` exists for the case the GUI cannot help with: the sole
admin of a local-auth deployment has forgotten their password. It therefore cannot
require a working login — it is an operator command run against the database, in the
shape of the existing `angreal db` / `kairos-server` subcommands rather than the
authenticated CLI.

That makes it powerful, so it belongs where `drop-tenant` lives: a `kairos-server`
subcommand, deployment-admin territory, documented as such.

### Admin surfaces

Create a local user, set a password, list and revoke that user's sessions. These are
org-admin operations and go where `configure_*` and `administer_members` already
live — and note that [[KAIROS-T-0182]] removed two capabilities for being
unenforceable, so **do not invent a new capability here without checking it can be
enforced.** Org-admin is the honest gate.

### One person, one row

An admin creating a local account for an email that already has an OIDC user must
**set a password on the existing row**, not create a second one. This is
[[KAIROS-T-0197]]'s identity question returning: that task established one person is
one row and joined on a verified email. Local accounts are a second source of
identity for the same address and must not fork it.

## Acceptance Criteria

## Acceptance Criteria

- [x] An org admin can create a local user, set a password, and revoke sessions
- [x] Adding a password to an email that already has an OIDC identity updates that
      row rather than creating a second person
- [x] `KAIROS_BOOTSTRAP_*` works only when no users exist, is inert afterwards, and
      says so in the log when consumed
- [x] A `kairos-server` subcommand can set a password without a working login, and
      is documented alongside the other destructive operator commands
- [x] Setting a password revokes that user's existing sessions (the mechanism is
      [[KAIROS-T-0203]]'s; this proves it fires)
- [x] No new grantable capability unless it is genuinely enforced
- [x] `angreal test lint`, `unit`, `integration` green

## Status Updates

### 2026-09-26 — done

Four endpoints under `/api/local-accounts`, two `kairos-server` subcommands, and a
single-use first-boot admin.

#### The admin surfaces

`crates/kairos-server/src/api/local_accounts.rs`: create, reset a password, list
sessions, revoke sessions. Mounted only when `KAIROS_LOCAL_AUTH` is on, behind the full
auth → tenant stack, **org-admin on every route including the reads** — a list of
someone's live sessions is a security surface, not work content, so A-0006's read-open
rule does not reach it.

`manage_local_accounts` is a **pseudo-capability**: a string in the 403, enforced as
org-admin, exactly like `manage_org_members` and `manage_service_accounts`.
[[KAIROS-T-0182]] deleted two capabilities for being grantable but unenforceable, so
nothing here invents a third.

Creating an account also makes the person a **member of the current organization**, and
that is the point rather than a convenience: an account belonging to no organization can
log in and then see nothing, which reads as a broken deployment. Membership uses
`ON CONFLICT DO NOTHING` — the admin's intent is "this person should be able to work
here", and someone already a member satisfies it, so a 409 would only make the obvious
retry after a network blip fail.

Revoking sessions is **separate from a reset**, deliberately. "Log this person out
everywhere" and "they have forgotten their password" are different incidents, and
forcing the first to change the password would mean telling someone a password they did
not ask for.

`require_member_of` is the isolation check, and it matters more than it looks:
`public.users` is deployment-wide, so without it an org admin could reset the password of
anybody in any other organization — a cross-tenant write through a tenant-scoped
endpoint. Its 404 is identical for "no such user" and "not in this org", because telling
them apart would say whether an address exists elsewhere in the deployment. Asserted.

#### One person, one row

`upsert_local_user` adopts an existing row for the email and returns whether it created
or adopted, because the admin means something different by each. It does **not** rewrite
the adopted row's `external_id`: turning an OIDC `sub` into `local:…` would break that
person's next SSO login, which is the opposite of additive. A brand-new row gets the
synthetic `local:<email>`.

The password always goes through `set_password`, even on an insert, so there is exactly
one place a password is written and the revoke-on-change guarantee has no second path to
leak through.

#### The first-boot admin

Single-use: `bootstrap_admin_if_empty` checks `users` is empty and inserts in one
transaction, and a boot with the same email later is **inert rather than idempotent**. An
idempotent bootstrap would silently restore a known password on every restart, which is a
backdoor with a documented name — asserted directly.

Two replicas booting together can both see zero users, so a unique violation is reported
as `RacedAnotherReplica` rather than failing a boot. Exactly one admin exists either way.

All three outcomes are logged, and the two that are not the happy path are WARN, because
each is something to act on. The inert case is the important one: silence there is how a
bootstrap password survives in a values file for a year.

**The email also becomes a deployment admin** (`local:<email>` appended to
`deployment_admins`). Not a convenience — a fresh deployment has no *organization*
either, and creating one is a deployment-admin action, so without this the bootstrap
admin logs in and can do nothing, which reads as a broken install rather than a missing
variable. It is appended, so an OIDC deployment adding a break-glass admin keeps the
admins it had.

`config.rs` refuses an email with no password and a password with no email. The second is
refused rather than ignored because the operator believes they have bootstrapped an admin,
and would otherwise find out at the login screen.

#### The two subcommands

`set-password --email <email>` is the break-glass path and takes no credential, because
the case it exists for is that nobody can log in. It **refuses to create an account** —
that would make it a way to mint an admin on any deployment whose database you can reach,
and the empty-deployment case already has a single-use switch. It reads the password from
stdin when `--password` is absent, since an argument is visible in `ps`, in shell history
and in a container's command line. It reports how many sessions it revoked, and says so
when the account also has an OIDC identity that still works.

`hash-password` is answered **before `run()` connects to the database**, which is the
whole point: it produces a value for `KAIROS_BOOTSTRAP_PASSWORD_HASH` before the
deployment exists, so the plaintext never enters a manifest. Verified by running it with
a deliberately unreachable `DATABASE_URL`.

#### A password floor, and no composition rules

`validate_password`: 12 characters minimum, counted as **characters not bytes** — `.len()`
would accept a four-character CJK password and reject an 11-character ASCII one, wrong in
both directions. No composition rules: a length floor reliably buys entropy, while "one
upper, one digit, one symbol" mostly buys `Password1!` and makes people write it down.
Whitespace is not trimmed, because trimming here while the login path does not would lock
someone out of the password they just set. All three are asserted.

#### Two defects found

1. **`bootstrap_admin_if_empty` returned a row with a NULL password.** The `RETURNING`
   row predates the `set_password` write, so the caller got a user whose `password_hash`
   was still NULL. Caught by the test asserting the hash; fixed by re-reading, the same
   way `upsert_local_user` already did.
2. **The docs pointed at something that did not exist.** "Documented alongside the other
   destructive operator commands" turned out to have nowhere to point: `drop-tenant` was
   documented in `main.rs` and in no user-facing page at all. Added an **Operator
   subcommands** section to `docs/src/reference/cli.md` covering the whole `kairos-server`
   surface, so the cross-reference is now true.

One more thing worth recording: the integration test failed at first because the
adoption step revoked the *worker's own* session — the [[KAIROS-T-0203]] guarantee firing
on an adopted row, which is exactly where it would have been easiest to miss. The test now
asserts that and logs in again.

**Files:** `api/local_accounts.rs` (new), `kairos-db/src/local_auth.rs`
(`list_sessions_for_user`, `upsert_local_user`, `bootstrap_admin_if_empty`,
`local_external_id`), `local_auth.rs` (`validate_password`, `MIN_PASSWORD_LEN`),
`config.rs` (three bootstrap variables + the deployment-admin append), `app.rs`
(`bootstrap_first_admin`, the conditional mount), `main.rs` (two subcommands),
`api/openapi.rs`, `tests/local_accounts.rs` (new, 2 tests), `kairos-db/tests/local_auth.rs`
(2 more), the chart (`bootstrap-secret.yaml`, three helpers, a render guard, deployment
env, values, CI values set), compose, `.env.example`, and both reference docs.

`angreal test lint`, `unit`, `integration` (51 suites), the chart renders and
`angreal docs build` are all green.