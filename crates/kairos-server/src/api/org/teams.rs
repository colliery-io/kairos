//! `/api/teams` (KAIROS-S-0005 Teams family, KAIROS-T-0019): team CRUD and
//! team membership. Writes are org-admin-only (tenant-wide org structure,
//! the A-0006 tenant-config fallback); reads are open tenant-wide.
//!
//! **Team creation creates the team's delivery board** (the KAIROS-A-0002 /
//! T-0010 deferred decision: delivery boards are per-team, seeded from the
//! `system_board_defaults` delivery config) in the same transaction; the
//! board slug is `{team_slug}-delivery` and the response carries
//! `delivery_board_id`. Team deletion requires that board to be empty (422
//! `BOARD_NOT_EMPTY`) and soft-deletes team + board together.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_db::models::enums::{ActivityAction, BoardLevel, TeamType};
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::teams::{NewTeam, NewTeamMember, Team, TeamChangeset};
use kairos_db::{boards, models::User};
use uuid::Uuid;

use super::super::convert_org::team_to_dto;
use super::super::{clamp_pagination, parse_enum, parse_uuid, require_capability};
use super::{
    count_live_board_items, is_unique_violation, map_config_error, require_user_exists,
    run_in_transaction,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s for these org-admin-only writes
/// (`board_id: null` — the A-0006 tenant-config fallback).
const MANAGE: &str = "manage_teams";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/teams", get(list_teams).post(create_team))
        .route("/api/teams/by-slug/{slug}", get(get_team_by_slug))
        .route("/api/teams/{id}/links", get(list_team_links))
        .route("/api/teams/{id}/work-documents", get(list_work_documents))
        .route(
            "/api/teams/{id}",
            get(get_team).patch(update_team).delete(delete_team),
        )
        .route(
            "/api/teams/{id}/members",
            get(list_members).post(add_member),
        )
        .route(
            "/api/teams/{id}/members/{user_id}",
            axum::routing::delete(remove_member),
        )
}

/// Load the live team with this id, or 404.
fn load_team(conn: &mut PgConnection, team_id: Uuid) -> Result<Team, ApiError> {
    use kairos_db::schema::teams::dsl;
    dsl::teams
        .filter(dsl::id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .select(Team::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no live team {team_id}")))
}

/// The team's ONE live delivery board, if any (exactly-one semantics,
/// shared with routing — KAIROS-T-0112).
fn delivery_board_of(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    use kairos_db::repositories::{RepositoryError, delivery_board_for_team};
    match delivery_board_for_team(conn, team_id) {
        Ok(board) => Ok(Some(board)),
        Err(RepositoryError::NoDeliveryBoard { .. }) => Ok(None),
        Err(e) => Err(super::repositories::map_error(e)),
    }
}

/// Insert one `activity_log` row for a team mutation.
fn log_team_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    team_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(team_id),
            entity_type: Some("team".to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// List teams (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams",
    tag = "teams",
    params(Pagination),
    responses(
        (status = 200, description = "Page of teams", body = ListEnvelope<dto::Team>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_teams(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<dto::Team>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::teams::dsl;
            let total: i64 = dsl::teams
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Team> = dsl::teams
                .filter(dsl::deleted_at.is_null())
                .order(dsl::slug.asc())
                .limit(limit)
                .offset(offset)
                .select(Team::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut items = Vec::with_capacity(rows.len());
            for team in rows {
                let board = delivery_board_of(conn, team.id)?;
                items.push(team_to_dto(team, board));
            }
            Ok(ListEnvelope {
                items,
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one team (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "The team", body = dto::Team),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_team(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::Team>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team = load_team(conn, team_id)?;
            let board = delivery_board_of(conn, team_id)?;
            Ok(team_to_dto(team, board))
        })
        .await?;
    Ok(Json(team))
}

/// One live team by SLUG (open tenant-wide; KAIROS-T-0083 — the web
/// client resolves `/teams/:slug` here instead of scanning the list).
#[utoipa::path(
    get,
    path = "/api/teams/by-slug/{slug}",
    tag = "teams",
    params(("slug" = String, Path, description = "Team slug")),
    responses(
        (status = 200, description = "The team", body = dto::Team),
        (status = 404, description = "Unknown slug", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_team_by_slug(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::Team>, ApiError> {
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::teams::dsl;
            let team: Option<Team> = dsl::teams
                .filter(dsl::slug.eq(&slug))
                .filter(dsl::deleted_at.is_null())
                .select(Team::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?;
            let team =
                team.ok_or_else(|| ApiError::not_found(format!("no team with slug {slug:?}")))?;
            let board = delivery_board_of(conn, team.id)?;
            Ok(team_to_dto(team, board))
        })
        .await?;
    Ok(Json(team))
}

/// Query of [`list_team_links`] (explicit struct — serde_urlencoded
/// cannot flatten).
#[derive(serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct TeamLinksQuery {
    /// Comma-separated states; defaults to `open,draft` — the panel's
    /// question is "what is in flight", not merged history.
    state: Option<String>,
    /// Result cap (default 100).
    limit: Option<i64>,
}

/// The team's in-flight forge links (KAIROS-T-0101): links whose item is
/// a task of this team, or an item on the team's delivery board, or whose
/// repository is attributed to the team. Open tenant-wide.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/links",
    tag = "teams",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        TeamLinksQuery,
    ),
    responses(
        (status = 200, description = "In-flight links, newest first", body = Vec<kairos_client::types_forge::TeamLink>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_team_links(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<TeamLinksQuery>,
) -> Result<Json<Vec<kairos_client::types_forge::TeamLink>>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let states: Vec<String> = query
        .state
        .as_deref()
        .unwrap_or("open,draft")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            load_team(conn, team_id)?;
            let board = delivery_board_of(conn, team_id)?;
            let refs: Vec<&str> = states.iter().map(String::as_str).collect();
            let rows = kairos_db::graph::team_link_rollup(conn, team_id, board, &refs, limit)
                .map_err(ApiError::internal)?;
            Ok(rows
                .into_iter()
                .map(|row| kairos_client::types_forge::TeamLink {
                    kind: row.kind,
                    external_id: row.external_id,
                    title: row.title,
                    url: row.url,
                    state: row.state,
                    author: row.author,
                    forge: row.forge,
                    repo_full_name: row.repo_full_name,
                    item_short_code: row.item_short_code,
                    item_title: row.item_title,
                    forge_updated_at: row.forge_updated_at.to_rfc3339(),
                })
                .collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// The documents attached to the team's WORK (KAIROS-T-0084): live
/// documents whose supports-parent is a task of the team or an item on
/// the team's delivery board. Open tenant-wide. Docs under org-level
/// items deliberately absent — team attribution follows the parent item.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/work-documents",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "Derived work documents, by short code", body = Vec<kairos_client::types_team_pages::TeamWorkDocument>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_work_documents(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<kairos_client::types_team_pages::TeamWorkDocument>>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            load_team(conn, team_id)?;
            let board = delivery_board_of(conn, team_id)?;
            let rows = kairos_db::graph::team_work_documents(conn, team_id, board)
                .map_err(ApiError::internal)?;
            Ok(rows
                .into_iter()
                .map(|row| kairos_client::types_team_pages::TeamWorkDocument {
                    short_code: row.short_code,
                    title: row.title,
                    lifecycle: row.lifecycle,
                    parent_short_code: row.parent_short_code,
                    parent_title: row.parent_title,
                    parent_type: row.parent_type,
                })
                .collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// Create a team AND its delivery board (seeded from the system delivery
/// defaults, slug `{slug}-delivery`) in one transaction. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/teams",
    tag = "teams",
    request_body = dto::CreateTeamRequest,
    responses(
        (status = 201, description = "Created; delivery_board_id names the team's new board", body = dto::Team),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Team or board slug already in use", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad team_type", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateTeamRequest>,
) -> Result<(StatusCode, Json<dto::Team>), ApiError> {
    let team_type = body
        .team_type
        .as_deref()
        .map(|v| parse_enum::<TeamType>(v, "team_type", TeamType::ALL))
        .transpose()?
        .unwrap_or(TeamType::StreamAligned);
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            run_in_transaction(conn, |conn| {
                use kairos_db::schema::teams;
                let team: Team = diesel::insert_into(teams::table)
                    .values(NewTeam {
                        name: body.name.clone(),
                        slug: body.slug.clone(),
                        team_type,
                    })
                    .returning(Team::as_returning())
                    .get_result(conn)
                    .map_err(|e| {
                        if is_unique_violation(&e) {
                            ApiError::conflict(format!(
                                "a team with slug {:?} already exists",
                                body.slug
                            ))
                        } else {
                            ApiError::internal(e)
                        }
                    })?;
                // The T-0010 deferred decision: the team's delivery board
                // is created WITH the team, from the seeded defaults.
                let board = boards::create_board(
                    conn,
                    BoardLevel::Delivery,
                    &format!("{} Delivery", body.name),
                    &format!("{}-delivery", body.slug),
                    Some(team.id),
                    Some(user),
                )
                .map_err(|e| match e {
                    boards::BoardError::Database(ref db) if is_unique_violation(db) => {
                        ApiError::conflict(format!(
                            "a board with slug {:?} already exists",
                            format!("{}-delivery", body.slug)
                        ))
                    }
                    e => map_config_error(e),
                })?;
                // KAIROS-T-0082: a team is never born bare — the page
                // scaffold (charter, support processes, documentation
                // tree) seeds in this same transaction.
                kairos_db::team_pages::seed_team_scaffold(conn, team.id, user)
                    .map_err(ApiError::internal)?;
                log_team_activity(
                    conn,
                    user,
                    ActivityAction::Create,
                    team.id,
                    format!("team:{} delivery_board:{}", team.slug, board.slug),
                )?;
                Ok(team_to_dto(team, Some(board.id)))
            })
        })
        .await?;
    Ok((StatusCode::CREATED, Json(team)))
}

/// Update a team (name/slug/team_type). Org-admin-only. The delivery
/// board's name/slug are independent and unchanged.
#[utoipa::path(
    patch,
    path = "/api/teams/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    request_body = dto::UpdateTeamRequest,
    responses(
        (status = 200, description = "Updated", body = dto::Team),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad team_type or empty body", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateTeamRequest>,
) -> Result<Json<dto::Team>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    if body.name.is_none() && body.slug.is_none() && body.team_type.is_none() {
        return Err(ApiError::validation(
            "at least one of name, slug, team_type is required",
        ));
    }
    let team_type = body
        .team_type
        .as_deref()
        .map(|v| parse_enum::<TeamType>(v, "team_type", TeamType::ALL))
        .transpose()?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::teams::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_team(conn, team_id)?;
            let updated: Team = diesel::update(dsl::teams.filter(dsl::id.eq(team_id)))
                .set((
                    TeamChangeset {
                        name: body.name.clone(),
                        slug: body.slug.clone(),
                        team_type,
                        ..Default::default()
                    },
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .returning(Team::as_returning())
                .get_result(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict("a team with that slug already exists")
                    } else {
                        ApiError::internal(e)
                    }
                })?;
            log_team_activity(
                conn,
                user,
                ActivityAction::Create,
                team_id,
                format!("team_settings:{}", updated.slug),
            )?;
            let board = delivery_board_of(conn, team_id)?;
            Ok(team_to_dto(updated, board))
        })
        .await?;
    Ok(Json(team))
}

/// Soft-delete a team and its delivery board together. The board must be
/// empty (422 `BOARD_NOT_EMPTY` — the T-0010 empty rule). Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/teams/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "Team and delivery board soft-deleted", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "BOARD_NOT_EMPTY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::boards::dsl as boards_dsl;
            use kairos_db::schema::teams::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            let team = load_team(conn, team_id)?;
            // KAIROS-T-0112: a team that still OWNS repositories cannot go —
            // symmetric with the repository delete guard; re-home them first.
            let owned = kairos_db::repositories::list(conn, Some(team_id))
                .map_err(super::repositories::map_error)?;
            if !owned.is_empty() {
                let slugs: Vec<&str> = owned.iter().map(|r| r.slug.as_str()).collect();
                return Err(ApiError::conflict(format!(
                    "team {:?} still owns {} repositor{}: [{}]; re-home them before removing the team",
                    team.name,
                    owned.len(),
                    if owned.len() == 1 { "y" } else { "ies" },
                    slugs.join(", ")
                ))
                .with_details(serde_json::json!({ "repositories": slugs })));
            }
            let board = delivery_board_of(conn, team_id)?;
            if let Some(board_id) = board {
                let item_count = count_live_board_items(conn, board_id)?;
                if item_count > 0 {
                    let items = super::live_board_item_codes(conn, board_id, 20)?;
                    return Err(ApiError::unprocessable(
                        "BOARD_NOT_EMPTY",
                        format!(
                            "team {:?}'s delivery board still holds {item_count} live card(s): [{}]; \
                             move them to another board (POST /api/tasks/{{code}}/move) or \
                             delete them, then retry",
                            team.name,
                            items.join(", ")
                        ),
                    )
                    .with_details(serde_json::json!({
                        "board_id": board_id,
                        "item_count": item_count,
                        "items": items,
                    })));
                }
            }
            run_in_transaction(conn, |conn| {
                diesel::update(dsl::teams.filter(dsl::id.eq(team_id)))
                    .set((
                        dsl::deleted_at.eq(diesel::dsl::now),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(ApiError::internal)?;
                if let Some(board_id) = board {
                    diesel::update(boards_dsl::boards.filter(boards_dsl::id.eq(board_id)))
                        .set((
                            boards_dsl::deleted_at.eq(diesel::dsl::now),
                            boards_dsl::updated_at.eq(diesel::dsl::now),
                        ))
                        .execute(conn)
                        .map_err(ApiError::internal)?;
                }
                log_team_activity(
                    conn,
                    user,
                    ActivityAction::Delete,
                    team_id,
                    format!("team:{}", team.slug),
                )?;
                Ok(dto::OrgDeleteResponse {
                    id: team_id.to_string(),
                    deleted: true,
                })
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Team members
// ---------------------------------------------------------------------------

/// List a team's members (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/members",
    // The fn shares its name with `org::members::list_members`; OpenAPI
    // operationIds must be unique across the aggregated spec (T-0023).
    operation_id = "list_team_members",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "Members", body = [dto::TeamMember]),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_members(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::TeamMember>>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let members = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{team_members, users};
            load_team(conn, team_id)?;
            // The tenant/public schema split keeps `team_members` and
            // `public.users` out of the same diesel query — two queries.
            let rows: Vec<(Uuid, chrono::DateTime<chrono::Utc>)> = team_members::table
                .filter(team_members::team_id.eq(team_id))
                .select((team_members::user_id, team_members::joined_at))
                .load(conn)
                .map_err(ApiError::internal)?;
            let user_ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
            let identities: Vec<User> = users::table
                .filter(users::id.eq_any(&user_ids))
                .select(User::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut members: Vec<dto::TeamMember> = rows
                .into_iter()
                .map(|(user_id, joined_at)| {
                    let identity = identities.iter().find(|u| u.id == user_id);
                    dto::TeamMember {
                        user_id: user_id.to_string(),
                        email: identity.map(|u| u.email.clone()).unwrap_or_default(),
                        display_name: identity.map(|u| u.display_name.clone()).unwrap_or_default(),
                        joined_at: joined_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                    }
                })
                .collect();
            members.sort_by(|a, b| a.email.cmp(&b.email));
            Ok(members)
        })
        .await?;
    Ok(Json(members))
}

/// Add a member to a team. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/teams/{id}/members",
    // Unique operationId next to `org::members::add_member` (T-0023).
    operation_id = "add_team_member",
    tag = "teams",
    params(("id" = String, Path, description = "Team id (UUID)")),
    request_body = dto::AddTeamMemberRequest,
    responses(
        (status = 201, description = "Added", body = dto::TeamMember),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown user", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::AddTeamMemberRequest>,
) -> Result<(StatusCode, Json<dto::TeamMember>), ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let target = parse_uuid(&body.user_id, "user_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::team_members::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_team(conn, team_id)?;
            let identity = require_user_exists(conn, target)?;
            diesel::insert_into(dsl::team_members)
                .values(NewTeamMember {
                    team_id,
                    user_id: target,
                })
                .execute(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict(format!(
                            "user {target} is already a member of team {team_id}"
                        ))
                    } else {
                        ApiError::internal(e)
                    }
                })?;
            let joined_at: chrono::DateTime<chrono::Utc> = dsl::team_members
                .filter(dsl::team_id.eq(team_id))
                .filter(dsl::user_id.eq(target))
                .select(dsl::joined_at)
                .first(conn)
                .map_err(ApiError::internal)?;
            log_team_activity(
                conn,
                user,
                ActivityAction::Create,
                team_id,
                format!("team_member_add:{}", identity.email),
            )?;
            Ok(dto::TeamMember {
                user_id: target.to_string(),
                email: identity.email,
                display_name: identity.display_name,
                joined_at: joined_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            })
        })
        .await?;
    Ok((StatusCode::CREATED, Json(member)))
}

/// Remove a member from a team. Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/teams/{id}/members/{user_id}",
    // Unique operationId next to `org::members::remove_member` (T-0023).
    operation_id = "remove_team_member",
    tag = "teams",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        ("user_id" = String, Path, description = "User id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or not a member", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target)): Path<(String, String)>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let target = parse_uuid(&target, "user_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::team_members::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_team(conn, team_id)?;
            let deleted = diesel::delete(
                dsl::team_members
                    .filter(dsl::team_id.eq(team_id))
                    .filter(dsl::user_id.eq(target)),
            )
            .execute(conn)
            .map_err(ApiError::internal)?;
            if deleted == 0 {
                return Err(ApiError::not_found(format!(
                    "user {target} is not a member of team {team_id}"
                )));
            }
            log_team_activity(
                conn,
                user,
                ActivityAction::Delete,
                team_id,
                format!("team_member_remove:{target}"),
            )?;
            Ok(dto::OrgDeleteResponse {
                id: target.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
