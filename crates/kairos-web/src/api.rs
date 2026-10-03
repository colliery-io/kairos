//! The SPA data layer (KAIROS-T-0039): a thin fetch wrapper over the
//! same-origin `/api`, plus the mirror DTOs the shell needs.
//!
//! # Why not `kairos-client`? (decision, KAIROS-T-0039)
//!
//! `kairos-client` does not compile for `wasm32-unknown-unknown` — its
//! transport is native (`tokio` "full", `tokio-tungstenite`; `getrandom`
//! without a wasm backend), and A-0015 anticipated exactly this ("or a
//! `kairos-types` sub-crate if WASM feature-gating demands it"). Rather
//! than restructure that crate from this task, the GUI:
//!
//! - talks HTTP through `gloo-net` (the browser `fetch` API), and
//! - declares **mirror DTOs**: local structs with the exact wire field
//!   names, deserialized with serde like everywhere else. Mirrors are
//!   *partial on purpose* — declare only the fields the view consumes;
//!   serde ignores the rest, so additive API changes never break the GUI.
//!
//! Follow-up recorded in the task: split the plain-serde types out of
//! `kairos-client` into a `kairos-types` crate both sides consume. Until
//! then, every mirror carries a `// mirror of:` comment naming its source
//! type so drift is greppable.
//!
//! # Conventions (binding for T-0040..T-0044 — see docs/gui-conventions.md)
//!
//! - Every call goes through [`get_json`] (add `post_json`/`patch_json`
//!   siblings here as needed — same shape).
//! - Errors are `aurora_dark::tokens::ApiError` — exactly what
//!   `<ErrorState/>` consumes; the S-0005 error envelope is mapped in one
//!   place ([`error_from_response`]).
//! - A 401 clears the session (the protected shell then re-runs the
//!   login redirect); components never handle 401 themselves.

use aurora_dark::tokens::ApiError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::Auth;

/// `GET {path}` with the bearer token; JSON-decode the body.
pub async fn get_json<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::get(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// mirror of: `kairos_client::types::ListEnvelope<T>` (partial): one page
/// of a list route, and the number of rows of the full list.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

/// The largest page that the server gives (`MAX_LIMIT` of the server).
pub const PAGE_LIMIT: i64 = 200;

/// The offset of the next page of a list, or `None` when the list is
/// complete (COLLIERY-T-0257). `fetched` is the number of rows that the
/// client has, and `last_page` is the number of rows of the last page.
///
/// An empty page stops the read, also when `total` says that there are more
/// rows: a row that a person deleted between two pages must not make a loop
/// with no end. Pure, host-tested.
pub fn next_offset(fetched: usize, last_page: usize, total: i64) -> Option<i64> {
    let fetched = i64::try_from(fetched).unwrap_or(i64::MAX);
    (last_page > 0 && fetched < total).then_some(fetched)
}

/// One value of a URL, with each character that is not unreserved as a
/// percent code (COLLIERY-T-0265): a value of a query, or one part of a
/// path. A slug from before the rule of a slug can have a space or a `/`.
/// Pure, host-tested.
pub fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The URL of one page of a list route (COLLIERY-T-0258). `path` can have
/// a query (`/api/metadata-definitions?entity_type=task`): the page
/// parameters go after it. Pure, host-tested.
pub fn page_url(path: &str, limit: i64, offset: i64) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{path}{separator}limit={limit}&offset={offset}")
}

/// The sentence that a page shows when it has only a part of a list
/// (COLLIERY-T-0258), or `None` when the page has the full list. `shown`
/// is the number of rows on the page, and `total` is the number of rows of
/// the full list. Pure, host-tested.
pub fn incomplete_list_note(shown: usize, total: i64) -> Option<String> {
    let shown = i64::try_from(shown).unwrap_or(i64::MAX);
    (shown < total).then(|| format!("The list shows {shown} of {total} items."))
}

/// The sentence of a pager (COLLIERY-T-0258): the rows that the page
/// shows, and the number of rows of the full list. `offset` is the offset
/// of the page, and `shown` is the number of rows on it. A page that does
/// not have the full list thus says so, and the pager gives the next
/// page. Pure, host-tested.
pub fn page_range_note(offset: i64, shown: usize, total: i64) -> String {
    let shown = i64::try_from(shown).unwrap_or(i64::MAX);
    if shown == 0 {
        return format!("The list shows 0 of {total} items.");
    }
    let first = offset + 1;
    let last = offset.saturating_add(shown);
    format!("The list shows items {first} to {last} of {total}.")
}

/// Each row of a list route: `GET {path}?limit=200&offset=N`, page after
/// page, until the client has the full list (COLLIERY-T-0257). `path` can
/// have a query (COLLIERY-T-0258).
///
/// For a list with a bound: the members, the teams, the boards, the
/// templates, the definitions and the streams of an organization. A feed
/// with no bound (the activity, the results of a search) has a pager, and
/// it does not use this function.
///
/// For a page that makes a decision from the FULL list. The page of the
/// boards in the administration counts the delivery boards of a team, and
/// a count from the first 200 boards only can be wrong.
pub async fn get_all<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<Vec<T>, ApiError> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let page: Page<T> = get_json(auth, &page_url(path, PAGE_LIMIT, offset)).await?;
        let last_page = page.items.len();
        all.extend(page.items);
        match next_offset(all.len(), last_page, page.total) {
            Some(next) => offset = next,
            None => return Ok(all),
        }
    }
}

/// `POST {path}` with a JSON body and the bearer token; JSON-decode the
/// response body (same shape as [`get_json`], per docs/gui-conventions.md).
pub async fn post_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::post(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| {
            ApiError::Unknown(format!(
                "The page cannot write the request for {path}: {e}."
            ))
        })?
        .send()
        .await
        .map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `POST {path}` with NO body and the bearer token; JSON-decode the
/// response body. For a route that accepts no body (the confirm and the
/// reject of a proposal): the server refuses a body there, and `null` is
/// a body (COLLIERY-T-0256).
pub async fn post_empty<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::post(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `PATCH {path}` with a JSON body and the bearer token; JSON-decode the
/// response body (same shape as [`get_json`], per docs/gui-conventions.md).
pub async fn patch_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::patch(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| {
            ApiError::Unknown(format!(
                "The page cannot write the request for {path}: {e}."
            ))
        })?
        .send()
        .await
        .map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `PUT {path}` with a JSON body and the bearer token (KAIROS-T-0109).
pub async fn put_json<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::put(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| {
            ApiError::Unknown(format!(
                "The page cannot write the request for {path}: {e}."
            ))
        })?
        .send()
        .await
        .map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// `DELETE {path}` with the bearer token; JSON-decode the response body.
pub async fn delete_json<T: DeserializeOwned>(auth: Auth, path: &str) -> Result<T, ApiError> {
    let token = auth.token();
    let mut request = gloo_net::http::Request::delete(path);
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|_| ApiError::Network)?;
    decode_response(auth, token, path, response).await
}

/// The shared response tail of every `*_json` helper: 401 clears the
/// session (the protected shell then re-runs the login redirect), non-2xx
/// maps through the S-0005 envelope, 2xx JSON-decodes.
///
/// `sent_token` is the bearer THIS request carried. A 401 only expires the
/// session when that token is still the current one (KAIROS-T-0071): a 401
/// from a token-less request (resources fire eagerly while the boot-time
/// session restore is in flight) or from a token that has since been
/// replaced proves nothing about the current session — expiring on those
/// stomped the freshly restored session and bounced reloads to the issuer.
async fn decode_response<T: DeserializeOwned>(
    auth: Auth,
    sent_token: Option<String>,
    path: &str,
    response: gloo_net::http::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    if status == 401 && sent_token.is_some() && auth.token() == sent_token {
        // Convention: expired/invalid CURRENT token → drop the session;
        // the protected shell reacts with the issuer redirect (silent
        // re-auth when the IdP still holds a session).
        auth.expire();
    }
    if !(200..300).contains(&status) {
        return Err(error_from_response(status, response).await);
    }
    response.json::<T>().await.map_err(|e| {
        ApiError::Unknown(format!("The page cannot read the response of {path}: {e}."))
    })
}

/// Map a non-2xx response onto [`ApiError`] via the S-0005 error envelope
/// (`{"error": {"code", "message", …}}`), falling back to the raw body.
async fn error_from_response(status: u16, response: gloo_net::http::Response) -> ApiError {
    let body = response.text().await.unwrap_or_default();
    refusal_from_body(status, body).error
}

/// A refusal of a write, with the field that it is about
/// (COLLIERY-T-0267). A form shows the message next to that field.
#[derive(Clone, PartialEq)]
pub struct Refusal {
    pub error: ApiError,
    /// `details.field` of the envelope: the name of the field in the
    /// request. `None` when the refusal is not about one field.
    pub field: Option<String>,
}

impl Refusal {
    /// A refusal that is not about a field.
    fn of(error: ApiError) -> Self {
        Self { error, field: None }
    }

    /// The message of the server, when the refusal is about `field`.
    pub fn message_for(&self, field: &str) -> Option<String> {
        match &self.error {
            ApiError::Http { message, .. } if self.field.as_deref() == Some(field) => {
                Some(message.clone())
            }
            _ => None,
        }
    }
}

/// The body of a non-2xx response as a [`Refusal`].
fn refusal_from_body(status: u16, body: String) -> Refusal {
    let envelope: Option<ErrorEnvelope> = serde_json::from_str(&body).ok();
    let (message, code, field) = match envelope {
        Some(envelope) => (
            envelope.error.message,
            Some(envelope.error.code),
            envelope.error.details["field"].as_str().map(str::to_string),
        ),
        None => (body, None, None),
    };
    Refusal {
        error: ApiError::Http {
            status,
            message,
            code,
        },
        field,
    }
}

/// Send `request` with a JSON body and the bearer token. A refusal keeps
/// the field that the server names (COLLIERY-T-0267). The 401 rule is that
/// of [`decode_response`].
async fn write_json_refusal<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    mut request: gloo_net::http::RequestBuilder,
    path: &str,
    body: &B,
) -> Result<T, Refusal> {
    let token = auth.token();
    if let Some(token) = &token {
        request = request.header("authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| {
            Refusal::of(ApiError::Unknown(format!(
                "The page cannot write the request for {path}: {e}."
            )))
        })?
        .send()
        .await
        .map_err(|_| Refusal::of(ApiError::Network))?;
    let status = response.status();
    if status == 401 && token.is_some() && auth.token() == token {
        auth.expire();
    }
    if !(200..300).contains(&status) {
        return Err(refusal_from_body(
            status,
            response.text().await.unwrap_or_default(),
        ));
    }
    response.json::<T>().await.map_err(|e| {
        Refusal::of(ApiError::Unknown(format!(
            "The page cannot read the response of {path}: {e}."
        )))
    })
}

/// [`post_json`], with the field of a refusal.
pub async fn post_json_refusal<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, Refusal> {
    write_json_refusal(auth, gloo_net::http::Request::post(path), path, body).await
}

/// [`patch_json`], with the field of a refusal.
pub async fn patch_json_refusal<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, Refusal> {
    write_json_refusal(auth, gloo_net::http::Request::patch(path), path, body).await
}

/// [`put_json`], with the field of a refusal.
pub async fn put_json_refusal<B: Serialize, T: DeserializeOwned>(
    auth: Auth,
    path: &str,
    body: &B,
) -> Result<T, Refusal> {
    write_json_refusal(auth, gloo_net::http::Request::put(path), path, body).await
}

/// mirror of: `kairos_client::types::ErrorEnvelope` (S-0005).
#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

/// mirror of: `kairos_client::types::ErrorBody` (S-0005).
#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
    /// `{}` when the refusal has no structured extras.
    #[serde(default)]
    details: serde_json::Value,
}

// ---- whoami (the shell's only data need) --------------------------------

/// `GET /api/whoami`.
pub async fn whoami(auth: Auth) -> Result<Whoami, ApiError> {
    get_json(auth, "/api/whoami").await
}

/// mirror of: `kairos_server::app::WhoamiResponse` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Whoami {
    pub user: WhoamiUser,
    pub organization: WhoamiOrganization,
    #[serde(default)]
    pub teams: Vec<WhoamiTeam>,
    /// Board-scoped capability grants (KAIROS-A-0006), grouped by board —
    /// drives per-board admin gating (KAIROS-T-0052) for non-admins.
    #[serde(default)]
    pub capabilities: Vec<WhoamiBoardCapabilities>,
    /// The COMPUTED capabilities that the server gives to each member of
    /// the tenant with no grant (KAIROS-T-0105). Today the list holds
    /// `file_backlog`: the person can send a request to the delivery board
    /// of any team (COLLIERY-T-0218).
    ///
    /// The board page reads it to offer "New request" (COLLIERY-T-0232).
    /// The GUI reads the list and does not assume it, so that a server that
    /// stops giving `file_backlog` also stops the button. A body with no
    /// `implicit` decodes to an empty list, which offers no request.
    #[serde(default)]
    pub implicit: Vec<String>,
}

/// mirror of: `kairos_server::app::WhoamiBoardCapabilities` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiBoardCapabilities {
    pub board_slug: String,
    pub grants: Vec<String>,
}

/// mirror of: `kairos_server::app::WhoamiUser` (partial). `id` drives
/// "mine" affordances (e.g. deleting my own team announcement,
/// KAIROS-T-0085).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiUser {
    pub id: String,
    pub display_name: String,
    pub email: String,
}

/// mirror of: `kairos_server::app::WhoamiOrganization` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiOrganization {
    pub slug: String,
    pub role: String,
}

/// mirror of: `kairos_server::app::WhoamiTeam` (partial). `id` feeds the
/// KAIROS-T-0072 client-side capability mirror (team-owned boards carry
/// `team_id`; matching it against my teams decides the implied powers).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WhoamiTeam {
    pub id: String,
    pub slug: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COLLIERY-T-0267: a refusal keeps the field that the server names.
    #[test]
    fn a_refusal_keeps_the_field_of_the_server() {
        let body = serde_json::json!({"error": {
            "code": "VALIDATION",
            "message": "The default_branch is empty. Send a value, for example main.",
            "details": {"field": "default_branch"},
        }})
        .to_string();
        let refusal = refusal_from_body(422, body);
        assert_eq!(refusal.field.as_deref(), Some("default_branch"));
        assert_eq!(
            refusal.message_for("default_branch").as_deref(),
            Some("The default_branch is empty. Send a value, for example main.")
        );
        assert_eq!(refusal.message_for("repo_url"), None);

        // No details, and a body that is not the envelope.
        let body = serde_json::json!({"error": {"code": "CONFLICT", "message": "Taken."}});
        let refusal = refusal_from_body(409, body.to_string());
        assert_eq!(refusal.field, None);
        assert!(matches!(
            refusal.error,
            ApiError::Http { status: 409, ref message, .. } if message == "Taken."
        ));
        let refusal = refusal_from_body(502, "Bad Gateway".to_string());
        assert_eq!(refusal.field, None);
        assert!(matches!(
            refusal.error,
            ApiError::Http { status: 502, ref message, code: None } if message == "Bad Gateway"
        ));
    }

    /// The mirror decodes a real WhoamiResponse body (field-name lock).
    #[test]
    fn whoami_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "user": {
                "id": "6e4ff04d-1c92-4c66-9e46-94e0d9e0f70f",
                "external_id": "sub-123",
                "email": "alice@kairos.test",
                "display_name": "alice"
            },
            "organization": {
                "id": "0a8e9f7d-58f7-4f6e-9f0f-4dbb1a8f3e21",
                "slug": "demo",
                "role": "admin"
            },
            "teams": [{"id": "b1e2…", "slug": "platform", "name": "Platform"}],
            "capabilities": [{
                "board_id": "c3f4a5b6-1234-4c66-9e46-94e0d9e0f70f",
                "board_slug": "platform-delivery",
                "grants": ["configure_boards", "administer_members"]
            }],
            "implicit": ["file_backlog"]
        });
        let whoami: Whoami = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(whoami.user.display_name, "alice");
        assert_eq!(whoami.organization.slug, "demo");
        assert_eq!(whoami.teams[0].slug, "platform");
        assert_eq!(whoami.teams[0].id, "b1e2…");
        assert_eq!(whoami.capabilities[0].board_slug, "platform-delivery");
        assert_eq!(whoami.capabilities[0].grants.len(), 2);
        // COLLIERY-T-0232: the computed capabilities are on the mirror.
        assert_eq!(whoami.implicit, ["file_backlog"]);
    }

    /// COLLIERY-T-0232: a body with no `implicit` gives an empty list. The
    /// board page then offers no request, which is the safe direction.
    #[test]
    fn whoami_mirror_decodes_a_body_with_no_implicit_list() {
        let body = serde_json::json!({
            "user": {"id": "u-1", "email": "u@x.test", "display_name": "u"},
            "organization": {"slug": "demo", "role": "member"}
        });
        let whoami: Whoami = serde_json::from_value(body).expect("mirror decodes");
        assert!(whoami.implicit.is_empty());
    }

    /// COLLIERY-T-0257: a list with more rows than one page. The client
    /// reads the next page from the number of rows that it has.
    #[test]
    fn the_next_page_starts_after_the_rows_that_the_client_has() {
        // 450 boards: 3 pages.
        assert_eq!(next_offset(200, 200, 450), Some(200));
        assert_eq!(next_offset(400, 200, 450), Some(400));
        assert_eq!(next_offset(450, 50, 450), None);
        // A list of one page, and an empty list.
        assert_eq!(next_offset(12, 12, 12), None);
        assert_eq!(next_offset(0, 0, 0), None);
        // A list of 200 rows is complete after one full page.
        assert_eq!(next_offset(200, 200, 200), None);
    }

    /// An empty page stops the read: a row that a person deleted between
    /// two pages makes `total` larger than the number of rows.
    #[test]
    fn an_empty_page_stops_the_read() {
        assert_eq!(next_offset(400, 0, 401), None);
        // A page that is not full, with more rows to come, reads on.
        assert_eq!(next_offset(399, 199, 401), Some(399));
    }

    /// COLLIERY-T-0258: the page parameters go after the query of the path.
    #[test]
    fn the_page_parameters_go_after_the_query_of_the_path() {
        assert_eq!(
            page_url("/api/teams", 200, 0),
            "/api/teams?limit=200&offset=0"
        );
        assert_eq!(
            page_url("/api/metadata-definitions?entity_type=task", 200, 400),
            "/api/metadata-definitions?entity_type=task&limit=200&offset=400"
        );
    }

    /// COLLIERY-T-0258: a page that has a part of a list says so.
    #[test]
    fn a_page_with_a_part_of_a_list_says_so() {
        assert_eq!(
            incomplete_list_note(100, 2340).as_deref(),
            Some("The list shows 100 of 2340 items.")
        );
        assert_eq!(
            incomplete_list_note(200, 201).as_deref(),
            Some("The list shows 200 of 201 items.")
        );
        // The full list, an empty list, and a list that lost a row.
        assert_eq!(incomplete_list_note(12, 12), None);
        assert_eq!(incomplete_list_note(0, 0), None);
        assert_eq!(incomplete_list_note(13, 12), None);
    }

    /// COLLIERY-T-0258: the pager gives the rows of the page and the number
    /// of rows of the full list.
    #[test]
    fn the_pager_gives_the_rows_of_the_page() {
        assert_eq!(
            page_range_note(0, 25, 2340),
            "The list shows items 1 to 25 of 2340."
        );
        assert_eq!(
            page_range_note(2325, 15, 2340),
            "The list shows items 2326 to 2340 of 2340."
        );
        assert_eq!(
            page_range_note(0, 3, 3),
            "The list shows items 1 to 3 of 3."
        );
        // A page after the end of the list.
        assert_eq!(page_range_note(50, 0, 40), "The list shows 0 of 40 items.");
    }

    /// The mirror of a page reads `items` and `total`.
    #[test]
    fn a_page_decodes_the_list_envelope() {
        let body = serde_json::json!({
            "items": [{"slug": "platform"}, {"slug": "web"}],
            "total": 450, "limit": 200, "offset": 0
        });
        let page: Page<serde_json::Value> = serde_json::from_value(body).expect("decodes");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total, 450);
    }
}
