//! Pure ABAC capability semantics (KAIROS-A-0006, KAIROS-T-0011): the
//! capability vocabulary, glob matching, and the authorization decision over
//! already-loaded grants.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no async, no I/O. `kairos-db::abac` runs the single-query SQL
//! check; [`capability_matches`] is the pure mirror of that SQL's semantics
//! so both layers can never disagree.
//!
//! # Glob semantics (the A-0006 LIKE translation, exactly)
//!
//! A-0006 defines the access check as SQL:
//!
//! ```sql
//! $required LIKE replace(capability, '*', '%') OR capability = '*'
//! ```
//!
//! with one hardening refinement carried by both layers: LIKE's own
//! metacharacters (`%`, `_`, and the escape char `\`) occurring in a STORED
//! capability are escaped before the `*`→`%` translation, so they match
//! themselves literally and can never smuggle a wildcard into the check. The
//! resulting semantics, mirrored by [`capability_matches`]:
//!
//! - every `*` in the granted string matches any sequence of characters
//!   (including the empty sequence),
//! - every other character (including `%`, `_`, `\`) matches only itself,
//! - a bare `*` grant matches every required capability.
//!
//! The only SANCTIONED glob position in the A-0006 vocabulary is a trailing
//! `*` (`*`, `manage_*`, `configure_*`, `transition_*`). A `*` embedded
//! mid-string still wildcards — that is what the SQL translation does and
//! this function mirrors SQL truth rather than inventing a second dialect —
//! but such grants are outside the vocabulary and should be rejected at
//! grant time by API-layer validation, not relied upon.

// ---------------------------------------------------------------------------
// Capability vocabulary (A-0006)
// ---------------------------------------------------------------------------

/// Create, update, delete strategies.
pub const MANAGE_STRATEGIES: &str = "manage_strategies";
/// Create, update, delete initiatives.
pub const MANAGE_INITIATIVES: &str = "manage_initiatives";
/// Create, update, delete tasks.
pub const MANAGE_TASKS: &str = "manage_tasks";
/// Create, update, delete documents.
pub const MANAGE_DOCUMENTS: &str = "manage_documents";
/// Create, update, delete ADRs.
pub const MANAGE_ADRS: &str = "manage_adrs";
/// Move items between board columns.
pub const TRANSITION_ITEMS: &str = "transition_items";
/// Modify board columns, transitions, settings.
pub const CONFIGURE_BOARDS: &str = "configure_boards";
// `configure_templates` and `configure_metadata` were in this vocabulary and
// are deliberately NOT (KAIROS-T-0182, amending KAIROS-A-0006). A grant is
// keyed (board_id, user_id, capability), and `templates` and
// `metadata_definitions` have no board_id — they are tenant-wide, scoped by
// entity_type at most. So "configure_metadata on board X" could only ever have
// authorised edits affecting every board, which is org-admin authority wearing
// a board-scoped costume. Both stayed unchecked by any handler for their whole
// life; the admin UI rendered them beside two that worked. Making templates and
// metadata board-scoped is a schema change, not a bug fix — see
// KAIROS-T-0182 if that is ever wanted.
/// Add/remove users from the board, grant/revoke capabilities.
///
/// Named `administer_members` and NOT `manage_members` (KAIROS-T-0183,
/// amending KAIROS-A-0006). Matching is textual, so while it was called
/// `manage_members` the [`GLOB_MANAGE`] glob covered it — an admin granting
/// `manage_*` as shorthand for "the work-item capabilities" also handed over
/// the power to grant and revoke other people's capabilities. Renaming it out
/// of the prefix makes `manage_*` mean what it reads, without a second rule in
/// the matcher.
pub const ADMINISTER_MEMBERS: &str = "administer_members";

/// Glob: all capabilities on the board (full access).
pub const GLOB_ALL: &str = "*";
/// Glob: all `manage_<entity>` capabilities — and, since KAIROS-T-0183
/// renamed [`ADMINISTER_MEMBERS`] out of the prefix, exactly those. It is a
/// genuine family glob now rather than one by appearance.
pub const GLOB_MANAGE: &str = "manage_*";
/// Glob: all `configure_*` capabilities — currently just
/// [`CONFIGURE_BOARDS`], since KAIROS-T-0182 removed the other two. Kept as a
/// glob rather than collapsed into the specific capability: it is a stable
/// grant an admin may already hold, and it keeps meaning "whatever configuring
/// this board comes to mean".
pub const GLOB_CONFIGURE: &str = "configure_*";
/// Glob: all `transition_*` capabilities (currently just
/// [`TRANSITION_ITEMS`], but future-proof).
pub const GLOB_TRANSITION: &str = "transition_*";

/// Every specific (non-glob) capability in the A-0006 vocabulary.
pub const CAPABILITIES: &[&str] = &[
    MANAGE_STRATEGIES,
    MANAGE_INITIATIVES,
    MANAGE_TASKS,
    MANAGE_DOCUMENTS,
    MANAGE_ADRS,
    TRANSITION_ITEMS,
    CONFIGURE_BOARDS,
    ADMINISTER_MEMBERS,
];

/// Every sanctioned glob form (trailing-`*` only, per A-0006).
pub const GLOBS: &[&str] = &[GLOB_ALL, GLOB_MANAGE, GLOB_CONFIGURE, GLOB_TRANSITION];

/// The capabilities IMPLIED by membership of a board's owning team
/// (KAIROS-T-0072 amendment to A-0006): day-to-day delivery work only.
/// Deliberately narrow — no `configure_*`, no `administer_members`, and none of
/// the strategy/initiative/ADR `manage_*` families: those remain explicit
/// grants (or org-admin).
pub const TEAM_IMPLIED_CAPABILITIES: &[&str] = &[MANAGE_TASKS, MANAGE_DOCUMENTS, TRANSITION_ITEMS];

/// Does membership of the board's owning team satisfy `required` on its
/// own (KAIROS-T-0072)? Exact vocabulary membership — implied capabilities
/// are specific, never globs, so no `capability_matches` translation
/// applies here.
pub fn team_implies(required: &str) -> bool {
    TEAM_IMPLIED_CAPABILITIES.contains(&required)
}

/// A request to any team (KAIROS-T-0105, amended by COLLIERY-T-0218 for
/// COLLIERY-A-0023 decisions 7 and 8): any member of the tenant may send a
/// request to any team: a TASK in the entry column of its delivery board,
/// in the support lane. Teams request work of each other; no team pushes
/// work to a different team. The repository is optional and is not part of
/// the condition. Until COLLIERY-T-0218 it was: the task had to link to a
/// repository, and the lane was whatever the caller sent. The name is kept,
/// though the entry column need not be called Backlog. COMPUTED, never
/// stored: it is not in the
/// grantable vocabulary ([`CAPABILITIES`]), so `board_member_capabilities`
/// can never carry it, and it is satisfied purely by tenant membership on
/// a delivery board. Nothing past the entry column is opened by it.
pub const FILE_BACKLOG: &str = "file_backlog";

/// Every capability that is COMPUTED rather than granted — never in the
/// grantable vocabulary (so grant/revoke refuse it), reported by `whoami`
/// under `implicit`.
pub const COMPUTED_CAPABILITIES: &[&str] = &[FILE_BACKLOG];

// ---------------------------------------------------------------------------
// The edit rule and the link rule (COLLIERY-T-0228)
// ---------------------------------------------------------------------------

/// What the server knows about one principal and one item when it decides
/// an edit (COLLIERY-T-0228). The server loads the three facts; the
/// decision is [`may_edit_item`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditFacts {
    /// The principal created the item (`created_by`).
    pub created_item: bool,
    /// The principal holds `manage_<type>` on the authorization board of
    /// the item. `false` when the item has no authorization board.
    pub holds_manage: bool,
    /// The principal is an admin of the organization.
    pub is_org_admin: bool,
}

/// THE EDIT RULE (COLLIERY-T-0228). A principal, a person or a service
/// account, may edit an item when ONE of these is true:
///
/// 1. the principal created the item,
/// 2. the principal holds `manage_<type>` on the authorization board of the
///    item,
/// 3. the principal is an admin of the organization.
///
/// Creation is the primary mechanism of ownership. A capability on a board
/// is how a team shares that ownership. The rule applies to each item type:
/// strategy, initiative, task, document, ADR. It reads who created the
/// item and not where the item is, so the right stays with the creator when
/// the item moves to a different board.
///
/// An edit is: the title and the content, the metadata, the repository of a
/// task, the editorial lifecycle of a document, archive, and restore.
///
/// Creation does NOT grant movement. To move an item between columns, to
/// change its lane, or to move it to a different board, the principal needs
/// the capability on the board (`transition_items`, or `manage_tasks` on
/// the two boards of a move). A team controls its own plan
/// (COLLIERY-T-0218, COLLIERY-A-0023): a person who sends a request to a
/// team can correct its text, link it and archive it, and cannot put it in
/// the plan of that team. Creation grants nothing on a board, a team, a
/// member, a capability, a repository or the configuration of the tenant.
pub fn may_edit_item(facts: EditFacts) -> bool {
    facts.created_item || facts.holds_manage || facts.is_org_admin
}

/// THE LINK RULE (COLLIERY-T-0228). A principal may write an edge, which
/// is to create it or to remove it, when the principal may edit the item
/// at EITHER end ([`may_edit_item`]). The rule is the same for each
/// relationship type: `parent`, `blocks`, `supports`, `informs`,
/// `supersedes`.
///
/// Until COLLIERY-T-0228 `supports`, `informs` and `supersedes` needed the
/// admin role, and `parent` and `blocks` looked at the creator of the
/// source only. One rule for each type is easier to learn, and a person
/// who can edit an item could not say what the item relates to.
///
/// The rule decides WHO. It does not decide WHICH edges can exist: the
/// type rules (`graph::check_link`), the cycle check and the duplicate
/// check do not change.
pub fn may_write_edge(may_edit_source: bool, may_edit_target: bool) -> bool {
    may_edit_source || may_edit_target
}

// ---------------------------------------------------------------------------
// Tenant-wide configuration policy (A-0006 "items not on boards")
// ---------------------------------------------------------------------------

/// Tenant-wide configuration resources that live on no board. Per A-0006
/// these are org-admin-only: no board-scoped capability can ever authorize
/// writes to them.
///
/// Relationships were in this list until COLLIERY-T-0228. An edge is not
/// configuration of the tenant: it is a statement about two items, so the
/// person who may edit one of those items may write it. The server decides
/// that (`require_edge_write`), for each relationship type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenantConfigResource {
    /// `templates` rows.
    Templates,
    /// `metadata_definitions` (and their enum options).
    MetadataDefinitions,
}

impl TenantConfigResource {
    /// Whether writes to this resource require `organization_members.role =
    /// 'admin'`. Constant `true` for every variant — encoded as a function so
    /// the policy has one citable home (A-0006: "Templates, metadata
    /// definitions … only org admins can create, modify, or delete them").
    pub const fn org_admin_only(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Glob matching
// ---------------------------------------------------------------------------

/// Does the stored grant `granted` satisfy the `required` capability?
///
/// Implements exactly the A-0006 SQL LIKE translation (module docs): `*` in
/// `granted` is a multi-character wildcard, everything else — including
/// LIKE's own `%`/`_`/`\`, which the SQL escapes — is literal, and a bare
/// `*` grants all. Degenerate inputs fall out of the same rules: an empty
/// grant matches only an empty required string (`'' LIKE ''` is true), and
/// an empty required string matches only grants whose literal parts are all
/// empty (`''`, `*`, `**`, …).
pub fn capability_matches(granted: &str, required: &str) -> bool {
    if granted == GLOB_ALL {
        // The SQL's explicit `capability = '*'` arm; also subsumed by the
        // pattern arm ('%' matches anything), kept for exact parity.
        return true;
    }
    glob_match(granted, required)
}

/// LIKE-style match: `*` in `pattern` matches any (possibly empty) sequence;
/// every other character matches itself.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().expect("split yields at least one part");
    let Some(mut remaining) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        // No '*' in the pattern: exact match.
        return remaining.is_empty();
    };
    // Each middle part must appear in order; '*' absorbs the gaps.
    for part in middle {
        match remaining.find(part) {
            Some(idx) => remaining = &remaining[idx + part.len()..],
            None => return false,
        }
    }
    remaining.ends_with(last)
}

/// The A-0006 decision over a user's loaded grants for one board: allowed
/// iff any grant matches the required capability (whitelist — no grants, no
/// access).
pub fn is_authorized(grants: &[String], required: &str) -> bool {
    grants
        .iter()
        .any(|granted| capability_matches(granted, required))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- exact matches ---------------------------------------------------------

    #[test]
    fn exact_capability_matches_itself_only() {
        for capability in CAPABILITIES {
            assert!(
                capability_matches(capability, capability),
                "{capability} must match itself"
            );
        }
        assert!(!capability_matches(MANAGE_TASKS, MANAGE_DOCUMENTS));
        assert!(!capability_matches(MANAGE_TASKS, "manage_task"));
        assert!(!capability_matches(MANAGE_TASKS, "manage_taskss"));
        // Matching is case-sensitive, like LIKE.
        assert!(!capability_matches(MANAGE_TASKS, "MANAGE_TASKS"));
    }

    // -- '*' grants everything ---------------------------------------------------

    #[test]
    fn bare_star_grants_every_capability() {
        for capability in CAPABILITIES {
            assert!(capability_matches(GLOB_ALL, capability));
        }
        // '%' matches even the empty string in SQL; mirrored here.
        assert!(capability_matches(GLOB_ALL, ""));
        assert!(capability_matches(GLOB_ALL, "anything_at_all"));
    }

    // -- prefix globs -------------------------------------------------------------

    #[test]
    fn manage_glob_matches_manage_capabilities_only() {
        for capability in [
            MANAGE_STRATEGIES,
            MANAGE_INITIATIVES,
            MANAGE_TASKS,
            MANAGE_DOCUMENTS,
            MANAGE_ADRS,
        ] {
            assert!(
                capability_matches(GLOB_MANAGE, capability),
                "manage_* must cover {capability}"
            );
        }
        assert!(!capability_matches(GLOB_MANAGE, TRANSITION_ITEMS));
        assert!(!capability_matches(GLOB_MANAGE, CONFIGURE_BOARDS));
        // 'manage_tasks' LIKE 'manage_%' semantics: the bare prefix matches too.
        assert!(capability_matches(GLOB_MANAGE, "manage_"));
        // But not a non-prefix ('manage' is shorter than 'manage_').
        assert!(!capability_matches(GLOB_MANAGE, "manage"));
    }

    /// KAIROS-T-0183, asserted BY NAME rather than left to be derived from the
    /// matching rule: board administration is not part of the `manage_*` family.
    ///
    /// Before the rename this was the reverse, and asserted as such, so the
    /// behaviour looked intentional whatever anyone intended. The decision
    /// recorded here is that granting `manage_*` must NOT hand over the power to
    /// grant capabilities to others.
    ///
    /// Note that the matcher itself is unchanged and still a plain textual
    /// translation of A-0006's `LIKE` â which is why this test is about the
    /// vocabulary rather than about `capability_matches`.
    #[test]
    fn manage_glob_does_not_confer_board_administration() {
        assert!(!capability_matches(GLOB_MANAGE, ADMINISTER_MEMBERS));
        assert!(!is_authorized(
            &[GLOB_MANAGE.to_string()],
            ADMINISTER_MEMBERS
        ));
        // It takes its own grant, or `*`.
        assert!(is_authorized(
            &[ADMINISTER_MEMBERS.to_string()],
            ADMINISTER_MEMBERS
        ));
        assert!(is_authorized(&[GLOB_ALL.to_string()], ADMINISTER_MEMBERS));
        // And the old name is not a back door: it is out of the vocabulary, so
        // a stored grant of it matches nothing the API will ever require.
        assert!(!CAPABILITIES.contains(&"manage_members"));
    }

    #[test]
    fn configure_and_transition_globs() {
        // Textual, not vocabulary-aware: the glob matches any `configure_`
        // prefix, including capabilities that do not exist. That is what the
        // A-0006 LIKE translation does, and KAIROS-T-0182 shrinking the
        // vocabulary to one member must not quietly change it.
        for capability in [CONFIGURE_BOARDS, "configure_boards_someday"] {
            assert!(capability_matches(GLOB_CONFIGURE, capability));
        }
        assert!(!capability_matches(GLOB_CONFIGURE, MANAGE_TASKS));
        assert!(capability_matches(GLOB_TRANSITION, TRANSITION_ITEMS));
        assert!(!capability_matches(GLOB_TRANSITION, MANAGE_TASKS));
        assert!(!capability_matches(GLOB_TRANSITION, CONFIGURE_BOARDS));
    }

    #[test]
    fn non_matching_prefixes_are_rejected() {
        assert!(!capability_matches("manag_*", MANAGE_TASKS));
        assert!(!capability_matches("xmanage_*", MANAGE_TASKS));
        assert!(!capability_matches("manage_t*", MANAGE_DOCUMENTS));
        // A specific grant never satisfies a glob-shaped requirement.
        assert!(!capability_matches(MANAGE_TASKS, GLOB_MANAGE));
    }

    // -- empty strings --------------------------------------------------------------

    #[test]
    fn empty_strings_mirror_sql_like() {
        // '' LIKE '' is true; degenerate but harmless (neither should exist).
        assert!(capability_matches("", ""));
        // '' LIKE 'manage\_%' is false.
        assert!(!capability_matches(GLOB_MANAGE, ""));
        assert!(!capability_matches(MANAGE_TASKS, ""));
        // 'manage_tasks' LIKE '' is false.
        assert!(!capability_matches("", MANAGE_TASKS));
        // '' LIKE '%' is true (bare star).
        assert!(capability_matches(GLOB_ALL, ""));
    }

    // -- hostile inputs: LIKE metacharacters stored in grants --------------------------

    #[test]
    fn stored_percent_is_literal_not_wildcard() {
        // The SQL escapes '%' in stored capabilities; mirrored here: it only
        // matches itself.
        assert!(!capability_matches("manage%", "manage_tasks"));
        assert!(!capability_matches("manage%", "manageX"));
        assert!(capability_matches("manage%", "manage%"));
        assert!(!capability_matches("%", MANAGE_TASKS));
        assert!(capability_matches("%", "%"));
    }

    #[test]
    fn stored_underscore_is_literal_not_single_char_wildcard() {
        // '_' in LIKE matches any ONE char; escaped in SQL, literal here.
        assert!(!capability_matches("manage_tasks", "manageXtasks"));
        assert!(capability_matches("manage_tasks", "manage_tasks"));
        assert!(!capability_matches("_", "x"));
        assert!(capability_matches("_", "_"));
    }

    #[test]
    fn stored_backslash_is_literal() {
        // '\' is LIKE's escape char; escaped in SQL, literal here.
        assert!(capability_matches("manage\\tasks", "manage\\tasks"));
        assert!(!capability_matches("manage\\tasks", "manage_tasks"));
        assert!(!capability_matches("manage\\_tasks", "manage_tasks"));
    }

    #[test]
    fn embedded_star_wildcards_like_the_sql_translation() {
        // Mid-string '*' is OUTSIDE the sanctioned vocabulary (trailing only)
        // but the matcher mirrors SQL truth: replace(capability,'*','%')
        // translates every '*'.
        assert!(capability_matches("manage_*s", "manage_tasks"));
        assert!(capability_matches("manage_*s", "manage_s"));
        assert!(!capability_matches("manage_*s", "manage_tasks_x"));
        assert!(capability_matches("*_tasks", "manage_tasks"));
        assert!(!capability_matches("*_tasks", "manage_adrs"));
        assert!(capability_matches("m*_*s", "manage_tasks"));
        // '**' behaves like '%%' — same as one wildcard.
        assert!(capability_matches("manage_**", "manage_tasks"));
        assert!(capability_matches("**", ""));
    }

    // -- decision helper -----------------------------------------------------------

    #[test]
    fn is_authorized_is_a_whitelist_over_grants() {
        let grants = vec![MANAGE_TASKS.to_string(), GLOB_CONFIGURE.to_string()];
        assert!(is_authorized(&grants, MANAGE_TASKS));
        assert!(is_authorized(&grants, CONFIGURE_BOARDS));
        assert!(!is_authorized(&grants, MANAGE_DOCUMENTS));
        assert!(!is_authorized(&grants, TRANSITION_ITEMS));
        // No grants, no access.
        assert!(!is_authorized(&[], MANAGE_TASKS));
        // A '*' grant anywhere in the list authorizes everything.
        let star = vec![MANAGE_TASKS.to_string(), GLOB_ALL.to_string()];
        assert!(is_authorized(&star, TRANSITION_ITEMS));
    }

    // -- tenant-wide config policy ----------------------------------------------------

    #[test]
    fn tenant_config_resources_are_org_admin_only() {
        for resource in [
            TenantConfigResource::Templates,
            TenantConfigResource::MetadataDefinitions,
        ] {
            assert!(resource.org_admin_only());
        }
    }

    /// KAIROS-T-0072: team membership implies exactly the day-to-day
    /// delivery capabilities — and nothing configuration- or
    /// membership-shaped.
    #[test]
    fn team_implies_only_the_delivery_set() {
        assert!(team_implies(MANAGE_TASKS));
        assert!(team_implies(MANAGE_DOCUMENTS));
        assert!(team_implies(TRANSITION_ITEMS));

        assert!(!team_implies(MANAGE_STRATEGIES));
        assert!(!team_implies(MANAGE_INITIATIVES));
        assert!(!team_implies(MANAGE_ADRS));
        assert!(!team_implies(CONFIGURE_BOARDS));
        assert!(!team_implies(ADMINISTER_MEMBERS));
        // Globs are grant-side forms, never implied requirements.
        assert!(!team_implies(GLOB_ALL));
        assert!(!team_implies(GLOB_MANAGE));
    }

    /// COLLIERY-T-0228: each of the three facts is sufficient, and with
    /// none of them there is no edit. All eight combinations.
    #[test]
    fn the_edit_rule_needs_one_fact_of_three() {
        for created_item in [false, true] {
            for holds_manage in [false, true] {
                for is_org_admin in [false, true] {
                    let facts = EditFacts {
                        created_item,
                        holds_manage,
                        is_org_admin,
                    };
                    assert_eq!(
                        may_edit_item(facts),
                        created_item || holds_manage || is_org_admin,
                        "{facts:?}"
                    );
                }
            }
        }
        assert!(!may_edit_item(EditFacts {
            created_item: false,
            holds_manage: false,
            is_org_admin: false,
        }));
        // Creation alone: no capability, no role.
        assert!(may_edit_item(EditFacts {
            created_item: true,
            holds_manage: false,
            is_org_admin: false,
        }));
    }

    /// COLLIERY-T-0228: one end is sufficient; neither end is a refusal.
    #[test]
    fn the_link_rule_needs_one_end() {
        assert!(may_write_edge(true, true));
        assert!(may_write_edge(true, false));
        assert!(may_write_edge(false, true));
        assert!(!may_write_edge(false, false));
    }

    /// COLLIERY-T-0228: creation is not a capability. It is not in the
    /// vocabulary that can be granted, nor in the computed one, so no grant
    /// and no `whoami` can carry it to a board.
    #[test]
    fn creation_is_not_a_capability() {
        for name in ["creator", "created_by", "owner", "edit_items"] {
            assert!(!CAPABILITIES.contains(&name));
            assert!(!COMPUTED_CAPABILITIES.contains(&name));
        }
    }

    #[test]
    fn file_backlog_is_computed_never_grantable_never_team_implied() {
        assert!(COMPUTED_CAPABILITIES.contains(&FILE_BACKLOG));
        // Not in the grantable vocabulary: no grant row can ever carry it,
        // and no family glob resolves to it (the bare `*` matches every
        // string by construction — moot, since a `*` holder already has
        // manage_tasks and the server never consults grants for it).
        assert!(!CAPABILITIES.contains(&FILE_BACKLOG));
        for glob in GLOBS.iter().filter(|g| **g != GLOB_ALL) {
            assert!(
                !capability_matches(glob, FILE_BACKLOG),
                "{glob} must not match {FILE_BACKLOG}"
            );
        }
        // Not implied by team membership either — it is tenant-wide.
        assert!(!team_implies(FILE_BACKLOG));
        for capability in CAPABILITIES {
            assert!(!COMPUTED_CAPABILITIES.contains(capability));
        }
    }
}
