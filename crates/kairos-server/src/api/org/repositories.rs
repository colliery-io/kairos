//! `/api/repositories` (KAIROS-T-0106, design in KAIROS-I-0010 §D4,
//! decision KAIROS-A-0019): the repository directory — the unit a ticket
//! is issued against and executed in. Every repository has exactly one
//! owning team; tasks filed against it route to that team's delivery
//! board (KAIROS-T-0104).
//!
//! Gating (A-0006, with one deliberate widening recorded in I-0010):
//! - reads are open tenant-wide, like teams and boards — an agent in one
//!   repo needs to find who owns another;
//! - **create and edit** are org admin OR a `manage_tasks` holder on the
//!   owning team's delivery board (which team membership implies,
//!   KAIROS-T-0072), so a team — or its agent during `/kairos:bootstrap` —
//!   can register its own repositories without an admin;
//! - **delete** is org admin only, and refused while live tasks or a live
//!   webhook connection still reference the repository.
//!
//! Webhook wiring is a separate resource (`/api/forge-connections`,
//! [`super::forge`]) that hangs off a repository.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_forge::TeamLink;
use kairos_client::types_repositories as dto;
use kairos_db::models::enums::{BoardLevel, Forge};
use kairos_db::models::repositories::{NewRepository, Repository, RepositoryChangeset};
use kairos_db::models::teams::Team;
use kairos_db::repositories::{self, RepositoryError};
use uuid::Uuid;

use super::super::{parse_enum, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 capability that lets a team manage its own repositories
/// (held implicitly by every member of the owning team).
const MANAGE: &str = "manage_tasks";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/api/repositories/{slug}",
            get(get_repository)
                .patch(update_repository)
                .delete(delete_repository),
        )
}

/// [`RepositoryError`] → HTTP.
pub(crate) fn map_error(e: RepositoryError) -> ApiError {
    match e {
        RepositoryError::NotFound(id) => {
            ApiError::not_found(format!("no live repository {id} exists"))
        }
        RepositoryError::SlugNotFound(slug) => {
            ApiError::not_found(format!("no live repository {slug:?} exists"))
        }
        RepositoryError::InvalidSlug(slug) => ApiError::validation(format!(
            "invalid repository slug {slug:?}: expected ^[a-z0-9][a-z0-9-]{{1,62}}$"
        )),
        RepositoryError::SlugTaken(slug) => {
            ApiError::conflict(format!("repository slug {slug:?} is already taken"))
        }
        RepositoryError::AlreadyRegistered { forge, repo } => {
            ApiError::conflict(format!("{forge} repository {repo:?} is already registered"))
        }
        RepositoryError::TeamNotFound(id) => {
            ApiError::validation(format!("team {id} does not exist"))
        }
        RepositoryError::NoDeliveryBoard { team, count } => ApiError::validation(format!(
            "team {team} has {count} live delivery boards; exactly one is needed"
        )),
        RepositoryError::InUse {
            id,
            tasks,
            connections,
        } => ApiError::conflict(format!(
            "repository {id} is still referenced by {tasks} live task(s) and \
             {connections} live webhook connection(s); unbind them first"
        )),
        RepositoryError::Database(e) => ApiError::internal(e),
    }
}

/// Resolve a team reference (UUID or slug) to its live row, or 422.
fn resolve_team(conn: &mut PgConnection, reference: &str) -> Result<Team, ApiError> {
    use kairos_db::schema::teams::dsl;
    let mut query = dsl::teams.filter(dsl::deleted_at.is_null()).into_boxed();
    query = match reference.parse::<Uuid>() {
        Ok(id) => query.filter(dsl::id.eq(id)),
        Err(_) => query.filter(dsl::slug.eq(reference)),
    };
    query
        .select(Team::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::validation(format!("team {reference:?} does not exist")))
}

/// The team's live delivery board id, if any.
fn delivery_board_of(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::board_level.eq(BoardLevel::Delivery))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()
        .map_err(ApiError::internal)
}

/// Org admin, or `manage_tasks` on the team's delivery board (team
/// membership implies it). A team with no delivery board is admin-only.
fn require_manage_for_team(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
) -> Result<(), ApiError> {
    match delivery_board_of(conn, team_id)? {
        Some(board) => require_capability(conn, slug, Some(board), user, MANAGE),
        None => require_capability(conn, slug, None, user, MANAGE),
    }
}

/// Render repositories with their team, delivery board and counts — two
/// batched queries for the whole list, never per row.
fn render(
    conn: &mut PgConnection,
    rows: Vec<Repository>,
) -> Result<Vec<dto::Repository>, ApiError> {
    use kairos_db::schema::{boards, teams};
    use std::collections::HashMap;

    let team_ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = rows.iter().map(|r| r.team_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let team_rows: Vec<Team> = teams::table
        .filter(teams::id.eq_any(&team_ids))
        .select(Team::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    let by_team: HashMap<Uuid, Team> = team_rows.into_iter().map(|t| (t.id, t)).collect();
    let board_rows: Vec<(Uuid, Uuid)> = boards::table
        .filter(boards::team_id.eq_any(team_ids.iter().map(|id| Some(*id)).collect::<Vec<_>>()))
        .filter(boards::board_level.eq(BoardLevel::Delivery))
        .filter(boards::deleted_at.is_null())
        .select((boards::team_id.assume_not_null(), boards::id))
        .load(conn)
        .map_err(ApiError::internal)?;
    let board_of: HashMap<Uuid, Uuid> = board_rows.into_iter().collect();
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let counts: HashMap<Uuid, (i64, bool)> = repositories::counts(conn, &ids)
        .map_err(map_error)?
        .into_iter()
        .map(|c| (c.repository_id, (c.open_tasks, c.has_webhook)))
        .collect();

    rows.into_iter()
        .map(|repo| {
            let team = by_team.get(&repo.team_id).ok_or_else(|| {
                ApiError::internal(format!("repository {} references missing team", repo.id))
            })?;
            let (open_tasks, has_webhook) = counts.get(&repo.id).copied().unwrap_or((0, false));
            Ok(dto::Repository {
                id: repo.id.to_string(),
                slug: repo.slug,
                forge: repo.forge.to_string(),
                repo_full_name: repo.repo_full_name,
                repo_url: repo.repo_url,
                default_branch: repo.default_branch,
                description: repo.description,
                team: dto::RepositoryTeam {
                    id: team.id.to_string(),
                    slug: team.slug.clone(),
                    name: team.name.clone(),
                },
                delivery_board_id: board_of.get(&repo.team_id).map(|b| b.to_string()),
                open_tasks,
                has_webhook,
                created_at: repo.created_at.to_rfc3339(),
                updated_at: repo.updated_at.to_rfc3339(),
            })
        })
        .collect()
}

fn render_one(conn: &mut PgConnection, repo: Repository) -> Result<dto::Repository, ApiError> {
    let mut rendered = render(conn, vec![repo])?;
    Ok(rendered.remove(0))
}

/// Query of `GET /api/repositories`.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct ListRepositoriesQuery {
    /// Only this team's repositories (UUID or slug).
    pub team: Option<String>,
}

/// The repository directory (open tenant-wide), by slug. `?team=` narrows
/// to one owning team.
#[utoipa::path(
    get,
    path = "/api/repositories",
    tag = "repositories",
    params(ListRepositoriesQuery),
    responses(
        (status = 200, description = "Repositories, by slug", body = Vec<dto::Repository>),
        (status = 422, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_repositories(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<ListRepositoriesQuery>,
) -> Result<Json<Vec<dto::Repository>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = query
                .team
                .as_deref()
                .map(|reference| resolve_team(conn, reference).map(|t| t.id))
                .transpose()?;
            let rows = repositories::list(conn, team_id).map_err(map_error)?;
            render(conn, rows)
        })
        .await?;
    Ok(Json(rows))
}

/// One repository (open tenant-wide): the directory row plus its webhook
/// connection id and in-flight links — everything an agent reads before
/// working in, or filing against, a codebase.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The repository", body = dto::RepositoryDetail),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_repository(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryDetail>, ApiError> {
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            let repo_id = repo.id;
            let connection_id = kairos_db::forge::find_connection_for_repository(conn, repo_id)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map(|c| c.id.to_string());
            let in_flight =
                kairos_db::graph::repository_link_rollup(conn, repo_id, &["open", "draft"], 100)
                    .map_err(ApiError::internal)?
                    .into_iter()
                    .map(|row| TeamLink {
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
                    .collect();
            Ok(dto::RepositoryDetail {
                repository: render_one(conn, repo)?,
                connection_id,
                in_flight,
            })
        })
        .await?;
    Ok(Json(detail))
}

/// Register a repository under its owning team (org admin, or a
/// `manage_tasks` holder on that team's delivery board — self-serve for
/// the team and its agents).
#[utoipa::path(
    post,
    path = "/api/repositories",
    tag = "repositories",
    request_body = dto::CreateRepositoryRequest,
    responses(
        (status = 201, description = "Registered", body = dto::Repository),
        (status = 403, description = "Neither org admin nor the owning team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug or (forge, name) already registered", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad slug/forge or unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<dto::Repository>), ApiError> {
    let forge: Forge = parse_enum(&body.forge, "forge", Forge::ALL)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team = resolve_team(conn, &body.team)?;
            require_manage_for_team(conn, &slug, user, team.id)?;
            let created = repositories::create(
                conn,
                NewRepository {
                    slug: body.slug.clone().unwrap_or_else(|| {
                        kairos_core::repositories::slug_from_full_name(&body.repo_full_name)
                    }),
                    forge,
                    repo_full_name: body.repo_full_name.clone(),
                    repo_url: body.repo_url.clone(),
                    default_branch: body
                        .default_branch
                        .clone()
                        .unwrap_or_else(|| "main".to_string()),
                    team_id: team.id,
                    description: body.description.clone().unwrap_or_default(),
                    created_by: user,
                    updated_by: user,
                },
            )
            .map_err(map_error)?;
            render_one(conn, created)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Edit a repository (same gate as create, evaluated against the CURRENT
/// owner). Re-homing to another team does not touch its tasks.
#[utoipa::path(
    patch,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    request_body = dto::UpdateRepositoryRequest,
    responses(
        (status = 200, description = "Updated", body = dto::Repository),
        (status = 403, description = "Neither org admin nor the owning team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already taken", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad slug or unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    Json(body): Json<dto::UpdateRepositoryRequest>,
) -> Result<Json<dto::Repository>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let current = repositories::resolve(conn, &slug).map_err(map_error)?;
            require_manage_for_team(conn, &tenant_slug, user, current.team_id)?;
            let team_id = body
                .team
                .as_deref()
                .map(|reference| resolve_team(conn, reference).map(|t| t.id))
                .transpose()?;
            let updated = repositories::update(
                conn,
                current.id,
                RepositoryChangeset {
                    slug: body.slug.clone(),
                    repo_url: body.repo_url.clone(),
                    default_branch: body.default_branch.clone(),
                    team_id,
                    description: body.description.clone(),
                    ..Default::default()
                },
                user,
            )
            .map_err(map_error)?;
            render_one(conn, updated)
        })
        .await?;
    Ok(Json(updated))
}

/// Remove a repository (org admin). Refused with 409 while live tasks or a
/// live webhook connection still reference it.
#[utoipa::path(
    delete,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "Removed", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Still referenced by tasks or a connection", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let id = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let current = repositories::resolve(conn, &slug).map_err(map_error)?;
            require_capability(conn, &tenant_slug, None, user, MANAGE)?;
            repositories::soft_delete(conn, current.id, user).map_err(map_error)?;
            Ok(current.id.to_string())
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id,
        deleted: true,
    }))
}
