---
id: admin-surfaces-and-the-break-glass
level: task
title: "Admin surfaces and the break-glass CLI, including the first-boot admin"
short_code: "KAIROS-T-0204"
created_at: 2026-09-26T12:44:50.779267+00:00
updated_at: 2026-09-26T12:44:50.779267+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0203]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] An org admin can create a local user, set a password, and revoke sessions
- [ ] Adding a password to an email that already has an OIDC identity updates that
      row rather than creating a second person
- [ ] `KAIROS_BOOTSTRAP_*` works only when no users exist, is inert afterwards, and
      says so in the log when consumed
- [ ] A `kairos-server` subcommand can set a password without a working login, and
      is documented alongside the other destructive operator commands
- [ ] Setting a password revokes that user's existing sessions (the mechanism is
      [[KAIROS-T-0203]]'s; this proves it fires)
- [ ] No new grantable capability unless it is genuinely enforced
- [ ] `angreal test lint`, `unit`, `integration` green

## Status Updates

*To be added during implementation*
