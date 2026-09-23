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

use super::{require_edge_capability, resolve_family_item};
use crate::api::convert::IntoDto;
use crate::api::convert_meta::timestamp;
use crate::api::{Liveness, parse_enum, parse_uuid, resolve_short_code};
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
        .route("/api/{entity_type}/{short_code}/graph", get(get_item_graph))
        .route("/api/{entity_type}/{short_code}/links", get(get_item_links))
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/{id}", delete(delete_relationship))
}

/// Direct-children progress rollup for one item (KAIROS-T-0080): the
/// live `parent`-edge children grouped by their board column, plus the
/// `(done, total)` summary. Supports/informs material never counts;
/// resolution 404s exactly like the relationships GET.
///
/// **Archived children are excluded, deliberately** (ADR-20 rule 5,
/// re-confirmed by KAIROS-T-0158): archived work is not live work, so a
/// progress bar must not count it — an initiative would otherwise look
/// less finished the more of its work had been put away. This is the
/// opposite call from the sibling relationships GET, which now names
/// archived children, and the pair is the whole distinction: containment
/// is a fact about the record, progress is a fact about live work.
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
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
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

/// The branches and pull/merge requests linked to one item
/// (KAIROS-T-0100). Ordering is server-side (pull requests before
/// branches, newest first) so every client agrees. Open tenant-wide like
/// its sibling reads.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/links",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Entity family (plural URL segment)"),
        ("short_code" = String, Path, description = "The item's short code"),
    ),
    responses(
        (status = 200, description = "Linked branches and pull/merge requests", body = Vec<kairos_client::types_forge::ItemLink>),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_item_links(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<Vec<kairos_client::types_forge::ItemLink>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            let links =
                kairos_db::forge::links_for_item(conn, item_id).map_err(ApiError::internal)?;
            Ok(links
                .into_iter()
                .map(|row| kairos_client::types_forge::ItemLink {
                    id: row.link.id.to_string(),
                    item_id: row.link.item_id.to_string(),
                    kind: row.link.kind.to_string(),
                    external_id: row.link.external_id,
                    title: row.link.title,
                    url: row.link.url,
                    state: row.link.state.to_string(),
                    author: row.link.author,
                    forge: row.forge.to_string(),
                    repo_full_name: row.repo_full_name,
                    forge_updated_at: row.link.forge_updated_at.to_rfc3339(),
                })
                .collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// Query of [`get_item_graph`] (explicit struct — serde_urlencoded
/// cannot flatten, T-0021 lesson).
#[derive(serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct GraphQuery {
    /// Hop bound; defaults to 2, capped at MAX_TRAVERSE_DEPTH.
    depth: Option<u32>,
}

/// The focal subgraph (KAIROS-T-0088): every node within `depth` hops
/// over any relationship type in either direction, plus ALL edges among
/// the returned nodes — the wire contract the graph view draws from.
/// Open tenant-wide like the other graph reads.
///
/// **Archived nodes are drawn, marked** with `archived_at`
/// (KAIROS-T-0158). The explorer and the relationships panel render the
/// same edges, and ADR-20 warns that a half-applied visibility rule is
/// worse than none — a node listed in the panel and missing from the
/// picture invites the reader to trust whichever they saw last. Omitting
/// them also broke paths that merely passed THROUGH archived work: the
/// walk hops over `item_relationships` directly, so the far side stayed
/// in the node set with its connecting node deleted out of the middle.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/graph",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Entity family (plural URL segment)"),
        ("short_code" = String, Path, description = "The focal item's short code"),
        GraphQuery,
    ),
    responses(
        (status = 200, description = "The focal subgraph", body = kairos_client::types_graph::GraphResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_item_graph(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<GraphQuery>,
) -> Result<Json<kairos_client::types_graph::GraphResponse>, ApiError> {
    use kairos_client::types_graph as graph_dto;
    let depth = query
        .depth
        .unwrap_or(2)
        .clamp(1, kairos_core::search::MAX_TRAVERSE_DEPTH);
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            let (nodes, edges) =
                graph::item_subgraph(conn, item_id, depth).map_err(ApiError::internal)?;
            Ok(graph_dto::GraphResponse {
                focus: short_code,
                depth,
                nodes: nodes
                    .into_iter()
                    .map(|node| graph_dto::GraphNode {
                        id: node.id.to_string(),
                        short_code: node.short_code,
                        entity_type: node.entity_type.to_string(),
                        title: node.title,
                        status: node.status,
                        depth: node.depth,
                        degree: node.degree,
                        archived_at: node.archived_at.map(timestamp),
                    })
                    .collect(),
                edges: edges
                    .into_iter()
                    .map(|edge| graph_dto::GraphEdge {
                        source_id: edge.source_id.to_string(),
                        target_id: edge.target_id.to_string(),
                        relationship: edge.relationship.to_string(),
                        depth: edge.depth,
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
///
/// Archived neighbours are among them (KAIROS-T-0158) and carry
/// `archived_at`; nothing here filters, because filtering here is what
/// made "what did this initiative contain?" answer short.
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
            archived_at: neighbor.archived_at.map(timestamp),
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
/// tenant-wide read; hydrated through `entity_directory`).
///
/// **Archived neighbours are included, marked** with `archived_at`
/// (KAIROS-T-0158, ADR-20). They used to drop out of the hydrating join,
/// which made this endpoint answer "what does this item contain / depend
/// on?" with fewer rows than the truth — silently, with no flag that
/// could recover them, and about a LIVE item. There is deliberately no
/// opt-out: an item's edges are a property of the item being viewed, so
/// the honest default is to report every edge and say which ends are put
/// away. The item itself may be archived too (resolution is
/// [`Liveness::IncludeArchived`] since KAIROS-T-0154).
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
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
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
    let relationship = parse_enum(&body.relationship, "relationship", RelationshipType::ALL)?;
    let user = auth.user_id;
    let tenant_ctx = tenant.clone();
    let created =
        state
            .blocking
            .run(&tenant.slug, move |conn| {
                let (source_id, source_type) =
                    resolve_short_code(conn, &body.source_short_code, Liveness::LiveOnly)?
                        .ok_or_else(|| {
                            ApiError::validation(format!(
                                "source_short_code {:?} does not name a live item",
                                body.source_short_code
                            ))
                        })?;
                let (target_id, target_type) =
                    resolve_short_code(conn, &body.target_short_code, Liveness::LiveOnly)?
                        .ok_or_else(|| {
                            ApiError::validation(format!(
                                "target_short_code {:?} does not name a live item",
                                body.target_short_code
                            ))
                        })?;
                // KAIROS-T-0111: collaborative edges (parent, blocks) by members
                // who manage either end or authored the source; the rest admin.
                require_edge_capability(
                    conn,
                    &tenant_ctx,
                    user,
                    relationship.as_str(),
                    (source_id, source_type),
                    (target_id, target_type),
                )?;
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
    let id = parse_uuid(&id, "relationship id")?;
    let user = auth.user_id;
    let tenant_ctx = tenant.clone();
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
            // KAIROS-T-0111: removing an edge is gated exactly like writing it.
            let source_type = crate::api::resolve_item_type(conn, edge.source_id)?
                .ok_or_else(|| ApiError::not_found(format!("no relationship {id} exists")))?;
            let target_type = crate::api::resolve_item_type(conn, edge.target_id)?
                .ok_or_else(|| ApiError::not_found(format!("no relationship {id} exists")))?;
            require_edge_capability(
                conn,
                &tenant_ctx,
                user,
                edge.relationship.as_str(),
                (edge.source_id, source_type),
                (edge.target_id, target_type),
            )?;
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
