//! `/api/{entity_type}/{short_code}/teams` (KAIROS-T-0321, COLLIERY-I-0602):
//! the teams of an initiative or a strategy.
//!
//! The teams of an item are the teams that it gets FROM ITS TASKS, plus
//! the teams that are SET ON IT BY HAND ([`kairos_db::item_teams`]). A
//! team set by hand is the link `impacts` from the item to the team. A
//! team gives no right on the item, and the link takes no right on the
//! team.
//!
//! # The rules
//!
//! The rules are asked in this order, and [`add`] and [`remove`] are the
//! ONE implementation for REST and for the MCP tools `set_team` and
//! `clear_team`.
//!
//! 1. WHICH SUBJECT. Only an initiative or a strategy: 422
//!    `RELATIONSHIP_RULE` for each other type, also for a read.
//! 2. WHO. The edit rule ([`crate::api::require_item_edit`]): the
//!    principal created the item, or holds `manage_<type>` on its board,
//!    or is an organization admin. The refusal is 403 `FORBIDDEN`.
//! 3. WHICH TEAM. A new link needs a live team of the tenant: 422
//!    `VALIDATION` names the one that is not. A remove finds the team
//!    among the hand-set teams of the item, so a link to an archived team
//!    can be removed.
//! 4. The link that is there already is 422 `ALREADY_LINKED`. The remove
//!    of a link that is not there is 404 `NOT_FOUND`. A team that the item
//!    gets from its tasks is not a link: the remove of it is 404, and the
//!    text says why.
//!
//! Reads are open tenant-wide.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use kairos_client::types_org as dto;
use kairos_core::short_code::ItemType;
use kairos_db::board_items::TeamFilter;
use kairos_db::item_teams::{self, ItemTeam, ItemTeamError};
use serde_json::json;
use uuid::Uuid;

use super::resolve_family_item;
use crate::api::{Liveness, manage_capability_of, missing_item_edit, short_code_of};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/{entity_type}/{short_code}/teams",
            get(get_teams).post(set_team),
        )
        .route(
            "/api/{entity_type}/{short_code}/teams/{team}",
            delete(clear_team),
        )
}

/// One team, as the wire type.
pub(crate) fn team_dto(team: ItemTeam) -> dto::ItemTeam {
    dto::ItemTeam {
        slug: team.slug,
        name: team.name,
        from_tasks: team.from_tasks,
        set_by_hand: team.set_by_hand,
    }
}

/// Rule 1: only an initiative or a strategy has teams.
pub(crate) fn require_subject(item_type: ItemType, short_code: &str) -> Result<(), ApiError> {
    if item_teams::is_subject(item_type) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "RELATIONSHIP_RULE",
        format!(
            "{short_code} is a {item_type}. Only an initiative or a strategy can have a team. \
             A task gets its team from its board."
        ),
    )
    .with_details(json!({
        "relationship": item_teams::TARGET_TEAM,
        "source_type": item_type.entity_type(),
        "allowed_source_types": ["initiative", "strategy"],
    })))
}

/// Rule 2: the edit rule, with a text that says what the link needs.
fn require_edit(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
) -> Result<(), ApiError> {
    let Some(board_id) = missing_item_edit(conn, slug, user, item_id, item_type)? else {
        return Ok(());
    };
    let short_code = short_code_of(conn, item_id)?;
    let capability = manage_capability_of(item_type);
    let need = match board_id {
        Some(_) => format!("You need {capability:?} on the board of {short_code}."),
        None => format!("{short_code} has no board."),
    };
    Err(ApiError::forbidden(format!(
        "To set or clear a team of {short_code}, you must be able to edit {short_code}. \
         {need} The creator of {short_code} and an organization admin can also do it. \
         You need no right on the team."
    ))
    .with_details(json!({
        "relationship": item_teams::TARGET_TEAM,
        "required_capability": capability,
        "board_id": board_id,
    })))
}

/// [`ItemTeamError`] → HTTP. The subject rule and the liveness of the item
/// are asked before the service, so those arms are the answer to a race.
pub(crate) fn map_item_team_error(e: ItemTeamError) -> ApiError {
    match e {
        e @ ItemTeamError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        e @ ItemTeamError::NotLinked { .. } => ApiError::not_found(e.to_string()),
        e @ ItemTeamError::SubjectType { .. } => {
            ApiError::unprocessable("RELATIONSHIP_RULE", e.to_string())
        }
        e @ (ItemTeamError::ItemNotFound(_) | ItemTeamError::TeamNotFound(_)) => {
            ApiError::validation(e.to_string())
        }
        ItemTeamError::Database(e) => ApiError::internal(e),
    }
}

/// The team filter of a board read, from the two query values. `team` and
/// `no_team` together are refused: they ask for two different sets.
pub(crate) fn team_filter(
    conn: &mut PgConnection,
    team: Option<&str>,
    no_team: bool,
) -> Result<Option<TeamFilter>, ApiError> {
    match (team, no_team) {
        (Some(_), true) => Err(ApiError::validation(
            "Send team or no_team, not the two. The value team gives the items of one team. \
             The value no_team gives the items with no team.",
        )),
        (Some(reference), false) => {
            let team = crate::api::org::repositories::resolve_team(conn, reference)?;
            Ok(Some(TeamFilter::Team(team.id)))
        }
        (None, true) => Ok(Some(TeamFilter::NoTeam)),
        (None, false) => Ok(None),
    }
}

/// The teams of the items of a board, keyed by short code: what the board
/// read sends. One query for the board.
pub(crate) fn board_item_teams(
    conn: &mut PgConnection,
    board_id: Uuid,
    item_codes: &[(Uuid, String)],
) -> Result<BTreeMap<String, Vec<dto::ItemTeam>>, ApiError> {
    let mut teams = item_teams::board_teams(conn, board_id).map_err(ApiError::internal)?;
    Ok(item_codes
        .iter()
        .filter_map(|(id, code)| {
            teams
                .remove(id)
                .map(|item| (code.clone(), item.into_iter().map(team_dto).collect()))
        })
        .collect())
}

#[derive(QueryableByName)]
struct LinkedTeam {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// The team that `reference` names (a slug or a UUID) among the teams SET
/// BY HAND on the item, archived or not. A live team comes first.
fn linked_team(
    conn: &mut PgConnection,
    item_id: Uuid,
    reference: &str,
) -> Result<Option<Uuid>, ApiError> {
    let id = reference.parse::<Uuid>().ok();
    let row: Option<LinkedTeam> = sql_query(
        "SELECT t.id FROM item_impacts i JOIN teams t ON t.id = i.target_id \
          WHERE i.item_id = $1 AND i.target_kind = 'team' \
            AND (t.id = $2 OR t.slug = $3) \
          ORDER BY t.deleted_at NULLS FIRST LIMIT 1",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<SqlUuid, _>(id.unwrap_or_else(Uuid::nil))
    .bind::<Text, _>(reference)
    .get_result(conn)
    .optional()
    .map_err(ApiError::internal)?;
    Ok(row.map(|row| row.id))
}

/// Set `team` on `item` by hand as `user`. See the module docs for the
/// rules. The activity row and the event come from [`item_teams::link`].
pub(crate) fn add(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    team: &str,
) -> Result<ItemTeam, ApiError> {
    let short_code = short_code_of(conn, item_id)?;
    require_subject(item_type, &short_code)?;
    require_edit(conn, slug, user, (item_id, item_type))?;
    let team = crate::api::org::repositories::resolve_team(conn, team)?;
    item_teams::link(conn, item_id, team.id, user).map_err(map_item_team_error)
}

/// Clear the team `team` set by hand on `item` as `user`. See the module
/// docs for the rules. The result is the slug of the team.
pub(crate) fn remove(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    team: &str,
) -> Result<String, ApiError> {
    let short_code = short_code_of(conn, item_id)?;
    require_subject(item_type, &short_code)?;
    require_edit(conn, slug, user, (item_id, item_type))?;
    let Some(team_id) = linked_team(conn, item_id, team)? else {
        let from_tasks = item_teams::teams_of(conn, item_id)
            .map_err(ApiError::internal)?
            .into_iter()
            .any(|t| t.from_tasks && (t.slug == team || t.team_id.to_string() == team));
        let why = if from_tasks {
            format!(
                " {short_code} gets the team {team:?} from its tasks. To remove it, move \
                 or archive those tasks."
            )
        } else {
            String::new()
        };
        return Err(ApiError::not_found(format!(
            "The team {team:?} is not set on {short_code} by hand.{why}"
        )));
    };
    let slug_of_team: String = {
        use kairos_db::schema::teams::dsl;
        dsl::teams
            .filter(dsl::id.eq(team_id))
            .select(dsl::slug)
            .first(conn)
            .map_err(ApiError::internal)?
    };
    item_teams::unlink(conn, item_id, team_id, user).map_err(map_item_team_error)?;
    Ok(slug_of_team)
}

/// The teams of an initiative or a strategy, by slug. Each person in the
/// tenant can read them. They come from its tasks, and by hand.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/teams",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (initiatives|strategies)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "The teams of the item, by slug", body = dto::ItemTeamsResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not an initiative and not a strategy", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_teams(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ItemTeamsResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            require_subject(item_type, &short_code)?;
            let teams = item_teams::teams_of(conn, item_id).map_err(ApiError::internal)?;
            Ok(dto::ItemTeamsResponse {
                short_code,
                teams: teams.into_iter().map(team_dto).collect(),
            })
        })
        .await?;
    Ok(Json(response))
}

/// Set a team on an initiative or a strategy by hand, for example before
/// it has tasks. The team stays when tasks come.
///
/// The edit rule of the item applies. The caller needs no right on the
/// team. The team gives no right on the item.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/teams",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (initiatives|strategies)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    request_body = dto::SetItemTeamRequest,
    responses(
        (status = 201, description = "Team set (relationship_add activity row written)", body = dto::ItemTeam),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not an initiative and not a strategy. ALREADY_LINKED: the team is set already. VALIDATION: no live team has that slug or id", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::SetItemTeamRequest>,
) -> Result<(StatusCode, Json<dto::ItemTeam>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            add(conn, &slug, user, item, &body.team).map(team_dto)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Clear a team that is set on an initiative or a strategy by hand. A team
/// that the item gets from its tasks stays.
///
/// The edit rule of the item applies. The team can be archived.
#[utoipa::path(
    delete,
    path = "/api/{entity_type}/{short_code}/teams/{team}",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (initiatives|strategies)"),
        ("short_code" = String, Path, description = "Item short code"),
        ("team" = String, Path, description = "Team slug (or UUID)"),
    ),
    responses(
        (status = 200, description = "Team cleared (relationship_remove activity row written)", body = dto::ClearedItemTeamResponse),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code, or the team is not set on the item by hand", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not an initiative and not a strategy", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn clear_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code, team)): Path<(String, String, String)>,
) -> Result<Json<dto::ClearedItemTeamResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let cleared = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let team = remove(conn, &slug, user, item, &team)?;
            Ok(dto::ClearedItemTeamResponse { short_code, team })
        })
        .await?;
    Ok(Json(cleared))
}
