//! The tool profiles of `/mcp` (KAIROS-T-0454, Dylan 2026-10-14).
//!
//! `tools/list` costs an agent its full text in each session. Most sessions
//! work items and never administer the organization, so the default
//! profile lists the tools of the daily work. `?profile=admin` lists the
//! administration tools, and `?profile=all` lists each tool.
//!
//! Only the list changes. The server runs a call to a tool that the profile
//! does not list, so a caller that knows a name does not break. A client
//! such as Claude Code calls only the tools of the list (KAIROS-T-0559).

use axum::http::request::Parts;
use rmcp::model::Tool;

/// The tools of the daily work: the default profile. Picked from the calls
/// of the agents (2026-10-14): these make nearly all of them.
pub(super) const WORK: [&str; 48] = [
    "whoami",
    "my_boards",
    // KAIROS-T-0489: the work of the person, in two lanes.
    "my_work",
    "board_items",
    "search",
    "get_item",
    "get_history",
    "related_work",
    "create_item",
    "edit_item",
    "update_item",
    "transition_item",
    "move_item",
    "link_items",
    "unlink_items",
    "propose_edge",
    "append_note",
    // KAIROS-T-0505: a note becomes a draft ADR or document.
    "promote_note",
    // KAIROS-T-0555: a document moves between draft, review and published.
    "set_lifecycle",
    // KAIROS-T-0506: the live state of a task.
    "set_state",
    "set_metadata",
    "set_repository",
    // KAIROS-T-0572: one parent per item, changed in one call.
    "set_parent",
    "set_work_class",
    "set_complexity",
    "set_team",
    "clear_team",
    "cancel_item",
    // KAIROS-T-0564: a done task back to work.
    "reopen_item",
    "delete_item",
    "restore_item",
    "list_repositories",
    "get_repository",
    // The bootstrap skill adds the repository of a checkout and sets how
    // to work in it.
    "add_repository",
    "update_repository",
    // The implement and triage skills: claims, the notifications, the
    // scales and templates, and the housekeeping of a board.
    "claim_item",
    "hand_off_item",
    "release_item",
    // KAIROS-T-0556: the people, by the user name that a hand-off and a
    // mention take.
    "list_members",
    "get_events",
    "mark_events_read",
    "list_metadata_definitions",
    "list_templates",
    "archive_completed",
    // A board session reads the charter of a team: its scope, and how a
    // request reaches it. The four writes of team pages stay admin.
    "team_pages",
    // KAIROS-T-0559: two reads and a build request that a task worker
    // needs. `get_repository` names `rebuild_code_index`.
    "board_flow",
    "list_dispatch_rules",
    "rebuild_code_index",
];

/// A profile of `tools/list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Profile {
    /// [`WORK`]: the default.
    Work,
    /// Each tool that is not in [`WORK`].
    Admin,
    /// Each tool.
    All,
}

impl Profile {
    /// The profile of the request: the query `profile` of the URL of
    /// `/mcp`. No query, or an empty value, is [`Profile::Work`]. The value
    /// is read without regard to case or blanks (KAIROS-T-0465). An unknown
    /// value, or `profile` more than one time, is an error that names it.
    pub(super) fn of(parts: Option<&Parts>) -> Result<Self, String> {
        let query = parts.and_then(|parts| parts.uri.query()).unwrap_or("");
        let values: Vec<&str> = query
            .split('&')
            .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
            .filter(|(key, _)| *key == "profile")
            .map(|(_, value)| value)
            .collect();
        if values.len() > 1 {
            return Err(
                "The URL has profile more than one time. Send one: /mcp?profile=admin.".into(),
            );
        }
        let value = values
            .first()
            .map(|value| value.trim().to_ascii_lowercase());
        match value.as_deref() {
            None | Some("") | Some("work") => Ok(Self::Work),
            Some("admin") => Ok(Self::Admin),
            Some("all") => Ok(Self::All),
            Some(other) => Err(format!(
                "The profile {other:?} is not known. The profiles are work (the default), \
                 admin and all: /mcp?profile=admin."
            )),
        }
    }

    /// The tools of the profile, from each tool.
    pub(super) fn filter(self, tools: Vec<Tool>) -> Vec<Tool> {
        tools
            .into_iter()
            .filter(|tool| {
                let work = WORK.contains(&tool.name.as_ref());
                match self {
                    Self::Work => work,
                    Self::Admin => !work,
                    Self::All => true,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(uri: &str) -> Parts {
        axum::http::Request::builder()
            .uri(uri)
            .body(())
            .unwrap()
            .into_parts()
            .0
    }

    #[test]
    fn the_query_chooses_the_profile() {
        assert_eq!(Profile::of(None), Ok(Profile::Work));
        assert_eq!(Profile::of(Some(&parts("/mcp"))), Ok(Profile::Work));
        assert_eq!(
            Profile::of(Some(&parts("/mcp?profile=admin"))),
            Ok(Profile::Admin)
        );
        assert_eq!(
            Profile::of(Some(&parts("/mcp?x=1&profile=all"))),
            Ok(Profile::All)
        );
        assert!(
            Profile::of(Some(&parts("/mcp?profile=every")))
                .unwrap_err()
                .contains("every")
        );
        // KAIROS-T-0465: an empty value is the default; case and blanks do
        // not matter; profile two times is refused.
        assert_eq!(
            Profile::of(Some(&parts("/mcp?profile="))),
            Ok(Profile::Work)
        );
        assert_eq!(Profile::of(Some(&parts("/mcp?profile"))), Ok(Profile::Work));
        assert_eq!(
            Profile::of(Some(&parts("/mcp?profile=Admin"))),
            Ok(Profile::Admin)
        );
        assert_eq!(
            Profile::of(Some(&parts("/mcp?profile=ALL"))),
            Ok(Profile::All)
        );
        assert!(
            Profile::of(Some(&parts("/mcp?profile=admin&profile=all")))
                .unwrap_err()
                .contains("more than one time")
        );
    }
}
