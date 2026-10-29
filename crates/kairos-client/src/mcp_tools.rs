//! What each MCP tool of the server does to the data (KAIROS-T-0693): it
//! reads, it is an idempotent write, or it is a write that is not
//! idempotent. One table for the server and the CLI.
//!
//! - The `kairos mcp-proxy` retries a read, a write that carries a
//!   `request_key` (KAIROS-T-0682, KAIROS-T-0687) and an idempotent write
//!   (KAIROS-T-0699). It takes the kind of each tool from [`TOOLS`].
//! - A test of the server fails when a tool of `tools/list` is not in the
//!   table, when a name of the table is not a tool, and when the
//!   `request_key` of a tool does not agree with its kind: a write that is
//!   not idempotent takes a `request_key` (KAIROS-A-0065), unless the table
//!   gives the reason why not.

/// What a tool does to the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// The tool only reads. A second call is safe.
    Read,
    /// A second call with the same arguments gives the same state and the
    /// same answer: a set, a patch, a replace.
    Idempotent,
    /// A second call writes again, or it is refused. `unkeyed` is `None`
    /// for a tool that takes a `request_key` (decision 7 of KAIROS-A-0065);
    /// for a tool that does not take one, it is the reason why.
    NotIdempotent { unkeyed: Option<&'static str> },
}

/// The kind of a write that takes a `request_key`.
const KEYED: ToolKind = ToolKind::NotIdempotent { unkeyed: None };

/// The reason of the writes that are not in decision 7 of KAIROS-A-0065.
const SAME_STATE: ToolKind = ToolKind::NotIdempotent {
    unkeyed: Some(
        "Not in decision 7 of KAIROS-A-0065. A second call writes nothing new: it gives \
         the same state, or a refusal that names the current state.",
    ),
};

/// Each tool of the server, by name, with its kind.
pub const TOOLS: &[(&str, ToolKind)] = &[
    // Reads.
    ("whoami", ToolKind::Read),
    ("my_boards", ToolKind::Read),
    ("my_work", ToolKind::Read),
    ("board_items", ToolKind::Read),
    ("search", ToolKind::Read),
    ("get_item", ToolKind::Read),
    ("get_history", ToolKind::Read),
    ("related_work", ToolKind::Read),
    ("list_repositories", ToolKind::Read),
    ("get_repository", ToolKind::Read),
    ("list_members", ToolKind::Read),
    ("get_events", ToolKind::Read),
    ("list_metadata_definitions", ToolKind::Read),
    ("list_templates", ToolKind::Read),
    ("team_pages", ToolKind::Read),
    ("board_flow", ToolKind::Read),
    ("list_dispatch_rules", ToolKind::Read),
    ("list_org_read_tokens", ToolKind::Read),
    ("refusal_counts", ToolKind::Read),
    // The writes of decision 7 of KAIROS-A-0065: they take a request_key.
    ("create_item", KEYED),
    ("append_note", KEYED),
    ("link_items", KEYED),
    ("propose_edge", KEYED),
    ("promote_note", KEYED),
    ("transition_item", KEYED),
    ("move_item", KEYED),
    ("edit_item", KEYED),
    ("add_repository", KEYED),
    ("create_team", KEYED),
    ("create_board", KEYED),
    ("add_column", KEYED),
    ("create_team_page", KEYED),
    ("create_dispatch_rule", KEYED),
    // The sets, the patches and the replaces.
    ("set_agent_triage", ToolKind::Idempotent),
    ("set_board_policy", ToolKind::Idempotent),
    ("set_org_read_token", ToolKind::Idempotent),
    ("mark_events_read", ToolKind::Idempotent),
    ("set_repository", ToolKind::Idempotent),
    ("set_parent", ToolKind::Idempotent),
    ("set_work_class", ToolKind::Idempotent),
    ("set_complexity", ToolKind::Idempotent),
    ("set_lifecycle", ToolKind::Idempotent),
    ("set_state", ToolKind::Idempotent),
    ("set_review", ToolKind::Idempotent),
    ("set_team", ToolKind::Idempotent),
    ("clear_team", ToolKind::Idempotent),
    ("set_metadata", ToolKind::Idempotent),
    ("update_repository", ToolKind::Idempotent),
    ("update_team", ToolKind::Idempotent),
    ("update_column", ToolKind::Idempotent),
    ("set_board_capabilities", ToolKind::Idempotent),
    ("set_transition", ToolKind::Idempotent),
    ("save_metadata_definition", ToolKind::Idempotent),
    ("save_template", ToolKind::Idempotent),
    ("move_team_page", ToolKind::Idempotent),
    // The writes that are not idempotent and take no request_key.
    (
        "update_item",
        ToolKind::NotIdempotent {
            unkeyed: Some(
                "It carries `version`: a second call is refused with CONFLICT and the \
                 current content, and writes nothing (KAIROS-A-0065 decision 7).",
            ),
        },
    ),
    (
        "test_webhook_target",
        ToolKind::NotIdempotent {
            unkeyed: Some(
                "Each call sends one more test event (`ping`). It changes no item and no \
                 configuration.",
            ),
        },
    ),
    (
        "rebuild_code_index",
        ToolKind::NotIdempotent {
            unkeyed: Some(
                "A second call while the run is active is refused with \
                 CODE_INDEX_BUILD_RUNNING. After the run, a second build makes the same \
                 index again.",
            ),
        },
    ),
    ("delete_dispatch_rule", SAME_STATE),
    ("remove_webhook_target", SAME_STATE),
    ("enable_webhook_target", SAME_STATE),
    ("remove_org_read_token", SAME_STATE),
    ("unlink_items", SAME_STATE),
    ("claim_item", SAME_STATE),
    ("hand_off_item", SAME_STATE),
    ("release_item", SAME_STATE),
    ("delete_item", SAME_STATE),
    ("restore_item", SAME_STATE),
    ("cancel_item", SAME_STATE),
    ("reopen_item", SAME_STATE),
    ("purge_task", SAME_STATE),
    ("archive_completed", SAME_STATE),
    ("add_team_member", SAME_STATE),
    ("grant_capability", SAME_STATE),
    ("remove_team_member", SAME_STATE),
    ("remove_column", SAME_STATE),
    ("delete_repository", SAME_STATE),
    ("delete_metadata_definition", SAME_STATE),
    ("delete_template", SAME_STATE),
    ("edit_team_page", SAME_STATE),
    ("delete_team_page", SAME_STATE),
];

/// The kind of the tool `name`, or `None` for a name that is not a tool of
/// the table.
pub fn kind(name: &str) -> Option<ToolKind> {
    TOOLS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, kind)| *kind)
}

/// True for a tool that only reads.
pub fn is_read(name: &str) -> bool {
    kind(name) == Some(ToolKind::Read)
}

/// True for a write that a second call does not change (KAIROS-T-0699).
pub fn is_idempotent(name: &str) -> bool {
    kind(name) == Some(ToolKind::Idempotent)
}

/// True for a tool that takes a `request_key`.
pub fn takes_request_key(name: &str) -> bool {
    kind(name) == Some(KEYED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_is_in_the_table_one_time() {
        let mut names: Vec<&str> = TOOLS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "a name is in the table two times");
    }

    #[test]
    fn the_table_has_19_reads_and_14_keyed_writes() {
        let reads = TOOLS.iter().filter(|(_, k)| *k == ToolKind::Read).count();
        let keyed = TOOLS.iter().filter(|(_, k)| *k == KEYED).count();
        assert_eq!((reads, keyed), (19, 14));
        assert!(is_read("get_item") && !is_read("create_item") && !is_read("no_such_tool"));
        assert!(takes_request_key("create_item") && !takes_request_key("set_state"));
        let idempotent = TOOLS
            .iter()
            .filter(|(_, k)| *k == ToolKind::Idempotent)
            .count();
        assert_eq!(idempotent, 22);
        assert!(is_idempotent("set_state") && is_idempotent("set_metadata"));
        assert!(!is_idempotent("get_item") && !is_idempotent("create_item"));
        assert!(!is_idempotent("claim_item") && !is_idempotent("no_such_tool"));
    }

    #[test]
    fn a_reason_is_a_sentence() {
        for (name, kind) in TOOLS {
            if let ToolKind::NotIdempotent {
                unkeyed: Some(reason),
            } = kind
            {
                assert!(
                    reason.ends_with('.') && reason.len() > 20,
                    "{name}: {reason}"
                );
            }
        }
    }
}
