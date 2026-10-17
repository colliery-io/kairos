//! The owner of a strategy or an initiative on the server (KAIROS-T-0411,
//! KAIROS-A-0025 decisions 2 to 7). The state is in
//! `kairos_db::item_owners`; this module has the RULES of who may change
//! it, and what REST and MCP share.
//!
//! # The rules
//!
//! Only a strategy or an initiative has an owner (422 `RELATIONSHIP_RULE`
//! for each other type). Each change needs a live item.
//!
//! - CLAIM ([`claim`]): a person who is a member of the board of the item
//!   (any grant on it, `kairos_db::abac::is_board_member`), or an org
//!   admin, takes an item that has NO owner. A service account cannot be
//!   an owner. An item with an owner is 409 `CONFLICT`, with the name of
//!   the owner.
//! - HAND OFF ([`hand_off`]): the owner gives the item to a person who is
//!   a member of the board of the item (any grant on it). An org admin can
//!   do it for each item, also to SET the owner of an item with no owner,
//!   with the same rule for the new owner. Nobody else. A person who is not
//!   a member of the board is 422 `NOT_BOARD_MEMBER`.
//! - RELEASE ([`release`]): the owner gives up the item, or an org admin
//!   removes the owner. The item then has no owner. Nobody else.
//!
//! The owner gives no right: nothing in the ABAC check reads it. And it
//! gives no warning on a write, as the claim of a task does
//! (`crate::claims::note`): a member of the board edits and moves each
//! item on it.
//!
//! # An owner who leaves
//!
//! `DELETE /api/members/{user_id}` and the SCIM deprovision (PATCH or PUT
//! `active: false`, DELETE) call [`drop_on_leave`] in the transaction that
//! removes the membership: the items of the person have no owner from that
//! moment, and a `release` row with `reason:left` says why. The claims of
//! the person on tasks end in the same way (KAIROS-T-0531): each task stays
//! in its column, free, and the team of its board is told
//! (`kairos_db::task_claims::drop_claims_of_user`).

use std::collections::HashMap;

use diesel::pg::PgConnection;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::item_owners::{self, ItemOwner, OwnerError};
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;

/// A wire type that carries the owner: a strategy or an initiative.
pub trait OwnedDto {
    /// The id of the item (UUID text).
    fn item_id(&self) -> &str;
    /// Put the owner on the DTO.
    fn set_owner(&mut self, owner: Option<dto::ItemOwner>);
}

impl OwnedDto for dto::Strategy {
    fn item_id(&self) -> &str {
        &self.id
    }
    fn set_owner(&mut self, owner: Option<dto::ItemOwner>) {
        self.owner = owner;
    }
}

impl OwnedDto for dto::Initiative {
    fn item_id(&self) -> &str {
        &self.id
    }
    fn set_owner(&mut self, owner: Option<dto::ItemOwner>) {
        self.owner = owner;
    }
}

pub(crate) fn owner_dto(owner: &ItemOwner, names: &HashMap<Uuid, String>) -> dto::ItemOwner {
    dto::ItemOwner {
        user_id: owner.user_id.to_string(),
        display_name: names
            .get(&owner.user_id)
            .cloned()
            .unwrap_or_else(|| owner.user_id.to_string()),
        claimed_at: owner.claimed_at.to_rfc3339(),
    }
}

/// The owners of the items, as wire types, by item. Two queries.
pub fn owner_dtos(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
) -> Result<HashMap<Uuid, dto::ItemOwner>, diesel::result::Error> {
    let owners = item_owners::owners_of(conn, item_ids)?;
    if owners.is_empty() {
        return Ok(HashMap::new());
    }
    let users: Vec<Uuid> = owners.values().map(|o| o.user_id).collect();
    let names = kairos_db::task_claims::display_names(conn, &users)?;
    Ok(owners
        .into_iter()
        .map(|(id, owner)| (id, owner_dto(&owner, &names)))
        .collect())
}

/// Put the owners of the items on their DTOs. Two queries for the list.
pub fn attach_owners<T: OwnedDto>(
    conn: &mut PgConnection,
    items: &mut [T],
) -> Result<(), ApiError> {
    let ids: Vec<Uuid> = items
        .iter()
        .filter_map(|i| i.item_id().parse().ok())
        .collect();
    let mut owners = owner_dtos(conn, &ids).map_err(ApiError::internal)?;
    for item in items {
        let owner = item
            .item_id()
            .parse::<Uuid>()
            .ok()
            .and_then(|id| owners.remove(&id));
        item.set_owner(owner);
    }
    Ok(())
}

/// Put the owner of the item on its DTO.
pub fn attach_owner<T: OwnedDto>(conn: &mut PgConnection, mut item: T) -> Result<T, ApiError> {
    attach_owners(conn, std::slice::from_mut(&mut item))?;
    Ok(item)
}

/// The owner of one item, as the wire type.
pub fn owner_of(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<dto::ItemOwner>, ApiError> {
    Ok(owner_dtos(conn, &[item_id])
        .map_err(ApiError::internal)?
        .remove(&item_id))
}

/// The names of the owners of the items, by item: for the cards of
/// `board_items`.
pub fn owner_names(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, ApiError> {
    Ok(owner_dtos(conn, item_ids)
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|(id, owner)| (id, owner.display_name))
        .collect())
}

/// The line of `get_item` for the owner of a strategy or an initiative:
/// `- owner: Alice since 2026-10-12T10:00:00Z`, or `- owner: none ...`.
/// Empty for each other type.
pub fn owner_text(
    conn: &mut PgConnection,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<String, ApiError> {
    if !item_owners::is_subject(item_type) {
        return Ok(String::new());
    }
    Ok(match owner_of(conn, item_id)? {
        Some(owner) => format!(
            "- owner: {} since {}\n",
            owner.display_name,
            owner
                .claimed_at
                .parse::<chrono::DateTime<chrono::Utc>>()
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or(owner.claimed_at)
        ),
        None => "- owner: none. A member of the board can claim it (claim_item).\n".to_string(),
    })
}

/// Only a strategy or an initiative has an owner.
pub fn require_subject(item_type: ItemType, short_code: &str) -> Result<(), ApiError> {
    if item_owners::is_subject(item_type) {
        return Ok(());
    }
    let task = if item_type == ItemType::Task {
        " A task has a claim: claim_item on a task in Active takes it, and a move to Active \
         gives it."
    } else {
        ""
    };
    Err(ApiError::unprocessable(
        "RELATIONSHIP_RULE",
        format!(
            "{short_code} is a {item_type}. Only a strategy or an initiative has an owner.{task}"
        ),
    )
    .with_details(json!({
        "source_type": item_type.entity_type(),
        "allowed_source_types": ["strategy", "initiative"],
    })))
}

/// [`OwnerError`] → HTTP.
pub fn map_owner_error(e: OwnerError) -> ApiError {
    match e {
        OwnerError::ItemNotFound(_) => ApiError::not_found(e.to_string()),
        OwnerError::HasOwner { .. } | OwnerError::AlreadyOwner { .. } => {
            ApiError::conflict(e.to_string())
        }
        OwnerError::NoOwner { .. } => ApiError::unprocessable("NO_OWNER", e.to_string()),
        OwnerError::NotAPerson(_) => ApiError::validation(e.to_string()),
        OwnerError::Database(db) => ApiError::internal(db),
    }
}

fn is_org_admin(conn: &mut PgConnection, slug: &str, user: Uuid) -> Result<bool, ApiError> {
    kairos_db::abac::is_org_admin(conn, slug, user).map_err(ApiError::internal)
}

/// The board of the item, and whether `user` is a member of it (any
/// grant on it).
fn board_membership(
    conn: &mut PgConnection,
    item_id: Uuid,
    user: Uuid,
) -> Result<(Option<Uuid>, bool), ApiError> {
    let board =
        kairos_db::abac::resolve_authorization_board(conn, item_id).map_err(ApiError::internal)?;
    let member = match board {
        Some(board_id) => {
            kairos_db::abac::is_board_member(conn, board_id, user).map_err(ApiError::internal)?
        }
        None => false,
    };
    Ok((board, member))
}

/// CLAIM (module docs): `user` takes the item, which has no owner.
pub fn claim(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    short_code: &str,
) -> Result<dto::ItemOwner, ApiError> {
    require_subject(item_type, short_code)?;
    if !kairos_db::task_claims::is_person(conn, user).map_err(ApiError::internal)? {
        return Err(ApiError::forbidden(format!(
            "A service account cannot be the owner of {short_code}. Only a person can claim it."
        )));
    }
    let (board, member) = board_membership(conn, item_id, user)?;
    if !member && !is_org_admin(conn, slug, user)? {
        return Err(ApiError::forbidden(format!(
            "To claim {short_code}, you must be a member of its board: you need a grant on the \
             board. An organization admin can also do it."
        ))
        .with_details(json!({ "board_id": board })));
    }
    item_owners::claim(conn, item_id, user).map_err(map_owner_error)?;
    owner_of(conn, item_id)?.ok_or_else(|| ApiError::internal("the owner vanished"))
}

/// HAND OFF (module docs): the item gets the person that `to` names (a
/// user id, an email or a user name), who must be a member of the board.
pub fn hand_off(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    short_code: &str,
    to: &str,
) -> Result<dto::ItemOwner, ApiError> {
    require_subject(item_type, short_code)?;
    let owner = item_owners::owner_of(conn, item_id).map_err(ApiError::internal)?;
    let is_owner = owner.as_ref().is_some_and(|o| o.user_id == user);
    if !is_owner && !is_org_admin(conn, slug, user)? {
        let why = match owner {
            Some(_) => format!("Only the owner of {short_code} can hand it off."),
            None => format!(
                "{short_code} has no owner. A member of its board can claim it (claim_item)."
            ),
        };
        return Err(ApiError::forbidden(format!(
            "{why} An organization admin can also set the owner."
        )));
    }
    let to_name = to;
    let to = crate::claims::resolve_person(conn, slug, to, user)?;
    let (board, member) = board_membership(conn, item_id, to)?;
    if !member {
        return Err(ApiError::unprocessable(
            "NOT_BOARD_MEMBER",
            format!(
                "{to_name} is not a member of the board of {short_code}. The owner must have a \
                 grant on the board. Give the person a grant first, or send a member of the board."
            ),
        )
        .with_details(json!({ "board_id": board, "user_id": to })));
    }
    item_owners::hand_off(conn, item_id, to, user).map_err(map_owner_error)?;
    owner_of(conn, item_id)?.ok_or_else(|| ApiError::internal("the owner vanished"))
}

/// RELEASE (module docs): the owner gives up the item, or an org admin
/// removes the owner. Returns the owner that the item had.
pub fn release(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    short_code: &str,
) -> Result<ItemOwner, ApiError> {
    require_subject(item_type, short_code)?;
    let owner = item_owners::owner_of(conn, item_id).map_err(ApiError::internal)?;
    let is_owner = owner.as_ref().is_some_and(|o| o.user_id == user);
    if owner.is_some() && !is_owner && !is_org_admin(conn, slug, user)? {
        return Err(ApiError::forbidden(format!(
            "Only the owner of {short_code} can release it. An organization admin can also \
             clear the owner."
        )));
    }
    // KAIROS-T-0643: an initiative in Active or Monitoring keeps an owner
    // while the board policy owner_gate is on. A hand-off changes it.
    if owner.is_some()
        && let Some(column) =
            kairos_db::board_policy::release_blocked(conn, item_id).map_err(ApiError::internal)?
    {
        return Err(ApiError::unprocessable(
            "OWNER_REQUIRED",
            format!(
                "{short_code} is in {column}, and an initiative in Active or Monitoring keeps \
                 an owner. Hand it to a member of the board: hand_off_item."
            ),
        )
        .with_details(
            json!({ "column": column, "policy": "owner_gate", "fix": "hand_off_item" }),
        ));
    }
    item_owners::clear(conn, item_id, user).map_err(map_owner_error)
}

/// The person `user_id` leaves the organization (module docs): the items
/// that the person owns have no owner, and the tasks that the person has
/// the claim on are free (KAIROS-T-0531). Call it in the transaction that
/// removes the membership. Returns the items that had the person as owner.
pub fn drop_on_leave(
    conn: &mut PgConnection,
    user_id: Uuid,
    actor: Uuid,
) -> Result<Vec<Uuid>, diesel::result::Error> {
    let owned = item_owners::drop_owner_of_user(conn, user_id, actor)?;
    kairos_db::task_claims::drop_claims_of_user(conn, user_id, actor)?;
    Ok(owned)
}
