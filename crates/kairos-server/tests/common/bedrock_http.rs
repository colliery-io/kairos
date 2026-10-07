//! A fake Bedrock endpoint (KAIROS-T-0342): `POST /model/{id}/converse` on
//! an ephemeral port. It checks the shape of the Signature Version 4
//! `Authorization` header (the access key id, the signed headers, a
//! signature), answers a `ThrottlingException` for the first `throttle`
//! requests, records each `Authorization` header, and gives one Converse
//! answer for each prompt: "A Bedrock summary of `<name>`.".

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};

#[derive(Clone)]
struct Shared {
    access_key_id: String,
    throttle: Arc<AtomicUsize>,
    authorizations: Arc<Mutex<Vec<String>>>,
    model_ids: Arc<Mutex<Vec<String>>>,
}

/// The fake endpoint, and what it saw.
pub struct BedrockHttp {
    /// The base URL: `http://127.0.0.1:<port>`.
    pub base_url: String,
    authorizations: Arc<Mutex<Vec<String>>>,
    model_ids: Arc<Mutex<Vec<String>>>,
}

impl BedrockHttp {
    /// Each `Authorization` header the endpoint saw.
    pub fn authorizations(&self) -> Vec<String> {
        self.authorizations.lock().unwrap().clone()
    }

    /// The model id of each request, as the path carried it.
    pub fn model_ids(&self) -> Vec<String> {
        self.model_ids.lock().unwrap().clone()
    }
}

/// Serve the fake endpoint. Requests with another access key id in the
/// `Authorization` header get a 403; the first `throttle` requests get a
/// `ThrottlingException`.
pub async fn serve(access_key_id: &str, throttle: usize) -> BedrockHttp {
    let shared = Shared {
        access_key_id: access_key_id.to_string(),
        throttle: Arc::new(AtomicUsize::new(throttle)),
        authorizations: Arc::new(Mutex::new(Vec::new())),
        model_ids: Arc::new(Mutex::new(Vec::new())),
    };
    let authorizations = shared.authorizations.clone();
    let model_ids = shared.model_ids.clone();
    let router = Router::new()
        .route("/model/{model_id}/converse", post(converse))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("bedrock server");
    });
    BedrockHttp {
        base_url: format!("http://{addr}"),
        authorizations,
        model_ids,
    }
}

async fn converse(
    State(shared): State<Shared>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let authorization = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    shared
        .authorizations
        .lock()
        .unwrap()
        .push(authorization.clone());
    shared.model_ids.lock().unwrap().push(model_id);
    let well_formed = authorization.starts_with(&format!(
        "AWS4-HMAC-SHA256 Credential={}/",
        shared.access_key_id
    )) && authorization.contains("SignedHeaders=content-type;host;x-amz-date")
        && authorization.contains(", Signature=")
        && headers.contains_key("x-amz-date");
    if !well_formed {
        return (
            StatusCode::FORBIDDEN,
            json!({"message": "The security token included in the request is invalid."})
                .to_string(),
        )
            .into_response();
    }
    if shared
        .throttle
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        return (
            StatusCode::BAD_REQUEST,
            [("x-amzn-errortype", "ThrottlingException")],
            json!({"message": "Too many requests, please wait before trying again."}).to_string(),
        )
            .into_response();
    }
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let prompt = parsed["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    let name = prompt.split('`').nth(1).unwrap_or("something").to_string();
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        json!({
            "output": {"message": {"role": "assistant",
                "content": [{"text": format!("A Bedrock summary of `{name}`.")}]}},
            "stopReason": "end_turn",
            "usage": {"inputTokens": 10, "outputTokens": 8, "totalTokens": 18},
        })
        .to_string(),
    )
        .into_response()
}
