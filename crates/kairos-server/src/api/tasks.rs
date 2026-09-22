//! `/api/tasks` (KAIROS-S-0005) — see [`super`] for the shared T-0018
//! handler pattern.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::enums::{TaskType, WorkClass};
use kairos_db::models::items::Task;
use kairos_db::{boards, items, repositories};
use serde_json::json;
use uuid::Uuid;

use super::convert::{IntoDto, attach_repositories, attach_repository};
use super::{
    clamp_pagination, map_board_error, map_item_error, parse_enum, parse_opt_uuid, parse_uuid,
    require_capability, short_code_not_found,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family.
const MANAGE: &str = "manage_tasks";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/tasks/{short_code}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/api/tasks/{short_code}/transition", post(transition_task))
        .route("/api/tasks/{short_code}/work-class", post(set_work_class))
        .route(
            "/api/tasks/{short_code}/repository",
            axum::routing::put(set_repository),
        )
}

/// Where a new task lands (KAIROS-T-0104, A-0019 §2).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaskRoute {
    pub board_id: Uuid,
    pub team_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
}

/// [`RepositoryError`] → HTTP for the routing paths.
pub(crate) fn map_repository_error(e: repositories::RepositoryError) -> ApiError {
    use repositories::RepositoryError as E;
    match e {
        E::NotFound(id) => ApiError::validation(format!("repository {id} does not exist")),
        E::SlugNotFound(slug) => {
            ApiError::validation(format!("repository {slug:?} does not exist"))
        }
        E::NoDeliveryBoard { team, count } => ApiError::validation(format!(
            "repository's owning team {team} has {count} live delivery boards; \
             exactly one is needed to route the task"
        )),
        E::Database(e) => ApiError::internal(e),
        other => ApiError::validation(other.to_string()),
    }
}

/// Resolve the routing triple for a task write from what the caller sent
/// (A-0019 §2; I-0010 §D2):
///
/// 1. repository only → the repo's owning team and that team's delivery
///    board;
/// 2. repository + board → the board must BE that delivery board, and an
///    explicit team must be the owning team (else 422);
/// 3. no repository → today's behaviour exactly (`board_id` required).
///
/// One helper so the HTTP handler and the MCP `create_item` path agree —
/// the class of divergence KAIROS-T-0096 recorded for `set_metadata`.
pub(crate) fn resolve_routing(
    conn: &mut PgConnection,
    board_id: Option<Uuid>,
    team_id: Option<Uuid>,
    repository: Option<&str>,
) -> Result<TaskRoute, ApiError> {
    let Some(reference) = repository else {
        let Some(board_id) = board_id else {
            return Err(ApiError::validation(
                "board_id is required unless repository_id is given (a repository routes \
                 the task to its owning team's delivery board)",
            ));
        };
        return Ok(TaskRoute {
            board_id,
            team_id,
            repository_id: None,
        });
    };
    let repo = repositories::resolve(conn, reference).map_err(map_repository_error)?;
    let delivery_board =
        repositories::delivery_board_for_team(conn, repo.team_id).map_err(map_repository_error)?;
    if let Some(board_id) = board_id
        && board_id != delivery_board
    {
        return Err(ApiError::validation(format!(
            "repository {} belongs to team {}, whose delivery board is {delivery_board}, \
             not {board_id}",
            repo.slug, repo.team_id
        )));
    }
    if let Some(team_id) = team_id
        && team_id != repo.team_id
    {
        return Err(ApiError::validation(format!(
            "repository {} belongs to team {}, not {team_id}",
            repo.slug, repo.team_id
        )));
    }
    Ok(TaskRoute {
        board_id: delivery_board,
        team_id: Some(repo.team_id),
        repository_id: Some(repo.id),
    })
}

/// Load the live task with this short code, or 404.
fn load(conn: &mut PgConnection, short_code: &str) -> Result<Task, ApiError> {
    use kairos_db::schema::tasks::dsl;
    dsl::tasks
        .filter(dsl::short_code.eq(short_code))
        .filter(dsl::deleted_at.is_null())
        .select(Task::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("task", short_code))
}

/// List tasks (open tenant-wide, S-0005 list envelope).
#[utoipa::path(
    get,
    path = "/api/tasks",
    tag = "tasks",
    params(dto::Pagination),
    responses(
        (status = 200, description = "Page of tasks", body = dto::ListEnvelope<dto::Task>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<dto::Pagination>,
) -> Result<Json<dto::ListEnvelope<dto::Task>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::tasks::dsl;
            let total: i64 = dsl::tasks
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Task> = dsl::tasks
                .filter(dsl::deleted_at.is_null())
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Task::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut items: Vec<dto::Task> = rows.into_iter().map(IntoDto::into_dto).collect();
            attach_repositories(conn, &mut items).map_err(ApiError::internal)?;
            Ok(dto::ListEnvelope {
                items,
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one task by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/tasks/{short_code}",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    responses(
        (status = 200, description = "The task", body = dto::Task),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_task(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Task>, ApiError> {
    let task = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?.into_dto();
            attach_repository(conn, task).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(task))
}

/// Create a task (requires `manage_tasks` on the target board).
/// `task_type` defaults to `task`.
#[utoipa::path(
    post,
    path = "/api/tasks",
    tag = "tasks",
    request_body = dto::CreateTaskRequest,
    responses(
        (status = 201, description = "Created", body = dto::Task),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown board/column or bad enum value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateTaskRequest>,
) -> Result<(StatusCode, Json<dto::Task>), ApiError> {
    let board_id = parse_opt_uuid(body.board_id.as_deref(), "board_id")?;
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let team_id = parse_opt_uuid(body.team_id.as_deref(), "team_id")?;
    let repository = body.repository_id.clone();
    let task_type = body
        .task_type
        .as_deref()
        .map(|v| parse_enum(v, "task_type", TaskType::ALL))
        .transpose()?
        .unwrap_or(TaskType::Task);
    // KAIROS-T-0077: a support-type ticket is born in the Support lane
    // unless the caller says otherwise; everything else defaults planned.
    let work_class = body
        .work_class
        .as_deref()
        .map(|v| parse_enum(v, "work_class", WorkClass::ALL))
        .transpose()?
        .unwrap_or(if task_type == TaskType::Support {
            WorkClass::Support
        } else {
            WorkClass::Planned
        });
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let route = resolve_routing(conn, board_id, team_id, repository.as_deref())?;
            require_capability(conn, &slug, Some(route.board_id), user, MANAGE)?;
            let created = items::create_task(
                conn,
                items::CreateTask {
                    board_id: route.board_id,
                    column_id,
                    title: &body.title,
                    content: &body.content,
                    task_type,
                    work_class,
                    team_id: route.team_id,
                    repository_id: route.repository_id,
                },
                user,
            )
            .map_err(map_item_error)?;
            attach_repository(conn, created.into_dto()).map_err(ApiError::internal)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update task content (KAIROS-A-0004 optimistic concurrency; requires
/// `manage_tasks` on the task's board).
#[utoipa::path(
    patch,
    path = "/api/tasks/{short_code}",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Task),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::UpdateContentRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(task.board_id), user, MANAGE)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Task, task.id, update, user) {
                Ok(_) => {
                    let task = load(conn, &short_code)?.into_dto();
                    attach_repository(conn, task).map_err(ApiError::internal)
                }
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code)?.into_dto();
                    Err(ApiError::conflict(format!(
                        "version mismatch: expected {expected_version}, current is {current_version}"
                    ))
                    .with_details(json!({ "current": current })))
                }
                Err(e) => Err(map_item_error(e)),
            }
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a task (KAIROS-A-0001; requires `manage_tasks` on the
/// task's board).
#[utoipa::path(
    delete,
    path = "/api/tasks/{short_code}",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::DeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(task.board_id), user, MANAGE)?;
            let outcome = items::soft_delete_item(conn, ItemType::Task, task.id, user)
                .map_err(map_item_error)?;
            Ok(dto::DeleteResponse {
                short_code: outcome.root_short_code,
                cascade_count: outcome.cascaded_short_codes.len() as i64,
                cascaded_short_codes: outcome.cascaded_short_codes,
            })
        })
        .await?;
    Ok(Json(outcome))
}

/// Move a task between the Planned/Support lanes (KAIROS-T-0077;
/// requires `transition_items` on the task's board — lane moves are
/// board moves in UX terms, though the rules engine is never consulted).
#[utoipa::path(
    post,
    path = "/api/tasks/{short_code}/work-class",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = dto::SetWorkClassRequest,
    responses(
        (status = 200, description = "Lane updated", body = dto::Task),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Bad work_class value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_work_class(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::SetWorkClassRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let work_class = parse_enum(&body.work_class, "work_class", WorkClass::ALL)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(task.board_id), user, "transition_items")?;
            let updated = items::set_task_work_class(conn, task.id, work_class, user)
                .map_err(map_item_error)?;
            attach_repository(conn, updated.into_dto()).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(updated))
}

/// Bind a task to a repository, re-home it to another repository of the
/// same team, or clear it (KAIROS-T-0104, A-0019). The repository must be
/// owned by the team whose delivery board the task sits on — the same
/// rule create enforces. Requires `manage_tasks` on the task's board.
#[utoipa::path(
    put,
    path = "/api/tasks/{short_code}/repository",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = kairos_client::types_repositories::SetTaskRepositoryRequest,
    responses(
        (status = 200, description = "Repository binding updated", body = dto::Task),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown repository, or one owned by another team", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<kairos_client::types_repositories::SetTaskRepositoryRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(task.board_id), user, MANAGE)?;
            let repository_id = match body.repository.as_deref() {
                None => None,
                Some(reference) => {
                    // Same rule as create: the repo must route to THIS board.
                    let route = resolve_routing(conn, Some(task.board_id), None, Some(reference))?;
                    route.repository_id
                }
            };
            let updated = items::set_task_repository(conn, task.id, repository_id, user)
                .map_err(map_item_error)?;
            attach_repository(conn, updated.into_dto()).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(updated))
}

/// Move a task to another column (requires `transition_items` on the
/// task's board).
#[utoipa::path(
    post,
    path = "/api/tasks/{short_code}/transition",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = dto::TransitionRequest,
    responses(
        (status = 200, description = "Transitioned (new column)", body = dto::Task),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Invalid transition; details.allowed_targets lists valid moves", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn transition_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::TransitionRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(task.board_id), user, "transition_items")?;
            boards::transition_task(conn, task.id, to_column_id, user).map_err(map_board_error)?;
            Ok(load(conn, &short_code)?.into_dto())
        })
        .await?;
    Ok(Json(transitioned))
}
