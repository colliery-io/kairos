//! The request timeout of `/api` and `/mcp` (KAIROS-T-0613).
//!
//! A request under `/api` or `/mcp` that has no answer after
//! `KAIROS_REQUEST_TIMEOUT_SECS` (default 30) gets a 503 with the code
//! `REQUEST_TIMEOUT`, and the server drops the work of the request. 503, not
//! 408: 408 says that the client was slow to send the request, and here the
//! server was slow to answer it. `0` turns the timeout off.
//!
//! The timeout ends when the head of the answer is ready. The body of an
//! answer that streams (a download) is not in it.
//!
//! Not in the timeout:
//!
//! - the upload and the download of a code index,
//!   `PUT|GET /api/repositories/{slug}/code-indexes/{commit}`: up to 256 MB
//!   in, and the download assembles the file before the head;
//! - the publish of a site, `PUT /api/repositories/{slug}/site`: an archive
//!   with no size limit, sent on to the site store;
//! - each path outside `/api` and `/mcp`: the WebSocket `/ws/events` stays
//!   open, and the probes, the GUI and `/pages` are not in the scope.
//!
//! `/mcp` is in the timeout. It runs stateless with JSON answers
//! (KAIROS-T-0392, KAIROS-T-0611), so it has no stream of server-sent events
//! to keep open.

use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::error::ApiError;

/// Is `path` under the timeout for `method`? See the module docs.
pub fn applies(method: &Method, path: &str) -> bool {
    let in_scope = under(path, "/api") || under(path, "/mcp");
    in_scope && !is_exempt(method, path)
}

/// `path` is `prefix` or a path below it.
fn under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The routes that can take longer than the timeout.
fn is_exempt(method: &Method, path: &str) -> bool {
    let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    match segments.as_slice() {
        // The static routes next to `{commit}` are in the timeout.
        ["", "api", "repositories", _, "code-indexes", commit] => {
            !matches!(*commit, "commits" | "nearest" | "builds" | "rebuild")
                && matches!(*method, Method::GET | Method::HEAD | Method::PUT)
        }
        ["", "api", "repositories", _, "site"] => method == Method::PUT,
        _ => false,
    }
}

/// The refusal when the timeout ends.
pub fn timed_out(timeout: Duration) -> ApiError {
    let secs = timeout.as_secs_f64();
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "REQUEST_TIMEOUT",
        format!(
            "The server did not complete the request in {secs} seconds, so it stopped the \
             request. A write of the request can be complete or not complete. Read the data \
             again before you send the request again."
        ),
    )
    .with_details(json!({ "timeout_secs": secs }))
}

/// The middleware: `timeout` on each request that [`applies`]. A zero
/// `timeout` turns it off.
pub async fn enforce(State(timeout): State<Duration>, req: Request, next: Next) -> Response {
    if timeout.is_zero() || !applies(req.method(), req.uri().path()) {
        return next.run(req).await;
    }
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    match tokio::time::timeout(timeout, next.run(req)).await {
        Ok(response) => response,
        Err(_) => {
            tracing::warn!(%method, %path, timeout_secs = timeout.as_secs_f64(), "request timeout");
            timed_out(timeout).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, put};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    #[test]
    fn the_scope_is_api_and_mcp() {
        assert!(applies(&Method::GET, "/api/whoami"));
        assert!(applies(&Method::GET, "/api"));
        assert!(applies(&Method::POST, "/mcp"));
        assert!(!applies(&Method::GET, "/apix"));
        assert!(!applies(&Method::GET, "/mcpx/a"));
        assert!(!applies(&Method::GET, "/ws/events"));
        assert!(!applies(&Method::GET, "/healthz"));
        assert!(!applies(&Method::GET, "/pages/acme/kairos/index.html"));
        assert!(!applies(&Method::GET, "/boards/x"));
    }

    #[test]
    fn the_uploads_and_the_index_download_are_exempt() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let index = format!("/api/repositories/kairos/code-indexes/{commit}");
        assert!(!applies(&Method::PUT, &index));
        assert!(!applies(&Method::GET, &index));
        assert!(!applies(&Method::PUT, "/api/repositories/kairos/site"));
        // The other routes of the two families are in the timeout.
        assert!(applies(&Method::DELETE, "/api/repositories/kairos/site"));
        assert!(applies(&Method::DELETE, &index));
        for name in ["commits", "nearest", "builds", "rebuild"] {
            let path = format!("/api/repositories/kairos/code-indexes/{name}");
            assert!(applies(&Method::GET, &path), "{path}");
            assert!(applies(&Method::POST, &path), "{path}");
        }
        assert!(applies(
            &Method::GET,
            "/api/repositories/kairos/code-indexes"
        ));
    }

    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_millis(500)).await;
        "done"
    }

    fn app(timeout: Duration) -> Router {
        // Paths of real routes: `tests/openapi.rs` reads each `.route(` of
        // the source, and a path under `/api` must be in the OpenAPI document.
        Router::new()
            .route("/api/whoami", get(slow))
            .route("/mcp", get(slow))
            .route("/slow", get(slow))
            .route("/api/repositories/{slug}/site", put(slow))
            .layer(axum::middleware::from_fn_with_state(timeout, enforce))
    }

    async fn call(router: &Router, method: Method, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
        (status, body)
    }

    #[tokio::test]
    async fn a_slow_request_gets_503_request_timeout() {
        let router = app(Duration::from_millis(100));
        for uri in ["/api/whoami", "/mcp"] {
            let started = std::time::Instant::now();
            let (status, body) = call(&router, Method::GET, uri).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}: {body}");
            assert_eq!(body["error"]["code"], "REQUEST_TIMEOUT", "{body}");
            assert_eq!(body["error"]["details"]["timeout_secs"], 0.1, "{body}");
            assert!(started.elapsed() < Duration::from_millis(450), "{uri}");
        }
    }

    #[tokio::test]
    async fn the_exempt_and_the_other_paths_run_to_the_end() {
        let router = app(Duration::from_millis(100));
        let (status, body) = call(&router, Method::PUT, "/api/repositories/kairos/site").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = call(&router, Method::GET, "/slow").await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    #[tokio::test]
    async fn zero_turns_the_timeout_off() {
        let router = app(Duration::ZERO);
        let (status, body) = call(&router, Method::GET, "/api/whoami").await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
}
