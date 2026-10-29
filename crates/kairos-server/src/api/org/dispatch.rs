//! The webhook targets, the agent dispatch rules and the delivery log of a
//! board (KAIROS-T-0384, KAIROS-T-0385, COLLIERY-I-0614).
//!
//! - A board manager (`configure_boards` on the board, an organization
//!   admin, or `manage_structure`) makes, removes, turns on and tests the
//!   targets, sends their `skipped` deliveries again (KAIROS-T-0577), and
//!   makes and removes the rules.
//! - A person who belongs to the board ([`kairos_db::dispatch::may_read`]:
//!   an organization admin, a grant on the board, a member of its team, or
//!   `manage_structure`) reads the targets, the rules and the delivery log.
//!   That person gets the URL of a target with no query and no fragment.
//!
//! The functions `*_as` hold the rules of each action, for REST, MCP and
//! the CLI (which calls REST). The path takes the slug or the id of the
//! board.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, SecondsFormat, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_dispatch as dto;
use kairos_db::dispatch::{self, NewRule};
use kairos_db::models::boards::Board;
use kairos_db::models::enums::BoardLevel;
use kairos_db::webhooks as db;
use serde_json::json;
use uuid::Uuid;

use super::load_board_by_ref;
use crate::api::{parse_uuid, require_capability};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::secrets::{SECRETS_KEY_VAR, SecretsKey};

/// The deliveries of a page of the log, by default.
pub const DEFAULT_LOG_LIMIT: i64 = 50;
/// The most deliveries of a page of the log.
pub const MAX_LOG_LIMIT: i64 = 200;
/// The longest name of a target.
const MAX_NAME: usize = 100;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/boards/{id}/webhook-targets",
            get(list_webhook_targets).post(create_webhook_target),
        )
        .route(
            "/api/boards/{id}/webhook-targets/{target_id}",
            delete(delete_webhook_target),
        )
        .route(
            "/api/boards/{id}/webhook-targets/{target_id}/enable",
            post(enable_webhook_target),
        )
        .route(
            "/api/boards/{id}/webhook-targets/{target_id}/test",
            post(test_webhook_target),
        )
        .route(
            "/api/boards/{id}/webhook-targets/{target_id}/redeliver",
            post(redeliver_webhook_target),
        )
        .route(
            "/api/boards/{id}/dispatch-rules",
            get(list_dispatch_rules).post(create_dispatch_rule),
        )
        .route(
            "/api/boards/{id}/agent-triage",
            get(get_agent_triage).put(set_agent_triage),
        )
        .route(
            "/api/boards/{id}/dispatch-rules/{rule_id}",
            delete(delete_dispatch_rule),
        )
        .route(
            "/api/boards/{id}/webhook-deliveries",
            get(list_webhook_deliveries),
        )
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The URL with no query and no fragment, for a person who cannot
/// configure the board: the query of a URL can hold a token of the
/// receiver.
pub fn redacted_url(raw: &str) -> String {
    match reqwest::Url::parse(raw) {
        Ok(mut url) => {
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => String::new(),
    }
}

/// The DTO of a target. `full_url` false: [`redacted_url`].
pub fn target_view(target: &db::Target, full_url: bool) -> dto::WebhookTarget {
    dto::WebhookTarget {
        id: target.id.to_string(),
        board_id: target.board_id.to_string(),
        name: target.name.clone(),
        url: if full_url {
            target.url.clone()
        } else {
            redacted_url(&target.url)
        },
        enabled: target.enabled,
        consecutive_failures: target.consecutive_failures,
        disabled_at: target.disabled_at.map(rfc3339),
        disabled_reason: target.disabled_reason.clone(),
        created_by: target.created_by.to_string(),
        created_at: rfc3339(target.created_at),
        redelivered: None,
    }
}

/// The DTO of a rule.
pub fn rule_view(rule: &dispatch::Rule) -> dto::DispatchRule {
    dto::DispatchRule {
        id: rule.id.to_string(),
        board_id: rule.board_id.to_string(),
        trigger: rule.trigger.clone(),
        column_id: rule.column_id.map(|id| id.to_string()),
        column: rule.column_name.clone(),
        target_id: rule.target_id.to_string(),
        target: rule.target_name.clone(),
        task_type: rule.task_type.clone(),
        metadata_slug: rule.metadata_slug.clone(),
        metadata_value: rule.metadata_value.clone(),
        repository: rule.repository_slug.clone(),
        created_by: rule.created_by.to_string(),
        created_at: rfc3339(rule.created_at),
    }
}

/// The DTO of a delivery, with its tries and the code of its task now.
fn delivery_views(
    conn: &mut PgConnection,
    deliveries: Vec<db::Delivery>,
) -> Result<Vec<dto::WebhookDelivery>, ApiError> {
    let ids: Vec<Uuid> = deliveries.iter().map(|d| d.id).collect();
    let tries = db::tries_of(conn, &ids).map_err(ApiError::internal)?;
    let items: Vec<Uuid> = deliveries.iter().filter_map(|d| d.item_id).collect();
    let codes: std::collections::HashMap<Uuid, String> = {
        #[derive(diesel::QueryableByName)]
        struct Row {
            #[diesel(sql_type = diesel::sql_types::Uuid)]
            id: Uuid,
            #[diesel(sql_type = diesel::sql_types::Text)]
            short_code: String,
        }
        diesel::sql_query("SELECT id, short_code FROM entity_directory WHERE id = ANY($1)")
            .bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(&items)
            .load::<Row>(conn)
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|r| (r.id, r.short_code))
            .collect()
    };
    Ok(deliveries
        .into_iter()
        .map(|d| dto::WebhookDelivery {
            id: d.id.to_string(),
            target_id: d.target_id.to_string(),
            rule_id: d.rule_id.map(|id| id.to_string()),
            event: d.event.clone(),
            short_code: d.item_id.and_then(|id| codes.get(&id).cloned()),
            status: d.status.clone(),
            tries: d.tries,
            response_code: d.response_code,
            last_error: d.last_error.clone(),
            next_try_at: d.next_try_at.map(rfc3339),
            created_at: rfc3339(d.created_at),
            finished_at: d.finished_at.map(rfc3339),
            log: tries
                .iter()
                .filter(|t| t.delivery_id == d.id)
                .map(|t| dto::WebhookDeliveryTry {
                    try_number: t.try_number,
                    at: rfc3339(t.at),
                    ok: t.ok,
                    response_code: t.response_code,
                    error: t.error.clone(),
                    duration_ms: t.duration_ms,
                })
                .collect(),
        })
        .collect())
}

/// The board, when `user` may read its dispatch, else 403. The second
/// value: `user` may configure the board too.
fn readable(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
) -> Result<(Board, bool), ApiError> {
    let board = load_board_by_ref(conn, board_ref)?;
    let manager = require_capability(
        conn,
        slug,
        Some(board.id),
        user,
        kairos_core::abac::CONFIGURE_BOARDS,
    )
    .is_ok();
    if manager || dispatch::may_read(conn, slug, board.id, user).map_err(ApiError::internal)? {
        return Ok((board, manager));
    }
    Err(ApiError::forbidden(format!(
        "You do not belong to the board {}. A person with a grant on the board, a member of \
         its team, or an organization admin reads its dispatch rules and its delivery log.",
        board.slug
    ))
    .with_details(json!({ "board_id": board.id })))
}

/// The board, when `user` may configure it, else 403.
fn configurable(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
) -> Result<Board, ApiError> {
    let board = load_board_by_ref(conn, board_ref)?;
    require_capability(
        conn,
        slug,
        Some(board.id),
        user,
        kairos_core::abac::CONFIGURE_BOARDS,
    )?;
    Ok(board)
}

/// The target `target_id` of `board`, else 404.
fn target_of(
    conn: &mut PgConnection,
    board: &Board,
    target_id: &str,
) -> Result<db::Target, ApiError> {
    let id = parse_uuid(target_id, "target_id")?;
    match db::get_target(conn, id).map_err(ApiError::internal)? {
        Some(target) if target.board_id == board.id => Ok(target),
        _ => Err(ApiError::not_found(format!(
            "The board {} has no webhook target with the id {id}.",
            board.slug
        ))
        .with_details(json!({ "field": "target_id" }))),
    }
}

/// The targets of a board.
pub fn list_targets_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
) -> Result<Vec<dto::WebhookTarget>, ApiError> {
    let (board, manager) = readable(conn, slug, user, board_ref)?;
    Ok(db::list_targets(conn, board.id)
        .map_err(ApiError::internal)?
        .iter()
        .map(|t| target_view(t, manager))
        .collect())
}

/// The refusal when the deployment has no `KAIROS_SECRETS_KEY`.
fn no_secrets_key() -> ApiError {
    ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "SECRETS_NOT_CONFIGURED",
        "This deployment has no KAIROS_SECRETS_KEY, so Kairos cannot keep the secret of a \
         webhook target. An operator sets KAIROS_SECRETS_KEY to 32 random bytes in base64 and \
         starts the server again.",
    )
    .with_details(json!({ "setting": SECRETS_KEY_VAR }))
}

/// Make a target. The secret is in the answer, this one time.
pub fn create_target_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    request: &dto::CreateWebhookTargetRequest,
    key: Option<&SecretsKey>,
    allow_private: bool,
) -> Result<dto::CreatedWebhookTarget, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(ApiError::validation(format!(
            "The name of a webhook target has 1 to {MAX_NAME} characters."
        ))
        .with_details(json!({ "field": "name" })));
    }
    let url = crate::webhooks::check_url(&request.url, allow_private)
        .map_err(|e| ApiError::validation(e).with_details(json!({ "field": "url" })))?;
    let key = key.ok_or_else(no_secrets_key)?;
    let (row, secret) = crate::webhooks::new_target(key, slug, board.id, name, url.as_str(), user);
    let target = db::insert_target(conn, &row).map_err(ApiError::internal)?;
    Ok(dto::CreatedWebhookTarget {
        target: target_view(&target, true),
        secret,
    })
}

/// Remove a target, with its rules and its deliveries.
pub fn delete_target_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    target_id: &str,
) -> Result<dto::WebhookTarget, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let target = target_of(conn, &board, target_id)?;
    db::delete_target(conn, target.id).map_err(ApiError::internal)?;
    Ok(target_view(&target, true))
}

/// Turn a target on again, with its failures in a row at 0, and queue its
/// `skipped` deliveries again (KAIROS-T-0577).
pub fn enable_target_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    target_id: &str,
) -> Result<dto::WebhookTarget, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let target = target_of(conn, &board, target_id)?;
    let (target, redelivered) = db::enable_target(conn, target.id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::not_found("The webhook target is gone.")
                .with_details(json!({ "field": "target_id" }))
        })?;
    let mut view = target_view(&target, true);
    view.redelivered = Some(redelivered.len() as i64);
    Ok(view)
}

/// Queue the `skipped` deliveries of a target that is on again
/// (KAIROS-T-0577): each one of the last [`db::KEEP_DAYS`] days, and with
/// `since`, only the events at or after it. Needs `configure_boards`, as
/// [`enable_target_as`].
pub fn redeliver_target_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    target_id: &str,
    query: &dto::RedeliverQuery,
) -> Result<dto::WebhookTarget, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let since: Option<DateTime<Utc>> = query
        .since
        .as_deref()
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| {
                    ApiError::validation(format!(
                        "The value {value:?} of since is not a timestamp. Send an RFC 3339 \
                         timestamp, for example 2026-10-17T09:00:00Z."
                    ))
                    .with_details(json!({ "field": "since" }))
                })
        })
        .transpose()?;
    let target = target_of(conn, &board, target_id)?;
    if !target.enabled {
        return Err(ApiError::conflict(format!(
            "The webhook target {} is off, so Kairos skips its deliveries again. Turn it on \
             with POST /api/boards/{}/webhook-targets/{}/enable: that sends its skipped \
             deliveries too.",
            target.name, board.slug, target.id
        ))
        .with_details(json!({ "target_id": target.id, "enabled": false })));
    }
    let redelivered = db::redeliver_skipped(conn, target.id, since).map_err(ApiError::internal)?;
    let mut view = target_view(&target, true);
    view.redelivered = Some(redelivered.len() as i64);
    Ok(view)
}

/// Queue a `ping` delivery to a target.
pub fn test_target_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    target_id: &str,
) -> Result<dto::WebhookDelivery, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let target = target_of(conn, &board, target_id)?;
    let data = json!({
        "board": { "id": board.id, "slug": board.slug, "name": board.name },
        "target": { "id": target.id, "name": target.name },
        "actor": { "id": user },
    });
    let delivery = db::queue_delivery(conn, target.id, "ping", None, None, &data)
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::not_found("The webhook target is gone.")
                .with_details(json!({ "field": "target_id" }))
        })?;
    Ok(delivery_views(conn, vec![delivery])?.remove(0))
}

/// The rules of a board.
pub fn list_rules_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
) -> Result<Vec<dto::DispatchRule>, ApiError> {
    let (board, _) = readable(conn, slug, user, board_ref)?;
    Ok(dispatch::list_rules(conn, board.id)
        .map_err(ApiError::internal)?
        .iter()
        .map(rule_view)
        .collect())
}

/// A text that is set and not blank, trimmed.
fn given(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The live column of `board` with the name (no case) or the id `column`.
pub(crate) fn column_of(
    conn: &mut PgConnection,
    board: &Board,
    column: &str,
) -> Result<Uuid, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    let columns: Vec<(Uuid, String)> = dsl::board_columns
        .filter(dsl::board_id.eq(board.id))
        .filter(dsl::deleted_at.is_null())
        .order(dsl::position.asc())
        .select((dsl::id, dsl::name))
        .load(conn)
        .map_err(ApiError::internal)?;
    // KAIROS-T-0689: the forms of crate::references::COLUMN_FORMS.
    let wanted = crate::references::text(column);
    crate::references::column(
        columns.iter().map(|(id, name)| (*id, name.as_str())),
        wanted,
    )
    .ok_or_else(|| {
        ApiError::validation(format!(
            "The board {} has no column {wanted:?}. Its columns are: {}.",
            board.slug,
            columns
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .with_details(json!({ "field": "column" }))
    })
}

/// Whether the column is a done column.
fn column_is_done(conn: &mut PgConnection, column_id: Uuid) -> Result<bool, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::id.eq(column_id))
        .select(dsl::is_done)
        .first(conn)
        .map_err(ApiError::internal)
}

/// Make a rule.
pub fn create_rule_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    request: &dto::CreateDispatchRuleRequest,
) -> Result<dto::DispatchRule, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let trigger =
        given(&request.trigger).unwrap_or_else(|| dispatch::TRIGGER_ENTERED_COLUMN.to_string());
    if !dispatch::TRIGGERS.contains(&trigger.as_str()) {
        return Err(ApiError::validation(format!(
            "{trigger:?} is not a trigger. The triggers are: {}.",
            dispatch::TRIGGERS.join(", ")
        ))
        .with_details(json!({ "field": "trigger" })));
    }
    let column_id = match given(&request.column) {
        Some(column) => {
            let id = column_of(conn, &board, &column)?;
            // KAIROS-T-0457: an unblocked rule on a done column never fires
            // (a task in a done column waits for nothing). KAIROS-T-0718: a
            // review_changed rule neither (the review of done work fires
            // nothing).
            if dispatch::TRIGGERS_WITH_OPTIONAL_COLUMN.contains(&trigger.as_str())
                && column_is_done(conn, id)?
            {
                return Err(ApiError::validation(format!(
                    "A {trigger} rule cannot name a done column: a task in {} waits for \
                     nothing.",
                    column.trim()
                ))
                .with_details(json!({ "field": "column" })));
            }
            Some(id)
        }
        None if dispatch::TRIGGERS_WITH_OPTIONAL_COLUMN.contains(&trigger.as_str()) => None,
        None => {
            return Err(ApiError::validation(
                "A rule with the trigger entered_column needs a column.",
            )
            .with_details(json!({ "field": "column" })));
        }
    };
    let target = target_of(conn, &board, &request.target_id)
        .map_err(|e| e.with_details(json!({ "field": "target_id" })))?;
    // KAIROS-T-0719: an initiative or a strategy has no task type and no
    // repository, so a rule on its board cannot filter on them.
    if board.board_level != BoardLevel::Delivery {
        for (field, value) in [
            ("task_type", &request.task_type),
            ("repository", &request.repository),
        ] {
            if given(value).is_some() {
                return Err(ApiError::validation(format!(
                    "A rule on a {} board cannot filter on {field}: its items have none. \
                     Such a rule filters on metadata only.",
                    board.board_level.as_str()
                ))
                .with_details(json!({ "field": field })));
            }
        }
    }
    let task_type = given(&request.task_type);
    if let Some(t) = &task_type
        && !dispatch::TASK_TYPES.contains(&t.as_str())
    {
        return Err(ApiError::validation(format!(
            "{t:?} is not a task type. The task types are: {}.",
            dispatch::TASK_TYPES.join(", ")
        ))
        .with_details(json!({ "field": "task_type" })));
    }
    let (metadata_slug, metadata_value) = match (
        given(&request.metadata_slug),
        given(&request.metadata_value),
    ) {
        (None, None) => (None, None),
        (Some(slug), Some(value)) => {
            use kairos_db::schema::metadata_definitions::dsl;
            let known: bool = diesel::select(diesel::dsl::exists(
                dsl::metadata_definitions.filter(dsl::slug.eq(&slug)),
            ))
            .get_result(conn)
            .map_err(ApiError::internal)?;
            if !known {
                return Err(ApiError::validation(format!(
                    "No metadata definition has the slug {slug:?}."
                ))
                .with_details(json!({ "field": "metadata_slug" })));
            }
            (Some(slug), Some(value))
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(ApiError::validation(
                "Send metadata_slug and metadata_value together, or send neither.",
            )
            .with_details(json!({ "field": "metadata_slug" })));
        }
    };
    let repository_id = match given(&request.repository) {
        None => None,
        Some(reference) => Some(
            kairos_db::repositories::resolve(conn, &reference)
                .map_err(crate::api::org::repositories::map_error)
                .map_err(|e| e.with_details(json!({ "field": "repository" })))?
                .id,
        ),
    };
    let rule = dispatch::create_rule(
        conn,
        &NewRule {
            board_id: board.id,
            trigger,
            column_id,
            target_id: target.id,
            task_type,
            metadata_slug,
            metadata_value,
            repository_id,
            created_by: user,
        },
    )
    .map_err(ApiError::internal)?;
    Ok(rule_view(&rule))
}

/// Remove a rule.
pub fn delete_rule_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    rule_id: &str,
) -> Result<dto::DispatchRule, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let id = parse_uuid(rule_id, "rule_id")?;
    let rule = match dispatch::get_rule(conn, id).map_err(ApiError::internal)? {
        Some(rule) if rule.board_id == board.id => rule,
        _ => {
            return Err(ApiError::not_found(format!(
                "The board {} has no dispatch rule with the id {id}.",
                board.slug
            ))
            .with_details(json!({ "field": "rule_id" })));
        }
    };
    dispatch::delete_rule(conn, board.id, id).map_err(ApiError::internal)?;
    Ok(rule_view(&rule))
}

/// The delivery log of a board, newest first.
pub fn list_deliveries_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    query: &dto::WebhookDeliveriesQuery,
) -> Result<Vec<dto::WebhookDelivery>, ApiError> {
    let (board, _) = readable(conn, slug, user, board_ref)?;
    let limit = query.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if !(1..=MAX_LOG_LIMIT).contains(&limit) {
        return Err(ApiError::validation(format!(
            "The limit {limit} is out of range. Send a limit from 1 to {MAX_LOG_LIMIT}."
        ))
        .with_details(json!({ "field": "limit" })));
    }
    let targets: Vec<Uuid> = match given(&query.target_id) {
        Some(id) => vec![target_of(conn, &board, &id)?.id],
        None => db::list_targets(conn, board.id)
            .map_err(ApiError::internal)?
            .iter()
            .map(|t| t.id)
            .collect(),
    };
    let rule_id = given(&query.rule_id)
        .map(|id| parse_uuid(&id, "rule_id"))
        .transpose()?;
    // KAIROS-T-0584: the cursor of the next page is a delivery of the board.
    let before = match given(&query.before) {
        Some(id) => {
            let id = parse_uuid(&id, "before")?;
            match db::get_delivery(conn, id).map_err(ApiError::internal)? {
                Some(delivery) if targets.contains(&delivery.target_id) => Some(delivery),
                _ => {
                    return Err(ApiError::not_found(format!(
                        "The log of the board {} has no delivery with the id {id}.",
                        board.slug
                    ))
                    .with_details(json!({ "field": "before" })));
                }
            }
        }
        None => None,
    };
    let deliveries = db::list_deliveries(conn, &targets, rule_id, before.as_ref(), limit)
        .map_err(ApiError::internal)?;
    delivery_views(conn, deliveries)
}

// ---------------------------------------------------------------------------
// REST
// ---------------------------------------------------------------------------

/// The webhook targets of a board (KAIROS-T-0384). A person who belongs to
/// the board reads them. A person who cannot configure the board gets each
/// URL with no query.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/webhook-targets",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "The targets of the board", body = [dto::WebhookTarget]),
        (status = 403, description = "The caller does not belong to the board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_webhook_targets(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::WebhookTarget>>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let targets = state
        .blocking
        .run(&tenant.slug, move |conn| {
            list_targets_as(conn, &slug, user, &id)
        })
        .await?;
    Ok(Json(targets))
}

/// Make a webhook target on a board. Kairos makes the secret, and shows it
/// in this answer only. Needs `configure_boards` on the board, and
/// `KAIROS_SECRETS_KEY` on the deployment.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/webhook-targets",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    request_body = dto::CreateWebhookTargetRequest,
    responses(
        (status = 201, description = "The target, and its secret (shown this one time)", body = dto::CreatedWebhookTarget),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "VALIDATION: a bad name, a URL that is not http(s), or a private address", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "SECRETS_NOT_CONFIGURED: the deployment has no KAIROS_SECRETS_KEY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_webhook_target(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::CreateWebhookTargetRequest>,
) -> Result<(StatusCode, Json<dto::CreatedWebhookTarget>), ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let key = state.config.secrets_key.clone();
    let allow_private = state.config.webhooks.allow_private;
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            create_target_as(conn, &slug, user, &id, &body, key.as_ref(), allow_private)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Remove a webhook target of a board, with its dispatch rules and its
/// delivery log. Needs `configure_boards` on the board.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/webhook-targets/{target_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("target_id" = String, Path, description = "The id (UUID) of the target"),
    ),
    responses(
        (status = 200, description = "The target that was removed", body = dto::WebhookTarget),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or target", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_webhook_target(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target_id)): Path<(String, String)>,
) -> Result<Json<dto::WebhookTarget>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let target = state
        .blocking
        .run(&tenant.slug, move |conn| {
            delete_target_as(conn, &slug, user, &id, &target_id)
        })
        .await?;
    Ok(Json(target))
}

/// Turn a webhook target on again, with its failed tries in a row at 0.
/// Kairos queues the `skipped` deliveries of the last 30 days again, oldest
/// first (KAIROS-T-0577); `redelivered` counts them. Needs
/// `configure_boards` on the board.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/webhook-targets/{target_id}/enable",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("target_id" = String, Path, description = "The id (UUID) of the target"),
    ),
    responses(
        (status = 200, description = "The target, on, with the count of the skipped deliveries queued again", body = dto::WebhookTarget),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or target", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn enable_webhook_target(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target_id)): Path<(String, String)>,
) -> Result<Json<dto::WebhookTarget>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let target = state
        .blocking
        .run(&tenant.slug, move |conn| {
            enable_target_as(conn, &slug, user, &id, &target_id)
        })
        .await?;
    state.jobs.wake();
    Ok(Json(target))
}

/// Queue the `skipped` deliveries of a webhook target again
/// (KAIROS-T-0577). Kairos takes each one of the last 30 days, oldest
/// first. With `since`, it takes only the events at or after that time.
/// Each delivery keeps its id. The target must be on. Needs
/// `configure_boards` on the board, as `enable`.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/webhook-targets/{target_id}/redeliver",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("target_id" = String, Path, description = "The id (UUID) of the target"),
        dto::RedeliverQuery,
    ),
    responses(
        (status = 200, description = "The target, with the count of the skipped deliveries queued again", body = dto::WebhookTarget),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or target", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "CONFLICT: the target is off; turn it on with enable", body = kairos_client::types::ErrorEnvelope),
        (status = 400, description = "VALIDATION: an unknown query parameter", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "VALIDATION: since is not an RFC 3339 timestamp", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn redeliver_webhook_target(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target_id)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<dto::RedeliverQuery>,
) -> Result<Json<dto::WebhookTarget>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let target = state
        .blocking
        .run(&tenant.slug, move |conn| {
            redeliver_target_as(conn, &slug, user, &id, &target_id, &query)
        })
        .await?;
    state.jobs.wake();
    Ok(Json(target))
}

/// Send a test delivery (the event `ping`) to a webhook target. Needs
/// `configure_boards` on the board.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/webhook-targets/{target_id}/test",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("target_id" = String, Path, description = "The id (UUID) of the target"),
    ),
    responses(
        (status = 201, description = "The delivery, queued (or skipped when the target is off)", body = dto::WebhookDelivery),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or target", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn test_webhook_target(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<dto::WebhookDelivery>), ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let delivery = state
        .blocking
        .run(&tenant.slug, move |conn| {
            test_target_as(conn, &slug, user, &id, &target_id)
        })
        .await?;
    state.jobs.wake();
    Ok((StatusCode::CREATED, Json(delivery)))
}

/// The triage setting of a board (KAIROS-T-0428), for a reader of its
/// dispatch.
pub fn agent_triage_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
) -> Result<dto::AgentTriage, ApiError> {
    let (board, _) = readable(conn, slug, user, board_ref)?;
    Ok(dto::AgentTriage {
        board_id: board.id.to_string(),
        agents_decide: crate::agent_triage::agents_decide(conn, board.id)?,
    })
}

/// Set the triage setting of a board (KAIROS-T-0428). Needs
/// `configure_boards`.
pub fn set_agent_triage_as(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    board_ref: &str,
    agents_decide: bool,
) -> Result<dto::AgentTriage, ApiError> {
    let board = configurable(conn, slug, user, board_ref)?;
    let agents_decide =
        crate::agent_triage::set_agents_decide(conn, board.id, agents_decide, user)?;
    Ok(dto::AgentTriage {
        board_id: board.id.to_string(),
        agents_decide,
    })
}

/// Who decides the triage of intake on the board (KAIROS-T-0428). A person
/// who belongs to the board reads it.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/agent-triage",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "The setting", body = dto::AgentTriage),
        (status = 403, description = "The caller does not belong to the board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_agent_triage(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::AgentTriage>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let setting = state
        .blocking
        .run(&tenant.slug, move |conn| {
            agent_triage_as(conn, &slug, user, &id)
        })
        .await?;
    Ok(Json(setting))
}

/// Let agents decide the triage of intake on the board, or not
/// (KAIROS-T-0428). Off: Kairos refuses an agent's move of intake with 403
/// `AGENT_TRIAGE_OFF`. The agent proposes with a note. Needs
/// `configure_boards`.
#[utoipa::path(
    put,
    path = "/api/boards/{id}/agent-triage",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    request_body = dto::SetAgentTriageRequest,
    responses(
        (status = 200, description = "The setting", body = dto::AgentTriage),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_agent_triage(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::SetAgentTriageRequest>,
) -> Result<Json<dto::AgentTriage>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let setting = state
        .blocking
        .run(&tenant.slug, move |conn| {
            set_agent_triage_as(conn, &slug, user, &id, body.agents_decide)
        })
        .await?;
    Ok(Json(setting))
}

/// The dispatch rules of a board (KAIROS-T-0385). A person who belongs to
/// the board reads them.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/dispatch-rules",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "The rules of the board", body = [dto::DispatchRule]),
        (status = 403, description = "The caller does not belong to the board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_dispatch_rules(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::DispatchRule>>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let rules = state
        .blocking
        .run(&tenant.slug, move |conn| {
            list_rules_as(conn, &slug, user, &id)
        })
        .await?;
    Ok(Json(rules))
}

/// Make a dispatch rule. A task can enter the column and match the
/// filter. Then Kairos sends one signed delivery to the target. Needs
/// `configure_boards` on the board.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/dispatch-rules",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board"), ("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC)),
    request_body = dto::CreateDispatchRuleRequest,
    responses(
        (status = 201, description = "The rule", body = dto::DispatchRule),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board, target or repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "VALIDATION: an unknown column, task type or metadata definition", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_dispatch_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::CreateDispatchRuleRequest>,
) -> Result<crate::api::idempotency::Answer<dto::DispatchRule>, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let rule = state
        .blocking
        .run(&tenant.slug, move |conn| {
            crate::api::idempotency::run(conn, keyed, StatusCode::CREATED, |conn| {
                create_rule_as(conn, &slug, user, &id, &body)
            })
        })
        .await?;
    Ok(rule)
}

/// Remove a dispatch rule. Its deliveries stay in the log. Needs
/// `configure_boards` on the board.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/dispatch-rules/{rule_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("rule_id" = String, Path, description = "The id (UUID) of the rule"),
    ),
    responses(
        (status = 200, description = "The rule that was removed", body = dto::DispatchRule),
        (status = 403, description = "Missing configure_boards", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or rule", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_dispatch_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, rule_id)): Path<(String, String)>,
) -> Result<Json<dto::DispatchRule>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let rule = state
        .blocking
        .run(&tenant.slug, move |conn| {
            delete_rule_as(conn, &slug, user, &id, &rule_id)
        })
        .await?;
    Ok(Json(rule))
}

/// The delivery log of a board, newest first: each delivery with its
/// status and its tries. A person who belongs to the board reads it.
/// `before` (the id of a delivery) gives the next page: the deliveries
/// older than that one (KAIROS-T-0584).
#[utoipa::path(
    get,
    path = "/api/boards/{id}/webhook-deliveries",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board"), dto::WebhookDeliveriesQuery),
    responses(
        (status = 200, description = "The deliveries, newest first", body = [dto::WebhookDelivery]),
        (status = 403, description = "The caller does not belong to the board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board or target, or `before` is not a delivery of the log", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "VALIDATION: a limit out of range, or an id that is not a UUID", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<dto::WebhookDeliveriesQuery>,
) -> Result<Json<Vec<dto::WebhookDelivery>>, ApiError> {
    let (slug, user) = (tenant.slug.clone(), auth.user_id);
    let deliveries = state
        .blocking
        .run(&tenant.slug, move |conn| {
            list_deliveries_as(conn, &slug, user, &id, &query)
        })
        .await?;
    Ok(Json(deliveries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_gets_the_url_with_no_query() {
        assert_eq!(
            redacted_url("https://hooks.example.com/run/abc?token=s3cret#x"),
            "https://hooks.example.com/run/abc"
        );
        assert_eq!(redacted_url("not a url"), "");
    }
}
