---
id: two-grantable-capabilities-are
level: task
title: "Two grantable capabilities are never checked: configure_templates and configure_metadata"
short_code: "KAIROS-T-0182"
created_at: 2026-09-23T23:27:21.143890+00:00
updated_at: 2026-09-25T00:28:26.566819+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Each of `configure_templates` and `configure_metadata` is **either**
      consulted by the handlers it names **or** removed from
      `kairos_core::abac::CAPABILITIES` and from the admin interface.
- [x] Whichever way it goes, `reference/capabilities.md` and the generated
      REST reference agree.
- [x] A test that every member of `CAPABILITIES` is consulted by at least one
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
### 2026-09-25 â removed, not wired, and the data model made that call

The ticket framed this as a product decision: **are templates and metadata
definitions board-scoped or tenant-wide?** The schema answers it, so this was not
a matter of taste.

A grant is `board_member_capabilities (board_id, user_id, capability)`. Neither
`templates` nor `metadata_definitions` has a `board_id` â `metadata_definitions`
is scoped by `metadata_definition_scopes (metadata_definition_id, entity_type)`,
which is item type, not board, and `templates` is not scoped at all. Both are
tenant-wide.

So `configure_metadata` was never enforceable as written. Granting it "on board X"
could only have authorised edits affecting *every* board â which is exactly the
org-admin authority it appeared to delegate away from. There was no correct
handler to add the check to.

**Both capabilities are removed from the vocabulary.** Template and metadata
writes stay org-admin. Making them delegable means giving those resources a board
scope first, which is a schema change and a feature, not a permissions fix.

### What changed

- `kairos_core::abac`: the two consts and their `CAPABILITIES` entries are gone,
  replaced by a comment explaining why, so the next reader does not re-add them.
  `GLOB_CONFIGURE` stays â it still covers `configure_boards`, and it is a grant
  people may already hold.
- The admin UI (`kairos-web/src/pages/admin/capabilities.rs`) loses both rows,
  the two signals, and the two `SINGLES` entries. Removing them exposed a latent
  bug on the way out: `single_signal`'s match never listed `configure_metadata`,
  so it fell through to `_ => self.manage_members`. Harmless only because nothing
  read the result.
- A tenant migration deletes inert grant rows. **It changes no access decision** â
  nothing consulted them, so every holder already had precisely the access they
  have afterwards. What it changes is that `whoami` and the admin interface stop
  reporting a capability outside the vocabulary. `configure_*` glob grants are
  deliberately untouched.
- [[KAIROS-A-0006]] is amended inline, in the style A-0013 uses, with the
  structural reason rather than just the removal.
- `reference/capabilities.md` replaces its "Two of these do not work" section with
  "Templates and metadata definitions are not delegable", and the `configure_*`
  row now reads `configure_boards` (its only member today).

### The reference disagreement is resolved

The two pages contradicted each other because each was faithful to its own
source: `capabilities.md` read the vocabulary, the generated
`rest/tenant-configuration.md` read the handler annotations. The vocabulary was
the one that was wrong. The generated page still says *org admin only* on every
one of those endpoints, and the hand-written page now agrees.

### The test, and it was verified by breaking it

`every_grantable_capability_is_consulted_somewhere` scans every `.rs` under the
server's `src/` for each member of `CAPABILITIES`. Crude, and it catches exactly
the failure that happened: a word in a list that nothing reads.

Verified rather than assumed â re-adding `configure_metadata` to the vocabulary
fails it, naming the capability. Two companions: one asserts the removed pair
stays out (so a re-add argues with a test), and one asserts `configure_*` still
matches `configure_boards`, since a glob that lost two of its three members must
not quietly stop covering the third.

A first attempt at this test **passed while the phantom capability was present** â
the substitution that was supposed to re-add it had silently not applied. Worth
recording: a guard test that has never been seen to fail is not evidence.

### A fourth re-pin, and the pattern fixed instead

The new migration broke `tenant_provisioning_lifecycle`, whose fleet-upgrade
block simulates an old tenant by deleting `max(version)` from the bookkeeping
table. That means "whatever migration was added last", so **every** new tenant
migration breaks a test about `edge_proposals` â T-0186, T-0187, T-0192 and now
this one, four re-pins. The failure is also misleading: the newest migration is
what gets re-applied while `edge_proposals` stays dropped, so the assertion that
fails is three screens from the cause.

Now pinned to the migration it actually means (`20260924000001`), with an
assertion that the delete hit exactly one row â so a renamed version fails loudly
instead of simulating nothing. Diesel re-applies any absent version, not just the
newest, so a middle row is a valid old tenant and stays valid as migrations
accumulate. Further evidence for [[KAIROS-T-0093]].

### Gates

lint clean, **393 unit tests**, integration **47/47 exit 0**, `angreal web lint`
clean, `angreal docs build` green, REST reference current.