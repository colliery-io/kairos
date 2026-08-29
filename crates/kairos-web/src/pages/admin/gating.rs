//! Pure, whoami-driven admin access gating (KAIROS-T-0052).
//!
//! The GUI decides which admin surfaces to show from `/api/whoami`'s org
//! role and per-board capability grants ([`crate::api::Whoami`]). The
//! decision logic lives here, free of Leptos, so it is unit-testable on the
//! host target and mirrors the server's A-0006 authorization exactly.
//!
//! kairos-web does not depend on `kairos-core` (it does not compile to
//! wasm32), so [`capability_matches`] is a local copy of
//! `kairos_core::abac::capability_matches` — the same glob semantics the
//! server enforces. The server remains the authority; this only governs
//! what the UI offers.

use crate::api::{Whoami, WhoamiBoardCapabilities};

/// Capabilities that unlock the per-board admin configuration surface
/// (A-0006): board configuration and board member/grant management. A
/// non-admin holding either on some board may reach `/admin/boards`.
pub(crate) const BOARD_CONFIG_CAPS: [&str; 2] = ["configure_boards", "manage_members"];

/// Does the stored grant `granted` satisfy the concrete `required`
/// capability? A local mirror of the A-0006 SQL LIKE translation
/// (`kairos_core::abac::capability_matches`): `*` in `granted` is a
/// multi-character wildcard, a bare `*` grants all, everything else is
/// literal.
pub(crate) fn capability_matches(granted: &str, required: &str) -> bool {
    if granted == "*" {
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
    for part in middle {
        match remaining.find(part) {
            Some(idx) => remaining = &remaining[idx + part.len()..],
            None => return false,
        }
    }
    remaining.ends_with(last)
}

/// Does the caller hold `required` on ANY board (whitelist — no grant, no
/// access)?
pub(crate) fn holds_any(caps: &[WhoamiBoardCapabilities], required: &str) -> bool {
    caps.iter()
        .any(|board| board.grants.iter().any(|g| capability_matches(g, required)))
}

/// Whether the caller is an org admin (the A-0006 bypass — full access to
/// every admin surface).
pub(crate) fn is_org_admin(me: &Whoami) -> bool {
    me.organization.role == "admin"
}

/// Whether the caller holds a per-board configuration capability
/// ([`BOARD_CONFIG_CAPS`]) on at least one board — the non-admin path into
/// the admin boards surface.
pub(crate) fn has_board_config(me: &Whoami) -> bool {
    BOARD_CONFIG_CAPS
        .iter()
        .any(|cap| holds_any(&me.capabilities, cap))
}

/// May the caller reach the `/admin` section at all? Org admins always;
/// other users iff they hold a board-config capability on some board.
pub(crate) fn can_access(me: &Whoami) -> bool {
    is_org_admin(me) || has_board_config(me)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Whoami, WhoamiBoardCapabilities, WhoamiOrganization, WhoamiUser};

    fn me(role: &str, grants: &[(&str, &[&str])]) -> Whoami {
        Whoami {
            user: WhoamiUser {
                id: "u-1".into(),
                display_name: "u".into(),
                email: "u@x".into(),
            },
            organization: WhoamiOrganization {
                slug: "demo".into(),
                role: role.into(),
            },
            teams: vec![],
            capabilities: grants
                .iter()
                .map(|(slug, gs)| WhoamiBoardCapabilities {
                    board_slug: (*slug).into(),
                    grants: gs.iter().map(|g| (*g).into()).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn glob_semantics_mirror_a0006() {
        assert!(capability_matches("*", "configure_boards"));
        assert!(capability_matches("configure_*", "configure_boards"));
        assert!(capability_matches("manage_*", "manage_members"));
        assert!(capability_matches("configure_boards", "configure_boards"));
        // Non-matches.
        assert!(!capability_matches("configure_*", "manage_members"));
        assert!(!capability_matches("manage_tasks", "manage_members"));
        assert!(!capability_matches("transition_items", "configure_boards"));
    }

    #[test]
    fn org_admin_can_access_everything() {
        let admin = me("admin", &[]);
        assert!(is_org_admin(&admin));
        assert!(can_access(&admin));
    }

    #[test]
    fn plain_member_without_grants_is_denied() {
        let member = me("member", &[]);
        assert!(!can_access(&member));
        assert!(!has_board_config(&member));
    }

    #[test]
    fn member_with_board_config_grant_can_access_but_is_not_admin() {
        // A non-admin holding manage_members on one board.
        let holder = me("member", &[("platform-delivery", &["manage_members"])]);
        assert!(!is_org_admin(&holder));
        assert!(has_board_config(&holder));
        assert!(can_access(&holder));

        // configure_* glob on a board also unlocks access.
        let configurer = me("member", &[("initiatives", &["configure_*"])]);
        assert!(can_access(&configurer));

        // A member with only workflow/manage-content grants (no board-config)
        // stays out of the admin section.
        let worker = me(
            "member",
            &[("initiatives", &["manage_tasks", "transition_items"])],
        );
        assert!(!can_access(&worker));
    }
}
