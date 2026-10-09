//! `POST /api/code-index/query-vector` (KAIROS-T-0360): the vector of the
//! text of a code search, made by the model that made the vectors of the
//! index.
//!
//! The local code tools (`kairos index mcp`) compare the vector of a query
//! with the vectors of the summaries. The two vectors must come from one
//! model. A CLI with no local model of the index (a build with no
//! `vectors`, or an index of a remote model, whose key is sealed on the
//! server) asks this route.
//!
//! - The route is for the tenant, not for a repository: the vector
//!   providers are settings of the organization (KAIROS-T-0339), and each
//!   repository of the tenant has the same model. So the CLI needs no
//!   repository and sends one request.
//! - The model is `<provider>/<model>/<dimension>`. The server embeds the
//!   text with its embedded vector model when the model is that one, else
//!   with the remote provider of the organization when the model is
//!   `remote/<vectors.model>/<dimension>`. Any other model is refused with
//!   `NO_QUERY_PROVIDER`, which names the model.
//! - Each member of the tenant can call it: it embeds a short text only.
//!   The text has 1 to [`MAX_TEXT_CHARS`] characters. The server has no
//!   general rate limit ([`crate::rate_limit`] is for failed logins only),
//!   so this route has none.
//! - The text is not logged above debug level, and then only its length.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use kairos_client::types_code_index as dto;
use kairos_embed::{EmbeddingProvider, ModelId};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

/// The longest text of a query, in characters.
pub const MAX_TEXT_CHARS: usize = 2000;

/// How long the remote endpoint can take for the vector of one query.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn router() -> Router<AppState> {
    Router::new().route("/api/code-index/query-vector", post(query_vector))
}

/// `<provider>/<model>/<dimension>` as its parts. The model name can have a
/// `/` (`nomic-ai/nomic-embed-text`).
pub(crate) fn parse_model(model: &str) -> Option<ModelId> {
    let (provider, rest) = model.split_once('/')?;
    let (name, dimension) = rest.rsplit_once('/')?;
    let dimension: usize = dimension.parse().ok()?;
    (!provider.is_empty() && !name.is_empty() && dimension > 0)
        .then(|| ModelId::new(provider, name, dimension))
}

fn model_name(id: &ModelId) -> String {
    format!("{}/{}/{}", id.provider, id.model, id.dimension)
}

/// The refusal of a model that the tenant has no provider for.
fn no_provider(model: &str) -> ApiError {
    ApiError::unprocessable(
        "NO_QUERY_PROVIDER",
        format!(
            "Kairos has no provider for the vectors of the model {model} in this organization. \
             The vector provider of the organization or of the server changed after the index \
             was made: build the index again."
        ),
    )
    .with_details(json!({ "model": model }))
}

/// The refusal of a provider that did not give the vector.
fn failed(model: &str, why: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "QUERY_VECTOR_FAILED",
        format!("The provider of the model {model} did not give the vector of the query ({why})."),
    )
    .with_details(json!({ "model": model }))
}

/// The vector of the text of a code search, made by the model of the
/// vectors of the index. Each member of the organization can call it.
#[utoipa::path(
    post,
    path = "/api/code-index/query-vector",
    tag = "repositories",
    request_body = dto::QueryVectorRequest,
    responses(
        (status = 200, description = "The vector of the text", body = dto::QueryVector),
        (status = 422, description = "The organization has no provider for the model (NO_QUERY_PROVIDER), or the model or the text is not correct (VALIDATION)", body = kairos_client::types::ErrorEnvelope),
        (status = 502, description = "The provider did not give the vector (QUERY_VECTOR_FAILED)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn query_vector(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    crate::body::ApiJson(body): crate::body::ApiJson<dto::QueryVectorRequest>,
) -> Result<Json<dto::QueryVector>, ApiError> {
    let chars = body.text.chars().count();
    if body.text.trim().is_empty() || chars > MAX_TEXT_CHARS {
        return Err(ApiError::validation(format!(
            "The text of the query has {chars} characters. Send 1 to {MAX_TEXT_CHARS} \
             characters."
        ))
        .with_details(json!({ "field": "text", "max": MAX_TEXT_CHARS })));
    }
    let Some(wanted) = parse_model(&body.model) else {
        return Err(ApiError::validation(format!(
            "The model {:?} is not <provider>/<model>/<dimension>, for example \
             local/bge-small-en-v1.5-q/384.",
            body.model
        ))
        .with_details(json!({ "field": "model" })));
    };
    let model = body.model.clone();
    tracing::debug!(%model, chars, "the vector of a code search query");

    // The embedded model of the server.
    let embedded = state
        .embedding
        .as_ref()
        .map(|service| service.provider())
        .filter(|provider| model_name(provider.model_id()) == model);
    // A remote provider is made off the async threads too: `reqwest::blocking`
    // must not start or stop in an async context.
    let make: Box<dyn FnOnce() -> Result<Arc<dyn EmbeddingProvider>, String> + Send> =
        match embedded {
            Some(provider) => Box::new(move || Ok(provider)),
            // The remote provider of the organization.
            None if wanted.provider == "remote" => {
                let settings = state
                    .blocking
                    .run(&tenant.slug, |conn| {
                        kairos_db::code_index_settings::load_or_default(conn)
                            .map_err(ApiError::internal)
                    })
                    .await?;
                let (Some(base_url), Some(name)) = (
                    settings.vector_base_url.clone(),
                    settings.vector_model.clone(),
                ) else {
                    return Err(no_provider(&model));
                };
                if settings.vector_provider != "remote" || name != wanted.model {
                    return Err(no_provider(&model));
                }
                let api_key = crate::code_index::open_secret(
                    state.config.secrets_key.as_ref(),
                    &tenant.slug,
                    "vectors.secret",
                    settings.vector_secret(),
                )
                .map_err(|why| failed(&model, why))?;
                let mut config = kairos_embed::remote::RemoteConfig::new(base_url, name);
                config.api_key = api_key;
                config.timeout = REMOTE_TIMEOUT;
                let dimension = wanted.dimension;
                Box::new(move || {
                    kairos_embed::remote::RemoteProvider::with_dimension(config, dimension)
                        .map(|p| Arc::new(p) as Arc<dyn EmbeddingProvider>)
                        .map_err(|e| e.to_string())
                })
            }
            None => return Err(no_provider(&model)),
        };

    // A provider is blocking (ONNX, or `reqwest::blocking`): off the async
    // threads.
    let text = body.text;
    let vector =
        tokio::task::spawn_blocking(move || make()?.embed_one(&text).map_err(|e| e.to_string()))
            .await
            .map_err(ApiError::internal)?
            .map_err(|why| failed(&model, why))?;
    if vector.len() != wanted.dimension {
        return Err(failed(
            &model,
            format!(
                "the vector has {} numbers, not {}",
                vector.len(),
                wanted.dimension
            ),
        ));
    }
    Ok(Json(dto::QueryVector { model, vector }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_provider_name_and_dimension() {
        let id = parse_model("local/bge-small-en-v1.5-q/384").expect("a model");
        assert_eq!(
            (id.provider.as_str(), id.model.as_str(), id.dimension),
            ("local", "bge-small-en-v1.5-q", 384)
        );
        let id = parse_model("remote/nomic-ai/nomic-embed-text/768").expect("a model");
        assert_eq!(id.model, "nomic-ai/nomic-embed-text");
        assert_eq!(model_name(&id), "remote/nomic-ai/nomic-embed-text/768");
        for bad in ["", "local", "local/384", "local/m/x", "local/m/0", "/m/3"] {
            assert!(parse_model(bad).is_none(), "{bad}");
        }
    }
}
