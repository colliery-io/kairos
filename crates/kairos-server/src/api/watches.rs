//! The watches of a person (KAIROS-T-0736, decision 8 of COLLIERY-I-0664):
//!
//! - `GET /api/me/watches`: each watch of the caller;
//! - `GET`, `PUT` and `DELETE /api/items/{short_code}/watch`: the watch of
//!   the caller on one item. `PUT` takes `with_children`.
//!
//! The MCP tools `watch_item`, `unwatch_item` and `list_watches` call the
//! same functions ([`watch_for`], [`unwatch_for`], [`list_for`]). An agent
//! key watches for its person: the watch is the person's. A service account
//! gets no notifications, so it cannot watch (403). Each member reads each
//! item, so a watch needs no capability on the board.
//!
//! A coarse change of a watched item (moved, edited, note added, done)
//! makes a `watch` notification ([`kairos_db::watches`]).
//!
//! WATCHERS THAT PEOPLE CAN SEE (KAIROS-T-0762, COLLIERY-I-0674):
//!
//! - `GET /api/items/{short_code}/watchers`: each person who watches the
//!   item. Each member can read it.
//! - `PUT /api/items/{short_code}/watchers/{user}`: add a person of the
//!   organization as a watcher. The added person gets a `watch`
//!   notification; a self-watch makes none.
//! - `DELETE /api/items/{short_code}/watchers/{user}`: remove a watcher.
//!   Only the watcher or the person who added the watch can (403).
//!
//! `{user}` is a user id, an email, a user name, or `me`. The MCP tools
//! `watch_item` and `unwatch_item` take `user`; `list_watches` takes
//! `short_code` for the watchers of one item.

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Bool, Nullable, Text, Uuid as SqlUuid};
use kairos_client::types_notifications::{
    ItemWatcher, ItemWatchers, WatchList, WatchRequest, WatchState, WatchedItem,
};
use uuid::Uuid;

use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/me/watches", get(list_watches))
        .route(
            "/api/items/{short_code}/watch",
            get(get_watch).put(put_watch).delete(delete_watch),
        )
        .route("/api/items/{short_code}/watchers", get(get_watchers))
        .route(
            "/api/items/{short_code}/watchers/{user}",
            axum::routing::put(put_watcher).delete(delete_watcher),
        )
}

#[derive(QueryableByName)]
struct Target {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Bool)]
    archived: bool,
    #[diesel(sql_type = Nullable<Text>)]
    retired: Option<String>,
}

/// The item of `short_code`, or of a retired code of it. `live`: a retired
/// code and an archived item are refused, as for each write.
fn target(conn: &mut PgConnection, short_code: &str, live: bool) -> Result<Target, ApiError> {
    let code = crate::references::short_code(short_code);
    let found: Option<Target> = sql_query(
        "SELECT d.id, d.short_code, (d.deleted_at IS NOT NULL) AS archived, \
                NULL::text AS retired, 0 AS pick \
           FROM entity_directory d WHERE d.short_code = $1 \
         UNION ALL \
         SELECT d.id, d.short_code, (d.deleted_at IS NOT NULL), r.code, 1 \
           FROM retired_codes r JOIN entity_directory d ON d.id = r.item_id \
          WHERE r.code = $1 \
         ORDER BY pick LIMIT 1",
    )
    .bind::<Text, _>(&code)
    .get_result(conn)
    .optional()
    .map_err(ApiError::internal)?;
    let Some(found) = found else {
        return Err(
            ApiError::not_found(format!("No item has the short code {short_code:?}."))
                .with_details(serde_json::json!({ "field": "short_code" })),
        );
    };
    if live {
        if found.retired.is_some() {
            return Err(crate::api::retired_code_refused(
                short_code,
                &found.short_code,
            ));
        }
        if found.archived {
            return Err(crate::api::archived_write_refused(&code));
        }
    }
    Ok(found)
}

/// Refuse a service account: a watch makes notifications, and
/// notifications are for people.
fn require_person(conn: &mut PgConnection, user: Uuid) -> Result<(), ApiError> {
    if kairos_db::task_claims::is_person(conn, user).map_err(ApiError::internal)? {
        return Ok(());
    }
    Err(ApiError::forbidden(
        "A service account cannot watch an item. A watch makes notifications, and \
         notifications are for people.",
    ))
}

fn state_of(short_code: String, watch: Option<kairos_db::watches::Watch>) -> WatchState {
    WatchState {
        short_code,
        watching: watch.is_some(),
        with_children: watch.is_some_and(|w| w.with_children),
    }
}

/// The watch of `user` on the item.
pub(crate) fn state_for(
    conn: &mut PgConnection,
    user: Uuid,
    short_code: &str,
) -> Result<WatchState, ApiError> {
    require_person(conn, user)?;
    let item = target(conn, short_code, false)?;
    let watch = kairos_db::watches::watch_of(conn, user, item.id).map_err(ApiError::internal)?;
    Ok(state_of(item.short_code, watch))
}

/// `user` watches the live item, with or without its children. A second
/// watch changes `with_children`.
pub(crate) fn watch_for(
    conn: &mut PgConnection,
    user: Uuid,
    short_code: &str,
    with_children: bool,
) -> Result<WatchState, ApiError> {
    require_person(conn, user)?;
    let item = target(conn, short_code, true)?;
    let watch = kairos_db::watches::watch(conn, user, item.id, with_children)
        .map_err(ApiError::internal)?;
    Ok(state_of(item.short_code, Some(watch)))
}

/// `user` stops the watch on the item, archived or not. Returns the state
/// after, and whether there was a watch.
pub(crate) fn unwatch_for(
    conn: &mut PgConnection,
    user: Uuid,
    short_code: &str,
) -> Result<(WatchState, bool), ApiError> {
    require_person(conn, user)?;
    let item = target(conn, short_code, false)?;
    let removed = kairos_db::watches::unwatch(conn, user, item.id).map_err(ApiError::internal)?;
    Ok((state_of(item.short_code, None), removed))
}

/// Each watch of `user`, by code.
pub(crate) fn list_for(conn: &mut PgConnection, user: Uuid) -> Result<Vec<WatchedItem>, ApiError> {
    require_person(conn, user)?;
    let rows = kairos_db::watches::list(conn, user).map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|w| WatchedItem {
            short_code: w.short_code,
            entity_type: w.entity_type,
            title: w.title,
            archived: w.archived,
            with_children: w.with_children,
            created_at: w
                .created_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            board: w.board,
            column: w.column,
            claim: w.claim,
            updated_at: w
                .updated_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            state: w.state,
        })
        .collect())
}

/// Each watch of the caller, by code. An agent key gives the watches of
/// its person.
#[utoipa::path(
    get,
    path = "/api/me/watches",
    tag = "me",
    responses(
        (status = 200, description = "Each watch of the caller, by code", body = WatchList),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_watches(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<WatchList>, ApiError> {
    let user = auth.user_id;
    let items = state
        .blocking
        .run(&tenant.slug, move |conn| list_for(conn, user))
        .await?;
    Ok(Json(WatchList { items }))
}

/// The watch of the caller on the item.
#[utoipa::path(
    get,
    path = "/api/items/{short_code}/watch",
    tag = "me",
    params(("short_code" = String, Path, description = "The item")),
    responses(
        (status = 200, description = "Whether the caller watches the item, and with its children", body = WatchState),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item has the short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_watch(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<WatchState>, ApiError> {
    let user = auth.user_id;
    let watch = state
        .blocking
        .run(&tenant.slug, move |conn| state_for(conn, user, &short_code))
        .await?;
    Ok(Json(watch))
}

/// Watch the item. A coarse change of the item makes a `watch`
/// notification for the caller: moved, edited, note added, done. A change
/// that the caller made makes none. With `with_children: true`, a change of
/// a child of the item makes one too. A second `PUT` changes
/// `with_children`.
#[utoipa::path(
    put,
    path = "/api/items/{short_code}/watch",
    tag = "me",
    params(("short_code" = String, Path, description = "The item")),
    request_body = WatchRequest,
    responses(
        (status = 200, description = "The watch after the change", body = WatchState),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item has the short code, the code is retired, or the item is archived", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn put_watch(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<WatchRequest>,
) -> Result<Json<WatchState>, ApiError> {
    let user = auth.user_id;
    let with_children = body.with_children.unwrap_or(false);
    let watch = state
        .blocking
        .run(&tenant.slug, move |conn| {
            watch_for(conn, user, &short_code, with_children)
        })
        .await?;
    Ok(Json(watch))
}

/// Stop the watch of the caller on the item. No watch: nothing changes,
/// and the answer is the same.
#[utoipa::path(
    delete,
    path = "/api/items/{short_code}/watch",
    tag = "me",
    params(("short_code" = String, Path, description = "The item")),
    responses(
        (status = 200, description = "The caller does not watch the item now", body = WatchState),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item has the short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_watch(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<WatchState>, ApiError> {
    let user = auth.user_id;
    let (watch, _) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            unwatch_for(conn, user, &short_code)
        })
        .await?;
    Ok(Json(watch))
}

/// The person that `reference` names (KAIROS-T-0762): a user id, an email,
/// a user name, or `me` for `me`. `members_only`: the person must be a
/// member of the organization `slug` and not a service account (an add).
/// A remove finds any person who watches, also a former member.
fn resolve_watcher(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    me: Uuid,
    members_only: bool,
) -> Result<Uuid, ApiError> {
    let reference = crate::references::text(reference);
    let mut found = None;
    for user in crate::references::people(conn, me, reference)? {
        if members_only
            && !kairos_db::abac::is_org_member(conn, slug, user.id).map_err(ApiError::internal)?
        {
            continue;
        }
        if found.is_some() {
            return Err(ApiError::validation(format!(
                "More than one person has the name {reference:?}. Send the id of the person."
            ))
            .with_details(serde_json::json!({ "field": "user" })));
        }
        found = Some(user);
    }
    let Some(user) = found else {
        return Err(ApiError::validation(format!(
            "No member of the organization has the id, the email or the user name \
             {reference:?}. Send me, or a person of the organization."
        ))
        .with_details(serde_json::json!({ "field": "user" })));
    };
    if members_only && user.kind != kairos_db::models::public::USER_KIND_HUMAN {
        return Err(ApiError::validation(format!(
            "{reference:?} is a service account. A service account cannot watch an item. Send \
             a person of the organization."
        ))
        .with_details(serde_json::json!({ "field": "user" })));
    }
    Ok(user.id)
}

fn rfc3339(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The watchers of the item `item`, for the caller `caller`.
fn watchers_of(
    conn: &mut PgConnection,
    caller: Uuid,
    item: Target,
) -> Result<ItemWatchers, ApiError> {
    let rows = kairos_db::watches::item_watchers(conn, item.id).map_err(ApiError::internal)?;
    Ok(ItemWatchers {
        short_code: item.short_code,
        watchers: rows
            .into_iter()
            .map(|w| ItemWatcher {
                user_id: w.user_id.to_string(),
                user_name: w.user_name,
                display_name: w.display_name,
                added_by: w.added_by.to_string(),
                added_by_name: w.added_by_name,
                with_children: w.with_children,
                created_at: rfc3339(w.created_at),
                can_remove: caller == w.user_id || caller == w.added_by,
            })
            .collect(),
    })
}

/// Each person who watches the item (KAIROS-T-0762). Each member reads it.
pub(crate) fn watchers_for(
    conn: &mut PgConnection,
    caller: Uuid,
    short_code: &str,
) -> Result<ItemWatchers, ApiError> {
    let item = target(conn, short_code, false)?;
    watchers_of(conn, caller, item)
}

/// `caller` adds the person `user` as a watcher of the live item
/// (KAIROS-T-0762). Returns the watchers after, and whether the watch is
/// new. A watch that is there does not change.
pub(crate) fn add_watcher_for(
    conn: &mut PgConnection,
    slug: &str,
    caller: Uuid,
    short_code: &str,
    user: &str,
    with_children: bool,
) -> Result<(ItemWatchers, Uuid, bool), ApiError> {
    require_person(conn, caller)?;
    let watcher = resolve_watcher(conn, slug, user, caller, true)?;
    let item = target(conn, short_code, true)?;
    let added = kairos_db::watches::add_watcher(conn, watcher, item.id, with_children, caller)
        .map_err(ApiError::internal)?;
    Ok((watchers_of(conn, caller, item)?, watcher, added.new))
}

/// `caller` removes the watch of the person `user` on the item, archived
/// or not (KAIROS-T-0762). Only the watcher or the person who added the
/// watch can: anyone else gets 403. Returns the watchers after, and
/// whether there was a watch.
pub(crate) fn remove_watcher_for(
    conn: &mut PgConnection,
    slug: &str,
    caller: Uuid,
    short_code: &str,
    user: &str,
) -> Result<(ItemWatchers, Uuid, bool), ApiError> {
    let watcher = resolve_watcher(conn, slug, user, caller, false)?;
    let item = target(conn, short_code, false)?;
    let removed = match kairos_db::watches::remove_watcher(conn, watcher, item.id, caller)
        .map_err(ApiError::internal)?
    {
        kairos_db::watches::Removal::Removed => true,
        kairos_db::watches::Removal::NotWatching => false,
        kairos_db::watches::Removal::Refused { added_by } => {
            return Err(ApiError::forbidden(format!(
                "You cannot remove this watcher of {}. Only the watcher or the person who \
                 added the watch can remove it.",
                item.short_code
            ))
            .with_details(serde_json::json!({
                "field": "user",
                "watcher": watcher,
                "added_by": added_by,
            })));
        }
    };
    Ok((watchers_of(conn, caller, item)?, watcher, removed))
}

/// Each person who watches the item, oldest watch first, with who added
/// each watch. A person who watches a parent with its children is not in
/// the list. Each member can read it.
#[utoipa::path(
    get,
    path = "/api/items/{short_code}/watchers",
    tag = "me",
    params(("short_code" = String, Path, description = "The item")),
    responses(
        (status = 200, description = "The watchers of the item", body = ItemWatchers),
        (status = 404, description = "No item has the short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_watchers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<ItemWatchers>, ApiError> {
    let user = auth.user_id;
    let watchers = state
        .blocking
        .run(&tenant.slug, move |conn| {
            watchers_for(conn, user, &short_code)
        })
        .await?;
    Ok(Json(watchers))
}

/// Add a person of the organization as a watcher of the item. The person
/// gets a `watch` notification that you added them, and then one for each
/// coarse change of the item. A person who watches the item already: the
/// watch does not change. `me` watches the item for you.
#[utoipa::path(
    put,
    path = "/api/items/{short_code}/watchers/{user}",
    tag = "me",
    params(
        ("short_code" = String, Path, description = "The item"),
        ("user" = String, Path, description = "The watcher: a user id, an email, a user name, or me"),
    ),
    request_body = WatchRequest,
    responses(
        (status = 200, description = "The watchers of the item after the change", body = ItemWatchers),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item has the short code, the code is retired, or the item is archived", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The user is not a person of the organization", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn put_watcher(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((short_code, user)): Path<(String, String)>,
    ApiJson(body): ApiJson<WatchRequest>,
) -> Result<Json<ItemWatchers>, ApiError> {
    let caller = auth.user_id;
    let slug = tenant.slug.clone();
    let with_children = body.with_children.unwrap_or(false);
    let (watchers, _, _) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            add_watcher_for(conn, &slug, caller, &short_code, &user, with_children)
        })
        .await?;
    Ok(Json(watchers))
}

/// Remove a watcher of the item. Only the watcher or the person who added
/// the watch can remove it. No watch: nothing changes.
#[utoipa::path(
    delete,
    path = "/api/items/{short_code}/watchers/{user}",
    tag = "me",
    params(
        ("short_code" = String, Path, description = "The item"),
        ("user" = String, Path, description = "The watcher: a user id, an email, a user name, or me"),
    ),
    responses(
        (status = 200, description = "The watchers of the item after the change", body = ItemWatchers),
        (status = 403, description = "The caller is not the watcher and did not add the watch", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item has the short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No person has the user reference", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_watcher(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((short_code, user)): Path<(String, String)>,
) -> Result<Json<ItemWatchers>, ApiError> {
    let caller = auth.user_id;
    let slug = tenant.slug.clone();
    let (watchers, _, _) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            remove_watcher_for(conn, &slug, caller, &short_code, &user)
        })
        .await?;
    Ok(Json(watchers))
}
