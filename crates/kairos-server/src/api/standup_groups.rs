//! The AdHoc stand-up (KAIROS-T-0764, COLLIERY-I-0674, KAIROS-A-0084): a
//! saved, named group of people of the organization.
//!
//! - `GET` and `POST /api/standup-groups`: each group, and a new group.
//! - `GET`, `PATCH` and `DELETE /api/standup-groups/{id}`: one group.
//! - `GET /api/standup-groups/{id}/people/{user_id}`: the stop of one
//!   person of the group. It is the work of the person by ownership on each
//!   board (`kairos_db::my_work::person_work`), not in one team. There is
//!   no Unclaimed stop.
//!
//! Each member of the organization can create a group, edit each group and
//! read each group. Only the creator of a group can delete it (403 for the
//! others). An agent key acts for its person. The team stand-up has no MCP
//! tools, so the AdHoc stand-up has none.

use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types_work::{
    CreateStandupGroupRequest, MyWork, StandupGroup, StandupGroupList, StandupGroupPerson,
    UpdateStandupGroupRequest,
};
use kairos_db::standup_groups::{self as groups, DeleteOutcome, GroupError};
use uuid::Uuid;

use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The longest name of a group, in characters.
const NAME_MAX: usize = 100;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/standup-groups",
            get(list_standup_groups).post(create_standup_group),
        )
        .route(
            "/api/standup-groups/{id}",
            get(get_standup_group)
                .patch(update_standup_group)
                .delete(delete_standup_group),
        )
        .route(
            "/api/standup-groups/{id}/people/{user_id}",
            get(get_standup_group_person),
        )
}

fn not_found(reference: &str) -> ApiError {
    ApiError::not_found(format!(
        "No stand-up group has the id {reference:?}. GET /api/standup-groups gives each group."
    ))
    .with_details(serde_json::json!({ "field": "id" }))
}

/// The id of the group `reference`, or 404.
fn group_id(conn: &mut PgConnection, reference: &str) -> Result<groups::StandupGroup, ApiError> {
    let id = Uuid::parse_str(reference).map_err(|_| not_found(reference))?;
    groups::get(conn, id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| not_found(reference))
}

/// The name, trimmed, or 422.
fn checked_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(ApiError::validation(format!(
            "The name of a stand-up group must have 1 to {NAME_MAX} characters."
        ))
        .with_details(serde_json::json!({ "field": "name" })));
    }
    Ok(name.to_string())
}

/// The ids of the people that `references` name: each a person of the
/// organization (not a service account), with no duplicates. Else 422.
fn checked_people(
    conn: &mut PgConnection,
    slug: &str,
    caller: Uuid,
    references: &[String],
) -> Result<Vec<Uuid>, ApiError> {
    let mut out: Vec<Uuid> = Vec::new();
    for reference in references {
        let user = crate::api::org::structure_grants::resolve_member(
            conn, slug, caller, reference, "people",
        )?;
        if !kairos_db::task_claims::is_person(conn, user.id).map_err(ApiError::internal)? {
            return Err(ApiError::validation(format!(
                "{reference:?} is a service account. A stand-up group has people only."
            ))
            .with_details(serde_json::json!({ "field": "people" })));
        }
        if !out.contains(&user.id) {
            out.push(user.id);
        }
    }
    Ok(out)
}

/// The refusal of a write of a group.
fn write_error(error: GroupError, name: Option<&str>, reference: &str) -> ApiError {
    match error {
        GroupError::NameTaken => ApiError::conflict(format!(
            "A different stand-up group has the name {:?}. Use a different name.",
            name.unwrap_or_default()
        ))
        .with_details(serde_json::json!({ "field": "name" })),
        GroupError::NotFound => not_found(reference),
        GroupError::Db(e) => ApiError::internal(e),
    }
}

/// The groups as the wire type, with their people. Three queries.
fn to_wire(
    conn: &mut PgConnection,
    caller: Uuid,
    rows: Vec<groups::StandupGroup>,
) -> Result<Vec<StandupGroup>, ApiError> {
    let ids: Vec<Uuid> = rows.iter().map(|g| g.id).collect();
    let mut people = groups::people_of(conn, &ids).map_err(ApiError::internal)?;
    let creators: Vec<Uuid> = rows.iter().map(|g| g.created_by).collect();
    let display: HashMap<Uuid, String> =
        kairos_db::task_claims::display_names(conn, &creators).map_err(ApiError::internal)?;
    let user: HashMap<Uuid, String> =
        kairos_db::task_claims::user_names(conn, &creators).map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|g| StandupGroup {
            id: g.id.to_string(),
            name: g.name,
            created_by: StandupGroupPerson {
                user_id: g.created_by.to_string(),
                user_name: user.get(&g.created_by).cloned().unwrap_or_default(),
                display_name: display.get(&g.created_by).cloned().unwrap_or_default(),
            },
            can_delete: g.created_by == caller,
            people: people
                .remove(&g.id)
                .unwrap_or_default()
                .into_iter()
                .map(|p| StandupGroupPerson {
                    user_id: p.user_id.to_string(),
                    user_name: p.user_name,
                    display_name: p.display_name,
                })
                .collect(),
            created_at: g
                .created_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            updated_at: g
                .updated_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
        .collect())
}

fn one(
    conn: &mut PgConnection,
    caller: Uuid,
    row: groups::StandupGroup,
) -> Result<StandupGroup, ApiError> {
    to_wire(conn, caller, vec![row])?
        .pop()
        .ok_or_else(|| ApiError::internal("the group is gone"))
}

/// Each stand-up group of the organization, by name, with its people.
/// Each member of the organization reads each group.
#[utoipa::path(
    get,
    path = "/api/standup-groups",
    tag = "teams",
    responses(
        (status = 200, description = "Each group, by name", body = StandupGroupList),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_standup_groups(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<StandupGroupList>, ApiError> {
    let caller = auth.user_id;
    let items = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let rows = groups::list(conn).map_err(ApiError::internal)?;
            to_wire(conn, caller, rows)
        })
        .await?;
    Ok(Json(StandupGroupList { items }))
}

/// Create a stand-up group. Each member of the organization can. The name
/// is unique in the organization, with no regard to case. Each person must
/// be a person of the organization: a user id, an email or a user name.
#[utoipa::path(
    post,
    path = "/api/standup-groups",
    tag = "teams",
    request_body = CreateStandupGroupRequest,
    responses(
        (status = 201, description = "The new group", body = StandupGroup),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A different group has the name", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "A name with no characters or too many, or a person who is not a person of the organization", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_standup_group(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<CreateStandupGroupRequest>,
) -> Result<(StatusCode, Json<StandupGroup>), ApiError> {
    let caller = auth.user_id;
    let slug = tenant.slug.clone();
    let group = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let name = checked_name(&body.name)?;
            let people = checked_people(conn, &slug, caller, &body.people)?;
            let row = groups::create(conn, &name, caller, &people)
                .map_err(|e| write_error(e, Some(&name), ""))?;
            one(conn, caller, row)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(group)))
}

/// One stand-up group, with its people.
#[utoipa::path(
    get,
    path = "/api/standup-groups/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "The id (UUID) of the group")),
    responses(
        (status = 200, description = "The group", body = StandupGroup),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown group", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_standup_group(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<StandupGroup>, ApiError> {
    let caller = auth.user_id;
    let group = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let row = group_id(conn, &id)?;
            one(conn, caller, row)
        })
        .await?;
    Ok(Json(group))
}

/// Change the name, the people, or both, of a stand-up group. Each member
/// of the organization can edit each group. `people` is the full list
/// after the change.
#[utoipa::path(
    patch,
    path = "/api/standup-groups/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "The id (UUID) of the group")),
    request_body = UpdateStandupGroupRequest,
    responses(
        (status = 200, description = "The group after the change", body = StandupGroup),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown group", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A different group has the name", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No field to change, a bad name, or a person who is not a person of the organization", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_standup_group(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<UpdateStandupGroupRequest>,
) -> Result<Json<StandupGroup>, ApiError> {
    let caller = auth.user_id;
    let slug = tenant.slug.clone();
    let group = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let row = group_id(conn, &id)?;
            if body.name.is_none() && body.people.is_none() {
                return Err(ApiError::validation(
                    "Send `name`, `people`, or both: the request has no change.",
                ));
            }
            let name = body.name.as_deref().map(checked_name).transpose()?;
            let people = body
                .people
                .as_deref()
                .map(|p| checked_people(conn, &slug, caller, p))
                .transpose()?;
            let row = groups::update(conn, row.id, name.as_deref(), people.as_deref())
                .map_err(|e| write_error(e, name.as_deref(), &id))?;
            one(conn, caller, row)
        })
        .await?;
    Ok(Json(group))
}

/// Delete a stand-up group and its people. Only the person who created the
/// group can delete it. The answer is the group before the delete.
#[utoipa::path(
    delete,
    path = "/api/standup-groups/{id}",
    tag = "teams",
    params(("id" = String, Path, description = "The id (UUID) of the group")),
    responses(
        (status = 200, description = "The group that was deleted", body = StandupGroup),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "The caller did not create the group", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown group", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_standup_group(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<StandupGroup>, ApiError> {
    let caller = auth.user_id;
    let group = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let row = group_id(conn, &id)?;
            let before = one(conn, caller, row.clone())?;
            match groups::delete(conn, row.id, caller).map_err(ApiError::internal)? {
                DeleteOutcome::Deleted => Ok(before),
                DeleteOutcome::NotFound => Err(not_found(&id)),
                DeleteOutcome::NotCreator => Err(ApiError::forbidden(format!(
                    "Only the person who created the stand-up group can delete it: {}. You \
                     can change its name and its people.",
                    before.created_by.display_name
                ))),
            }
        })
        .await?;
    Ok(Json(group))
}

/// The stop of one person of a stand-up group: the work of the person by
/// ownership on each board, in two lanes.
///
/// - Initiatives: the initiatives that the person owns, of each team.
/// - Tasks: the tasks on each board that the person, or an agent key of
///   the person, claims.
///
/// Any member of the organization can read the stop of any person of a
/// group.
#[utoipa::path(
    get,
    path = "/api/standup-groups/{id}/people/{user_id}",
    tag = "teams",
    params(
        ("id" = String, Path, description = "The id (UUID) of the group"),
        ("user_id" = String, Path, description = "The id (UUID) of a person of the group"),
    ),
    responses(
        (status = 200, description = "The two lanes of the work of the person", body = MyWork),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown group, or the person is not in the group", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_standup_group_person(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<Json<MyWork>, ApiError> {
    let work = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let row = group_id(conn, &id)?;
            let not_in_group = || {
                ApiError::not_found(format!(
                    "No person of the group has the id {user_id:?}. Send the user_id of a \
                     person of GET /api/standup-groups/{{id}}."
                ))
                .with_details(serde_json::json!({ "field": "user_id" }))
            };
            let user = Uuid::parse_str(&user_id).map_err(|_| not_in_group())?;
            if !groups::has_person(conn, row.id, user).map_err(ApiError::internal)? {
                return Err(not_in_group());
            }
            let work = kairos_db::my_work::person_work(conn, user).map_err(ApiError::internal)?;
            crate::api::work::to_wire(conn, work)
        })
        .await?;
    Ok(Json(work))
}
