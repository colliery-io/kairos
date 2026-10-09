//! A fake OpenAI-compatible embeddings endpoint (KAIROS-T-0360): `POST
//! /v1/embeddings` on an ephemeral port. It checks the bearer token,
//! counts the requests, and gives one vector of `dimension` numbers for
//! each input: each number is the length of the input in characters.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};

#[derive(Clone)]
struct Shared {
    key: String,
    dimension: usize,
    requests: Arc<AtomicUsize>,
}

/// The fake endpoint, and what it saw.
pub struct EmbedHttp {
    /// The base URL of the endpoint: `http://127.0.0.1:<port>/v1`.
    pub base_url: String,
    requests: Arc<AtomicUsize>,
}

impl EmbedHttp {
    /// The requests that reached the endpoint, the refused ones included.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

/// Serve the fake endpoint. `key` is the bearer token it takes.
pub async fn serve(key: &str, dimension: usize) -> EmbedHttp {
    let shared = Shared {
        key: key.to_string(),
        dimension,
        requests: Arc::new(AtomicUsize::new(0)),
    };
    let requests = shared.requests.clone();
    let router = Router::new()
        .route("/v1/embeddings", post(embed))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("embeddings server");
    });
    EmbedHttp {
        base_url: format!("http://{addr}/v1"),
        requests,
    }
}

async fn embed(State(shared): State<Shared>, headers: HeaderMap, body: String) -> Response {
    shared.requests.fetch_add(1, Ordering::SeqCst);
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if bearer != format!("Bearer {}", shared.key) {
        return (
            StatusCode::UNAUTHORIZED,
            json!({"error": {"message": "invalid api key"}}).to_string(),
        )
            .into_response();
    }
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let data: Vec<Value> = parsed["input"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let length = text.as_str().unwrap_or("").chars().count() as f32;
            json!({"object": "embedding", "index": index, "embedding": vec![length; shared.dimension]})
        })
        .collect();
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        json!({"object": "list", "model": parsed["model"], "data": data}).to_string(),
    )
        .into_response()
}
