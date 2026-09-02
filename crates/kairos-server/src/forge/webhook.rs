//! `POST /webhooks/{forge}/{tenant}/{connection_id}` (KAIROS-T-0099,
//! design in KAIROS-I-0009): the ingestion endpoint.
//!
//! # Why this mounts outside the auth stack
//!
//! A delivery from GitHub/GitLab carries no bearer token and no tenant
//! header — the forge knows only the URL it was configured with. So the
//! URL carries the routing (tenant slug + connection id) and the HMAC
//! signature carries the authenticity, exactly as the SCIM surface
//! resolves its tenant from the token rather than the subdomain. The
//! route is non-`/api` by design: it is not part of the S-0005 surface
//! and is invisible to the openapi route-vs-spec scanner, like
//! `/healthz` and `/metrics`.
//!
//! # Uniform failures
//!
//! A malformed slug, an unknown tenant, an unknown connection, and a bad
//! signature all return the SAME response. Distinguishing them would turn
//! this endpoint into a tenant/connection enumeration oracle (the rule
//! [`crate::scim::auth`] follows).
//!
//! # Why almost everything else is a 200
//!
//! A non-2xx makes forges retry, and retry, and eventually disable the
//! hook. So a delivery we understand but cannot act on — an event type we
//! do not consume, a payload naming no live short code — is a successful
//! no-op. Only authenticity failures reject.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use kairos_core::forge::{ForgeEvent, LinkKind as CoreKind, LinkState as CoreState};
use kairos_db::events::{EventKind, ThinEvent, emit_event};
use kairos_db::forge::{self, ForgeError};
use kairos_db::models::enums::{Forge, LinkKind, LinkState};
use kairos_db::models::forge::NewItemLink;
use kairos_db::tenant::is_valid_slug;
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::forge::auth::{derive_secret, github_signature, secure_eq};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/webhooks/{forge}/{tenant}/{connection_id}",
        post(receive),
    )
}

/// The single response every authenticity failure produces.
fn rejected() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": {
                "code": "WEBHOOK_REJECTED",
                "message": "unknown or unverifiable webhook delivery"
            }
        })),
    )
        .into_response()
}

/// Accepted — including the many "understood but nothing to do" cases.
fn accepted(detail: &str, linked: usize) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "status": detail, "links_written": linked })),
    )
        .into_response()
}

fn core_kind(kind: CoreKind) -> LinkKind {
    match kind {
        CoreKind::Branch => LinkKind::Branch,
        CoreKind::PullRequest => LinkKind::PullRequest,
    }
}

fn core_state(state: CoreState) -> LinkState {
    match state {
        CoreState::Open => LinkState::Open,
        CoreState::Merged => LinkState::Merged,
        CoreState::Closed => LinkState::Closed,
        CoreState::Draft => LinkState::Draft,
    }
}

/// Ingest one delivery.
pub(crate) async fn receive(
    State(state): State<AppState>,
    Path((forge_segment, tenant, connection_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // ---- routing + authenticity (uniform failures) -----------------------
    let Some(forge) = Forge::ALL
        .iter()
        .copied()
        .find(|f| f.as_str() == forge_segment)
    else {
        return rejected();
    };
    if !is_valid_slug(&tenant) {
        return rejected();
    }
    let Ok(connection_uuid) = connection_id.parse::<Uuid>() else {
        return rejected();
    };
    let Some(signing_key) = state.config.webhook_signing_key.clone() else {
        // Not configured: nothing can be verified, so nothing is accepted.
        return rejected();
    };

    // Verify over the RAW bytes, before any parsing — the signature covers
    // exactly what arrived, not a re-serialization of it.
    let secret = derive_secret(&signing_key, connection_uuid);
    let verified = match forge {
        Forge::Github => headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|presented| {
                secure_eq(presented, &github_signature(&secret, body.as_ref()))
            }),
        Forge::Gitlab => headers
            .get("x-gitlab-token")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|presented| secure_eq(presented, &secret)),
    };
    if !verified {
        return rejected();
    }

    // ---- normalize (pure; unknown event types are a no-op) ---------------
    let Ok(raw) = std::str::from_utf8(body.as_ref()) else {
        return accepted("body was not utf-8", 0);
    };
    let event_type = match forge {
        Forge::Github => headers.get("x-github-event"),
        Forge::Gitlab => headers.get("x-gitlab-event"),
    }
    .and_then(|v| v.to_str().ok())
    .unwrap_or_default()
    .to_string();
    let parsed: Option<ForgeEvent> = match forge {
        Forge::Github => kairos_core::forge::parse_github(&event_type, raw),
        Forge::Gitlab => kairos_core::forge::parse_gitlab(&event_type, raw),
    };
    let Some(event) = parsed else {
        return accepted("event type not consumed", 0);
    };

    // The slug is syntactically valid, but the tenant may not exist —
    // dispatching to a missing schema would 500 and leak the difference.
    // Resolve it in the PUBLIC schema first, exactly as SCIM does.
    let probe_slug = tenant.clone();
    let tenant_exists = state
        .blocking
        .run_public(move |conn| {
            use diesel::prelude::*;
            use kairos_db::schema::organizations::dsl;
            let found: Option<Uuid> = dsl::organizations
                .filter(dsl::slug.eq(&probe_slug))
                .select(dsl::id)
                .first(conn)
                .optional()
                .map_err(crate::error::ApiError::internal)?;
            Ok(found.is_some())
        })
        .await;
    match tenant_exists {
        Ok(true) => {}
        Ok(false) => return rejected(),
        Err(e) => return e.into_response(),
    }

    // ---- resolve + write --------------------------------------------------
    let codes = kairos_core::forge::extract_short_codes(&event.match_text);
    let result = state
        .blocking
        .run(&tenant, move |conn| {
            // The connection must exist, be live, and belong to this
            // tenant — the URL alone proves nothing.
            let connection = match forge::load_connection(conn, connection_uuid) {
                Ok(connection) => connection,
                Err(ForgeError::ConnectionNotFound(_)) => return Ok(None),
                Err(e) => return Err(crate::error::ApiError::internal(e)),
            };
            if connection.forge != forge {
                return Ok(None);
            }

            let kind = core_kind(event.kind);
            let mut written = 0usize;
            let mut touched: Vec<(Uuid, String, kairos_core::short_code::ItemType)> = Vec::new();
            for code in &codes {
                // Unknown or foreign codes are IGNORED, not errors: a
                // branch may legitimately name a code from another
                // deployment.
                let Some((item_id, item_type)) = crate::api::resolve_short_code(conn, code)?
                else {
                    continue;
                };
                let advanced = forge::upsert_link(
                    conn,
                    NewItemLink {
                        item_id,
                        connection_id: connection.id,
                        kind,
                        external_id: event.external_id.clone(),
                        title: event.title.clone(),
                        url: event.url.clone(),
                        state: core_state(event.state),
                        author: event.author.clone(),
                        forge_updated_at: event.forge_updated_at,
                    },
                )
                .map_err(crate::error::ApiError::internal)?;
                if advanced {
                    written += 1;
                    touched.push((item_id, code.clone(), item_type));
                }
            }

            // A pull request edited to DROP a short code should lose that
            // link — links are derived, so a stale association is simply
            // wrong. Only prune for PR events: a branch push carries no
            // authoritative list of the codes it once mentioned.
            if kind == LinkKind::PullRequest {
                let previously =
                    forge::linked_items(conn, connection.id, kind, &event.external_id)
                        .map_err(crate::error::ApiError::internal)?;
                let still_named: Vec<Uuid> = codes
                    .iter()
                    .filter_map(|code| {
                        crate::api::resolve_short_code(conn, code)
                            .ok()
                            .flatten()
                            .map(|(id, _)| id)
                    })
                    .collect();
                for stale in previously.into_iter().filter(|id| !still_named.contains(id)) {
                    forge::delete_link(conn, connection.id, kind, &event.external_id, stale)
                        .map_err(crate::error::ApiError::internal)?;
                }
            }

            // Thin events so open item/team views refresh (T-0074 pattern).
            for (_, short_code, item_type) in &touched {
                emit_event(
                    conn,
                    &ThinEvent {
                        event: EventKind::ItemLinksChanged,
                        entity_type: item_type.entity_type().to_string(),
                        short_code: short_code.clone(),
                        board_id: None,
                        column_id: None,
                        actor: connection.created_by,
                    },
                )
                .map_err(crate::error::ApiError::internal)?;
            }
            Ok(Some(written))
        })
        .await;

    match result {
        Ok(Some(written)) => accepted("ingested", written),
        // Unknown connection (or wrong forge for it) is an authenticity
        // failure, and must look like every other one.
        Ok(None) => rejected(),
        Err(e) => e.into_response(),
    }
}
