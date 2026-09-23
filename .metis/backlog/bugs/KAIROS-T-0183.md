---
id: manage-grants-board-administration
level: task
title: "manage_* grants board administration, which nobody granting it expects"
short_code: "KAIROS-T-0183"
created_at: 2026-09-23T23:49:39.549854+00:00
updated_at: 2026-09-23T23:49:39.549854+00:00
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

# manage_* grants board administration, which nobody granting it expects

## Objective

Decide whether `manage_*` should satisfy `manage_members`, and make the
vocabulary say what it does.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (privilege granted by accident)

### Impact Assessment

- **Affected users**: any org admin who has granted `manage_*` as shorthand for
  "all the work-item capabilities". They have also granted board
  administration — **including granting and revoking other people's
  capabilities**.
- **Reproduction**: grant `manage_*` on a board, then add a member and grant
  them capabilities as the grantee. It works.
- **Why**: matching is textual, not family-aware. `manage_members` begins with
  `manage_`, so `manage_*` matches it.
  `kairos_core::abac::capability_matches` does exactly this, and
  `manage_glob_matches_manage_capabilities_only` in the same file **asserts**
  it — so the behaviour is intentional at the matcher level, whatever was
  intended at the vocabulary level.

### Why it reads as a mistake

The other three globs are family globs: `configure_*` covers the three
`configure_` capabilities, `transition_*` covers `transition_items`, and `*`
obviously means everything. `manage_*` reads the same way — a family of
`manage_<entity>` capabilities — and it is the one that is not, because
`manage_members` shares the prefix without being a work-item capability.

Nothing in the product surfaces the difference. The admin interface lists all
ten checkboxes flat, and `manage_*` is a plausible thing to type into a grant.

## Acceptance Criteria

- [ ] A decision, recorded: either `manage_*` is intended to include
      `manage_members`, or it is not.
- [ ] **If it is not**: `manage_members` is renamed out of the prefix (e.g.
      `administer_members`), or the matcher special-cases it, or the glob is
      removed from the recognised set.
- [ ] **If it is**: `reference/capabilities.md` keeps the warning it now
      carries, and the admin interface says so where a grant is made — not only
      in the documentation.
- [ ] Either way, a test asserts the decided semantics by name rather than by
      pattern, so the next reader does not have to derive it.

## Implementation Notes

Renaming is the cheapest honest fix and the most disruptive — it invalidates
stored grants, so it needs a migration mapping the old name.

Special-casing the matcher is smaller but makes the glob rules non-uniform,
which is its own cost: the current rules are one sentence, and `%`/`_` being
literal is already the surprising part.

Documentation-only is the weakest option, and it is what has been done so far
([[KAIROS-T-0176]]'s page now carries the warning). It does not help the admin
in the interface at the moment they grant.

## Status Updates

**2026-09-23 — filed.** Found by the independent Diátaxis review in
[[KAIROS-T-0175]], as a blocking R4/R6 finding against
`reference/capabilities.md`: the page listed `manage_*` as satisfying the five
`manage_<entity>` names, which is what it looks like and not what it does.
Verified independently before filing, by running the matcher's own algorithm
over the vocabulary and by reading the core test that asserts it.

The reference page is corrected. The product question is this ticket.
