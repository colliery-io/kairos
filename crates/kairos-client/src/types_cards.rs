//! Cards (KAIROS-T-0602): the items of a board and the hits of a search,
//! without the markdown content.
//!
//! A card has the fields of its item DTO ([`crate::types`]), in the same
//! order. Three fields are optional on a card: `content`, `created_by` and
//! `updated_by`. `GET /api/boards/{id}/items` sends them only with
//! `?fields=full`, so that response has the shape that the route had
//! before cards. `POST /api/search` never sends them. A search hit has a
//! `snippet` in their place. The item routes (`GET /api/tasks/{code}` and
//! the others) give the content.
//!
//! The defaults are for the client only: a response of an older server
//! has `content` and no `snippet`, and a card decodes from it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{ItemOwner, TaskCancellation, TaskClaim, TaskState};
use crate::types_repositories::{Impact, RepositoryRef};

/// The `fields` of `GET /api/boards/{id}/items` (KAIROS-T-0602).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardFields {
    /// The cards without `content`, `created_by` and `updated_by`. The
    /// default.
    #[default]
    Card,
    /// The cards with `content`, `created_by` and `updated_by`: the shape
    /// of the route before cards.
    Full,
}

/// A strategy on a board or in a search (KAIROS-T-0602).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StrategyCard {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-S-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content. Only with `fields=full` on the board route. A card
    /// has no notes (KAIROS-T-0630): the read of the item gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Board the strategy sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    pub hypothesis: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID). Only with `fields=full` on the board route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Last editor user id (UUID). Only with `fields=full` on the board
    /// route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live
    /// (KAIROS-A-0020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The owner of the item (KAIROS-T-0411). Absent when the item has no
    /// owner. Present on the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ItemOwner>,
    /// When the card entered its column, RFC 3339: its last move, or its
    /// creation (KAIROS-T-0649). Present on the board route; absent on a
    /// search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649). Present on
    /// the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_count: Option<i64>,
    /// About 200 characters of the content, near the first match of `q`.
    /// Only on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// An initiative on a board or in a search (KAIROS-T-0602).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct InitiativeCard {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-I-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content. Only with `fields=full` on the board route. A card
    /// has no notes (KAIROS-T-0630): the read of the item gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Board the initiative sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    /// T-shirt sizing (`xs|s|m|l|xl`).
    pub complexity: Option<String>,
    /// Whether this initiative is a bucket.
    pub is_bucket: bool,
    /// Bucket kind (`tech_debt|bug|ad_hoc`), set iff `is_bucket`.
    pub bucket_type: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID). Only with `fields=full` on the board route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Last editor user id (UUID). Only with `fields=full` on the board
    /// route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live
    /// (KAIROS-A-0020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The owner of the item (KAIROS-T-0411). Absent when the item has no
    /// owner. Present on the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ItemOwner>,
    /// When the card entered its column, RFC 3339: its last move, or its
    /// creation (KAIROS-T-0649). Present on the board route; absent on a
    /// search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649). Present on
    /// the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_count: Option<i64>,
    /// The cancel mark of the initiative (KAIROS-T-0678). Absent when it
    /// is not cancelled. Present on the board route; absent on a search
    /// hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<TaskCancellation>,
    /// About 200 characters of the content, near the first match of `q`.
    /// Only on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// A task on a board or in a search (KAIROS-T-0602).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskCard {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-T-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content. Only with `fields=full` on the board route. A card
    /// has no notes (KAIROS-T-0630): the read of the item gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Board the task sits on (UUID).
    pub board_id: String,
    /// Current column (UUID).
    pub column_id: String,
    /// `task|bug|tech_debt|support`.
    pub task_type: String,
    /// Planned/Support lane (`planned|support`, KAIROS-T-0077).
    pub work_class: String,
    /// Owning team (UUID), if assigned.
    pub team_id: Option<String>,
    /// The repository this task is issued against (UUID), if bound.
    #[serde(default)]
    pub repository_id: Option<String>,
    /// The bound repository, embedded (KAIROS-T-0104).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryRef>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID). Only with `fields=full` on the board route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Last editor user id (UUID). Only with `fields=full` on the board
    /// route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live
    /// (KAIROS-A-0020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// The claim of the task (KAIROS-T-0359). Absent when the task has no
    /// claim. Present on the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<TaskClaim>,
    /// The cancel mark of the task (KAIROS-T-0362). Absent when the task
    /// is not cancelled. Present on the board route; absent on a search
    /// hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<TaskCancellation>,
    /// The live state of the task (KAIROS-T-0506). Absent when the task
    /// has no state. Present on the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TaskState>,
    /// When the card entered its column, RFC 3339: its last move, or its
    /// creation (KAIROS-T-0649). Present on the board route; absent on a
    /// search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649). Present on
    /// the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_count: Option<i64>,
    /// About 200 characters of the content, near the first match of `q`.
    /// Only on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// A document in a search (KAIROS-T-0602). A document is on no board
/// column, so only search gives document cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DocumentCard {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-D-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// The owner board (UUID).
    pub board_id: String,
    /// The repositories that the document impacts (COLLIERY-T-0269).
    #[serde(default)]
    pub impacts: Vec<Impact>,
    /// Template the document was stamped from (UUID), if any.
    pub template_id: Option<String>,
    /// Editorial lifecycle: `draft|review|published|archived`.
    pub lifecycle: String,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live
    /// (KAIROS-A-0020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// About 200 characters of the content, near the first match of `q`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// An ADR on a board or in a search (KAIROS-T-0602).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AdrCard {
    /// Entity id (UUID).
    pub id: String,
    /// Tenant-scoped short code (`{PREFIX}-A-{NNNN}`).
    pub short_code: String,
    pub title: String,
    /// Markdown content. Only with `fields=full` on the board route. A card
    /// has no notes (KAIROS-T-0630): the read of the item gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// ADR board (UUID), if placed on one.
    pub board_id: Option<String>,
    /// Current column (UUID), if placed on a board.
    pub column_id: Option<String>,
    /// The repositories that the ADR impacts (COLLIERY-T-0269).
    #[serde(default)]
    pub impacts: Vec<Impact>,
    pub decision_maker: Option<String>,
    /// `YYYY-MM-DD`.
    pub decision_date: Option<String>,
    /// Optimistic-concurrency version (KAIROS-A-0004).
    pub version: i32,
    /// Creator user id (UUID). Only with `fields=full` on the board route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Last editor user id (UUID). Only with `fields=full` on the board
    /// route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// When this work was put away, RFC 3339; absent while it is live
    /// (KAIROS-A-0020).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// When the card entered its column, RFC 3339: its last move, or its
    /// creation (KAIROS-T-0649). Present on the board route; absent on a
    /// search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entered_column_at: Option<String>,
    /// The number of the notes of the item (KAIROS-T-0649). Present on
    /// the board route; absent on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_count: Option<i64>,
    /// About 200 characters of the content, near the first match of `q`.
    /// Only on a search hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card with no `content` decodes, and it serializes with no
    /// `content`, `created_by`, `updated_by` or `snippet`.
    #[test]
    fn a_card_has_no_content_by_default() {
        let card: TaskCard = serde_json::from_value(serde_json::json!({
            "id": "i", "short_code": "ACME-T-0001", "title": "t",
            "board_id": "b", "column_id": "c", "task_type": "task",
            "work_class": "planned", "team_id": null, "version": 1,
            "created_at": "2026-10-17T00:00:00Z",
            "updated_at": "2026-10-17T00:00:00Z"
        }))
        .expect("a card decodes");
        let value = serde_json::to_value(&card).expect("serializes");
        for field in ["content", "created_by", "updated_by", "snippet"] {
            assert!(value.get(field).is_none(), "{field} is absent: {value}");
        }
    }

    /// `fields` takes `card` and `full`, and refuses an other value.
    #[test]
    fn card_fields_values() {
        assert_eq!(
            serde_json::from_value::<CardFields>(serde_json::json!("full")).expect("full"),
            CardFields::Full
        );
        assert_eq!(
            serde_json::from_value::<CardFields>(serde_json::json!("card")).expect("card"),
            CardFields::Card
        );
        assert!(serde_json::from_value::<CardFields>(serde_json::json!("all")).is_err());
    }
}
