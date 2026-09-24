//! `kairos-embed` — turning text into vectors (KAIROS-A-0021 rule 1,
//! KAIROS-T-0189).
//!
//! Rule 1: **a local model by default**, an OpenAI-compatible endpoint as the
//! bring-your-own option. Default-local is a posture decision, not a performance
//! one — a work-management system whose search requires shipping every ticket
//! title to a third party is a system a lot of organisations cannot deploy at
//! all.
//!
//! # Why a separate crate
//!
//! `kairos-core` is pure by KAIROS-A-0009 — no I/O — and loading an ONNX model
//! or calling an HTTP endpoint is I/O. The providers could have gone in
//! `kairos-server`, which already does both, but then the CLI and the soak
//! driver would link an ML runtime to get at a trait. Here, with `local` and
//! `remote` behind features, they do not.
//!
//! # The three providers
//!
//! | provider | what it is for |
//! |---|---|
//! | [`local::LocalProvider`] | the default: ONNX in-process, CPU, no GPU |
//! | [`remote::RemoteProvider`] | OpenAI-compatible HTTP: OpenAI, Ollama, vLLM, anything speaking the shape |
//! | [`DeterministicProvider`] | tests: stable, free, instant, and always available |
//!
//! The deterministic one is not a convenience. Every test tier above unit needs
//! embeddings, and needs them to be the same embeddings twice; a real model in
//! CI is slow, and a remote one is a network dependency in a test suite.
//!
//! # Model identity is recorded, not assumed
//!
//! Changing provider or model changes the vector space, and vectors from two
//! spaces cannot be compared — the numbers still subtract, which is exactly what
//! makes it dangerous. So every provider reports a [`ModelId`], the store records
//! it per row (KAIROS-T-0187's `provider`/`model`/`dimension` columns), and
//! [`ModelId::mismatch`] says what disagrees. Detecting it is this crate's job;
//! re-embedding in response is KAIROS-T-0190's.

use std::fmt;

#[cfg(feature = "local")]
pub mod local;
#[cfg(feature = "remote")]
pub mod remote;

mod config;
mod deterministic;
pub use config::{ConfigError, EmbedConfig, ProviderKind};
pub use deterministic::DeterministicProvider;

/// One embedding: a dense vector in whichever space its [`ModelId`] names.
pub type Embedding = Vec<f32>;

/// Which model produced a vector, and how wide it is.
///
/// Recorded alongside every stored vector so that a configuration change is
/// **detectable**. Two vectors are comparable only if their `ModelId`s agree;
/// nothing enforces that at the type level, because the vectors are `Vec<f32>`
/// either way, so the check has to be made rather than relied upon.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelId {
    /// Provider family: `local`, `remote`, or `deterministic`.
    pub provider: String,
    /// The model within that provider, e.g. `bge-small-en-v1.5-q`.
    pub model: String,
    /// Vector width. Redundant with `model` in principle, recorded anyway
    /// because a width mismatch is the one failure that would otherwise panic
    /// deep inside a dot product rather than at the boundary.
    pub dimension: usize,
}

impl ModelId {
    /// A new identity.
    pub fn new(provider: impl Into<String>, model: impl Into<String>, dimension: usize) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            dimension,
        }
    }

    /// How `stored` differs from `self`, or `None` if they agree.
    ///
    /// Ordered by how much it matters: a different width cannot be compared at
    /// all, a different model is a different space, a different provider serving
    /// the same model at the same width probably *is* comparable — Ollama and
    /// OpenAI both serving the same weights — but it is still worth saying,
    /// because "probably" is not a thing to bet a duplicate-detection claim on.
    pub fn mismatch(&self, stored: &ModelId) -> Option<Mismatch> {
        if self.dimension != stored.dimension {
            return Some(Mismatch::Dimension {
                configured: self.dimension,
                stored: stored.dimension,
            });
        }
        if self.model != stored.model {
            return Some(Mismatch::Model {
                configured: self.model.clone(),
                stored: stored.model.clone(),
            });
        }
        if self.provider != stored.provider {
            return Some(Mismatch::Provider {
                configured: self.provider.clone(),
                stored: stored.provider.clone(),
            });
        }
        None
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} ({}d)", self.provider, self.model, self.dimension)
    }
}

/// A disagreement between the configured model and a stored vector's model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Mismatch {
    /// Widths differ: not comparable at all.
    #[error(
        "stored vectors are {stored}-dimensional but the configured model produces \
         {configured}: they cannot be compared. Re-embed, or point at the model \
         that wrote them."
    )]
    Dimension {
        /// What the configured model produces.
        configured: usize,
        /// What the stored rows say they are.
        stored: usize,
    },
    /// Same width, different model: a different vector space, and the numbers
    /// will still subtract, which is the trap.
    #[error(
        "stored vectors came from model {stored:?} but the configured model is \
         {configured:?}. Same width is not the same space; distances between them \
         are meaningless. Re-embed."
    )]
    Model {
        /// The configured model name.
        configured: String,
        /// The model name recorded on the stored rows.
        stored: String,
    },
    /// Same model and width, different provider. Probably comparable; said
    /// anyway.
    #[error(
        "stored vectors were produced through provider {stored:?} and the \
         configured provider is {configured:?}, for the same model and width. \
         Likely comparable, but unverified."
    )]
    Provider {
        /// The configured provider.
        configured: String,
        /// The provider recorded on the stored rows.
        stored: String,
    },
}

impl Mismatch {
    /// Whether this mismatch makes stored vectors unusable rather than merely
    /// suspicious. A caller that must decide between degrading to lexical and
    /// carrying on asks this.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Mismatch::Dimension { .. } | Mismatch::Model { .. })
    }
}

/// Why embedding failed.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The provider could not be started — a missing model, an unreadable cache
    /// directory, a bad configuration.
    #[error("embedding provider unavailable: {0}")]
    Unavailable(String),
    /// The provider was reachable but refused or failed the request.
    #[error("embedding failed: {0}")]
    Failed(String),
    /// The provider returned a different number of vectors than texts given, or
    /// a width that disagrees with what it advertised. Checked rather than
    /// trusted, because a silently short batch would misalign every vector with
    /// the wrong item.
    #[error("embedding provider returned {got} vectors for {want} texts")]
    CountMismatch {
        /// Vectors returned.
        got: usize,
        /// Texts submitted.
        want: usize,
    },
    /// A vector came back with a width the provider did not advertise.
    #[error("embedding provider advertised {advertised}d but returned a {got}d vector")]
    WidthMismatch {
        /// The width [`EmbeddingProvider::model_id`] promised.
        advertised: usize,
        /// The width actually returned.
        got: usize,
    },
}

/// Turns text into vectors.
///
/// Batched, because per-text calls are the difference between a backfill that
/// takes minutes and one that takes hours — measured at 42 texts/s batched
/// against a corpus of 4,927 (KAIROS-T-0189).
///
/// Deliberately synchronous. The local provider is CPU-bound ONNX inference,
/// which wants a thread rather than an async task, and making the trait async
/// would force it to pretend. Callers embed off the request path anyway
/// (KAIROS-T-0190 queues it), so there is no async boundary to preserve.
///
/// **Callers must not invoke this from an async runtime's worker thread.** The
/// remote provider uses `reqwest::blocking`, which panics there rather than
/// returning an error — so use `tokio::task::spawn_blocking` or a dedicated
/// thread. Stated here, on the trait, because a caller holding a
/// `dyn EmbeddingProvider` cannot see which implementation it has.
pub trait EmbeddingProvider: Send + Sync {
    /// Which model this provider speaks for.
    fn model_id(&self) -> &ModelId;

    /// Embed a batch. Returns one vector per text, **in the same order** —
    /// callers zip the result against their inputs, so order is part of the
    /// contract, not an implementation detail.
    fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError>;

    /// Embed one text. Convenience; prefer [`Self::embed`] wherever there is
    /// more than one.
    fn embed_one(&self, text: &str) -> Result<Embedding, EmbedError> {
        let mut out = self.embed(std::slice::from_ref(&text.to_string()))?;
        Ok(out.pop().unwrap_or_default())
    }
}

/// Check a provider's own output before it reaches a caller: one vector per
/// text, each the advertised width.
///
/// Every provider runs its result through this. A short batch would misalign
/// every subsequent vector with the wrong item — a failure that produces
/// confidently wrong retrieval rather than an error, so it is checked at the
/// boundary instead of being assumed of three separate implementations.
pub(crate) fn validate_batch(
    id: &ModelId,
    texts: &[String],
    vectors: Vec<Embedding>,
) -> Result<Vec<Embedding>, EmbedError> {
    if vectors.len() != texts.len() {
        return Err(EmbedError::CountMismatch {
            got: vectors.len(),
            want: texts.len(),
        });
    }
    if let Some(bad) = vectors.iter().find(|v| v.len() != id.dimension) {
        return Err(EmbedError::WidthMismatch {
            advertised: id.dimension,
            got: bad.len(),
        });
    }
    Ok(vectors)
}

/// Cosine similarity between two vectors in the same space.
///
/// Returns `None` when the widths differ, rather than panicking or silently
/// comparing a prefix. A zero-magnitude vector also yields `None`: its direction
/// is undefined, and returning 0.0 would read as "unrelated" when the truth is
/// "no answer".
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return None;
    }
    Some(dot / (na * nb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(provider: &str, model: &str, dim: usize) -> ModelId {
        ModelId::new(provider, model, dim)
    }

    #[test]
    fn matching_identities_do_not_mismatch() {
        let a = id("local", "bge-small-en-v1.5-q", 384);
        assert_eq!(a.mismatch(&a.clone()), None);
    }

    /// Width first, because it is the one that cannot be compared at all.
    #[test]
    fn a_width_difference_outranks_a_model_difference() {
        let configured = id("local", "new-model", 768);
        let stored = id("local", "old-model", 384);
        assert_eq!(
            configured.mismatch(&stored),
            Some(Mismatch::Dimension {
                configured: 768,
                stored: 384
            })
        );
    }

    /// The trap this exists to catch: same width, different space. Nothing at
    /// runtime would object — the dot product works fine and means nothing.
    #[test]
    fn same_width_different_model_is_fatal() {
        let m = id("local", "bge-small-en-v1.5-q", 384)
            .mismatch(&id("local", "all-minilm-l6-v2", 384))
            .expect("a different model is a mismatch");
        assert!(m.is_fatal(), "a different vector space is not usable: {m}");
        assert!(m.to_string().contains("not the same space"), "{m}");
    }

    /// Same model and width through a different provider is reported but not
    /// fatal — Ollama and OpenAI can serve the same weights.
    #[test]
    fn same_model_different_provider_is_reported_but_not_fatal() {
        let m = id("local", "bge-small-en-v1.5-q", 384)
            .mismatch(&id("remote", "bge-small-en-v1.5-q", 384))
            .expect("a different provider is worth saying");
        assert!(!m.is_fatal(), "likely comparable: {m}");
    }

    #[test]
    fn cosine_is_none_across_widths_and_on_zero_vectors() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), None);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), None);
        assert_eq!(cosine(&[], &[]), None);
    }

    #[test]
    fn cosine_is_one_for_identical_and_zero_for_orthogonal() {
        let one = cosine(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]).unwrap();
        assert!((one - 1.0).abs() < 1e-6, "{one}");
        let zero = cosine(&[1.0, 0.0], &[0.0, 1.0]).unwrap();
        assert!(zero.abs() < 1e-6, "{zero}");
    }

    #[test]
    fn a_short_batch_is_refused_rather_than_misaligned() {
        let id = id("deterministic", "t", 2);
        let texts = vec!["a".to_string(), "b".to_string()];
        let err = validate_batch(&id, &texts, vec![vec![0.0, 1.0]]).expect_err("short batch");
        assert!(matches!(err, EmbedError::CountMismatch { got: 1, want: 2 }));
    }

    #[test]
    fn a_wrong_width_is_refused() {
        let id = id("deterministic", "t", 3);
        let texts = vec!["a".to_string()];
        let err = validate_batch(&id, &texts, vec![vec![0.0, 1.0]]).expect_err("wrong width");
        assert!(matches!(
            err,
            EmbedError::WidthMismatch {
                advertised: 3,
                got: 2
            }
        ));
    }
}
