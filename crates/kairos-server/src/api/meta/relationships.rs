//! `/api/relationships` + `GET /api/{entity_type}/{short_code}/relationships`
//! (KAIROS-S-0005, graph semantics per KAIROS-A-0001 / T-0013).
//!
//! Reads are open tenant-wide; POST/DELETE are org-admin only (A-0006:
//! relationships are tenant-wide configuration). The T-0013 typed link
//! errors map to 422 with a machine-readable reason:
//! `RELATIONSHIP_RULE` (type-rule matrix violation), `CYCLE_DETECTED`
//! (acyclic relationship would close a cycle), `ALREADY_LINKED`
//! (duplicate edge); unknown endpoints and self-links are 422
//! `VALIDATION`.

use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use kairos_client::types_meta as dto;
use kairos_db::graph::{self, GraphError, Neighbor};
use kairos_db::models::enums::RelationshipType;
use kairos_db::models::graph::ItemRelationship;
use uuid::Uuid;

use diesel::prelude::*;

use super::{require_org_admin, resolve_family_item};
use crate::api::convert::IntoDto;
use crate::api::{parse_enum, parse_uuid, resolve_short_code};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/{entity_type}/{short_code}/relationships",
            get(get_relationships),
        )
        .route(
            "/api/{entity_type}/{short_code}/children-progress",
            get(get_children_progress),
        )
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/{id}", delete(delete_relationship))
}

/// Direct-children progress rollup for one item (KAIROS-T-0080): the
/// live `parent`-edge children grouped by their board column, plus the
/// `(done, total)` summary. Soft-deleted children drop out and
/// supports/informs material never counts; resolution 404s exactly like
/// the relationships GET.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/children-progress",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Entity family (plural URL segment)"),
        ("short_code" = String, Path, description = "The parent item's short code"),
    ),
    responses(
        (status = 200, description = "The rollup", body = dto::ChildrenProgressResponse),
        (status = 404, description = "Unknown short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_children_progress(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ChildrenProgressResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) = resolve_family_item(conn, &family, &short_code)?;
            let rows = graph::children_progress(conn, item_id).map_err(ApiError::internal)?;
            let (done, total) = kairos_core::items::children_progress_counts(
                &rows
                    .iter()
                    .map(|row| (row.is_done, row.count))
                    .collect::<Vec<_>>(),
            );
            let has_done_columns = rows.iter().any(|row| row.board_has_done);
            Ok(dto::ChildrenProgressResponse {
                short_code,
                total,
                done,
                has_done_columns,
                by_column: rows
                    .into_iter()
                    .map(|row| dto::ChildColumnProgress {
                        column_id: row.column_id.to_string(),
                        column_name: row.column_name,
                        board_id: row.board_id.to_string(),
                        is_done: row.is_done,
                        count: row.count,
                    })
                    .collect(),
            })
        })
        .await?;
    Ok(Json(response))
}

/// [`GraphError`] → HTTP for the relationship write endpoints: the typed
/// T-0013 rejections become 422 with a reason code (module docs).
fn map_link_error(e: GraphError) -> ApiError {
    match e {
        GraphError::Rule(rule) => ApiError::unprocessable("RELATIONSHIP_RULE", rule.to_string()),
        e @ GraphError::CycleDetected { .. } => {
            ApiError::unprocessable("CYCLE_DETECTED", e.to_string())
        }
        e @ GraphError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        e @ (GraphError::SelfLink(_) | GraphError::ItemNotFound(_)) => {
            ApiError::validation(e.to_string())
        }
        e @ GraphError::NotLinked { .. } => ApiError::not_found(e.to_string()),
        GraphError::Database(e) => ApiError::internal(e),
    }
}

/// Fold one direction's neighbors (already ordered by relationship, then
/// edge creation) into groups, attaching each edge's id from `edge_ids`
/// (keyed `(relationship, neighbor id, outgoing?)`).
fn group_neighbors(
    neighbors: Vec<Neighbor>,
    edge_ids: &HashMap<(RelationshipType, Uuid, bool), Uuid>,
    outgoing: bool,
) -> Vec<dto::RelationshipGroup> {
    let mut groups: Vec<dto::RelationshipGroup> = Vec::new();
    for neighbor in neighbors {
        let relationship = neighbor.relationship.to_string();
        let item = dto::RelatedItem {
            relationship_id: edge_ids
                .get(&(neighbor.relationship, neighbor.id, outgoing))
                .map(Uuid::to_string)
                .unwrap_or_default(),
            id: neighbor.id.to_string(),
            short_code: neighbor.short_code,
            entity_type: neighbor.entity_type.entity_type().to_string(),
            title: neighbor.title,
        };
        match groups.last_mut() {
            Some(group) if group.relationship == relationship => group.items.push(item),
            _ => groups.push(dto::RelationshipGroup {
                relationship,
                items: vec![item],
            }),
        }
    }
    groups
}

/// All relationships of an item, both directions, grouped by type (open
/// tenant-wide read; hydrated through `entity_directory`, so soft-deleted
/// neighbors drop out).
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/relationships",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "Both directions, grouped by relationship type", body = dto::ItemRelationshipsResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_relationships(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ItemRelationshipsResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) = resolve_family_item(conn, &family, &short_code)?;
            let relationships =
                graph::relationships_for(conn, item_id).map_err(ApiError::internal)?;

            // Edge ids for the delete endpoint: every edge touching the
            // item, keyed by (relationship, neighbor, direction) — unique
            // per the `UNIQUE (source_id, target_id, relationship)` DDL.
            use kairos_db::schema::item_relationships as edges;
            let rows: Vec<ItemRelationship> = edges::table
                .filter(
                    edges::source_id
                        .eq(item_id)
                        .or(edges::target_id.eq(item_id)),
                )
                .select(ItemRelationship::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut edge_ids: HashMap<(RelationshipType, Uuid, bool), Uuid> = HashMap::new();
            for edge in rows {
                if edge.source_id == item_id {
                    edge_ids.insert((edge.relationship, edge.target_id, true), edge.id);
                }
                if edge.target_id == item_id {
                    edge_ids.insert((edge.relationship, edge.source_id, false), edge.id);
                }
            }

            Ok(dto::ItemRelationshipsResponse {
                short_code: short_code.clone(),
                outgoing: group_neighbors(relationships.outgoing, &edge_ids, true),
                incoming: group_neighbors(relationships.incoming, &edge_ids, false),
            })
        })
        .await?;
    Ok(Json(response))
}

/// Create a relationship edge (org admin only, A-0006). The T-0013 graph
/// service enforces the A-0001 type-rule matrix, cycle prevention for
/// `parent`/`blocks`, and duplicate detection — each rejection is a 422
/// with its typed reason (module docs).
#[utoipa::path(
    post,
    path = "/api/relationships",
    tag = "relationships",
    request_body = dto::CreateRelationshipRequest,
    responses(
        (status = 201, description = "Edge created (relationship_add activity row written)", body = dto::Relationship),
        (status = 403, description = "Caller is not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Rule violation, cycle, duplicate edge, or unknown endpoint", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_relationship(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateRelationshipRequest>,
) -> Result<(StatusCode, Json<dto::Relationship>), ApiError> {
    require_org_admin(&tenant)?;
    let relationship = parse_enum(&body.relationship, "relationship", RelationshipType::ALL)?;
    let user = auth.user_id;
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (source_id, _) =
                resolve_short_code(conn, &body.source_short_code)?.ok_or_else(|| {
                    ApiError::validation(format!(
                        "source_short_code {:?} does not name a live item",
                        body.source_short_code
                    ))
                })?;
            let (target_id, _) =
                resolve_short_code(conn, &body.target_short_code)?.ok_or_else(|| {
                    ApiError::validation(format!(
                        "target_short_code {:?} does not name a live item",
                        body.target_short_code
                    ))
                })?;
            let created = graph::link_items(conn, source_id, target_id, relationship, user)
                .map_err(map_link_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Remove a relationship edge by id (org admin only, A-0006). Goes
/// through the T-0013 unlink service so the `relationship_remove`
/// activity row is written.
#[utoipa::path(
    delete,
    path = "/api/relationships/{id}",
    tag = "relationships",
    params(("id" = String, Path, description = "Relationship edge id (UUID)")),
    responses(
        (status = 200, description = "Edge removed", body = dto::DeletedResponse),
        (status = 403, description = "Caller is not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No such edge", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_relationship(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::DeletedResponse>, ApiError> {
    require_org_admin(&tenant)?;
    let id = parse_uuid(&id, "relationship id")?;
    let user = auth.user_id;
    let deleted = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::item_relationships as edges;
            let edge: ItemRelationship = edges::table
                .filter(edges::id.eq(id))
                .select(ItemRelationship::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no relationship {id} exists")))?;
            graph::unlink_items(
                conn,
                edge.source_id,
                edge.target_id,
                edge.relationship,
                user,
            )
            .map_err(map_link_error)?;
            Ok(dto::DeletedResponse { id: id.to_string() })
        })
        .await?;
    Ok(Json(deleted))
}
