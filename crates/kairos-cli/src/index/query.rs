//! The vector of the query of `code_search` (KAIROS-T-0351, KAIROS-T-0360).
//!
//! The search compares the vector of the query with the vectors of the
//! summaries, so the two must come from one model: the model of the pool,
//! `<provider>/<model>/<dimension>`. The CLI takes the first source that
//! it has, in this order:
//!
//! 1. A model of this binary: the deterministic test model, or the local
//!    model (feature `vectors`) when its model is the model of the index.
//! 2. Kairos: `POST /api/code-index/query-vector`, with the connection of
//!    `kairos index update` for the checkout. Kairos embeds the query with
//!    its embedded model, or with the remote provider of the organization,
//!    whose key the CLI never has. One request for each search.
//! 3. No vector: the search uses the text of the summaries, and its first
//!    line says why.
//!
//! The MCP server keeps the source for each checkout and model, so a search
//! does not load a model or look for a connection again. A refusal of
//! Kairos that does not change (no provider for the model, or a server
//! with no route) is kept as the reason. A timeout or a failed connection
//! is not kept: the next search asks again.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use kairos_client::{Error as ApiError, KairosClient};
use kairos_embed::{EmbedError, Embedding, EmbeddingProvider, ModelId};

/// How long Kairos can take for the vector of one query.
pub(crate) const ROUTE_TIMEOUT: Duration = Duration::from_secs(5);

/// The answer of the route: the vector, or why there is none.
pub(crate) type RouteAnswer = Pin<Box<dyn Future<Output = Result<Vec<f32>, RouteFailure>> + Send>>;

/// The route of Kairos: `(model, text)` to the vector of the text.
pub(crate) type QueryRoute = Arc<dyn Fn(String, String) -> RouteAnswer + Send + Sync>;

/// Why the route gave no vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RouteFailure {
    /// An answer that does not change: Kairos has no provider for the
    /// model, or it has no such route. The text is the reason.
    Lasting(String),
    /// A failure of this request only: the next search asks again.
    Once(String),
}

/// Where the vector of a query comes from, for the model of an index.
#[derive(Clone)]
pub(crate) enum QuerySource {
    /// A model of this binary.
    Local(Arc<dyn EmbeddingProvider>),
    /// The route of Kairos, for each query.
    Kairos(QueryRoute),
    /// No vector. The text says why.
    Text(String),
}

/// The source of the query vectors for the model of the index of the
/// checkout at `root` (no checkout: no connection).
pub(crate) fn query_source(model: &str, root: Option<&Path>) -> QuerySource {
    query_source_with(model, local_model, || {
        root.and_then(super::base::connection).map(kairos_route)
    })
}

/// The local vector model of this binary, or `Err("")` when the binary has
/// none.
fn local_model() -> Result<Box<dyn EmbeddingProvider>, String> {
    #[cfg(feature = "vectors")]
    {
        super::summaries::local_embedder().map(|p| Box::new(p) as Box<dyn EmbeddingProvider>)
    }
    #[cfg(not(feature = "vectors"))]
    {
        Err(String::new())
    }
}

/// [`query_source`], with the loader of the local model and the route
/// given, so that a test can choose them. The route is made only when no
/// model of this binary is the model of the index.
pub(crate) fn query_source_with(
    model: &str,
    local: impl FnOnce() -> Result<Box<dyn EmbeddingProvider>, String>,
    route: impl FnOnce() -> Option<QueryRoute>,
) -> QuerySource {
    let mut parts = model.splitn(3, '/');
    let (provider, name, dimension) = (parts.next(), parts.next(), parts.next());
    if let (Some("deterministic"), Some(dimension)) = (provider, dimension)
        && let Ok(dimension) = dimension.parse::<usize>()
    {
        let deterministic = kairos_embed::DeterministicProvider::new(dimension);
        if Some(deterministic.model_id().model.as_str()) == name {
            return QuerySource::Local(Arc::new(deterministic));
        }
    }
    let mut why_local = None;
    if provider == Some("local") {
        match local() {
            Ok(local) if model_name(local.model_id()) == model => {
                return QuerySource::Local(Arc::from(local));
            }
            Ok(_) => {}
            Err(why) if !why.is_empty() => why_local = Some(why),
            Err(_) => {}
        }
    }
    if let Some(route) = route() {
        return QuerySource::Kairos(route);
    }
    QuerySource::Text(why_local.unwrap_or_else(|| {
        format!("This kairos binary has no provider for the vectors of the model {model}.")
    }))
}

/// The provider of the vector of `query`: the model of the source, or the
/// vector that Kairos gives in `timeout`. The error says why there is none.
pub(crate) async fn query_provider(
    source: &QuerySource,
    model: &str,
    query: &str,
    timeout: Duration,
) -> Result<Arc<dyn EmbeddingProvider>, RouteFailure> {
    let route = match source {
        QuerySource::Local(provider) => return Ok(Arc::clone(provider)),
        QuerySource::Text(why) => return Err(RouteFailure::Lasting(why.clone())),
        QuerySource::Kairos(route) => route,
    };
    let Some(id) = model_id(model) else {
        return Err(RouteFailure::Lasting(format!(
            "The model {model} of the index is not <provider>/<model>/<dimension>."
        )));
    };
    let vector =
        match tokio::time::timeout(timeout, route(model.to_string(), query.to_string())).await {
            Ok(Ok(vector)) => vector,
            Ok(Err(failure)) => return Err(failure),
            Err(_) => {
                return Err(RouteFailure::Once(format!(
                    "Kairos gave no vector of the query in {} s.",
                    timeout.as_secs()
                )));
            }
        };
    if vector.len() != id.dimension {
        return Err(RouteFailure::Once(format!(
            "Kairos gave a vector of {} numbers for the model {model}, not {}.",
            vector.len(),
            id.dimension
        )));
    }
    Ok(Arc::new(GivenVector { id, vector }))
}

/// `<provider>/<model>/<dimension>`. The model name can have a `/`.
fn model_id(model: &str) -> Option<ModelId> {
    let (provider, rest) = model.split_once('/')?;
    let (name, dimension) = rest.rsplit_once('/')?;
    let dimension: usize = dimension.parse().ok()?;
    (dimension > 0).then(|| ModelId::new(provider, name, dimension))
}

fn model_name(id: &ModelId) -> String {
    format!("{}/{}/{}", id.provider, id.model, id.dimension)
}

/// The vector of one query, as Kairos gave it: the provider of the search
/// for that query.
struct GivenVector {
    id: ModelId,
    vector: Embedding,
}

impl EmbeddingProvider for GivenVector {
    fn model_id(&self) -> &ModelId {
        &self.id
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        Ok(texts.iter().map(|_| self.vector.clone()).collect())
    }
}

/// The route of the deployment of `client`.
fn kairos_route(client: KairosClient) -> QueryRoute {
    Arc::new(move |model: String, text: String| -> RouteAnswer {
        let client = client.clone();
        Box::pin(async move {
            client
                .query_vector(&model, &text)
                .await
                .map(|answer| answer.vector)
                .map_err(failure_of)
        })
    })
}

/// The failure of a request to the route.
fn failure_of(e: ApiError) -> RouteFailure {
    match e {
        ApiError::Transport(e) => {
            RouteFailure::Once(format!("The CLI cannot connect to Kairos: {e}."))
        }
        ApiError::Other { code, message, .. } if code == "NO_QUERY_PROVIDER" => {
            RouteFailure::Lasting(message)
        }
        ApiError::NotFound { .. } => RouteFailure::Lasting(
            "This Kairos server cannot make the vector of a query. Kairos 0.9.0 or later can."
                .to_string(),
        ),
        e => RouteFailure::Once(format!("Kairos gave no vector of the query: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const LOCAL: &str = "local/bge-small-en-v1.5-q/384";

    /// The loader of the local model of a case.
    type Loader = fn() -> Result<Box<dyn EmbeddingProvider>, String>;

    /// A model with any id, as the local model of a test.
    struct Fake(ModelId);

    impl EmbeddingProvider for Fake {
        fn model_id(&self) -> &ModelId {
            &self.0
        }

        fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
            Ok(texts.iter().map(|_| vec![1.0; self.0.dimension]).collect())
        }
    }

    fn fake(model: &str) -> Result<Box<dyn EmbeddingProvider>, String> {
        Ok(Box::new(Fake(model_id(model).expect("a model"))))
    }

    /// A route that gives `answer` and counts its calls.
    fn route(answer: Result<Vec<f32>, RouteFailure>, calls: Arc<AtomicUsize>) -> QueryRoute {
        Arc::new(move |_model, _text| -> RouteAnswer {
            calls.fetch_add(1, Ordering::SeqCst);
            let answer = answer.clone();
            Box::pin(async move { answer })
        })
    }

    fn no_route() -> Option<QueryRoute> {
        None
    }

    #[test]
    fn the_deterministic_vectors_have_a_query_provider() {
        let source = query_source_with("deterministic/sha256-384/384", fake_none, || {
            panic!("no route for a model of this binary")
        });
        let QuerySource::Local(provider) = source else {
            panic!("the deterministic model")
        };
        assert_eq!(provider.model_id().dimension, 384);
        let QuerySource::Text(why) = query_source_with("other/model/3", fake_none, no_route) else {
            panic!("no provider")
        };
        assert_eq!(
            why,
            "This kairos binary has no provider for the vectors of the model other/model/3."
        );
    }

    fn fake_none() -> Result<Box<dyn EmbeddingProvider>, String> {
        Err(String::new())
    }

    /// KAIROS-T-0360 (a): the local model of the index comes first; the
    /// route is not made.
    #[test]
    fn the_local_model_of_the_index_comes_first() {
        let source = query_source_with(LOCAL, || fake(LOCAL), || panic!("no route"));
        let QuerySource::Local(provider) = source else {
            panic!("the local model")
        };
        assert_eq!(model_name(provider.model_id()), LOCAL);
    }

    /// KAIROS-T-0351, with the real model: it loads the 65 MB model from
    /// `target/embed-cache` (the cache of kairos-embed/tests/local_model.rs),
    /// and downloads it when the cache is empty, so it is not in the unit
    /// tier. Run it with `cargo test -p kairos-cli -- --ignored`.
    #[cfg(feature = "vectors")]
    #[test]
    #[ignore = "loads the 65 MB vector model"]
    fn the_local_model_embeds_a_query() {
        let cache =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/embed-cache");
        let source = query_source_with(
            LOCAL,
            || {
                kairos_embed::local::LocalProvider::new(&kairos_embed::local::LocalConfig {
                    cache_dir: cache,
                    allow_download: true,
                })
                .map(|p| Box::new(p) as Box<dyn EmbeddingProvider>)
                .map_err(|e| e.to_string())
            },
            || panic!("no route"),
        );
        let QuerySource::Local(provider) = source else {
            panic!("the local model")
        };
        let vectors = provider
            .embed(&["the token of the second writer".to_string()])
            .expect("a vector");
        assert_eq!(vectors[0].len(), 384);
    }

    /// KAIROS-T-0360 (b): a local model of another model, no local model,
    /// and a remote model of the index go to Kairos when the checkout has a
    /// connection.
    #[test]
    fn a_miss_of_the_local_model_goes_to_kairos() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cases: [(&str, Loader); 3] = [
            (LOCAL, || fake("local/other-model/384")),
            (LOCAL, fake_none),
            ("remote/nomic-ai/nomic-embed-text/768", || fake(LOCAL)),
        ];
        for (model, local) in cases {
            let source = query_source_with(model, local, || {
                Some(route(Ok(vec![0.5; 3]), Arc::clone(&calls)))
            });
            assert!(matches!(source, QuerySource::Kairos(_)), "{model}");
        }
        // The choice sends no request.
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// KAIROS-T-0360 (c): no model and no connection: the text only, with
    /// the reason. A local model that did not start gives its own reason.
    #[test]
    fn no_model_and_no_connection_is_text_with_the_reason() {
        let QuerySource::Text(why) = query_source_with(LOCAL, fake_none, no_route) else {
            panic!("text only")
        };
        assert_eq!(
            why,
            format!("This kairos binary has no provider for the vectors of the model {LOCAL}.")
        );
        let QuerySource::Text(why) = query_source_with(
            LOCAL,
            || Err("The vector model did not start: no files.".into()),
            no_route,
        ) else {
            panic!("text only")
        };
        assert_eq!(why, "The vector model did not start: no files.");
    }

    /// The vector of Kairos is the provider of the search: its model is the
    /// model of the index, and it gives the vector of the query.
    #[tokio::test]
    async fn the_vector_of_kairos_is_the_query_vector() {
        let model = "remote/nomic-ai/nomic-embed-text/3";
        let calls = Arc::new(AtomicUsize::new(0));
        let source = QuerySource::Kairos(route(Ok(vec![0.1, 0.2, 0.3]), Arc::clone(&calls)));
        let provider = query_provider(&source, model, "the token", ROUTE_TIMEOUT)
            .await
            .unwrap_or_else(|e| panic!("a provider: {e:?}"));
        assert_eq!(model_name(provider.model_id()), model);
        assert_eq!(
            provider.embed_one("the token").expect("a vector"),
            vec![0.1, 0.2, 0.3]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // A vector of the wrong width is not used.
        let source = QuerySource::Kairos(route(Ok(vec![0.1; 4]), calls));
        let why = query_provider(&source, model, "q", ROUTE_TIMEOUT)
            .await
            .err()
            .expect("a refusal");
        assert!(
            matches!(why, RouteFailure::Once(ref w) if w.contains("4 numbers")),
            "{why:?}"
        );
    }

    /// A refusal of Kairos is the reason; a route that does not answer in
    /// time gives the text search and says so.
    #[tokio::test]
    async fn a_refusal_or_a_timeout_is_the_reason() {
        let model = LOCAL;
        let refused = QuerySource::Kairos(route(
            Err(RouteFailure::Lasting(format!(
                "Kairos has no provider for the vectors of the model {model}."
            ))),
            Arc::new(AtomicUsize::new(0)),
        ));
        let why = query_provider(&refused, model, "q", ROUTE_TIMEOUT)
            .await
            .err()
            .expect("a refusal");
        assert!(matches!(why, RouteFailure::Lasting(_)));

        let slow: QueryRoute = Arc::new(|_model, _text| -> RouteAnswer {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(vec![0.0; 384])
            })
        });
        let why = query_provider(
            &QuerySource::Kairos(slow),
            model,
            "q",
            Duration::from_millis(50),
        )
        .await
        .err()
        .expect("a timeout");
        assert!(
            matches!(why, RouteFailure::Once(ref w) if w.starts_with("Kairos gave no vector of the query in")),
            "{why:?}"
        );
    }

    #[test]
    fn the_answers_of_kairos_are_lasting_or_once() {
        let other = |status: u16, code: &str| ApiError::Other {
            status,
            code: code.to_string(),
            message: "The text.".to_string(),
            details: serde_json::Value::Null,
        };
        assert_eq!(
            failure_of(other(422, "NO_QUERY_PROVIDER")),
            RouteFailure::Lasting("The text.".to_string())
        );
        assert!(matches!(
            failure_of(other(502, "QUERY_VECTOR_FAILED")),
            RouteFailure::Once(_)
        ));
    }
}
