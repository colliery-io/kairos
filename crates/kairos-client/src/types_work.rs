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

/// A person of the group of an AdHoc stand-up (KAIROS-T-0764).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StandupGroupPerson {
    /// User id (UUID).
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
}

/// The group of an AdHoc stand-up (KAIROS-T-0764, KAIROS-A-0084): a saved,
/// named group of people of the organization. Each stop shows the work of
/// one person by ownership on each board. There is no Unclaimed stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StandupGroup {
    /// Group id (UUID).
    pub id: String,
    /// Unique in the organization, with no regard to case.
    pub name: String,
    /// The person who created the group. Only this person can delete it.
    pub created_by: StandupGroupPerson,
    /// `true` when the caller created the group, so the caller can delete
    /// it.
    pub can_delete: bool,
    /// The people of the round, by display name.
    pub people: Vec<StandupGroupPerson>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// `GET /api/standup-groups` response: each group, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StandupGroupList {
    pub items: Vec<StandupGroup>,
}

/// `POST /api/standup-groups` body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStandupGroupRequest {
    /// 1 to 100 characters. Unique in the organization, with no regard to
    /// case.
    pub name: String,
    /// The people of the group: the user id (UUID) or the user name of
    /// each. Each must be a person of the organization. Default: none.
    #[serde(default)]
    pub people: Vec<String>,
}

/// `PATCH /api/standup-groups/{id}` body: send `name`, `people`, or both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStandupGroupRequest {
    /// The new name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// All the people of the group after the change: the user id (UUID) or
    /// the user name of each. A person who is not in the list leaves the
    /// group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub people: Option<Vec<String>>,
}
