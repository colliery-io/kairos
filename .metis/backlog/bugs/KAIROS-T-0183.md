---
id: manage-grants-board-administration
level: task
title: "manage_* grants board administration, which nobody granting it expects"
short_code: "KAIROS-T-0183"
created_at: 2026-09-23T23:49:39.549854+00:00
updated_at: 2026-09-25T00:41:21.839151+00:00
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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A decision, recorded: either `manage_*` is intended to include
      `manage_members`, or it is not.
- [x] **If it is not**: `manage_members` is renamed out of the prefix (e.g.
      `administer_members`), or the matcher special-cases it, or the glob is
      removed from the recognised set.
- [n/a] **If it is**: `reference/capabilities.md` keeps the warning it now
      carries, and the admin interface says so where a grant is made — not only
      in the documentation.
- [x] Either way, a test asserts the decided semantics by name rather than by
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
### 2026-09-25 â decided: renamed to `administer_members`

**The decision, put to the human and answered: `manage_*` is NOT intended to
include board administration.** So `manage_members` is renamed out of the prefix.
`manage_*` now covers exactly the five `manage_<entity>` capabilities, which is
what it has always read as.

### Why renaming rather than special-casing

Special-casing was the smaller diff and the worse change. [[KAIROS-A-0006]]
defines the access check as one SQL `LIKE` expression, and requires the pure
matcher (`kairos_core::abac::capability_matches`) to mirror it exactly so the two
layers can never disagree. An exception would have to be written into both, in two
languages, and the glob rules would stop being one sentence â with `%`/`_` being
literal already the surprising part.

The rename needs no exception anywhere. `administer_members` is simply not in the
`manage_` family, and the GUI's `compose_selection` â which collapses singles
covered by an enabled family glob â became correct **without a line changing**,
because it tests `starts_with("manage_")`. When a rename makes three independent
pieces of logic right by construction, it is describing the model better than the
old name did.

### The breaking part, deliberately not softened

A holder of `manage_*` **loses board administration** on upgrade. The migration
renames explicit grants and pointedly does *not* grant `administer_members` to
glob holders to compensate.

Compensating would preserve precisely the access this ticket exists to remove.
And it cannot be done correctly even in principle: there is no way to tell an
admin who typed `manage_*` meaning "the work-item capabilities" from one who
meant "and administration too", because **the vocabulary never let them say
which** â that is the whole defect. An operator who did intend it now has a word
for it.

`*` holders are unaffected. The migration comment carries the query to find who
is affected before upgrading.

### Surfaces changed

- `kairos_core::abac`: `MANAGE_MEMBERS` â `ADMINISTER_MEMBERS`, with the reason
  on the const and on `GLOB_MANAGE`, which is a genuine family glob now rather
  than one by appearance.
- Handlers, the GUI editor and gating, the whoami-driven admin gate, and the
  OpenAPI 403 descriptions (regenerated).
- A tenant migration, with a reversible `down.sql` that restores the old name and
  says plainly that doing so restores the defect.
- [[KAIROS-A-0006]] amended inline: the vocabulary entry, the glob description,
  and the team-implication paragraph.
- `reference/capabilities.md`: the glob table, a rewritten explanation, an
  **Upgrading** note, and the vocabulary count corrected from fourteen to twelve
  (this and [[KAIROS-T-0182]] each removed entries).

### The test asserts the decision by name

`manage_glob_does_not_confer_board_administration` states it directly rather than
leaving the next reader to derive it from the matching rule: `manage_*` does not
satisfy `administer_members`, its own grant and `*` do, and `manage_members` is
not in the vocabulary so a stale stored grant is not a back door.

### Three tests asserted the OLD behaviour, which is the finding

The accidental privilege was not merely implemented â it was **consistently
tested**:

- `manage_glob_matches_manage_capabilities_only` (core) listed `manage_members`
  among what `manage_*` must cover.
- `glob_semantics_mirror_a0006` (GUI) asserted the same, so the interface and
  server agreed *about the wrong thing*.
- `abac_capability_lifecycle` (db) asserted the SQL check and the pure matcher
  both returned **true** for it â a test whose entire purpose is that the two
  layers agree, agreeing on a privilege nobody wanted.

That is why the ticket said the behaviour "is intentional at the matcher level,
whatever was intended at the vocabulary level". A test suite can lock in a
mistake as firmly as it locks in a requirement, and three of them did. All three
now assert the decided semantics, and the GUI one carries a note that its copy of
the rule has to agree with the server's or the interface shows access the API
will refuse.

One further consequence worth recording, because it is the kind of thing that
looks like a failure: `whoami` returns grants **sorted**, so
`administer_members` now sorts *before* `configure_boards` where `manage_members`
sorted after. An integration assertion flipped for that reason alone.

### Gates

lint clean, **394 unit tests**, integration **47/47 exit 0**, `angreal web lint`
clean, `angreal docs build` green, REST reference regenerated.