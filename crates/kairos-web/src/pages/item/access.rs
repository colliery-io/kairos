//! Who may edit the item on this page (KAIROS-T-0581). Pure decisions,
//! host-tested; the components in [`super`] read them through memos.
//!
//! The edit rule of the server is [`kairos_core::abac::may_edit_item`]:
//! the creator, `manage_<type>` where it counts, an org admin, and since
//! KAIROS-A-0032 the claim of a task. The client mirror of the first
//! three facts is [`crate::pages::boards::may_edit_item`]; this module
//! adds the claim. The claim counts for an edit and NOT for an archive
//! or a restore (KAIROS-A-0032, decision 2), so the archive power is the
//! three-fact mirror alone.
//!
//! The server stays the authority. The page only hides or disables what
//! the server would refuse, and says why.

use super::api::{Claim, Family, MetadataDefinition, MetadataValue};

/// What the page lets the signed-in person do with the content and the
/// metadata of the item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditAccess {
    /// `whoami` or the board read has not resolved. The page shows the
    /// content and no edit control, with no reason.
    Loading,
    /// The person may edit.
    Editable,
    /// The person may not edit: the text says why.
    ReadOnly(String),
}

impl EditAccess {
    /// No edit control on the page.
    pub(crate) fn is_read_only(&self) -> bool {
        !matches!(self, EditAccess::Editable)
    }

    /// The text next to the disabled controls. Empty while loading.
    pub(crate) fn reason(&self) -> String {
        match self {
            EditAccess::ReadOnly(reason) => reason.clone(),
            EditAccess::Loading | EditAccess::Editable => String::new(),
        }
    }
}

/// Does the signed-in person have the claim of the task (KAIROS-A-0032,
/// decision 1)? A claim through an agent key names the person, so the
/// person's own session passes too. An empty id matches nobody.
pub(crate) fn holds_claim(me: &crate::api::Whoami, claim: Option<&Claim>) -> bool {
    claim.is_some_and(|claim| !claim.user_id.is_empty() && claim.user_id == me.user.id)
}

/// Does membership of the board give `required` there with no grant
/// (KAIROS-T-0410: the edit of each item on a strategy or initiative
/// board)? It counts for an edit, and not for an archive or a restore.
pub(crate) fn membership_implies(
    me: &crate::api::Whoami,
    board_slug: &str,
    required: &str,
) -> bool {
    me.capabilities
        .iter()
        .filter(|board| board.board_slug == board_slug)
        .any(|board| board.implied.iter().any(|implied| implied == required))
}

/// The text for an item that is put away (archived). Every write resolves
/// live-only on the server.
pub(crate) const ARCHIVED_REASON: &str =
    "This item is put away (archived). Its content is read-only until a person restores it.";

/// Why the person may not edit this item. It names each fact of the
/// edit rule that the person does not have. `board_slug` is the board of
/// the item, when the page knows it.
pub(crate) fn refusal_reason(family: Family, board_slug: Option<&str>) -> String {
    let capability = super::manage_capability(family);
    let not_creator = if family == Family::Task {
        "You cannot edit this item: you did not create it, and you do not have its claim."
    } else {
        "You cannot edit this item: you did not create it."
    };
    let no_grant = match board_slug {
        Some(board) => format!("You do not have {capability} on the board {board}."),
        None => format!("You do not have {capability} for this item."),
    };
    format!("{not_creator} {no_grant}")
}

/// The decision of the page. `archived` wins: an archived item is
/// read-only for each person. `may_edit` is `None` until `whoami` and the
/// board read resolve.
pub(crate) fn edit_access(
    archived: bool,
    may_edit: Option<bool>,
    family: Family,
    board_slug: Option<&str>,
) -> EditAccess {
    if archived {
        return EditAccess::ReadOnly(ARCHIVED_REASON.to_string());
    }
    match may_edit {
        None => EditAccess::Loading,
        Some(true) => EditAccess::Editable,
        Some(false) => EditAccess::ReadOnly(refusal_reason(family, board_slug)),
    }
}

/// The text of the metadata panel when the tenant has no definition for
/// this item type. Only an org admin can add a definition.
pub(crate) fn no_definitions_text(is_admin: bool) -> &'static str {
    if is_admin {
        "No metadata definitions for this item type. Add them on the Admin page."
    } else {
        "No metadata definitions for this item type. An org admin can add them."
    }
}

/// The text of the metadata panel when the item has no value.
pub(crate) fn no_values_text(read_only: bool) -> &'static str {
    if read_only {
        "This item has no metadata."
    } else {
        "This item has no metadata. Add a field below if one applies."
    }
}

/// The values of the item that have no definition in scope: a definition
/// that left the scope of this item type, or one that the page cannot
/// read. The panel shows them read-only, so no value disappears.
pub(crate) fn orphan_values(
    definitions: &[MetadataDefinition],
    values: &[MetadataValue],
) -> Vec<MetadataValue> {
    values
        .iter()
        .filter(|value| !definitions.iter().any(|d| d.slug == value.slug))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(id: &str) -> crate::api::Whoami {
        serde_json::from_value(serde_json::json!({
            "user": {"id": id, "display_name": "Robin", "email": "robin@example.test"},
            "organization": {"slug": "demo", "role": "member"},
        }))
        .expect("test whoami")
    }

    fn claim(user_id: &str, agent: bool) -> Claim {
        Claim {
            user_id: user_id.to_string(),
            display_name: "Robin".to_string(),
            user_name: String::new(),
            agent,
            claimed_at: String::new(),
        }
    }

    /// KAIROS-A-0032: the claim holder may edit; an agent claim names the
    /// person; nobody else, and no claim, gives nothing.
    #[test]
    fn the_claim_is_a_fact_for_its_person_only() {
        let robin = me("u-1");
        assert!(holds_claim(&robin, Some(&claim("u-1", false))));
        assert!(holds_claim(&robin, Some(&claim("u-1", true))));
        assert!(!holds_claim(&robin, Some(&claim("u-2", false))));
        assert!(!holds_claim(&robin, None));
        // An empty id matches nobody.
        assert!(!holds_claim(&me(""), Some(&claim("", false))));
    }

    /// Membership of a board implies its capability there, and nowhere else.
    #[test]
    fn membership_implies_on_its_own_board_only() {
        let member: crate::api::Whoami = serde_json::from_value(serde_json::json!({
            "user": {"id": "u-1", "display_name": "Sam", "email": "sam@example.test"},
            "organization": {"slug": "demo", "role": "member"},
            "capabilities": [{
                "board_slug": "initiatives", "grants": [],
                "implied": ["manage_initiatives"],
            }],
        }))
        .expect("test whoami");
        assert!(membership_implies(
            &member,
            "initiatives",
            "manage_initiatives"
        ));
        assert!(!membership_implies(
            &member,
            "strategy",
            "manage_initiatives"
        ));
        assert!(!membership_implies(&member, "initiatives", "manage_tasks"));
        assert!(!membership_implies(
            &me("u-1"),
            "initiatives",
            "manage_initiatives"
        ));
    }

    /// The archive wins; then loading, editable, read-only with a reason.
    #[test]
    fn edit_access_reads_archive_first_then_the_rule() {
        assert_eq!(
            edit_access(true, Some(true), Family::Task, Some("b")),
            EditAccess::ReadOnly(ARCHIVED_REASON.to_string())
        );
        assert_eq!(
            edit_access(false, None, Family::Task, None),
            EditAccess::Loading
        );
        assert_eq!(
            edit_access(false, Some(true), Family::Task, None),
            EditAccess::Editable
        );
        let refused = edit_access(false, Some(false), Family::Task, Some("platform-delivery"));
        assert!(refused.is_read_only());
        assert!(refused.reason().starts_with("You cannot edit this item: "));
        assert!(EditAccess::Loading.is_read_only());
        assert_eq!(EditAccess::Loading.reason(), "");
        assert!(!EditAccess::Editable.is_read_only());
    }

    /// The reason names the missing facts: the claim only for a task, the
    /// capability of the type, and the board when the page knows it.
    #[test]
    fn the_reason_names_the_missing_facts() {
        assert_eq!(
            refusal_reason(Family::Task, Some("platform-delivery")),
            "You cannot edit this item: you did not create it, and you do not have its claim. \
             You do not have manage_tasks on the board platform-delivery."
        );
        assert_eq!(
            refusal_reason(Family::Adr, None),
            "You cannot edit this item: you did not create it. \
             You do not have manage_adrs for this item."
        );
        assert!(!refusal_reason(Family::Document, Some("d")).contains("claim"));
    }

    /// Simplified Technical English: each sentence has 20 words or fewer.
    #[test]
    fn each_text_has_short_sentences() {
        let texts = [
            ARCHIVED_REASON.to_string(),
            refusal_reason(Family::Initiative, Some("initiatives")),
            refusal_reason(Family::Task, Some("platform-delivery")),
            no_definitions_text(true).to_string(),
            no_definitions_text(false).to_string(),
            no_values_text(true).to_string(),
            no_values_text(false).to_string(),
        ];
        for text in texts {
            for sentence in text.split(['.', ':']).filter(|s| !s.trim().is_empty()) {
                let words = sentence.split_whitespace().count();
                assert!(words <= 20, "{words} words: {sentence:?}");
            }
        }
    }

    /// The empty text sends an admin to Admin, and tells a member who can.
    #[test]
    fn the_empty_text_follows_the_role() {
        assert!(no_definitions_text(true).contains("Admin page"));
        assert!(no_definitions_text(false).contains("An org admin"));
        assert!(!no_values_text(true).contains("Add a field"));
        assert!(no_values_text(false).contains("Add a field"));
    }

    /// A value with no definition in scope stays on the panel.
    #[test]
    fn orphan_values_are_the_values_with_no_definition() {
        let definition = MetadataDefinition {
            name: "Priority".to_string(),
            slug: "priority".to_string(),
            field_type: "string".to_string(),
            enum_options: vec![],
        };
        let value = |slug: &str| MetadataValue {
            slug: slug.to_string(),
            value: "x".to_string(),
        };
        let orphans = orphan_values(&[definition], &[value("priority"), value("old-field")]);
        assert_eq!(orphans, vec![value("old-field")]);
        assert!(orphan_values(&[], &[]).is_empty());
        assert_eq!(orphan_values(&[], &[value("a")]), vec![value("a")]);
    }
}
