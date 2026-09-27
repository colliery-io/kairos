//! `GET /api/{entity_type}/{short_code}/cascade-preview` (KAIROS-T-0051):
//! the AUTHORITATIVE pre-delete cascade preview.
//!
//! Found by KAIROS-T-0041: the GUI delete confirm can only warn with an
//! item's DIRECT relationship children before the fact; the full
//! transitive descendant set (KAIROS-A-0001) was known only from the
//! post-delete `DeleteResponse`. This read-only endpoint closes that gap —
//! it returns exactly the descendant set a soft-delete WOULD cascade to,
//! computed by the same plan as the delete (single source of truth; see
//! [`kairos_db::items::preview_cascade_as`]), without deleting anything.
//!
//! Shape: a side-effect-free GET on a per-item subresource — the RESTful,
//! idempotent choice, and the same generic `{entity_type}` pattern as the
//! `relationships`/`metadata`/`history` reads. Open tenant-wide like every
//! other read (KAIROS-A-0006): no capability check beyond the middleware
//! stack.
//!
//! # The archive is here too (COLLIERY-T-0234)
//!
//! [`archive_item`] is the one archive of the server: five REST handlers
//! and the MCP tool `delete_item` call it, so the rule is the same on the
//! two surfaces. The preview answers FOR THE CALLER who asks, with the
//! same plan: what an archive by this caller takes, and what it leaves.

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use kairos_client::types as dto;
use kairos_db::items;

use kairos_core::short_code::ItemType;
use uuid::Uuid;

use super::meta::resolve_family_item;
use super::{map_item_error, require_item_edit};
use crate::api::Liveness;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/{entity_type}/{short_code}/cascade-preview",
        get(cascade_preview),
    )
}

/// Archive an item for a principal (COLLIERY-T-0234): the ONE archive of
/// the server.
///
/// 1. The edit rule for the NAMED item ([`require_item_edit`]). A caller
///    who may not edit it gets the 403, and nothing is archived.
/// 2. The archive, with the edit rule for EACH descendant
///    ([`items::soft_delete_item_as`]).
///
/// THE ATTACK that step 2 stops. A principal creates an initiative, links
/// the task of a different team below it (the link rule permits the edge),
/// and archives the initiative. Step 1 alone let the cascade archive that
/// task.
///
/// The named item is archived when some descendants stay. They are in
/// `not_reached`, with the reason, and they keep their `parent` edge.
pub(crate) fn archive_item(
    conn: &mut diesel::pg::PgConnection,
    slug: &str,
    user_id: Uuid,
    item_id: Uuid,
    item_type: ItemType,
) -> Result<items::SoftDeleteOutcome, ApiError> {
    require_item_edit(conn, slug, user_id, item_id, item_type)?;
    let principal = items::Principal {
        org_slug: slug,
        user_id,
    };
    items::soft_delete_item_as(conn, principal, item_type, item_id).map_err(map_item_error)
}

/// The response of an archive. `not_reached` is absent on the wire when
/// the archive reached each descendant, so that response is as it was.
pub(crate) fn delete_response(outcome: items::SoftDeleteOutcome) -> dto::DeleteResponse {
    dto::DeleteResponse {
        short_code: outcome.root_short_code,
        cascade_count: outcome.cascaded_short_codes.len() as i64,
        cascaded_short_codes: outcome.cascaded_short_codes,
        not_reached: not_reached_dto(outcome.not_reached),
    }
}

fn not_reached_dto(items: Vec<items::NotReached>) -> Vec<dto::NotReached> {
    items
        .into_iter()
        .map(|item| match item.reason {
            items::NotReachedReason::CannotEdit {
                capability,
                board_id,
            } => dto::NotReached {
                short_code: item.short_code,
                required_capability: Some(capability.to_string()),
                board_id: board_id.map(|id| id.to_string()),
                below: None,
            },
            items::NotReachedReason::Below { short_code } => dto::NotReached {
                short_code: item.short_code,
                required_capability: None,
                board_id: None,
                below: Some(short_code),
            },
        })
        .collect()
}

/// One line for each descendant that an archive did not reach, as MCP
/// `delete_item` prints it. Empty when the archive reached each one.
pub(crate) fn not_reached_lines(items: &[items::NotReached]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "The archive did not reach {} item(s). They stay live and keep their parent.\n",
        items.len()
    );
    for item in items {
        let reason = match &item.reason {
            items::NotReachedReason::CannotEdit {
                capability,
                board_id: Some(board_id),
            } => format!("you need `{capability}` on board {board_id}"),
            items::NotReachedReason::CannotEdit {
                capability,
                board_id: None,
            } => format!("it has no board for `{capability}`; ask an organization admin"),
            items::NotReachedReason::Below { short_code } => {
                format!("it is below {short_code}")
            }
        };
        out.push_str(&format!("- {}: {reason}.\n", item.short_code));
    }
    out
}

/// What an archive of this item BY THE CALLER would take, and what it
/// would leave (COLLIERY-T-0234), computed without deleting. An open
/// tenant-wide read: it names short codes, which each member can read.
///
/// The answer is for the caller who asks. It does not say whether the
/// caller may archive the item itself: `DELETE` checks that.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/cascade-preview",
    tag = "cascade",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "What a soft-delete by the caller would cascade to (root excluded), and the live descendants that it would leave, matching the eventual DeleteResponse", body = dto::CascadePreviewResponse),
        (status = 404, description = "Unknown family or short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn cascade_preview(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::CascadePreviewResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let principal = items::Principal {
                org_slug: &slug,
                user_id: user,
            };
            let preview = items::preview_cascade_as(conn, principal, item_type, item_id)
                .map_err(map_item_error)?;
            Ok(dto::CascadePreviewResponse {
                short_code: preview.root_short_code,
                cascade_count: preview.cascaded_short_codes.len() as i64,
                cascaded_short_codes: preview.cascaded_short_codes,
                not_reached: not_reached_dto(preview.not_reached),
            })
        })
        .await?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_db::items::{NotReached, NotReachedReason};

    fn left() -> Vec<NotReached> {
        vec![
            NotReached {
                short_code: "ACME-I-0002".into(),
                reason: NotReachedReason::CannotEdit {
                    capability: "manage_initiatives",
                    board_id: Some(Uuid::nil()),
                },
            },
            NotReached {
                short_code: "ACME-T-0009".into(),
                reason: NotReachedReason::Below {
                    short_code: "ACME-I-0002".into(),
                },
            },
        ]
    }

    /// COLLIERY-T-0234: MCP `delete_item` adds nothing to its output when
    /// the archive reached each descendant, and one line for each that
    /// stays.
    #[test]
    fn the_lines_name_each_item_and_its_reason() {
        assert_eq!(not_reached_lines(&[]), "");
        assert_eq!(
            not_reached_lines(&left()),
            "The archive did not reach 2 item(s). They stay live and keep their parent.\n\
             - ACME-I-0002: you need `manage_initiatives` on board \
             00000000-0000-0000-0000-000000000000.\n\
             - ACME-T-0009: it is below ACME-I-0002.\n"
        );
    }

    /// COLLIERY-T-0234: the wire has no `not_reached` when nothing stays,
    /// so a response for a caller who can edit each descendant is as it
    /// was. An entry has one reason, and the other fields are absent.
    #[test]
    fn the_response_is_as_it_was_when_nothing_stays() {
        let outcome = |not_reached| items::SoftDeleteOutcome {
            root_short_code: "ACME-S-0001".into(),
            cascaded_short_codes: vec!["ACME-I-0001".into()],
            not_reached,
        };
        assert_eq!(
            serde_json::to_value(delete_response(outcome(Vec::new()))).expect("json"),
            serde_json::json!({
                "short_code": "ACME-S-0001",
                "cascade_count": 1,
                "cascaded_short_codes": ["ACME-I-0001"],
            })
        );
        assert_eq!(
            serde_json::to_value(delete_response(outcome(left()))).expect("json")["not_reached"],
            serde_json::json!([
                {
                    "short_code": "ACME-I-0002",
                    "required_capability": "manage_initiatives",
                    "board_id": "00000000-0000-0000-0000-000000000000",
                },
                {"short_code": "ACME-T-0009", "below": "ACME-I-0002"},
            ])
        );
    }
}
