//! `/api/members/{user_id}/grants` (KAIROS-T-0413, KAIROS-A-0025): the
//! grants of ONE person on many boards, and the teams of the person.
//!
//! - `GET` lists the grants of the person on each live board that the
//!   caller administers. An organization admin administers each board. A
//!   different caller administers a board where it holds
//!   `administer_members` (or `manage_structure`, which covers it,
//!   `kairos_db::abac::authorize`). A caller that administers no board is
//!   refused (403).
//! - `PATCH` applies a set of grants and revokes over many boards in ONE
//!   transaction. A board that the caller cannot administer refuses the
//!   whole change, and the refusal names the board. Each grant and each
//!   revoke is the single grant or revoke of `kairos_db::abac`, with its
//!   activity row; nothing is written when the change is refused.
//!
//! A grant can give only a capability that applies at the level of the
//! board ([`kairos_core::abac::capabilities_of_level`]). A revoke can remove
//! any stored grant, a glob too. There are no role presets: each change is
//! a grant row.
//!
//! The teams of the person are in the answer when the caller can change
//! team members (the structure gate, [`super::super::require_structure`]).
//! The team member writes stay on `/api/teams/{id}/members`.

use std::collections::BTreeSet;

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_org as dto;
use kairos_core::abac::{ADMINISTER_MEMBERS, BOARD_LEVELS, capabilities_of_level};
use kairos_db::abac;
use kairos_db::models::boards::Board;
use serde_json::json;
use uuid::Uuid;

use super::super::{map_abac_error, parse_uuid, require_structure};
use super::{load_board, map_grant_error, require_user_exists, run_in_transaction};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/members/{user_id}/grants",
        get(get_user_grants).patch(change_user_grants),
    )
}

/// The live boards, by level ([`BOARD_LEVELS`]) and then by name.
fn live_boards(conn: &mut PgConnection) -> Result<Vec<Board>, ApiError> {
    use kairos_db::schema::boards::dsl;
    let mut boards: Vec<Board> = dsl::boards
        .filter(dsl::deleted_at.is_null())
        .select(Board::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    let rank = |board: &Board| {
        let level = board.board_level.to_string();
        BOARD_LEVELS
            .iter()
            .position(|known| *known == level)
            .unwrap_or(BOARD_LEVELS.len())
    };
    boards.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.slug.cmp(&b.slug))
    });
    Ok(boards)
}

/// May `caller` administer the members of `board`?
fn administers(
    conn: &mut PgConnection,
    slug: &str,
    caller: Uuid,
    board: &Board,
) -> Result<bool, ApiError> {
    abac::authorize(conn, slug, board.id, caller, ADMINISTER_MEMBERS).map_err(map_abac_error)
}

/// The 403 of a caller that administers no board.
fn administers_no_board() -> ApiError {
    ApiError::forbidden(format!(
        "This action requires the organization admin role, or the capability \
         {ADMINISTER_MEMBERS:?} on a board."
    ))
    .with_details(json!({
        "required_capability": ADMINISTER_MEMBERS,
        "board_id": null,
    }))
}

/// The 403 of a board that the caller cannot administer. It names the
/// board.
fn board_not_administered(board: &Board) -> ApiError {
    ApiError::forbidden(format!(
        "You cannot change the grants on the board {:?} ({}). This action requires the \
         capability {ADMINISTER_MEMBERS:?} on that board. Kairos changed no grant.",
        board.slug, board.id
    ))
    .with_details(json!({
        "required_capability": ADMINISTER_MEMBERS,
        "board_id": board.id,
        "board_slug": board.slug,
    }))
}

/// 404 unless `user_id` is a member of the organization.
fn require_org_member(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<kairos_db::models::User, ApiError> {
    use kairos_db::schema::organization_members::dsl;
    let member: bool = diesel::select(diesel::dsl::exists(
        dsl::organization_members
            .filter(dsl::organization_id.eq(org_id))
            .filter(dsl::user_id.eq(user_id)),
    ))
    .get_result(conn)
    .map_err(ApiError::internal)?;
    if !member {
        return Err(ApiError::not_found(format!(
            "The user {user_id} is not a member of this organization."
        ))
        .with_details(json!({ "field": "user" })));
    }
    require_user_exists(conn, user_id)
}

/// The view of the grants of `target` for `caller`: the boards that the
/// caller administers, and the teams when the caller may change them.
///
/// `refuse_empty`: refuse a caller that administers no board. The answer
/// of a change does not refuse: a caller can revoke its own last
/// `administer_members`, and then it administers no board.
fn user_grants_view(
    conn: &mut PgConnection,
    slug: &str,
    org_id: Uuid,
    caller: Uuid,
    target: Uuid,
    refuse_empty: bool,
) -> Result<dto::UserGrants, ApiError> {
    use kairos_db::schema::board_member_capabilities::dsl;
    let user = require_org_member(conn, org_id, target)?;
    // KAIROS-T-0536: with an agent key that has a scope, each board goes
    // through `administers`, which applies the scope.
    let is_admin = abac::is_org_admin(conn, slug, caller).map_err(map_abac_error)?
        && crate::blocking::key_scope_of(caller).is_none();
    let mut boards = Vec::new();
    for board in live_boards(conn)? {
        if is_admin || administers(conn, slug, caller, &board)? {
            boards.push(board);
        }
    }
    if refuse_empty && boards.is_empty() && !is_admin {
        return Err(administers_no_board());
    }
    let board_ids: Vec<Uuid> = boards.iter().map(|b| b.id).collect();
    let grants: Vec<(Uuid, String)> = dsl::board_member_capabilities
        .filter(dsl::user_id.eq(target))
        .filter(dsl::board_id.eq_any(&board_ids))
        .order(dsl::capability.asc())
        .select((dsl::board_id, dsl::capability))
        .load(conn)
        .map_err(ApiError::internal)?;
    let rows = boards
        .into_iter()
        .map(|board| {
            let level = board.board_level.to_string();
            dto::UserBoardGrants {
                board_id: board.id.to_string(),
                board_slug: board.slug,
                board_name: board.name,
                applicable: capabilities_of_level(&level)
                    .iter()
                    .map(|c| (*c).to_string())
                    .collect(),
                capabilities: grants
                    .iter()
                    .filter(|(b, _)| *b == board.id)
                    .map(|(_, c)| c.clone())
                    .collect(),
                board_level: level,
            }
        })
        .collect();
    let teams = match require_structure(conn, slug, caller) {
        Ok(()) => Some(teams_of(conn, target)?),
        Err(_) => None,
    };
    Ok(dto::UserGrants {
        user_id: user.id.to_string(),
        email: user.email,
        display_name: user.display_name,
        boards: rows,
        teams,
    })
}

/// The live teams of `user_id`, by name.
fn teams_of(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<dto::UserTeam>, ApiError> {
    use kairos_db::schema::{team_members, teams};
    let rows: Vec<(Uuid, String, String)> = teams::table
        .inner_join(team_members::table.on(team_members::team_id.eq(teams::id)))
        .filter(team_members::user_id.eq(user_id))
        .filter(teams::deleted_at.is_null())
        .order((teams::name.asc(), teams::slug.asc()))
        .select((teams::id, teams::slug, teams::name))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|(id, slug, name)| dto::UserTeam {
            team_id: id.to_string(),
            slug,
            name,
        })
        .collect())
}

/// The grants of one person on the boards that the caller administers
/// (KAIROS-T-0413). The answer also gives the teams of the person.
///
/// An organization admin sees each live board. A holder of
/// `administer_members` sees only the boards where it holds it. `teams` is
/// `null` unless the caller can change team members.
#[utoipa::path(
    get,
    path = "/api/members/{user_id}/grants",
    tag = "board-members",
    params(("user_id" = String, Path, description = "User id (UUID)")),
    responses(
        (status = 200, description = "The grants of the person on each board that the caller administers", body = dto::UserGrants),
        (status = 403, description = "The caller administers no board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "The user is not a member of the organization", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_user_grants(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
) -> Result<Json<dto::UserGrants>, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let caller = auth.user_id;
    let (slug, org_id) = (tenant.slug.clone(), tenant.org_id);
    let view = state
        .blocking
        .run(&tenant.slug, move |conn| {
            user_grants_view(conn, &slug, org_id, caller, target, true)
        })
        .await?;
    Ok(Json(view))
}

/// One checked change: the board, and what to grant and revoke on it.
struct CheckedChange {
    board: Board,
    grant: Vec<String>,
    revoke: Vec<String>,
}

/// 422 `VALIDATION` for a mistake in the change of one board. The details
/// name the field (`changes[i].grant` or `changes[i].revoke`) and the board.
fn change_refusal(
    board: &Board,
    field: String,
    message: String,
    extra: serde_json::Value,
) -> ApiError {
    let mut error = ApiError::validation(message).with_details(json!({
        "field": field,
        "board_id": board.id,
        "board_slug": board.slug,
    }));
    if let (Some(details), Some(extra)) = (error.details.as_object_mut(), extra.as_object()) {
        details.extend(extra.clone());
    }
    error
}

/// Check the form of one change against its board: no capability twice,
/// no capability in `grant` and in `revoke`, each grant applies at the
/// level of the board, each revoke is a name of the vocabulary.
fn check_change(
    board: &Board,
    index: usize,
    grant: &[String],
    revoke: &[String],
) -> Result<(), ApiError> {
    let grant_field = format!("changes[{index}].grant");
    let revoke_field = format!("changes[{index}].revoke");
    if grant.is_empty() && revoke.is_empty() {
        return Err(change_refusal(
            board,
            grant_field.clone(),
            format!(
                "The change of the board {:?} has no capability in \"grant\" or in \
                 \"revoke\". Send one capability or more, or remove the change.",
                board.slug
            ),
            json!({ "fields": [grant_field, revoke_field] }),
        ));
    }
    let level = board.board_level.to_string();
    let applicable = capabilities_of_level(&level);
    let mut seen = BTreeSet::new();
    for (position, capability) in grant.iter().chain(revoke).enumerate() {
        if !seen.insert(capability.as_str()) {
            let field = if position < grant.len() {
                grant_field
            } else {
                revoke_field
            };
            return Err(change_refusal(
                board,
                field,
                format!(
                    "The change of the board {:?} has the capability {capability:?} two \
                     times. Send each capability one time, in \"grant\" or in \"revoke\".",
                    board.slug
                ),
                json!({ "capability": capability }),
            ));
        }
    }
    for capability in grant {
        if !applicable.contains(&capability.as_str()) {
            return Err(change_refusal(
                board,
                grant_field,
                format!(
                    "The capability {capability:?} does not apply on the board {:?}, a board \
                     of the level {level:?}. On that board, you can grant: {}.",
                    board.slug,
                    applicable.join(", ")
                ),
                json!({ "capability": capability, "applicable": applicable }),
            ));
        }
    }
    let known = |c: &str| {
        kairos_core::abac::CAPABILITIES.contains(&c) || kairos_core::abac::GLOBS.contains(&c)
    };
    for capability in revoke {
        if !known(capability) {
            return Err(change_refusal(
                board,
                revoke_field,
                format!(
                    "{capability:?} is not a capability. Kairos cannot revoke it on the board \
                     {:?}.",
                    board.slug
                ),
                json!({ "capability": capability }),
            ));
        }
    }
    Ok(())
}

/// Add the board to the refusal of a grant or a revoke (409 or 404).
fn name_board(error: ApiError, board: &Board) -> ApiError {
    let message = format!(
        "On the board {:?}: {} Kairos changed no grant.",
        board.slug, error.message
    );
    let mut changed = error;
    changed.message = message;
    // KAIROS-T-0698: keep the keys of the refusal (its field), and add the board.
    if !changed.details.is_object() {
        changed.details = json!({});
    }
    if let Some(details) = changed.details.as_object_mut() {
        details.insert("board_id".to_owned(), json!(board.id));
        details.insert("board_slug".to_owned(), json!(board.slug));
    }
    changed
}

/// Change the grants of one person on one or more boards, in ONE
/// transaction (KAIROS-T-0413).
///
/// Each entry of `changes` names a board. It gives the capabilities to
/// grant and the stored grants to revoke.
///
/// The caller must administer each board. A board that the caller cannot
/// administer refuses all of the change (403). The refusal names the board.
/// A grant must apply at the level of the board (422).
///
/// A grant that the person has already refuses all of the change (409).
/// A revoke of a grant that the person does not have refuses it too (404).
/// Each grant and each revoke writes its activity row. The answer is the
/// new view, as `GET` gives it.
#[utoipa::path(
    patch,
    path = "/api/members/{user_id}/grants",
    tag = "board-members",
    params(("user_id" = String, Path, description = "User id (UUID)")),
    request_body = dto::ChangeUserGrantsRequest,
    responses(
        (status = 200, description = "All the changes were applied; the new grants", body = dto::UserGrants),
        (status = 403, description = "The caller cannot administer a board of the change; no change was applied", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or user, or a revoke of a grant that the person does not have", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A grant that the person has already", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No change, a board two times, or a capability that does not apply at the level of the board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn change_user_grants(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
    ApiJson(body): ApiJson<dto::ChangeUserGrantsRequest>,
) -> Result<Json<dto::UserGrants>, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    if body.changes.is_empty() {
        return Err(ApiError::validation(
            "The list \"changes\" is empty. Send one change or more.",
        )
        .with_details(json!({ "field": "changes" })));
    }
    let mut board_ids = Vec::with_capacity(body.changes.len());
    for change in &body.changes {
        let board_id = parse_uuid(&change.board_id, "board_id")?;
        if board_ids.contains(&board_id) {
            return Err(ApiError::validation(format!(
                "The list \"changes\" has the board {board_id} two times. Send one change \
                 for each board."
            ))
            .with_details(json!({ "field": "changes", "board_id": board_id })));
        }
        board_ids.push(board_id);
    }
    let caller = auth.user_id;
    let (slug, org_id) = (tenant.slug.clone(), tenant.org_id);
    let view = state
        .blocking
        .run(&tenant.slug, move |conn| {
            run_in_transaction(conn, |conn| {
                require_org_member(conn, org_id, target)?;
                // Check each board before the first write.
                let mut checked = Vec::with_capacity(body.changes.len());
                for (index, (change, board_id)) in
                    body.changes.into_iter().zip(board_ids).enumerate()
                {
                    let board = load_board(conn, board_id)?;
                    if !administers(conn, &slug, caller, &board)? {
                        return Err(board_not_administered(&board));
                    }
                    check_change(&board, index, &change.grant, &change.revoke)?;
                    checked.push(CheckedChange {
                        board,
                        grant: change.grant,
                        revoke: change.revoke,
                    });
                }
                for change in &checked {
                    for capability in &change.revoke {
                        abac::revoke_capability(conn, change.board.id, target, capability, caller)
                            .map_err(|e| name_board(map_grant_error(e), &change.board))?;
                    }
                    for capability in &change.grant {
                        abac::grant_capability(conn, change.board.id, target, capability, caller)
                            .map_err(|e| name_board(map_grant_error(e), &change.board))?;
                    }
                }
                user_grants_view(conn, &slug, org_id, caller, target, false)
            })
        })
        .await?;
    Ok(Json(view))
}
