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
    Liveness, board_id_by_ref, clamp_list, map_board_error, map_item_error, opt_board_id_by_ref,
    parse_enum, parse_opt_uuid, parse_uuid, require_capability, require_item_edit,
    short_code_not_found,
};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family. MCP `set_repository`
/// reads it from here, so the tool and the route name one capability
/// (COLLIERY-T-0220).
pub(crate) const MANAGE: &str = "manage_tasks";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/tasks/{short_code}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/api/tasks/{short_code}/transition", post(transition_task))
        .route("/api/tasks/{short_code}/move", post(move_task))
        .route("/api/tasks/{short_code}/work-class", post(set_work_class))
        .route(
            "/api/tasks/{short_code}/repository",
            axum::routing::put(set_repository),
        )
}

pub(crate) use kairos_db::repositories::TaskRoute;

/// [`RepositoryError`] → HTTP for the routing paths (422 for every
/// "you named something wrong", 500 for the database).
pub(crate) fn map_repository_error(e: repositories::RepositoryError) -> ApiError {
    use repositories::RepositoryError as E;
    match e {
        E::Database(e) => ApiError::internal(e),
        other => ApiError::validation(other.to_string()),
    }
}

/// The routing decision lives in `kairos_db::repositories::route_task`
/// (KAIROS-T-0112); this is its HTTP error mapping, shared by the task
/// handler, MCP `create_item`, and the board view.
pub(crate) fn resolve_routing(
    conn: &mut PgConnection,
    board_id: Option<Uuid>,
    team_id: Option<Uuid>,
    repository: Option<&str>,
) -> Result<TaskRoute, ApiError> {
    repositories::route_task(conn, board_id, team_id, repository).map_err(map_repository_error)
}

/// What [`require_task_create_capability`] decided about the caller of a
/// task create (COLLIERY-T-0218, COLLIERY-A-0023 decisions 7 and 8).
///
/// The decision is a value, not only a pass, because the rule has two
/// halves: who may create, and which lane the task is born in. HTTP and MCP
/// both take the lane from [`TaskCreateAccess::work_class`], so neither
/// holds a copy of the rule and the two cannot diverge (the KAIROS-T-0096
/// lesson).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskCreateAccess {
    /// The caller holds `manage_tasks` on the target board (or is an org
    /// admin). Any column, any work class, and the defaults of
    /// KAIROS-T-0077.
    Manager,
    /// The caller does not manage the target board. The task is a REQUEST
    /// to the team of that board: entry column, support lane.
    Request,
}

impl TaskCreateAccess {
    /// The work class to store on the created task.
    ///
    /// A manager gets what was always true (KAIROS-T-0077): the work class
    /// they sent, else `support` for a support-type task, else `planned`.
    ///
    /// A request is ALWAYS `support`, whatever its `task_type`
    /// (COLLIERY-A-0023 decision 8). A team plans its own work; a different
    /// team cannot put a card in its planned lane. The receiving team can
    /// move the card to the planned lane afterwards with the work-class
    /// endpoint. An explicit `planned` never reaches this function for a
    /// request: [`require_task_create_capability`] refuses it, so the answer
    /// here does not silently overrule what the caller asked for.
    pub(crate) fn work_class(self, requested: Option<WorkClass>, task_type: TaskType) -> WorkClass {
        match self {
            Self::Request => WorkClass::Support,
            Self::Manager => requested.unwrap_or(if task_type == TaskType::Support {
                WorkClass::Support
            } else {
                WorkClass::Planned
            }),
        }
    }
}

/// Authorize a task CREATE (KAIROS-T-0105, amended by COLLIERY-T-0218 for
/// COLLIERY-A-0023 decisions 7 and 8). `manage_tasks` on the target board
/// as always: that caller is a [`TaskCreateAccess::Manager`] and nothing
/// below applies to them.
///
/// Every other caller sends a REQUEST. Teams request work of each other; no
/// team pushes work to a different team. The computed `file_backlog`
/// applies ONLY when all of:
///
/// - the target board is a delivery board (the check inside
///   `abac::check_file_backlog`),
/// - the target column is the board's ENTRY column (the default when no
///   column is named — [`boards::entry_column`], so explicit and defaulted
///   agree),
/// - the caller did not ask for the work class `planned`.
///
/// The repository is NOT part of the condition. Until COLLIERY-T-0218 it
/// was: a create with no repository was refused, because the rule was
/// written when a repository chose the board (KAIROS-A-0019 §4). Since
/// COLLIERY-T-0217 the repository is only a link, so it could no longer say
/// anything about whose board the task is on, and a request for work that
/// has no codebase yet had no way in.
///
/// An explicitly requested non-entry column by a non-manager is a 403,
/// never silently re-routed. An explicitly requested `planned` is a 403 for
/// the same reason and of the same shape: the request is well formed, and
/// what the caller lacks is `manage_tasks` on that board. Shared by HTTP and
/// MCP so the two entry points cannot diverge (the KAIROS-T-0096 lesson).
pub(crate) fn require_task_create_capability(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    route: &TaskRoute,
    column_id: Option<Uuid>,
    requested_work_class: Option<WorkClass>,
) -> Result<TaskCreateAccess, ApiError> {
    use kairos_db::abac;
    let manages =
        abac::authorize(conn, slug, route.board_id, user, MANAGE).map_err(super::map_abac_error)?;
    if manages {
        return Ok(TaskCreateAccess::Manager);
    }
    let targets_entry = match column_id {
        None => true,
        Some(column) => {
            boards::entry_column(conn, route.board_id).map_err(ApiError::internal)? == Some(column)
        }
    };
    if !targets_entry {
        return Err(ApiError::capability_required(MANAGE, Some(route.board_id)));
    }
    if requested_work_class == Some(WorkClass::Planned) {
        return Err(ApiError::forbidden(format!(
            "A request to a board that you do not manage goes to the support lane. \
             Remove `work_class`, or send `support`. \
             The work class `planned` requires capability {MANAGE:?} on board {}.",
            route.board_id
        ))
        .with_details(json!({
            "required_capability": MANAGE,
            "board_id": route.board_id,
        })));
    }
    require_capability(
        conn,
        slug,
        Some(route.board_id),
        user,
        kairos_core::abac::FILE_BACKLOG,
    )?;
    Ok(TaskCreateAccess::Request)
}

/// Load the task with this short code, or 404. Archived work is served only
/// when the caller asks for it (KAIROS-A-0020) — every write path passes
/// [`Liveness::LiveOnly`], which is what keeps archived work read-only
/// without a second guard.
fn load(conn: &mut PgConnection, short_code: &str, liveness: Liveness) -> Result<Task, ApiError> {
    use kairos_db::schema::tasks::dsl;
    let mut query = dsl::tasks
        .filter(dsl::short_code.eq(short_code))
        .into_boxed();
    if liveness == Liveness::LiveOnly {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query
        .select(Task::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("task", short_code))
}

/// List tasks (open tenant-wide, S-0005 list envelope).
///
/// `?include_deleted=true` widens the listing to archived work, each row
/// marked with `archived_at` (KAIROS-A-0020 rule 2). Default false: rule 3
/// is that a listing nobody asked hides put-away work.
#[utoipa::path(
    get,
    path = "/api/tasks",
    tag = "tasks",
    params(dto::ListQuery),
    responses(
        (status = 200, description = "Page of tasks", body = dto::ListEnvelope<dto::Task>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<dto::ListQuery>,
) -> Result<Json<dto::ListEnvelope<dto::Task>>, ApiError> {
    let (limit, offset, liveness) = clamp_list(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::tasks::dsl;
            // ONE predicate, applied to both the count and the page: a
            // list that reports 40 and returns 12 is a worse bug than the
            // one the opt-in exists to fix (KAIROS-T-0159).
            let visible = || {
                let mut query = dsl::tasks.into_boxed();
                if liveness == Liveness::LiveOnly {
                    query = query.filter(dsl::deleted_at.is_null());
                }
                query
            };
            let total: i64 = visible()
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Task> = visible()
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
            let task = load(conn, &short_code, Liveness::IncludeArchived)?.into_dto();
            attach_repository(conn, task).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(task))
}

/// Create a task. `task_type` defaults to `task`.
///
/// A caller with `manage_tasks` on the target board can use any column and
/// any work class. Every other member of the tenant sends a request
/// (`file_backlog`, COLLIERY-T-0218, COLLIERY-A-0023): the target is a
/// delivery board, the task goes to the entry column, and the work class
/// is `support`.
///
/// Name the board with `board_id`, or name a team with `team_id` to use
/// the delivery board of that team (COLLIERY-T-0217, COLLIERY-A-0023).
/// `repository` is an optional link. It can be any live repository, and it
/// does not choose the board.
#[utoipa::path(
    post,
    path = "/api/tasks",
    tag = "tasks",
    request_body = dto::CreateTaskRequest,
    responses(
        (status = 201, description = "Created", body = dto::Task),
        (status = 403, description = "Missing capability. For a caller without `manage_tasks` on the board: a column that is not the entry column, the work class `planned`, or a board that is not a delivery board", body = dto::ErrorEnvelope),
        (status = 422, description = "No board and no team, a team that is not the team of the board, a team without exactly one delivery board, unknown board/column/repository, or bad enum value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateTaskRequest>,
) -> Result<(StatusCode, Json<dto::Task>), ApiError> {
    // KAIROS-T-0150: slug or UUID; resolved in the closure below.
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let team_id = parse_opt_uuid(body.team_id.as_deref(), "team_id")?;
    let repository = body.repository.clone();
    let task_type = body
        .task_type
        .as_deref()
        .map(|v| parse_enum(v, "task_type", TaskType::ALL))
        .transpose()?
        .unwrap_or(TaskType::Task);
    // The caller's wish only. Which work class is STORED depends on who the
    // caller is on the target board, so it is decided below, after the
    // capability check (COLLIERY-T-0218). Until then the KAIROS-T-0077
    // default was applied here, before anyone had asked who was calling.
    let requested_work_class = body
        .work_class
        .as_deref()
        .map(|v| parse_enum(v, "work_class", WorkClass::ALL))
        .transpose()?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board_id = opt_board_id_by_ref(conn, body.board_id.as_deref())?;
            let route = resolve_routing(conn, board_id, team_id, repository.as_deref())?;
            let access = require_task_create_capability(
                conn,
                &slug,
                user,
                &route,
                column_id,
                requested_work_class,
            )?;
            let work_class = access.work_class(requested_work_class, task_type);
            let created = items::create_task(
                conn,
                items::CreateTask {
                    board_id: route.board_id,
                    column_id,
                    title: &body.title,
                    content: &body.content,
                    task_type,
                    work_class,
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

/// Update task content (KAIROS-A-0004 optimistic concurrency).
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// task, holds `manage_tasks` on its board, or is an organization admin.
#[utoipa::path(
    patch,
    path = "/api/tasks/{short_code}",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Task),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateContentRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, task.id, ItemType::Task)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Task, task.id, update, user) {
                Ok(_) => {
                    let task = load(conn, &short_code, Liveness::LiveOnly)?.into_dto();
                    attach_repository(conn, task).map_err(ApiError::internal)
                }
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code, Liveness::LiveOnly)?.into_dto();
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

/// Soft-delete a task (KAIROS-A-0001).
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// task, holds `manage_tasks` on its board, or is an organization admin.
#[utoipa::path(
    delete,
    path = "/api/tasks/{short_code}",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
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
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            // The edit rule for the task, and then for each descendant
            // (COLLIERY-T-0234): `archive_item` does the two.
            let outcome = super::cascade::archive_item(conn, &slug, user, task.id, ItemType::Task)?;
            Ok(super::cascade::delete_response(outcome))
        })
        .await?;
    Ok(Json(outcome))
}

/// Move a task between the Planned/Support lanes (KAIROS-T-0077;
/// requires `transition_items` on the task's board — lane moves are
/// board moves in UX terms, though the rules engine is never consulted).
/// The creator of the task gets no right here (COLLIERY-T-0228).
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
    ApiJson(body): ApiJson<dto::SetWorkClassRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let work_class = parse_enum(&body.work_class, "work_class", WorkClass::ALL)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of an item needs this capability as all others do,
            // because a team controls its own plan (COLLIERY-T-0218).
            require_capability(conn, &slug, Some(task.board_id), user, "transition_items")?;
            let updated = items::set_task_work_class(conn, task.id, work_class, user)
                .map_err(map_item_error)?;
            attach_repository(conn, updated.into_dto()).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(updated))
}

/// Set or clear the link from a task to a repository: the ONE write path of
/// `PUT /api/tasks/{short_code}/repository` and of MCP `set_repository`
/// (COLLIERY-T-0220, COLLIERY-A-0023).
///
/// The two entry points load the task in their own way, and each calls the
/// edit rule (`require_item_edit`, COLLIERY-T-0228). All that follows the gate is
/// here, so the two cannot give different results (the KAIROS-T-0096 lesson):
/// which repository a reference names, what "unknown" means, and the write.
///
/// `repository` is a slug or a UUID. `None` clears the link, and so does a
/// reference that is empty or only whitespace (COLLIERY-T-0231). That rule
/// is HERE, not in the callers. COLLIERY-T-0220 put it in the MCP tool
/// alone, so MCP cleared the link for `""` while REST looked for a
/// repository with an empty name and answered 422: two entry points, one
/// input, two results, which is what this function exists to prevent.
///
/// COLLIERY-T-0217 (COLLIERY-A-0023): the repository is a link, so the only
/// question is whether it is live. Until then this went through
/// `resolve_routing` with the board of the task, which refused a repository
/// owned by any team but the team of that board. The owner is not read now,
/// and the board and the team of the task are not written.
pub(crate) fn link_task_to_repository(
    conn: &mut PgConnection,
    task_id: Uuid,
    repository: Option<&str>,
    user: Uuid,
) -> Result<Task, ApiError> {
    let repository_id = repository
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
        .map(|reference| repositories::resolve(conn, reference).map(|found| found.id))
        .transpose()
        .map_err(map_repository_error)?;
    items::set_task_repository(conn, task_id, repository_id, user).map_err(map_item_error)
}

/// Set the repository a task links to, or clear it (KAIROS-T-0104).
/// The link can be any live repository. The board and the team of the task
/// do not change (COLLIERY-T-0217, COLLIERY-A-0023).
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// task, holds `manage_tasks` on its board, or is an organization admin.
#[utoipa::path(
    put,
    path = "/api/tasks/{short_code}/repository",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = kairos_client::types_repositories::SetTaskRepositoryRequest,
    responses(
        (status = 200, description = "Repository link updated. `null` or an empty string clears it", body = dto::Task),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown repository", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<kairos_client::types_repositories::SetTaskRepositoryRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, task.id, ItemType::Task)?;
            let updated = link_task_to_repository(conn, task.id, body.repository.as_deref(), user)?;
            attach_repository(conn, updated.into_dto()).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(updated))
}

/// Move a task to another DELIVERY board (KAIROS-I-0012): it lands in the
/// target's entry column and follows the target's team. Requires
/// `manage_tasks` on the current board AND on the target (org admins
/// bypass, as everywhere). The move does not look at the repository of the
/// task, and the task keeps it (COLLIERY-T-0217, COLLIERY-A-0023). The
/// creator of the task gets no right here (COLLIERY-T-0228).
#[utoipa::path(
    post,
    path = "/api/tasks/{short_code}/move",
    tag = "tasks",
    params(("short_code" = String, Path, description = "Task short code")),
    request_body = dto::MoveTaskRequest,
    responses(
        (status = 200, description = "Moved (new board, entry column)", body = dto::Task),
        (status = 403, description = "Missing manage_tasks on either board", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code or board", body = dto::ErrorEnvelope),
        (status = 422, description = "SAME_BOARD | NOT_DELIVERY_BOARD | NO_ENTRY_COLUMN", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn move_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::MoveTaskRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let moved = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            let target = board_id_by_ref(conn, &body.board)?;
            // Two-sided: the work leaves one team's board and lands on
            // another's, so the caller needs the capability on the two.
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of a task needs the capability on the two boards
            // as all others do, because a team controls its own plan
            // (COLLIERY-T-0218).
            require_capability(conn, &slug, Some(task.board_id), user, MANAGE)?;
            require_capability(conn, &slug, Some(target), user, MANAGE)?;
            boards::move_task(conn, task.id, target, user).map_err(map_board_error)?;
            let moved = load(conn, &short_code, Liveness::LiveOnly)?.into_dto();
            attach_repository(conn, moved).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(moved))
}

/// Move a task to another column (requires `transition_items` on the
/// task's board). The creator of the task gets no right here
/// (COLLIERY-T-0228).
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
    ApiJson(body): ApiJson<dto::TransitionRequest>,
) -> Result<Json<dto::Task>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let task = load(conn, &short_code, Liveness::LiveOnly)?;
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of an item needs this capability as all others do,
            // because a team controls its own plan (COLLIERY-T-0218).
            require_capability(conn, &slug, Some(task.board_id), user, "transition_items")?;
            boards::transition_task(conn, task.id, to_column_id, user).map_err(map_board_error)?;
            Ok(load(conn, &short_code, Liveness::LiveOnly)?.into_dto())
        })
        .await?;
    Ok(Json(transitioned))
}

#[cfg(test)]
mod task_create_access_tests {
    //! COLLIERY-T-0218: the lane half of the create rule, with no database.
    //! The capability half needs one and is covered by
    //! `tests/file_backlog.rs`.
    use super::*;

    #[test]
    fn a_manager_keeps_the_defaults_of_kairos_t_0077() {
        let manager = TaskCreateAccess::Manager;
        assert_eq!(manager.work_class(None, TaskType::Task), WorkClass::Planned);
        assert_eq!(manager.work_class(None, TaskType::Bug), WorkClass::Planned);
        assert_eq!(
            manager.work_class(None, TaskType::Support),
            WorkClass::Support
        );
    }

    #[test]
    fn a_manager_gets_the_work_class_they_sent() {
        let manager = TaskCreateAccess::Manager;
        assert_eq!(
            manager.work_class(Some(WorkClass::Support), TaskType::Task),
            WorkClass::Support
        );
        assert_eq!(
            manager.work_class(Some(WorkClass::Planned), TaskType::Support),
            WorkClass::Planned
        );
    }

    #[test]
    fn a_request_is_support_for_every_task_type() {
        let request = TaskCreateAccess::Request;
        for task_type in TaskType::ALL {
            assert_eq!(
                request.work_class(None, *task_type),
                WorkClass::Support,
                "{task_type:?}"
            );
            assert_eq!(
                request.work_class(Some(WorkClass::Support), *task_type),
                WorkClass::Support,
                "{task_type:?}"
            );
        }
    }
}
