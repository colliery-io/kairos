//! Wire types of the work of a person (KAIROS-T-0489, COLLIERY-I-0624):
//! `GET /api/me/work`, the "for me" page, the MCP tool `my_work` and the
//! CLI `kairos me`.
//!
//! The stand-up of a team (KAIROS-T-0722, COLLIERY-I-0663): `GET
//! /api/teams/{id}/standup`, `GET /api/teams/{id}/standup/people/{user_id}`
//! and `GET /api/teams/{id}/standup/unclaimed`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{ItemOwner, TaskClaim};

/// `GET /api/me/work` response: the two lanes of the work of the caller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MyWork {
    /// The strategies and the initiatives that the caller owns, by short
    /// code.
    pub initiatives: Vec<WorkItem>,
    /// The tasks that the caller (or an agent key of the caller) has the
    /// claim on; the tasks in Todo, Active or Blocked under an initiative
    /// that the caller owns; and the tasks that one of these blocks. By
    /// short code.
    pub tasks: Vec<WorkItem>,
}

/// One item of the work. No item is in a done column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WorkItem {
    /// Entity id (UUID).
    pub id: String,
    pub short_code: String,
    pub title: String,
    /// `strategy`, `initiative` or `task`.
    pub entity_type: String,
    /// `todo`, `active` or `blocked`. Blocked: the item has an open
    /// blocker, or it is in a column named Blocked.
    pub state: String,
    /// Why the item is in the work: `owned`, `claimed`,
    /// `under_my_initiative`, `blocked_by_my_work`, and `unclaimed` (the
    /// last stop of a stand-up).
    pub reasons: Vec<String>,
    /// The board of the item.
    pub board: WorkBoard,
    /// The column of the item: id (UUID) and name.
    pub column_id: String,
    pub column_name: String,
    /// The claim of a task, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<TaskClaim>,
    /// The owner of a strategy or an initiative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ItemOwner>,
    /// The open blockers: the items, not in a done column, with a `blocks`
    /// edge to this item.
    #[serde(default)]
    pub blockers: Vec<WorkBlocker>,
}

/// The board of an item of the work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WorkBoard {
    /// Board id (UUID).
    pub id: String,
    pub slug: String,
    pub name: String,
}

/// An open blocker of an item of the work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WorkBlocker {
    pub short_code: String,
    pub title: String,
}

/// `GET /api/teams/{id}/standup` response: the team and its people, for
/// the round of a stand-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Standup {
    pub team: StandupTeam,
    /// The people of the round, by display name: the members of the team,
    /// and each person who holds work of the team and is not a member. No
    /// service accounts.
    pub people: Vec<StandupPerson>,
}

/// The team of a stand-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StandupTeam {
    /// Team id (UUID).
    pub id: String,
    pub slug: String,
    pub name: String,
}

/// A person of the round of a stand-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StandupPerson {
    /// User id (UUID).
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
    /// A member of the team. `false`: a person who is not a member, and who
    /// claims a task of the team or owns an initiative of the team.
    pub member: bool,
}

/// `GET /api/teams/{id}/standup/unclaimed` response: the last stop of a
/// stand-up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UnclaimedWork {
    /// The tasks on the boards of the team that no one claims, in the
    /// state `todo` or `blocked`, by short code.
    pub tasks: Vec<WorkItem>,
}
