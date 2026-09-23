---
id: two-grantable-capabilities-are
level: task
title: "Two grantable capabilities are never checked: configure_templates and configure_metadata"
short_code: "KAIROS-T-0182"
created_at: 2026-09-23T23:27:21.143890+00:00
updated_at: 2026-09-23T23:27:21.143890+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# Two grantable capabilities are never checked: configure_templates and configure_metadata

## Objective

Either enforce `configure_templates` and `configure_metadata`, or remove them
from the grantable vocabulary and the admin interface. Today a grant can be
made, saved, and displayed, and it authorises nothing.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (an admin can grant access that silently does not work)

### Impact Assessment

- **Affected users**: an org admin delegating template or metadata
  configuration, and the person they delegate it to. The admin grants it, the
  interface shows it granted, and the grantee gets 403 on every attempt.
- **Reproduction**: grant `configure_metadata` on a board, then try any
  metadata-definition write as the grantee.
- **Expected vs actual**: the grant authorises the writes it names. Instead
  `crates/kairos-server/src/api/meta/definitions.rs:431,510,579` and
  `templates.rs:265,314` gate on `require_org_admin`, and **no handler
  anywhere consults either capability** — verified by grepping both the
  `kairos_core::abac` constants and the literal strings across
  `crates/kairos-server/src`: zero hits.

### The precise shape, because two nearby capabilities are fine

Of the four non-`manage_<family>` capabilities, **two are enforced and two are
not**:

| Capability | Enforced? | Where |
|---|---|---|
| `configure_boards` | **yes** | `api/org/boards.rs:41` (local `CONFIGURE` const) |
| `manage_members` | **yes** | `api/org/boards.rs:43` |
| `configure_templates` | **no** | template writes are `require_org_admin` |
| `configure_metadata` | **no** | definition writes are `require_org_admin` |

So this is not "board-scoped configuration was never wired up" — two of them
were, in the same file, and these two were missed. That is also why it is
invisible: the admin interface
(`crates/kairos-web/src/pages/admin/capabilities.rs:44,119,151,283`) renders
all four checkboxes identically, and two of them work.

### What it already caused

A [[KAIROS-I-0016]] how-to guide promised `configure_boards` covered every
step of setting up a board, and step 4 — defining a metadata field — does not
work with it. A reader holding exactly what the page asked for would have hit
403 through the whole step. The guide was corrected; the product was not.

And two reference pages in the book **contradict each other**:
`reference/capabilities.md` lists both as grantable (from the vocabulary), while
the generated `reference/rest/tenant-configuration.md` reports the endpoints as
org-admin (from the handler annotations). Both are faithful to their source,
which is how the disagreement survived.

## Acceptance Criteria

- [ ] Each of `configure_templates` and `configure_metadata` is **either**
      consulted by the handlers it names **or** removed from
      `kairos_core::abac::CAPABILITIES` and from the admin interface.
- [ ] Whichever way it goes, `reference/capabilities.md` and the generated
      REST reference agree.
- [ ] A test that every member of `CAPABILITIES` is consulted by at least one
      handler. This class of defect is invisible by construction — a
      capability nobody checks looks exactly like one nobody has used yet.

## Implementation Notes

**Enforcing is probably the right direction**, since `configure_boards`
already is: A-0006 makes board-scoped configuration delegable, and templates
and metadata definitions are tenant-wide rather than board-scoped, which may be
precisely why they were left as org-admin. If that is the intent, then the
capabilities should be **removed** rather than wired — a vocabulary that
promises delegation the model does not support is worse than a shorter one.

That is the actual decision, and it is a product call: **are templates and
metadata definitions board-scoped or tenant-wide?** The answer determines which
half of the acceptance criteria applies.

## Status Updates

**2026-09-23 — filed.** Surfaced by [[KAIROS-I-0016]]: a how-to guide claimed a
capability covered a step it does not, and the independent review caught it as
a blocking finding. The narrower shape — that two of the four are enforced and
two are not — came from checking the claim before filing, and matters, because
"board-scoped configuration is unimplemented" would have been the wrong
diagnosis.
