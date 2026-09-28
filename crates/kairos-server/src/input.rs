//! The query and the body of a request (COLLIERY-T-0256).
//!
//! COLLIERY-T-0249 made each write route refuse a body with a field that it
//! does not know ([`crate::body`]). This module does the same for the two
//! other inputs of a route, so that no input is ignored:
//!
//! | The request | Status | `details` |
//! |---|---|---|
//! | has a query parameter that the route does not know | 400 | `parameter`, `allowed` |
//! | has a query parameter with a value that the route cannot read | 400 | `parameter` |
//! | has a body, and the route accepts no body | 400 | — |
//!
//! The code is `VALIDATION`. The status is 400: that is the status that
//! axum gave to a query that it could not read, so a client that reads the
//! status only sees no change.
//!
//! # The two parts
//!
//! - [`ApiQuery`] is the extractor that a handler with query parameters
//!   uses in place of `axum::extract::Query`. The type of the query has
//!   `#[serde(deny_unknown_fields)]`, so the type is the list of the
//!   parameters.
//! - [`refuse_unknown_input`] is a middleware. It is on each router of the
//!   first-party routes ([`crate::app::router`]), after the auth and the
//!   tenant. For a route with NO query parameters it refuses each query
//!   parameter. For a route with NO body it refuses a body that is not
//!   empty. A handler has nothing to do for the two.
//!
//! # How the middleware knows the route
//!
//! axum cannot tell which extractors a handler has. The OpenAPI document
//! can: `tests/openapi.rs` proves that it has each route of the API, and
//! each handler gives its query parameters and its body there. [`ROUTES`]
//! is that document as a table. A route that the document does not have
//! (`GET /api/docs`) has no query parameters and no body.
//!
//! `tests/unknown_inputs.rs` sends an unknown parameter to each route of
//! the document. For a route with query parameters it compares
//! `details.allowed`, which comes from the type, with the document. So a
//! handler cannot have a parameter that the document does not show.
//!
//! # A body that is empty
//!
//! A body of zero length is "no body". `{}` and `null` are not: they are
//! input that does nothing, and the rule of the owner is that the server
//! refuses such input. No standard tells a client to send them.

use std::collections::HashMap;
use std::sync::LazyLock;

use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::{FromRequestParts, MatchedPath, Query, Request};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::body::{innermost, unknown_field};
use crate::error::ApiError;

/// The query of a route. See the module docs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::try_from_uri(&parts.uri) {
            Ok(Query(query)) => Ok(ApiQuery(query)),
            Err(rejection) => Err(query_rejection(&rejection)),
        }
    }
}

/// The refusal of a query that the route cannot read.
fn query_rejection(rejection: &QueryRejection) -> ApiError {
    let message = innermost(rejection);
    if let Some((parameter, allowed)) = unknown_field(&message) {
        return unknown_parameter(&parameter, &allowed);
    }
    // `serde_path_to_error` puts the name of the parameter before the
    // message: `limit: invalid digit found in string`.
    match message.split_once(": ") {
        Some((parameter, fault)) if is_name(parameter) => ApiError::new(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            format!("The value of the query parameter {parameter:?} is not correct: {fault}."),
        )
        .with_details(json!({ "parameter": parameter })),
        _ => ApiError::new(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            format!("The query is not correct for this route: {message}."),
        ),
    }
}

/// True for the name of a field of a query type.
fn is_name(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The refusal of a query parameter that the route does not know.
pub fn unknown_parameter<S: AsRef<str>>(parameter: &str, allowed: &[S]) -> ApiError {
    let allowed: Vec<&str> = allowed.iter().map(AsRef::as_ref).collect();
    let known = if allowed.is_empty() {
        "This route has no query parameters.".to_string()
    } else {
        format!(
            "The query parameters of this route are: {}.",
            allowed.join(", ")
        )
    };
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "VALIDATION",
        format!(
            "The request has the query parameter {parameter:?}. This route does not accept \
             that parameter. {known}"
        ),
    )
    .with_details(json!({ "parameter": parameter, "allowed": allowed }))
}

/// The refusal of the first parameter of `query` that is not in `allowed`.
/// `None` when each parameter is in the list. For a route that reads its
/// query without a type (`/ws/events`), and for a route with no query.
pub fn refuse_parameters_not_in(uri: &Uri, allowed: &[&str]) -> Option<ApiError> {
    // A list of pairs reads each query, so the default is not reached.
    let Query(pairs) = Query::<Vec<(String, String)>>::try_from_uri(uri).unwrap_or_default();
    pairs
        .iter()
        .find(|(name, _)| !allowed.contains(&name.as_str()))
        .map(|(name, _)| unknown_parameter(name, allowed))
}

/// The refusal of a body on a route that accepts no body.
fn body_not_accepted() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "VALIDATION",
        "The request has a body. This route does not accept a body. Send the request with \
         no body.",
    )
}

/// The inputs that a route accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RouteInputs {
    /// The route has query parameters. [`ApiQuery`] does the check.
    query: bool,
    /// The route has a body. [`crate::body::ApiJson`] does the check.
    body: bool,
}

/// Each route of the OpenAPI document, by path and method. See the module
/// docs.
static ROUTES: LazyLock<HashMap<String, HashMap<Method, RouteInputs>>> = LazyLock::new(|| {
    let spec = serde_json::to_value(crate::api::openapi::spec())
        .expect("the derived OpenAPI document serializes");
    let mut routes = HashMap::new();
    let paths = spec["paths"].as_object().cloned().unwrap_or_default();
    for (path, item) in paths {
        let mut methods = HashMap::new();
        for (method, operation) in item.as_object().cloned().unwrap_or_default() {
            let Ok(method) = Method::from_bytes(method.to_uppercase().as_bytes()) else {
                continue;
            };
            let query = operation["parameters"]
                .as_array()
                .is_some_and(|all| all.iter().any(|p| p["in"] == "query"));
            let body = operation.get("requestBody").is_some();
            methods.insert(method, RouteInputs { query, body });
        }
        routes.insert(path, methods);
    }
    routes
});

/// The inputs of the route that matched. `HEAD` is `GET` without the body
/// of the answer.
fn inputs_of(path: Option<&str>, method: &Method) -> RouteInputs {
    let method = if method == Method::HEAD {
        &Method::GET
    } else {
        method
    };
    path.and_then(|path| ROUTES.get(path))
        .and_then(|methods| methods.get(method))
        .copied()
        .unwrap_or_default()
}

/// The middleware of the rule. See the module docs.
pub async fn refuse_unknown_input(req: Request, next: Next) -> Response {
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str);
    let inputs = inputs_of(path, req.method());

    if !inputs.query
        && let Some(refusal) = refuse_parameters_not_in(req.uri(), &[])
    {
        return refusal.into_response();
    }
    if inputs.body {
        return next.run(req).await;
    }
    // A limit of 0 bytes: the read fails when the body has 1 byte or more.
    let (parts, body) = req.into_parts();
    match axum::body::to_bytes(body, 0).await {
        Ok(_) => next.run(Request::from_parts(parts, Body::empty())).await,
        Err(_) => body_not_accepted().into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Page {
        #[serde(default)]
        limit: Option<i64>,
        #[serde(default)]
        offset: Option<i64>,
    }

    fn uri_of(text: &str) -> Uri {
        text.parse().expect("a URI")
    }

    fn refusal_of<T: DeserializeOwned + std::fmt::Debug>(uri: &str) -> ApiError {
        let rejection = Query::<T>::try_from_uri(&uri_of(uri)).expect_err("the query is refused");
        query_rejection(&rejection)
    }

    /// COLLIERY-T-0256: the refusal names the parameter, and it gives the
    /// parameters that the route has.
    #[test]
    fn an_unknown_parameter_is_named() {
        let err = refusal_of::<Page>("/api/boards?limit=5&page=2");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The request has the query parameter \"page\". This route does not accept that \
             parameter. The query parameters of this route are: limit, offset."
        );
        assert_eq!(
            err.details,
            json!({"parameter": "page", "allowed": ["limit", "offset"]})
        );
    }

    /// A value that the type cannot read names the parameter too.
    #[test]
    fn a_wrong_value_names_the_parameter() {
        let err = refusal_of::<Page>("/api/boards?limit=many");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(
            err.message,
            "The value of the query parameter \"limit\" is not correct: invalid digit found \
             in string."
        );
        assert_eq!(err.details, json!({"parameter": "limit"}));
    }

    /// A route with no query parameters refuses each parameter.
    #[test]
    fn a_route_with_no_parameters() {
        assert!(refuse_parameters_not_in(&uri_of("/api/whoami"), &[]).is_none());
        assert!(refuse_parameters_not_in(&uri_of("/api/whoami?"), &[]).is_none());
        let err =
            refuse_parameters_not_in(&uri_of("/api/whoami?force=true"), &[]).expect("a refusal");
        assert_eq!(
            err.message,
            "The request has the query parameter \"force\". This route does not accept that \
             parameter. This route has no query parameters."
        );
        assert_eq!(err.details, json!({"parameter": "force", "allowed": []}));
    }

    /// The list of `/ws/events`: the one parameter passes, a second does
    /// not.
    #[test]
    fn a_list_without_a_type() {
        let allowed = ["access_token"];
        let uri = uri_of("/ws/events?access_token=a.b.c");
        assert!(refuse_parameters_not_in(&uri, &allowed).is_none());
        let uri = uri_of("/ws/events?access_token=a.b.c&board=x");
        let err = refuse_parameters_not_in(&uri, &allowed).expect("a refusal");
        assert_eq!(
            err.details,
            json!({"parameter": "board", "allowed": ["access_token"]})
        );
    }

    /// The table has the routes of the document, with their inputs.
    #[test]
    fn the_table_has_the_inputs_of_a_route() {
        assert_eq!(
            inputs_of(Some("/api/tasks"), &Method::GET),
            RouteInputs {
                query: true,
                body: false
            }
        );
        assert_eq!(
            inputs_of(Some("/api/tasks"), &Method::HEAD),
            inputs_of(Some("/api/tasks"), &Method::GET)
        );
        assert_eq!(
            inputs_of(Some("/api/tasks"), &Method::POST),
            RouteInputs {
                query: false,
                body: true
            }
        );
        assert_eq!(
            inputs_of(
                Some("/api/{entity_type}/{short_code}/restore"),
                &Method::POST
            ),
            RouteInputs::default()
        );
        // Not in the document: no inputs.
        assert_eq!(
            inputs_of(Some("/api/docs"), &Method::GET),
            RouteInputs::default()
        );
        assert_eq!(inputs_of(None, &Method::GET), RouteInputs::default());
    }
}
