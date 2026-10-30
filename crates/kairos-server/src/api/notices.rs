//! Notices (KAIROS-T-0737, KAIROS-A-0078): `POST /api/notices`
//! and the MCP tool `send_notice` call [`send_for`].
//!
//! A person, or an agent key of a person, sends a text and an optional item
//! link to a person (`to`) or to each member of a team (`team`). Each
//! recipient gets a notification of the kind `notice`: the bell and the
//! push of the WebSocket. The rules are in [`kairos_db::notices`].
//!
//! - A service account cannot send a notice: a notice names a person.
//! - KAIROS-A-0063 (rule 6): a notice is the work of a member. The scope of
//!   an agent key must have `member`; the board is the board of the linked
//!   item, and with no item the action has no board (rule 7).
//! - The rate limit is `KAIROS_NOTICE_LIMIT_PER_HOUR` (default 20) for each
//!   key. Over the limit: 429 `NOTICE_RATE_LIMITED`, and nothing is sent.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use chrono::Utc;
use diesel::pg::PgConnection;
use kairos_client::types_notifications::{SendNoticeRequest, SendNoticeResponse};
use kairos_core::notifications::NOTICE_TEXT_MAX;
use kairos_db::notices::{SendError, Sender, Target};
use uuid::Uuid;

use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/api/notices", post(send_notice))
}

/// The input of a notice, from REST or MCP.
pub(crate) struct NoticeInput {
    pub to: Option<String>,
    pub team: Option<String>,
    pub text: String,
    pub item: Option<String>,
}

/// A notice that was sent, for the answer.
pub(crate) struct NoticeSent {
    pub id: Uuid,
    pub told: Vec<Uuid>,
    pub remaining: Option<i64>,
    /// `the person <name>` or `the team <slug>`.
    pub target: String,
    pub item: Option<String>,
}

/// The person that `reference` names: a person who is a member of the
/// organization `slug`. A service account is refused.
fn resolve_recipient(
    conn: &mut PgConnection,
    slug: &str,
    reference: &str,
    me: Uuid,
) -> Result<(Uuid, String), ApiError> {
    let reference = crate::references::text(reference);
    let mut member = None;
    for user in crate::references::people(conn, me, reference)? {
        if kairos_db::abac::is_org_member(conn, slug, user.id).map_err(ApiError::internal)? {
            if member.is_some() {
                return Err(ApiError::validation(format!(
                    "More than one member of the organization has the name {reference:?}. \
                     Send the id of the member."
                ))
                .with_details(serde_json::json!({ "field": "to" })));
            }
            member = Some(user);
        }
    }
    let Some(user) = member else {
        return Err(ApiError::validation(format!(
            "No member of the organization has the id, the email or the user name \
             {reference:?}. Send me, or a person of the organization."
        ))
        .with_details(serde_json::json!({ "field": "to" })));
    };
    if user.kind != kairos_db::models::public::USER_KIND_HUMAN {
        return Err(ApiError::validation(format!(
            "{reference:?} is a service account. A notice goes to a person. Send a person of \
             the organization, or a team."
        ))
        .with_details(serde_json::json!({ "field": "to" })));
    }
    let name = if user.display_name.trim().is_empty() {
        user.email.clone()
    } else {
        user.display_name.clone()
    };
    Ok((user.id, name))
}

/// Send a notice for the caller `user` (with the agent key `agent_key`)
/// in the organization `slug`. `limit` is the most notices of the sender
/// in one hour; 0 is no limit.
pub(crate) fn send_for(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    agent_key: Option<Uuid>,
    limit: u32,
    input: NoticeInput,
) -> Result<NoticeSent, ApiError> {
    if !kairos_db::task_claims::is_person(conn, user).map_err(ApiError::internal)? {
        return Err(ApiError::forbidden(
            "A service account cannot send a notice. A notice comes from a person, or from \
             an agent key of a person.",
        ));
    }
    let text = input.text.trim();
    if text.is_empty() {
        return Err(ApiError::validation(
            "The text of the notice is empty. Send 1 to 2000 characters.",
        )
        .with_details(serde_json::json!({ "field": "text" })));
    }
    let length = text.chars().count();
    if length > NOTICE_TEXT_MAX {
        return Err(ApiError::validation(format!(
            "The text of the notice has {length} characters. The limit is {NOTICE_TEXT_MAX}. \
             Write a note on an item, and link the item."
        ))
        .with_details(serde_json::json!({ "field": "text", "limit": NOTICE_TEXT_MAX })));
    }
    let (target, target_text) = match (input.to.as_deref(), input.team.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(
                ApiError::validation("Send `to` (a person) or `team` (a team), not both.")
                    .with_details(serde_json::json!({ "field": "to", "fields": ["to", "team"] })),
            );
        }
        (None, None) => {
            return Err(ApiError::validation(
                "Send `to` or `team`. `to` is a person: me, an id, an email or a user name. \
                 `team` is the slug or the id of a team.",
            )
            .with_details(serde_json::json!({ "field": "to", "fields": ["to", "team"] })));
        }
        (Some(to), None) => {
            let (id, name) = resolve_recipient(conn, slug, to, user)?;
            (Target::Person(id), format!("the person {name}"))
        }
        (None, Some(team)) => {
            let id = crate::api::org::teams::team_by_ref(conn, team)?;
            let team_slug = crate::references::team(conn, &id.to_string())?
                .map_or_else(|| id.to_string(), |team| team.slug);
            (Target::Team(id), format!("the team {team_slug}"))
        }
    };
    let item = match input.item.as_deref() {
        None => None,
        Some(code) => {
            let code_upper = crate::references::short_code(code);
            match crate::api::resolve_short_code(conn, &code_upper, crate::api::Liveness::LiveOnly)?
            {
                Some((id, _)) => Some((id, code_upper)),
                None => {
                    crate::api::follow_retired(conn, &code_upper, crate::api::Liveness::LiveOnly)?;
                    return Err(ApiError::not_found(format!(
                        "No live item has the short code {code_upper:?}. Send the code of a \
                         live item, or send no item."
                    ))
                    .with_details(serde_json::json!({ "field": "short_code" })));
                }
            }
        }
    };
    // KAIROS-A-0063 rules 6 and 7: `member` in the scope of the key.
    let board = match item {
        Some((id, _)) => {
            kairos_db::abac::resolve_authorization_board(conn, id).map_err(ApiError::internal)?
        }
        None => None,
    };
    if let Some(refusal) = ApiError::outside_key_scope(kairos_core::abac::MEMBER, board) {
        return Err(refusal);
    }
    let reader = crate::api::notifications::reader_for(conn, user, agent_key)?;
    let sender = Sender {
        user,
        key: reader.from_key(),
    };
    let sent = kairos_db::notices::send(
        conn,
        &sender,
        target,
        item.as_ref().map(|(id, code)| (*id, code.as_str())),
        text,
        limit,
        Utc::now(),
    )
    .map_err(|e| match e {
        SendError::RateLimited { limit, retry_after } => rate_limited(limit, retry_after),
        SendError::Database(e) => ApiError::internal(e),
    })?;
    Ok(NoticeSent {
        id: sent.id,
        told: sent.told,
        remaining: sent.remaining,
        target: target_text,
        item: item.map(|(_, code)| code),
    })
}

/// 429 `NOTICE_RATE_LIMITED`: the sender sent `limit` notices in the last
/// hour.
fn rate_limited(limit: u32, retry_after: i64) -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "NOTICE_RATE_LIMITED",
        format!(
            "You sent {limit} notices in the last hour. That is the limit for each agent key \
             (a person with no key counts as one). Kairos sent nothing. Send the notice again \
             after {retry_after} seconds."
        ),
    )
    .with_details(serde_json::json!({
        "limit": limit,
        "window_secs": kairos_db::notices::WINDOW.num_seconds(),
        "retry_after_secs": retry_after,
    }))
}

/// Send a notice to a person (`to`) or to each member of a team (`team`).
/// A notice is a text with an optional item link (KAIROS-T-0737). A notice from
/// an agent key names the key and its person. Each key can send
/// `KAIROS_NOTICE_LIMIT_PER_HOUR` notices in one hour (default 20); a
/// notice to a team counts as one. A recipient who turned the kind
/// `notice` off, or muted the key, gets nothing. A service account cannot
/// send a notice.
#[utoipa::path(
    post,
    path = "/api/notices",
    tag = "me",
    request_body = SendNoticeRequest,
    responses(
        (status = 201, description = "The notice, and the persons who got it", body = SendNoticeResponse),
        (status = 403, description = "The caller is a service account, or the agent key has no `member` in its scope", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "The team or the item is not found", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "`to` and `team` both or neither, an unknown person, or an empty or long text", body = kairos_client::types::ErrorEnvelope),
        (status = 429, description = "NOTICE_RATE_LIMITED: the sender is at the limit of one hour", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn send_notice(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<SendNoticeRequest>,
) -> Result<(StatusCode, Json<SendNoticeResponse>), ApiError> {
    let (user, key) = (auth.user_id, auth.agent_key);
    let limit = state.config.notice_limit_per_hour;
    let slug = tenant.slug.clone();
    let sent = state
        .blocking
        .run(&tenant.slug, move |conn| {
            send_for(
                conn,
                &slug,
                user,
                key,
                limit,
                NoticeInput {
                    to: body.to,
                    team: body.team,
                    text: body.text,
                    item: body.short_code,
                },
            )
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(SendNoticeResponse {
            id: sent.id.to_string(),
            told: sent.told.iter().map(Uuid::to_string).collect(),
            remaining: sent.remaining,
        }),
    ))
}
