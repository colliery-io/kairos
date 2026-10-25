//! `/api/relationships` + `GET /api/{entity_type}/{short_code}/relationships`
//! (KAIROS-S-0005, graph semantics per KAIROS-A-0001 / T-0013).
//!
//! Reads are open tenant-wide; POST/DELETE take the link rule
//! (COLLIERY-T-0228): the caller may edit the item at either end, for each
//! relationship type. The POST of a `blocks` edge needs the target, and
//! the source alone gives a proposal (202, KAIROS-T-0569). COLLIERY-T-0235 narrows it for the remove of the
//! `supports` edge of a document: see [`crate::api::require_edge_remove`].
//! The T-0013 typed link
//! errors map to 422 with a machine-readable reason:
//! `RELATIONSHIP_RULE` (type-rule matrix violation), `CYCLE_DETECTED`
//! (acyclic relationship would close a cycle), `ALREADY_LINKED`
//! (duplicate edge); unknown endpoints and self-links are 422
//! `VALIDATION`.

use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use kairos_client::types_meta as dto;
use kairos_db::graph::{self, GraphError, Neighbor};
use kairos_db::models::OrgRole;
use kairos_db::models::enums::RelationshipType;
use kairos_db::models::graph::ItemRelationship;
use uuid::Uuid;

use diesel::prelude::*;
use serde_json::json;

use super::resolve_family_item;
use crate::api::convert_meta::{relationship_dto, timestamp};
use crate::api::{
    EdgeEnd, LinkOutcome, LiveRef, Liveness, atomically, link_or_propose, link_proposal_text,
    parse_enum, parse_uuid, require_edge_remove, require_edge_write, require_item_edit,
    resolve_live_ref,
};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use kairos_core::short_code::ItemType;

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
        .route(
            "/api/{entity_type}/{short_code}/parent",
            axum::routing::put(set_parent),
        )
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/{id}", delete(delete_relationship))
}

/// Direct-children progress rollup for one item (KAIROS-T-0080): the
/// `parent`-edge children grouped by their board column, plus the
/// `(done, total)` summary. Supports/informs material never counts;
/// resolution 404s exactly like the relationships GET.
///
/// **An archived child counts by the column that it was archived from**
/// (KAIROS-T-0571). A child archived from a done column counts as done,
/// or as cancelled with the cancel mark. So "Archive completed" keeps the
/// progress of an initiative. A child archived from another column is in
/// `removed`. It is not open work, and it is not in `total` or in
/// `by_column`.
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
            let counts = kairos_core::items::children_progress_counts(
                &rows
                    .iter()
                    .map(|row| kairos_core::items::ColumnChildren {
                        is_done: row.is_done,
                        count: row.count,
                        cancelled: row.cancelled,
                        removed: row.removed,
                    })
                    .collect::<Vec<_>>(),
            );
            let has_done_columns = rows.iter().any(|row| row.board_has_done);
            Ok(dto::ChildrenProgressResponse {
                short_code,
                total: counts.total,
                done: counts.done,
                cancelled: counts.cancelled,
                open: counts.open,
                removed: counts.removed,
                has_done_columns,
                by_column: rows
                    .into_iter()
                    .filter(|row| row.count > 0)
                    .map(|row| dto::ChildColumnProgress {
                        column_id: row.column_id.to_string(),
                        column_name: row.column_name,
                        board_id: row.board_id.to_string(),
                        is_done: row.is_done,
                        count: row.count,
                        cancelled: row.cancelled,
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
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
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
///
/// **The mark `done: true` shows a node in a terminal column**
/// (COLLIERY-T-0233, the rule of COLLIERY-T-0214). A `blocks` edge with a
/// done node at one end is history, not a blocker. The graph keeps the
/// edge, and the client draws it with a different style.
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
    ApiQuery(query): ApiQuery<GraphQuery>,
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
                        done: node.done,
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
pub(crate) fn map_link_error(e: GraphError) -> ApiError {
    match e {
        GraphError::Rule(rule) => ApiError::unprocessable("RELATIONSHIP_RULE", rule.to_string()),
        e @ GraphError::CycleDetected { .. } => {
            ApiError::unprocessable("CYCLE_DETECTED", e.to_string())
        }
        e @ GraphError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        // KAIROS-T-0572: an item has one parent.
        e @ GraphError::ParentExists { .. } => {
            parent_exists(e, "set_parent: PUT /api/<family>/<short_code>/parent")
        }
        e @ (GraphError::SelfLink(_) | GraphError::ItemNotFound(_)) => {
            ApiError::validation(e.to_string())
        }
        e @ GraphError::NotLinked { .. } => ApiError::not_found(e.to_string()),
        GraphError::Database(e) => ApiError::internal(e),
    }
}

/// The 422 `PARENT_EXISTS` of a second parent (KAIROS-T-0572). `fix`
/// names the call that changes the parent on the surface of the caller.
/// `details` has the item and its parent, by short code.
pub(crate) fn parent_exists(e: GraphError, fix: &str) -> ApiError {
    let details = match &e {
        GraphError::ParentExists { item, parent } => json!({ "item": item, "parent": parent }),
        _ => json!({}),
    };
    ApiError::unprocessable("PARENT_EXISTS", format!("{e} Use {fix}.")).with_details(details)
}

/// Fold one direction's neighbors (already ordered by relationship, then
/// edge creation) into groups, attaching each edge's id from `edge_ids`
/// (keyed `(relationship, neighbor id, outgoing?)`).
///
/// Archived neighbours are among them (KAIROS-T-0158) and carry
/// `archived_at`; nothing here filters, because filtering here is what
/// made "what did this initiative contain?" answer short.
///
/// Neighbours in a terminal column are among them too, and carry `done`
/// (COLLIERY-T-0214): a completed blocker stays on the list as history,
/// marked, while the board's `blocks_summary` stops counting it.
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
            done: neighbor.done,
            cancelled: neighbor.cancelled,
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
///
/// **The mark `done: true` shows a neighbour in a terminal column**
/// (COLLIERY-T-0214). Done work does not block, and nothing blocks done
/// work. A `blocks` edge with a done end is history, not a blocker. The
/// list keeps the edge: only the counts on the board change.
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

/// The JSON of an answer that the Idempotency-Key keeps.
fn json_value<T: serde::Serialize>(value: T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value).map_err(ApiError::internal)
}

/// Create a relationship edge, or a proposal of a `blocks` edge.
///
/// The link rule applies (COLLIERY-T-0228). The caller can edit the source
/// or the target. A `blocks` edge needs the target, the blocked item
/// (KAIROS-T-0569). A caller who can edit the source only gets a proposal
/// of the edge. The answer is 202 with the proposal. The message says who
/// can confirm it (KAIROS-A-0060).
///
/// The T-0013 graph
/// service enforces the A-0001 type-rule matrix, cycle prevention for
/// `parent`/`blocks`, and duplicate detection — each rejection is a 422
/// with its typed reason (module docs).
#[utoipa::path(
    post,
    path = "/api/relationships",
    tag = "relationships",
    params(("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC)),
    request_body = dto::CreateRelationshipRequest,
    responses(
        (status = 201, description = "Edge created (relationship_add activity row written)", body = dto::Relationship),
        (status = 202, description = "No edge: a proposal of a blocks edge. The caller can edit the source only. The message says who can confirm it", body = dto::ProposedRelationship),
        (status = 403, description = "The caller may edit neither the source nor the target", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "CONFLICT: the same proposal is pending, or the item has too many pending proposals; or REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Rule violation, cycle, duplicate edge, or unknown endpoint", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_relationship(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::CreateRelationshipRequest>,
) -> Result<Response, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let relationship = parse_enum(&body.relationship, "relationship", RelationshipType::ALL)?;
    let user = auth.user_id;
    let tenant_ctx = tenant.clone();
    state
        .blocking
        .run(&tenant.slug, move |conn| {
            crate::api::idempotency::run_with_status(conn, keyed, |conn| {
                // KAIROS-T-0560: a retired code follows to its item, and the
                // answer has the current codes.
                let source = resolve_live_ref(conn, &body.source_short_code, "source_short_code")?;
                let target = resolve_live_ref(conn, &body.target_short_code, "target_short_code")?;
                // COLLIERY-T-0228, KAIROS-T-0569: the link rule and the write,
                // by the ONE function that MCP `link_items` calls.
                match link_or_propose(
                    conn,
                    tenant_ctx.role,
                    user,
                    relationship,
                    (source.typed(), target.typed()),
                    map_link_error,
                )? {
                    LinkOutcome::Linked(created) => Ok((
                        StatusCode::CREATED,
                        json_value(relationship_dto(created, source.code, target.code))?,
                    )),
                    LinkOutcome::Proposed { proposal, boards } => {
                        let message = link_proposal_text(
                            &source.code,
                            &target.code,
                            relationship.as_str(),
                            &proposal,
                            &boards,
                        );
                        let proposal = crate::api::proposals::to_dto(conn, *proposal)?;
                        Ok((
                            StatusCode::ACCEPTED,
                            json_value(dto::ProposedRelationship { message, proposal })?,
                        ))
                    }
                }
            })
        })
        .await
        .map(IntoResponse::into_response)
}

/// The parent of a create (KAIROS-T-0572): resolve `parent` and check the
/// type rules BEFORE the item is written, so that a parent that cannot be
/// is a refusal and not a rollback. A retired code finds its item
/// (KAIROS-T-0560). `details.field` is `parent`.
pub(crate) fn parent_for_create(
    conn: &mut PgConnection,
    parent: &str,
    item_type: ItemType,
) -> Result<LiveRef, ApiError> {
    let found = resolve_live_ref(conn, parent, "parent")
        .map_err(|e| e.with_details(json!({ "field": "parent" })))?;
    kairos_core::graph::check_link(
        kairos_core::graph::Relationship::Parent,
        found.item_type,
        item_type,
    )
    .map_err(|e| ApiError::validation(e.to_string()).with_details(json!({ "field": "parent" })))?;
    Ok(found)
}

/// Write the `parent` edge of a new item (KAIROS-T-0572). The caller runs
/// it in the transaction of the create ([`atomically`]), so a refusal here
/// leaves no item. WHO is the link rule, by the function that
/// `POST /api/relationships` asks; the creator of the new item can edit
/// it, so the rule lets the creator through.
pub(crate) fn link_created_parent(
    conn: &mut PgConnection,
    role: OrgRole,
    user: Uuid,
    parent: &LiveRef,
    item: EdgeEnd,
) -> Result<(), ApiError> {
    require_edge_write(
        conn,
        role,
        user,
        RelationshipType::Parent.as_str(),
        parent.typed(),
        item,
    )?;
    graph::link_items(conn, parent.id, item.0, RelationshipType::Parent, user)
        .map_err(crate::api::map_graph_error)?;
    Ok(())
}

/// Set the parent of an item, or remove it (KAIROS-T-0572): ONE function
/// for `PUT /api/{entity_type}/{short_code}/parent` and MCP `set_parent`.
///
/// - Only an initiative or a task has a parent: each other type is a 422
///   `VALIDATION`.
/// - `parent` is resolved as each reference of a write: a retired code
///   finds its item (KAIROS-T-0560), an archived one is refused.
/// - WHO: the link rule for each edge that changes. The remove of the old
///   edge asks [`require_edge_remove`], the new edge asks
///   [`require_edge_write`]: the caller may edit the item, or the parent at
///   that end. A write that changes nothing checks nothing and writes
///   nothing.
/// - The swap is [`graph::set_parent`], in one transaction with the checks.
///
/// `fix` names the call of the surface in a `PARENT_EXISTS` refusal. The
/// second value is the line that names the current code of a retired
/// `parent`, or `None`.
pub(crate) fn change_parent(
    conn: &mut PgConnection,
    role: OrgRole,
    user: Uuid,
    item: EdgeEnd,
    parent: Option<&str>,
    fix: &str,
) -> Result<(dto::SetParentResponse, Option<String>), ApiError> {
    let (item_id, item_type) = item;
    if !matches!(item_type, ItemType::Initiative | ItemType::Task) {
        let supports = if matches!(item_type, ItemType::Document | ItemType::Adr) {
            " A document or an ADR supports an item: use a supports edge."
        } else {
            ""
        };
        return Err(ApiError::validation(format!(
            "A {item_type} has no parent. An initiative has a strategy as its parent, and a \
             task has an initiative.{supports}"
        )));
    }
    atomically(conn, |conn| {
        let new_parent = parent
            .map(|code| resolve_live_ref(conn, code, "parent"))
            .transpose()?;
        let previous = graph::parent_of(conn, item_id).map_err(map_link_error)?;
        let parent_rel = RelationshipType::Parent.as_str();
        let changes_old = previous
            .as_ref()
            .is_some_and(|old| new_parent.as_ref().is_none_or(|new| new.id != old.id));
        if let (true, Some(old)) = (changes_old, &previous) {
            match old.item_type {
                Some(old_type) => {
                    require_edge_remove(conn, role, user, parent_rel, (old.id, old_type), item)?
                }
                // The parent is gone from the directory: the item decides.
                None => require_item_edit(conn, role, user, item_id, item_type)?,
            }
        }
        if let Some(new) = &new_parent
            && previous.as_ref().is_none_or(|old| old.id != new.id)
        {
            require_edge_write(conn, role, user, parent_rel, new.typed(), item)?;
        }
        let change = graph::set_parent(conn, item_id, new_parent.as_ref().map(|new| new.id), user)
            .map_err(|e| match e {
                e @ GraphError::ParentExists { .. } => parent_exists(e, fix),
                other => map_link_error(other),
            })?;
        Ok((
            dto::SetParentResponse {
                short_code: change.item,
                parent: change.parent,
                previous_parent: change.previous,
                changed: change.changed,
            },
            new_parent.and_then(|new| new.renamed()),
        ))
    })
}

/// Set the parent of an initiative or a task, or remove it with `null`
/// (KAIROS-T-0572). An item has one parent. The new edge replaces the old
/// one in one transaction, so no reader sees two parents.
///
/// The link rule applies to each edge that changes (COLLIERY-T-0228). The
/// caller may edit the item, or the parent at that end. The same parent
/// again changes nothing (`changed: false`). Kairos refuses a retired code
/// of the item. A retired code of the parent finds its item
/// (KAIROS-T-0560). The move rules do not apply: a change of parent is not
/// a move (KAIROS-A-0040 decision 7).
#[utoipa::path(
    put,
    path = "/api/{entity_type}/{short_code}/parent",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "initiatives or tasks"),
        ("short_code" = String, Path, description = "The item's short code"),
    ),
    request_body = dto::SetParentRequest,
    responses(
        (status = 200, description = "The parent now and before; `changed` is false when nothing changed", body = dto::SetParentResponse),
        (status = 403, description = "The caller may edit neither the item nor the parent at an end that changes", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No `parent` key, a type with no parent, an unknown or archived parent, RELATIONSHIP_RULE, or PARENT_EXISTS (a write at the same time set a parent)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_parent(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::SetParentRequest>,
) -> Result<Json<dto::SetParentResponse>, ApiError> {
    let user = auth.user_id;
    let role = tenant.role;
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let fix = format!("PUT /api/{family}/{short_code}/parent");
            change_parent(conn, role, user, item, body.parent.as_deref(), &fix)
                .map(|(response, _)| response)
        })
        .await?;
    Ok(Json(response))
}

/// Remove a relationship edge by id.
///
/// The link rule applies (COLLIERY-T-0228), as for the create.
///
/// The `supports` edge of a document has a rule of its own
/// (COLLIERY-T-0235). The caller must be able to edit the document. Each
/// document has an owner board (COLLIERY-T-3109). So the last `supports`
/// edge of a document can go.
///
/// Goes
/// through the T-0013 unlink service so the `relationship_remove`
/// activity row is written.
#[utoipa::path(
    delete,
    path = "/api/relationships/{id}",
    tag = "relationships",
    params(("id" = String, Path, description = "Relationship edge id (UUID)")),
    responses(
        (status = 200, description = "Edge removed", body = dto::DeletedResponse),
        (status = 403, description = "The caller may edit neither the source nor the target. For the supports edge of a document: the caller may not edit the document", body = kairos_client::types::ErrorEnvelope),
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
                .ok_or_else(|| {
                    ApiError::not_found(format!("The relationship {id} does not exist."))
                })?;
            // KAIROS-T-0111: removing an edge is gated exactly like writing
            // it. COLLIERY-T-0228: by the link rule. COLLIERY-T-0235: but
            // for the `supports` edge of a document.
            let source_type =
                crate::api::resolve_item_type(conn, edge.source_id)?.ok_or_else(|| {
                    ApiError::not_found(format!("The relationship {id} does not exist."))
                })?;
            let target_type =
                crate::api::resolve_item_type(conn, edge.target_id)?.ok_or_else(|| {
                    ApiError::not_found(format!("The relationship {id} does not exist."))
                })?;
            // COLLIERY-T-0235: the rule of the remove and the delete, in
            // one transaction, by the ONE function that MCP
            // `unlink_items` calls.
            crate::api::remove_edge(
                conn,
                tenant_ctx.role,
                user,
                edge.relationship,
                (edge.source_id, source_type),
                (edge.target_id, target_type),
            )?;
            Ok(dto::DeletedResponse { id: id.to_string() })
        })
        .await?;
    Ok(Json(deleted))
}
