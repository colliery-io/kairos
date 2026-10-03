//! `/api/repositories` (KAIROS-T-0106, design in KAIROS-I-0010 §D4,
//! decision KAIROS-A-0019): the repository directory — where the code is.
//! Every repository has exactly one owning team. A task on any team's board
//! may link to any repository; the owner does not choose the board
//! (COLLIERY-A-0023).
//!
//! The detail does not report "stale tasks" (COLLIERY-T-0219). That count
//! was the linked tasks on a board of a team that does not own the
//! repository, which COLLIERY-A-0023 makes normal work. `open_tasks` counts
//! the open tasks that link to the repository on all boards.
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
//! A document or an ADR can IMPACT a repository (COLLIERY-T-0269): the
//! link says what the item is about. The detail of a repository gives
//! those items. The link gives no right: the owner team of a repository
//! gets no right on a document that impacts it. The delete of a repository
//! does not look at the links, and they stay.
//!
//! Webhook wiring is a separate resource (`/api/forge-connections`,
//! [`super::forge`]) that hangs off a repository.

use axum::extract::{Extension, Path, State};
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
use serde_json::json;
use uuid::Uuid;

use super::super::{parse_enum, require_capability};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
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
            ApiError::not_found(format!("No live repository has the id {id}."))
        }
        RepositoryError::SlugNotFound(slug) => {
            ApiError::not_found(format!("No live repository has the slug {slug:?}."))
        }
        // COLLIERY-T-0265: the refusal names the field, so that a form
        // shows it below the field `Slug`.
        RepositoryError::InvalidSlug(slug) => ApiError::validation(format!(
            "The repository slug {slug:?} is not correct. A repository slug must match \
             ^[a-z0-9][a-z0-9-]{{1,62}}$."
        ))
        .with_details(json!({ "field": "slug" })),
        // COLLIERY-T-0267: the text and the name of the field come from
        // the function that has the rule of the field.
        RepositoryError::InvalidField(fault) => {
            let field = fault.field;
            ApiError::validation(fault.message).with_details(json!({ "field": field }))
        }
        RepositoryError::SlugTaken(slug) => {
            ApiError::conflict(format!("A repository has the slug {slug:?} already."))
        }
        RepositoryError::AlreadyRegistered { forge, repo } => ApiError::conflict(format!(
            "The {forge} repository {repo:?} is in the directory already."
        )),
        RepositoryError::TeamNotFound(id) => {
            ApiError::validation(format!("The team {id} is not in the organization."))
        }
        RepositoryError::NoDeliveryBoard { team, count } => ApiError::validation(format!(
            "The team {team} has {count} live delivery boards. A team must have one live \
             delivery board."
        )),
        RepositoryError::InUse {
            id,
            tasks,
            connections,
        } => ApiError::conflict(format!(
            "The repository {id} is in use. The number of live tasks that link to it is \
             {tasks}, and the number of its live webhook connections is {connections}. \
             Remove each link and each connection. Then delete the repository."
        )),
        RepositoryError::Database(e) => ApiError::internal(e),
        routing
        @ (RepositoryError::TeamNotBoardTeam { .. } | RepositoryError::NothingToRouteBy) => {
            ApiError::validation(routing.to_string())
        }
    }
}

/// Resolve a team reference (UUID or slug) to its live row, or 422.
pub(crate) fn resolve_team(conn: &mut PgConnection, reference: &str) -> Result<Team, ApiError> {
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
        .ok_or_else(|| {
            ApiError::validation(format!(
                "The team {reference:?} is not in the organization. Send the id or the \
                 slug of a team of the organization."
            ))
        })
}

/// The team's ONE live delivery board, or `None` when it has none or
/// several — the same exactly-one semantics routing uses
/// ([`repositories::delivery_board_for_team`]), so the directory, the
/// manage gate and routing never disagree (KAIROS-T-0112).
fn delivery_board_of(conn: &mut PgConnection, team_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    match repositories::delivery_board_for_team(conn, team_id) {
        Ok(board) => Ok(Some(board)),
        Err(RepositoryError::NoDeliveryBoard { .. }) => Ok(None),
        Err(e) => Err(map_error(e)),
    }
}

/// Org admin, or `manage_tasks` on the team's delivery board (team
/// membership implies it). A team with no delivery board is admin-only.
///
/// COLLIERY-T-0266: the refusal names the team, and it says who can do the
/// action. Until then it was the refusal of [`require_capability`], which
/// gives the id of a board and no more. The code and the `details` are the
/// same as before.
fn require_manage_for_team(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
) -> Result<(), ApiError> {
    let board = delivery_board_of(conn, team_id)?;
    let refusal = match require_capability(conn, slug, board, user, MANAGE) {
        Ok(()) => return Ok(()),
        Err(refusal) => refusal,
    };
    if refusal.code != "FORBIDDEN" {
        return Err(refusal);
    }
    let team: String = {
        use kairos_db::schema::teams::dsl;
        dsl::teams
            .filter(dsl::id.eq(team_id))
            .select(dsl::slug)
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?
            .unwrap_or_else(|| team_id.to_string())
    };
    let message = match board {
        Some(_) => format!(
            "This action requires the capability {MANAGE:?} on the delivery board of the \
             team {team:?}, the owner team of the repository. You do not have that \
             capability. Each member of the team has it, and an organization admin has \
             each capability. Ask a member of the team {team:?} or an organization admin \
             to do this."
        ),
        None => format!(
            "The team {team:?}, the owner team of the repository, does not have one live \
             delivery board. Thus this action requires the organization admin role. Ask \
             an organization admin to do this."
        ),
    };
    Err(ApiError::forbidden(message).with_details(refusal.details))
}

/// Add a repository to the directory as `user`: the ONE implementation of
/// `POST /api/repositories` and of the MCP tool `add_repository`
/// (COLLIERY-T-0266). The rule, the defaults and the refusals are here, so
/// the two surfaces cannot disagree. The activity row comes from
/// [`repositories::create`], with `user` as the actor.
pub(crate) fn add(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    body: dto::CreateRepositoryRequest,
) -> Result<dto::Repository, ApiError> {
    let forge: Forge = parse_enum(&body.forge, "forge", Forge::ALL)?;
    let team = resolve_team(conn, &body.team)?;
    require_manage_for_team(conn, tenant_slug, user, team.id)?;
    let created = repositories::create(
        conn,
        NewRepository {
            slug: body.slug.unwrap_or_else(|| {
                kairos_core::repositories::slug_from_full_name(&body.repo_full_name)
            }),
            forge,
            repo_full_name: body.repo_full_name,
            repo_url: body.repo_url,
            default_branch: body.default_branch.unwrap_or_else(|| "main".to_string()),
            team_id: team.id,
            description: body.description.unwrap_or_default(),
            created_by: user,
            updated_by: user,
        },
    )
    .map_err(map_error)?;
    render_one(conn, created)
}

/// The repository that `reference` names (slug or UUID), when `user` can
/// change it: the gate of [`change`], evaluated against the CURRENT owner
/// (COLLIERY-T-0266).
pub(crate) fn changeable(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
) -> Result<Repository, ApiError> {
    let current = repositories::resolve(conn, reference).map_err(map_error)?;
    require_manage_for_team(conn, tenant_slug, user, current.team_id)?;
    Ok(current)
}

/// What [`change`] did (COLLIERY-T-0267).
pub(crate) struct Change {
    /// The repository after the call.
    pub repository: dto::Repository,
    /// The names of the fields that have a new value, in the sequence of
    /// the body. It is empty when the call changed nothing.
    pub changed: Vec<&'static str>,
}

/// Change a repository as `user`: the ONE implementation of
/// `PATCH /api/repositories/{slug}` and of the MCP tool `update_repository`
/// (COLLIERY-T-0266). The activity row comes from [`repositories::update`],
/// with `user` as the actor.
///
/// COLLIERY-T-0267: only a value that is different from the value of the
/// row goes to the write. When no value is different, the function writes
/// nothing: `updated_at` stays, and the activity log gets no row. Until
/// then the MCP tool made the comparison and REST made none, so a PATCH
/// with the values of the row wrote a new `updated_at` and an activity row.
pub(crate) fn change(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
    body: dto::UpdateRepositoryRequest,
) -> Result<Change, ApiError> {
    let current = changeable(conn, tenant_slug, user, reference)?;
    // COLLIERY-T-0267: a body with no field is an input that does nothing.
    // The routes of boards, teams and streams refuse it, and this route
    // does the same. A body whose values are equal to the stored values is
    // a success that writes nothing: a client can send back what it read.
    if body.slug.is_none()
        && body.repo_url.is_none()
        && body.default_branch.is_none()
        && body.team.is_none()
        && body.description.is_none()
    {
        return Err(ApiError::validation(
            "The request has no field to change. Send one or more of slug, repo_url, \
             default_branch, team and description.",
        ));
    }
    let team_id = body
        .team
        .as_deref()
        .map(|reference| resolve_team(conn, reference).map(|t| t.id))
        .transpose()?
        .filter(|team| *team != current.team_id);
    // KAIROS-T-0112: re-homing needs the NEW owner's consent too —
    // manage on both delivery boards (org admin bypasses both).
    if let Some(new_team) = team_id {
        require_manage_for_team(conn, tenant_slug, user, new_team)?;
    }
    let different = |new: Option<String>, old: &str| new.filter(|new| new != old);
    let changes = RepositoryChangeset {
        slug: different(body.slug, &current.slug),
        repo_url: different(body.repo_url, &current.repo_url),
        default_branch: different(body.default_branch, &current.default_branch),
        team_id,
        description: different(body.description, &current.description),
        ..Default::default()
    };
    let changed: Vec<&'static str> = [
        ("slug", changes.slug.is_some()),
        ("repo_url", changes.repo_url.is_some()),
        ("default_branch", changes.default_branch.is_some()),
        ("team", changes.team_id.is_some()),
        ("description", changes.description.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect();
    let repository = if changed.is_empty() {
        current
    } else {
        repositories::update(conn, current.id, changes, user).map_err(map_error)?
    };
    Ok(Change {
        repository: render_one(conn, repository)?,
        changed,
    })
}

/// Render repositories with their team, delivery board and counts — two
/// batched queries for the whole list, never per row.
pub(crate) fn render(
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
        .filter(teams::deleted_at.is_null())
        .select(Team::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    let by_team: HashMap<Uuid, Team> = team_rows.into_iter().map(|t| (t.id, t)).collect();
    // One delivery board per team, with the SAME exactly-one semantics as
    // routing: a team with two delivery boards shows none here rather than
    // an arbitrary one (KAIROS-T-0112).
    let board_rows: Vec<(Uuid, Uuid)> = boards::table
        .filter(boards::team_id.eq_any(team_ids.iter().map(|id| Some(*id)).collect::<Vec<_>>()))
        .filter(boards::board_level.eq(BoardLevel::Delivery))
        .filter(boards::deleted_at.is_null())
        .select((boards::team_id.assume_not_null(), boards::id))
        .load(conn)
        .map_err(ApiError::internal)?;
    let mut board_count: HashMap<Uuid, usize> = HashMap::new();
    for (team, _) in &board_rows {
        *board_count.entry(*team).or_default() += 1;
    }
    let board_of: HashMap<Uuid, Uuid> = board_rows
        .into_iter()
        .filter(|(team, _)| board_count.get(team) == Some(&1))
        .collect();
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let counts: HashMap<Uuid, (i64, bool)> = repositories::counts(conn, &ids)
        .map_err(map_error)?
        .into_iter()
        .map(|c| (c.repository_id, (c.open_tasks, c.has_webhook)))
        .collect();
    // COLLIERY-T-3105: the status of the read token, never the token.
    let credentials =
        kairos_db::repository_credentials::statuses(conn, &ids).map_err(ApiError::internal)?;

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
                credential: crate::credentials::status_dto(credentials.get(&repo.id)),
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
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRepositoriesQuery {
    /// Only this team's repositories (UUID or slug).
    pub team: Option<String>,
    /// With `name`: the ONE repository registered under this forge
    /// (`github|gitlab|other`) and full name — how a checkout matches its
    /// git remote (KAIROS-T-0116). Returns an empty list when unknown.
    pub forge: Option<String>,
    /// `owner/repo`, paired with `forge`.
    pub name: Option<String>,
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
    ApiQuery(query): ApiQuery<ListRepositoriesQuery>,
) -> Result<Json<Vec<dto::Repository>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            if let (Some(forge), Some(name)) = (query.forge.as_deref(), query.name.as_deref()) {
                let forge: Forge = parse_enum(forge, "forge", Forge::ALL)?;
                let found =
                    repositories::find_by_forge_name(conn, forge, name).map_err(map_error)?;
                return render(conn, found.into_iter().collect());
            }
            if query.forge.is_some() != query.name.is_some() {
                return Err(ApiError::validation(
                    "The parameters forge and name go together. Send the two, or send \
                     none of them.",
                ));
            }
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

/// The documents and the ADRs that impact a repository, as the wire type
/// (COLLIERY-T-0269): the ONE read for `GET /api/repositories/{slug}` and
/// for the MCP tool `get_repository`.
pub(crate) fn impacted_by(
    conn: &mut PgConnection,
    repository_id: Uuid,
    include_archived: bool,
) -> Result<Vec<dto::ImpactingItem>, ApiError> {
    Ok(
        kairos_db::impacts::items_of_repository(conn, repository_id, include_archived)
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|item| dto::ImpactingItem {
                short_code: item.short_code,
                title: item.title,
                entity_type: item.entity_type,
                document_type: item.document_type,
                lifecycle: item.lifecycle,
                column: item.column_name,
                archived_at: item.archived_at.map(|at| at.to_rfc3339()),
            })
            .collect(),
    )
}

/// One repository (open tenant-wide): the directory row plus its webhook
/// connection id and in-flight links — everything an agent reads before
/// working in, or filing against, a codebase.
///
/// `impacted_by` has the documents and the ADRs that impact the
/// repository (COLLIERY-T-0269): its vision, its architecture, the
/// decisions about it. It has the live items only.
/// `?include_deleted=true` adds the archived items, each marked with
/// `archived_at`.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        dto::RepositoryDetailQuery,
    ),
    responses(
        (status = 200, description = "The repository", body = dto::RepositoryDetail),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_repository(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(query): ApiQuery<dto::RepositoryDetailQuery>,
) -> Result<Json<dto::RepositoryDetail>, ApiError> {
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            let repo_id = repo.id;
            let connection_id = kairos_db::forge::find_connection_for_repository(conn, repo_id)
                .map_err(super::forge::map_error)?
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
                impacted_by: impacted_by(conn, repo_id, query.include_deleted)?,
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
        (status = 422, description = "Bad slug/forge, unknown team, or a field that does not have its form", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<dto::Repository>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| add(conn, &slug, user, body))
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Edit a repository (same gate as create, evaluated against the CURRENT
/// owner). Re-homing to another team does not touch the tasks that link to
/// the repository.
///
/// A body can have only the values that the repository has. That request
/// gets 200 with the repository, and the server writes nothing
/// (COLLIERY-T-0267).
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
        (status = 422, description = "Bad slug, unknown team, or a field that does not have its form", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateRepositoryRequest>,
) -> Result<Json<dto::Repository>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            change(conn, &tenant_slug, user, &slug, body)
        })
        .await?;
    Ok(Json(updated.repository))
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
