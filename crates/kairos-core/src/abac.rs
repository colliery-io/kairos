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
/// Create/modify templates and metadata definitions.
pub const CONFIGURE_TEMPLATES: &str = "configure_templates";
/// Create/modify metadata definitions.
pub const CONFIGURE_METADATA: &str = "configure_metadata";
/// Add/remove users from the board, grant/revoke capabilities.
pub const MANAGE_MEMBERS: &str = "manage_members";

/// Glob: all capabilities on the board (full access).
pub const GLOB_ALL: &str = "*";
/// Glob: all `manage_*` capabilities.
pub const GLOB_MANAGE: &str = "manage_*";
/// Glob: all `configure_*` capabilities.
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
    CONFIGURE_TEMPLATES,
    CONFIGURE_METADATA,
    MANAGE_MEMBERS,
];

/// Every sanctioned glob form (trailing-`*` only, per A-0006).
pub const GLOBS: &[&str] = &[GLOB_ALL, GLOB_MANAGE, GLOB_CONFIGURE, GLOB_TRANSITION];

/// The capabilities IMPLIED by membership of a board's owning team
/// (KAIROS-T-0072 amendment to A-0006): day-to-day delivery work only.
/// Deliberately narrow — no `configure_*`, no `manage_members`, and none of
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

/// Cross-team Backlog filing (KAIROS-T-0105, A-0019 §4 amending A-0006):
/// any member of the tenant may create a TASK against another team's
/// repository, landing in that team's delivery-board Backlog (position 0)
/// behind their triage gate. COMPUTED, never stored: it is not in the
/// grantable vocabulary ([`CAPABILITIES`]), so `board_member_capabilities`
/// can never carry it, and it is satisfied purely by tenant membership on
/// a delivery board. Nothing past Backlog is opened by it.
pub const FILE_BACKLOG: &str = "file_backlog";

/// Every capability that is COMPUTED rather than granted — refused by the
/// grant/revoke endpoints, reported by `whoami` under `implicit`.
pub const COMPUTED_CAPABILITIES: &[&str] = &[FILE_BACKLOG];

/// Is `capability` computed (never stored)?
pub fn is_computed(capability: &str) -> bool {
    COMPUTED_CAPABILITIES.contains(&capability)
}

// ---------------------------------------------------------------------------
// Tenant-wide configuration policy (A-0006 "items not on boards")
// ---------------------------------------------------------------------------

/// Tenant-wide configuration resources that live on no board. Per A-0006
/// these are org-admin-only: no board-scoped capability can ever authorize
/// writes to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenantConfigResource {
    /// `templates` rows.
    Templates,
    /// `metadata_definitions` (and their enum options).
    MetadataDefinitions,
    /// `item_relationships` edges.
    Relationships,
}

impl TenantConfigResource {
    /// Whether writes to this resource require `organization_members.role =
    /// 'admin'`. Constant `true` for every variant — encoded as a function so
    /// the policy has one citable home (A-0006: "Templates, metadata
    /// definitions, relationships … only org admins can create, modify, or
    /// delete them").
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
            MANAGE_MEMBERS,
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

    #[test]
    fn configure_and_transition_globs() {
        for capability in [CONFIGURE_BOARDS, CONFIGURE_TEMPLATES, CONFIGURE_METADATA] {
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
        assert!(is_authorized(&grants, CONFIGURE_METADATA));
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
            TenantConfigResource::Relationships,
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
        assert!(!team_implies(CONFIGURE_TEMPLATES));
        assert!(!team_implies(CONFIGURE_METADATA));
        assert!(!team_implies(MANAGE_MEMBERS));
        // Globs are grant-side forms, never implied requirements.
        assert!(!team_implies(GLOB_ALL));
        assert!(!team_implies(GLOB_MANAGE));
    }

    #[test]
    fn file_backlog_is_computed_never_grantable_never_team_implied() {
        assert!(is_computed(FILE_BACKLOG));
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
            assert!(!is_computed(capability));
        }
    }
}
