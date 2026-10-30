//! `/api/teams` (KAIROS-S-0005 Teams family, KAIROS-T-0019): team CRUD and
//! team membership. Writes are for an organization admin, or a principal
//! with `manage_structure` at the organization (KAIROS-T-0337,
//! [`require_structure`]); reads are open tenant-wide.
//!
//! **Team creation creates the team's delivery board** (the KAIROS-A-0002 /
//! T-0010 deferred decision: delivery boards are per-team, seeded from the
//! `system_board_defaults` delivery config) in the same transaction; the
//! board slug is `{team_slug}-delivery` and the response carries
//! `delivery_board_id`. Team deletion requires that board to be empty (422
//! `BOARD_NOT_EMPTY`) and soft-deletes team + board together.

use axum::extract::{Extension, Path, State};
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
use super::super::{clamp_pagination, parse_enum, require_structure};
use super::{
    check_team_slug, count_live_board_items, is_unique_violation, map_config_error,
    require_user_exists, run_in_transaction,
};
use crate::api::org::structure_grants::{SignInRoutes, resolve_member, resolve_member_to_add};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/teams", get(list_teams).post(create_team))
        .route("/api/teams/by-slug/{slug}", get(get_team_by_slug))
        .route("/api/teams/{id}/links", get(list_team_links))
        .route(
            "/api/teams/{id}/unlinked-refs",
            get(list_team_unlinked_refs),
        )
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
        .ok_or_else(|| {
            ApiError::not_found(format!("No live team has the id {team_id}."))
                .with_details(serde_json::json!({ "field": "team" }))
        })
}

/// The id of the live team that `reference` names, or 404
/// (KAIROS-T-0607). The reference is the slug or the id of the team, as
/// for a board ([`super::load_board_by_ref`]): a reference with the form
/// of a UUID is an id, each other reference is a slug. Each
/// `/api/teams/{id}` route takes it, so that a client that has the slug of
/// a team makes one call.
pub(crate) fn team_by_ref(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError> {
    // KAIROS-T-0689: the forms of crate::references::TEAM_FORMS.
    let reference = crate::references::text(reference);
    // An id gets the refusal that names the id.
    if let Ok(team_id) = Uuid::parse_str(reference) {
        return load_team(conn, team_id).map(|team| team.id);
    }
    crate::references::team(conn, reference)?
        .map(|team| team.id)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "No live team has the slug or the id {reference:?}."
            ))
            .with_details(serde_json::json!({ "field": "team" }))
        })
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
    ApiQuery(pagination): ApiQuery<Pagination>,
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
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
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
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
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
            let team = team.ok_or_else(|| {
                ApiError::not_found(format!("No team has the slug {slug:?}."))
                    .with_details(serde_json::json!({ "field": "team" }))
            })?;
            let board = delivery_board_of(conn, team.id)?;
            Ok(team_to_dto(team, board))
        })
        .await?;
    Ok(Json(team))
}

/// Query of [`list_team_links`] (explicit struct — serde_urlencoded
/// cannot flatten).
#[derive(serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
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
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
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
    ApiQuery(query): ApiQuery<TeamLinksQuery>,
) -> Result<Json<Vec<kairos_client::types_forge::TeamLink>>, ApiError> {
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
            let team_id = team_by_ref(conn, &id)?;
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

/// The in-flight branches and pull requests with no linked item, in the
/// repositories of the team (KAIROS-T-0480). This is the other half of the
/// rollup of `/api/teams/{id}/links`. Open tenant-wide.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/unlinked-refs",
    tag = "teams",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        TeamLinksQuery,
    ),
    responses(
        (status = 200, description = "Branches and pull requests with no linked item, newest first", body = Vec<kairos_client::types_forge::ForgeRef>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_team_unlinked_refs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<TeamLinksQuery>,
) -> Result<Json<Vec<kairos_client::types_forge::ForgeRef>>, ApiError> {
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
            let team_id = team_by_ref(conn, &id)?;
            load_team(conn, team_id)?;
            let refs: Vec<&str> = states.iter().map(String::as_str).collect();
            let rows = kairos_db::forge::team_unlinked_refs(conn, team_id, &refs, limit)
                .map_err(super::forge::map_error)?;
            Ok(rows
                .into_iter()
                .map(super::repositories::ref_dto)
                .collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// The documents of the team (KAIROS-T-0084, KAIROS-T-0393). A board of
/// the team owns each live document, or the document supports a task of
/// the team or an item on its delivery board. Each row gives the reason.
/// Open tenant-wide.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/work-documents",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The documents of the team, by short code", body = Vec<kairos_client::types_team_pages::TeamWorkDocument>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_work_documents(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<kairos_client::types_team_pages::TeamWorkDocument>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
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
                    owned: row.owned,
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
/// defaults, slug `{slug}-delivery`) in one transaction. An organization admin,
/// or `manage_structure` on the organization (KAIROS-T-0337).
///
/// The slug must match `^[a-z][a-z0-9_-]{1,62}$`, and it cannot have the
/// form of a UUID (COLLIERY-T-0260). If not, the request is a 422
/// `VALIDATION` with `details.field` = `slug`.
///
/// A live board can have the slug `{slug}-delivery` already
/// (COLLIERY-T-0255). Then the request is a 409 `CONFLICT` that names that
/// board, and the server creates no team.
///
/// `code_prefix` is the short-code prefix of the delivery board
/// (COLLIERY-T-3099). It is required. A prefix that does not match
/// `^[A-Z][A-Z0-9]{1,9}$` is a 422 `VALIDATION` with `details.field` =
/// `code_prefix`. A prefix that a live delivery board has is a 409
/// `CONFLICT` that names that board. In each case the server creates no
/// team.
#[utoipa::path(
    post,
    path = "/api/teams",
    tag = "teams",
    params(("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC)),
    request_body = dto::CreateTeamRequest,
    responses(
        (status = 201, description = "Created; delivery_board_id names the team's new board", body = dto::Team),
        (status = 403, description = "Not an org admin, and no manage_structure on the organization", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A live team has the slug, a live board has the slug of the delivery board, or a live delivery board has the prefix; or REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad team_type, the slug does not have the form of a slug, or the code_prefix is absent or does not match the rule", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::CreateTeamRequest>,
) -> Result<crate::api::idempotency::Answer<dto::Team>, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            crate::api::idempotency::run(conn, keyed, StatusCode::CREATED, |conn| {
                create(conn, &slug, user, body)
            })
        })
        .await?;
    Ok(team)
}

/// Create a team and its delivery board as `user`: the ONE implementation
/// of `POST /api/teams` and of the MCP tool `create_team` (KAIROS-T-0337).
/// The gate is [`require_structure`].
pub(crate) fn create(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    body: dto::CreateTeamRequest,
) -> Result<dto::Team, ApiError> {
    let team_type = body
        .team_type
        .as_deref()
        .map(|v| parse_enum::<TeamType>(v, "team_type", TeamType::ALL))
        .transpose()?
        .unwrap_or(TeamType::StreamAligned);
    // COLLIERY-T-0260: the form of the slug that the caller sent. The
    // slug `{slug}-delivery` of the delivery board is a slug that the
    // server makes, so it gets no check.
    check_team_slug(&body.slug)?;
    require_structure(conn, slug, user)?;
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
                    ApiError::conflict(format!("A live team has the slug {:?} already.", body.slug))
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
            // COLLIERY-T-3099: the prefix of the delivery board.
            boards::CodePrefix::Given(&body.code_prefix),
            Some(team.id),
            Some(user),
        )
        .map_err(|e| match e {
            // COLLIERY-T-0255: the caller sent the slug of the
            // team, and not that of the board. The refusal says
            // where the slug of the board comes from. The
            // transaction rolls back, so no team is written.
            boards::BoardError::SlugTaken {
                slug,
                board_id,
                board_name,
            } => ApiError::conflict(format!(
                "The delivery board of the team gets the slug {slug:?}. The live \
                 board {board_name:?} has that slug. Send a different slug for the \
                 team, or change the slug of that board."
            ))
            .with_details(serde_json::json!({
                "slug": slug,
                "board": { "id": board_id, "name": board_name },
            })),
            e => map_config_error(e),
        })?;
        // KAIROS-T-0082, KAIROS-T-0436: a team is never born without its
        // charter, which seeds in this same transaction. The rest of the
        // tree is the team's to grow.
        kairos_db::team_pages::seed_team_charter(conn, team.id, user)
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
}

/// Update a team (name/slug/team_type). An organization admin, or
/// `manage_structure` on the organization (KAIROS-T-0337). The delivery
/// board's name/slug are independent and unchanged.
///
/// A new slug must match `^[a-z][a-z0-9_-]{1,62}$`, and it cannot have the
/// form of a UUID (COLLIERY-T-0260). If not, the request is a 422
/// `VALIDATION` with `details.field` = `slug`. A team with a slug from
/// before the rule keeps that slug.
#[utoipa::path(
    patch,
    path = "/api/teams/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    request_body = dto::UpdateTeamRequest,
    responses(
        (status = 200, description = "Updated", body = dto::Team),
        (status = 403, description = "Not an org admin, and no manage_structure on the organization", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad team_type, empty body, or the new slug does not have the form of a slug", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateTeamRequest>,
) -> Result<Json<dto::Team>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let team = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            update_as(conn, &slug, user, team_id, &body)
        })
        .await?;
    Ok(Json(team))
}

/// Change a team as `user`: the ONE implementation of
/// `PATCH /api/teams/{id}` and of the MCP tool `update_team`
/// (KAIROS-T-0443). The gate is [`require_structure`].
pub(crate) fn update_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
    body: &dto::UpdateTeamRequest,
) -> Result<dto::Team, ApiError> {
    if body.name.is_none()
        && body.slug.is_none()
        && body.team_type.is_none()
        && body.site_enabled.is_none()
    {
        return Err(ApiError::validation(
            "The request has no field to change. Send one or more of name, slug, team_type \
             and site_enabled.",
        )
        .with_details(serde_json::json!({
            "field": "name",
            "fields": ["name", "slug", "team_type", "site_enabled"],
        })));
    }
    let team_type = body
        .team_type
        .as_deref()
        .map(|v| parse_enum::<TeamType>(v, "team_type", TeamType::ALL))
        .transpose()?;
    use kairos_db::schema::teams::dsl;
    require_structure(conn, slug, user)?;
    let team = load_team(conn, team_id)?;
    // COLLIERY-T-0260: the form of a slug that the caller sent. A
    // team keeps a slug from before the rule, so a request that
    // sends the slug that the team has is not a change.
    if let Some(new_slug) = body.slug.as_deref()
        && new_slug != team.slug
    {
        check_team_slug(new_slug)?;
    }
    // KAIROS-T-0268: only a value that is different from the value
    // of the row goes to the write. When no value is different, the
    // route writes nothing and makes no activity entry: the forms of
    // the GUI send each field at each save.
    let changes = TeamChangeset {
        name: body.name.clone().filter(|name| *name != team.name),
        slug: body.slug.clone().filter(|new| *new != team.slug),
        team_type: team_type.filter(|kind| *kind != team.team_type),
        site_enabled: body
            .site_enabled
            .filter(|enabled| *enabled != team.site_enabled),
        ..Default::default()
    };
    if changes.name.is_none()
        && changes.slug.is_none()
        && changes.team_type.is_none()
        && changes.site_enabled.is_none()
    {
        let board = delivery_board_of(conn, team_id)?;
        return Ok(team_to_dto(team, board));
    }
    let updated: Team = diesel::update(dsl::teams.filter(dsl::id.eq(team_id)))
        .set((changes, dsl::updated_at.eq(diesel::dsl::now)))
        .returning(Team::as_returning())
        .get_result(conn)
        .map_err(|e| {
            if is_unique_violation(&e) {
                ApiError::conflict("A live team has that slug already.")
            } else {
                ApiError::internal(e)
            }
        })?;
    log_team_activity(
        conn,
        user,
        // COLLIERY-T-0265: a change is an update, not a create.
        ActivityAction::Update,
        team_id,
        format!("team_settings:{}", updated.slug),
    )?;
    let board = delivery_board_of(conn, team_id)?;
    Ok(team_to_dto(updated, board))
}

/// Soft-delete a team and its delivery board together. The board must be
/// empty (422 `BOARD_NOT_EMPTY` — the T-0010 empty rule). An organization
/// admin, or `manage_structure` on the organization (KAIROS-T-0337).
///
/// A team of old data can have 2 or more live delivery boards. The delete
/// applies the rule to each board, and it removes each board with the team
/// (COLLIERY-T-0250). The refusal names the board that is not empty.
#[utoipa::path(
    delete,
    path = "/api/teams/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "Team and delivery board soft-deleted", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin, and no manage_structure on the organization", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "BOARD_NOT_EMPTY, or BOARD_OWNS_DOCUMENTS", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            use kairos_db::schema::boards::dsl as boards_dsl;
            use kairos_db::schema::teams::dsl;
            require_structure(conn, &slug, user)?;
            let team = load_team(conn, team_id)?;
            // KAIROS-T-0112: a team that still OWNS repositories cannot go —
            // symmetric with the repository delete guard; re-home them first.
            let owned = kairos_db::repositories::list(conn, Some(team_id))
                .map_err(super::repositories::map_error)?;
            if !owned.is_empty() {
                let slugs: Vec<&str> = owned.iter().map(|r| r.slug.as_str()).collect();
                return Err(ApiError::conflict(format!(
                    "The team {:?} has {} repositor{}: [{}]. Give each repository to a \
                     different team. Then delete the team.",
                    team.name,
                    owned.len(),
                    if owned.len() == 1 { "y" } else { "ies" },
                    slugs.join(", ")
                ))
                .with_details(serde_json::json!({ "repositories": slugs })));
            }
            // COLLIERY-T-0250: one transaction holds the check and the
            // delete. The team has one delivery board by the rule
            // (COLLIERY-T-0240), and old data can have more. Each live
            // delivery board must be empty, and each goes with the team.
            run_in_transaction(conn, |conn| {
                // COLLIERY-T-3102: the ADR board of the team goes with it
                // too, and it must be empty.
                let team_boards =
                    kairos_db::boards_for_team_delete(conn, team_id).map_err(ApiError::internal)?;
                for board in &team_boards {
                    let item_count = count_live_board_items(conn, board.id)?;
                    if item_count > 0 {
                        let items = super::live_board_item_codes(conn, board.id, 20)?;
                        let (kind, how) = if board.board_level == BoardLevel::Adr {
                            ("ADR", "Archive each ADR")
                        } else {
                            (
                                "delivery",
                                "Move each card to a different board \
                                 (POST /api/tasks/{code}/move) or delete it",
                            )
                        };
                        return Err(ApiError::unprocessable(
                            "BOARD_NOT_EMPTY",
                            format!(
                                "The {kind} board {:?} of the team {:?} has {item_count} live \
                                 card{}: [{}]. {how}. Then delete the team.",
                                board.name,
                                team.name,
                                if item_count == 1 { "" } else { "s" },
                                items.join(", ")
                            ),
                        )
                        .with_details(serde_json::json!({
                            "board_id": board.id,
                            "board": {
                                "id": board.id,
                                "name": board.name,
                                "slug": board.slug,
                            },
                            "item_count": item_count,
                            "items": items,
                        })));
                    }
                    // COLLIERY-T-0269: the board goes with the team, and a
                    // document that names it would have no owner.
                    super::check_board_owns_no_document(
                        conn,
                        board.id,
                        &board.name,
                        "Then delete the team.",
                    )?;
                }
                diesel::update(dsl::teams.filter(dsl::id.eq(team_id)))
                    .set((
                        dsl::deleted_at.eq(diesel::dsl::now),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(ApiError::internal)?;
                let board_ids: Vec<Uuid> = team_boards.iter().map(|board| board.id).collect();
                diesel::update(boards_dsl::boards.filter(boards_dsl::id.eq_any(board_ids)))
                    .set((
                        boards_dsl::deleted_at.eq(diesel::dsl::now),
                        boards_dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(ApiError::internal)?;
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
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
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
    let members = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
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

/// Add a member to a team. An organization admin, or `manage_structure`
/// on the organization (KAIROS-T-0337).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/members",
    // Unique operationId next to `org::members::add_member` (T-0023).
    operation_id = "add_team_member",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    request_body = dto::AddTeamMemberRequest,
    responses(
        (status = 201, description = "Added", body = dto::TeamMember),
        (status = 403, description = "Not an org admin, and no manage_structure on the organization", body = kairos_client::types::ErrorEnvelope),
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
    ApiJson(body): ApiJson<dto::AddTeamMemberRequest>,
) -> Result<(StatusCode, Json<dto::TeamMember>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let routes = SignInRoutes::of(&state.config);
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            // KAIROS-T-0443: a UUID, an email or a user name.
            let target =
                resolve_member_to_add(conn, &slug, user, &body.user_id, "user_id", routes)?.id;
            add_member_as(conn, &slug, user, team_id, target)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(member)))
}

/// Add `target` to the team `team_id` as `user`: the ONE implementation of
/// `POST /api/teams/{id}/members` and of the MCP tool `add_team_member`
/// (KAIROS-T-0337). The gate is [`require_structure`].
pub(crate) fn add_member_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
    target: Uuid,
) -> Result<dto::TeamMember, ApiError> {
    use kairos_db::schema::team_members::dsl;
    require_structure(conn, slug, user)?;
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
                    "The user {target} is a member of the team {team_id} already."
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
}

/// Remove a member from a team. An organization admin, or
/// `manage_structure` on the organization (KAIROS-T-0337).
#[utoipa::path(
    delete,
    path = "/api/teams/{id}/members/{user_id}",
    // Unique operationId next to `org::members::remove_member` (T-0023).
    operation_id = "remove_team_member",
    tag = "teams",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("user_id" = String, Path, description = "User id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin, and no manage_structure on the organization", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or not a member", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target)): Path<(String, String)>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            // KAIROS-T-0443: a UUID, an email or a user name. A UUID is
            // taken as it is: a person who left the organization can still
            // be taken off the team.
            let target = match Uuid::parse_str(target.trim()) {
                Ok(id) => id,
                Err(_) => resolve_member(conn, &slug, user, &target, "user_id")?.id,
            };
            remove_member_as(conn, &slug, user, team_id, target)
        })
        .await?;
    Ok(Json(outcome))
}

/// Remove `target` from the team `team_id` as `user`: the ONE
/// implementation of `DELETE /api/teams/{id}/members/{user_id}` and of the
/// MCP tool `remove_team_member` (KAIROS-T-0443). The gate is
/// [`require_structure`].
pub(crate) fn remove_member_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
    target: Uuid,
) -> Result<dto::OrgDeleteResponse, ApiError> {
    use kairos_db::schema::team_members::dsl;
    require_structure(conn, slug, user)?;
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
            "The user {target} is not a member of the team {team_id}."
        ))
        .with_details(serde_json::json!({ "field": "user" })));
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
}
