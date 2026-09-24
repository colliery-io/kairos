//! The OpenAI-compatible provider (KAIROS-A-0021 rule 1, KAIROS-T-0189): the
//! bring-your-own option.
//!
//! "OpenAI-compatible" rather than "OpenAI" on purpose. The request and response
//! shape is `POST {base}/embeddings` with `{"model": …, "input": [...]}` and
//! `{"data": [{"index": n, "embedding": [...]}]}`, which OpenAI defined and a
//! local Ollama, a vLLM server, LM Studio, Azure OpenAI and several hosted
//! services all speak. So a deployment that wants a bigger model, a GPU, or a
//! provider it already pays for points this at it and nothing else changes.
//!
//! # Blocking, and why that is not an accident
//!
//! [`EmbeddingProvider`] is synchronous because the local provider is CPU-bound
//! ONNX inference, which wants a thread rather than a task. This provider
//! therefore uses `reqwest::blocking`, which means **callers must not invoke it
//! from inside an async runtime's worker** — `reqwest::blocking` panics there.
//! Embedding is off the request path by design (KAIROS-T-0190 queues it), so the
//! call site is a blocking worker anyway; the constraint is documented on
//! [`EmbeddingProvider`] and repeated here because the failure mode is a panic
//! rather than an error.
//!
//! # Order is restored, not assumed
//!
//! The response carries an `index` per embedding, and the spec does not promise
//! the array arrives sorted. Getting this wrong would attach every vector to the
//! wrong item — retrieval that is confidently incorrect rather than broken — so
//! the vectors are placed by their `index` rather than taken in arrival order.

use std::time::Duration;

use serde::Deserialize;

use crate::{EmbedError, Embedding, EmbeddingProvider, ModelId, validate_batch};

/// How to reach an OpenAI-compatible embeddings endpoint.
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// Base URL, without the `/embeddings` suffix — e.g.
    /// `https://api.openai.com/v1`, or `http://ollama:11434/v1`.
    pub base_url: String,
    /// The model name to request, passed through verbatim.
    pub model: String,
    /// Bearer token, if the endpoint wants one. A local Ollama does not.
    pub api_key: Option<String>,
    /// Per-request timeout. A slow embedding endpoint must not become a stuck
    /// backfill.
    pub timeout: Duration,
}

impl RemoteConfig {
    /// A configuration with the default timeout.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Embeddings from an OpenAI-compatible HTTP endpoint.
pub struct RemoteProvider {
    client: reqwest::blocking::Client,
    config: RemoteConfig,
    id: ModelId,
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    /// Which input this vector belongs to. Load-bearing: see the module docs.
    #[serde(default)]
    index: usize,
    embedding: Vec<f32>,
}

impl RemoteProvider {
    /// Connect, and learn the vector width by embedding one probe text.
    ///
    /// The width is discovered rather than configured because asking an operator
    /// to type it invites a wrong answer that would not surface until vectors
    /// were already stored at the wrong width. One request at startup is cheap
    /// and the endpoint has to be reachable anyway.
    pub fn new(config: RemoteConfig) -> Result<Self, EmbedError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| EmbedError::Unavailable(format!("could not build HTTP client: {e}")))?;

        // Width unknown until the probe answers; 0 would fail validate_batch, so
        // the probe deliberately bypasses it by calling `request` directly.
        let mut provider = Self {
            client,
            id: ModelId::new("remote", config.model.clone(), 0),
            config,
        };
        let probe = provider
            .request(&["dimension probe".to_string()])
            .map_err(|e| {
                EmbedError::Unavailable(format!(
                    "could not reach the embeddings endpoint at {}: {e}",
                    provider.config.base_url
                ))
            })?;
        let dimension = probe.first().map(|v| v.len()).unwrap_or(0);
        if dimension == 0 {
            return Err(EmbedError::Unavailable(format!(
                "the embeddings endpoint at {} returned no vector for a probe request; \
                 check the model name {:?}",
                provider.config.base_url, provider.config.model
            )));
        }
        provider.id.dimension = dimension;
        tracing::info!(
            base_url = %provider.config.base_url,
            model = %provider.config.model,
            dimension,
            "remote embedding provider ready"
        );
        Ok(provider)
    }

    /// One HTTP round trip, vectors restored to input order.
    fn request(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "model": self.config.model, "input": texts }));
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let response = req
            .send()
            .map_err(|e| EmbedError::Failed(format!("{url}: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            // The body usually says which of model name, key or quota is wrong,
            // and truncating it at 500 keeps a huge HTML error page out of the
            // logs while preserving the sentence that matters.
            let body = response.text().unwrap_or_default();
            let body: String = body.chars().take(500).collect();
            return Err(EmbedError::Failed(format!(
                "{url} returned {status}: {body}"
            )));
        }

        let parsed: EmbeddingsResponse = response
            .json()
            .map_err(|e| EmbedError::Failed(format!("{url}: malformed response: {e}")))?;

        // Place by `index`, do not trust arrival order.
        let mut out: Vec<Option<Embedding>> = vec![None; texts.len()];
        for datum in parsed.data {
            if datum.index >= out.len() {
                return Err(EmbedError::Failed(format!(
                    "{url}: response referenced input {} but only {} were sent",
                    datum.index,
                    texts.len()
                )));
            }
            out[datum.index] = Some(datum.embedding);
        }
        if let Some(missing) = out.iter().position(|v| v.is_none()) {
            return Err(EmbedError::Failed(format!(
                "{url}: response had no embedding for input {missing}"
            )));
        }
        Ok(out.into_iter().map(|v| v.unwrap()).collect())
    }
}

impl EmbeddingProvider for RemoteProvider {
    fn model_id(&self) -> &ModelId {
        &self.id
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let vectors = self.request(texts)?;
        validate_batch(&self.id, texts, vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_is_built_without_a_double_slash() {
        for base in ["http://x/v1", "http://x/v1/"] {
            let config = RemoteConfig::new(base, "m");
            let url = format!("{}/embeddings", config.base_url.trim_end_matches('/'));
            assert_eq!(url, "http://x/v1/embeddings");
        }
    }

    #[test]
    fn an_unreachable_endpoint_names_itself() {
        // Port 1 on localhost refuses immediately, so this needs no network and
        // runs in the unit tier.
        let err = match RemoteProvider::new(RemoteConfig {
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "some-model".into(),
            api_key: None,
            timeout: Duration::from_millis(500),
        }) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("nothing is listening on port 1"),
        };
        assert!(err.contains("http://127.0.0.1:1/v1"), "{err}");
        assert!(err.contains("could not reach"), "{err}");
    }

    /// Out-of-order responses are the failure that would silently attach every
    /// vector to the wrong item, so the placement logic is tested directly.
    #[test]
    fn vectors_are_placed_by_index_not_arrival_order() {
        let body = serde_json::json!({
            "data": [
                {"index": 2, "embedding": [3.0]},
                {"index": 0, "embedding": [1.0]},
                {"index": 1, "embedding": [2.0]},
            ]
        });
        let parsed: EmbeddingsResponse = serde_json::from_value(body).unwrap();
        let mut out: Vec<Option<Embedding>> = vec![None; 3];
        for d in parsed.data {
            out[d.index] = Some(d.embedding);
        }
        let placed: Vec<Embedding> = out.into_iter().map(|v| v.unwrap()).collect();
        assert_eq!(placed, vec![vec![1.0], vec![2.0], vec![3.0]]);
    }

    #[test]
    fn a_response_missing_an_input_is_detected() {
        let body = serde_json::json!({"data": [{"index": 0, "embedding": [1.0]}]});
        let parsed: EmbeddingsResponse = serde_json::from_value(body).unwrap();
        let mut out: Vec<Option<Embedding>> = vec![None; 2];
        for d in parsed.data {
            out[d.index] = Some(d.embedding);
        }
        assert_eq!(out.iter().position(|v| v.is_none()), Some(1));
    }
}
