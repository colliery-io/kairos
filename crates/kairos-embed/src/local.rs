//! The local provider (KAIROS-A-0021 rule 1, KAIROS-T-0189): ONNX inference
//! in-process, CPU only, no GPU, no sidecar.
//!
//! # The model, and why it is this one
//!
//! `bge-small-en-v1.5`, **statically quantized**, 384 dimensions, 65 MB on disk.
//! Chosen by measurement on 4,927 real Metis documents against ground truth taken
//! from the corpus itself — duplicate tickets, identified by identical titles
//! under different short codes — scored by recall@1 within the project:
//!
//! | model | dim | texts/s | on disk | recall@1 |
//! |---|---|---|---|---|
//! | `bge-small-en-v1.5` | 384 | 46 | 128 MB | 82% |
//! | **`bge-small-en-v1.5` (Q)** | **384** | **42** | **65 MB** | **82%** |
//! | `bge-base-en-v1.5` (Q) | 768 | 12 | 210 MB | 84% |
//!
//! Static quantization halves the model for no measured loss. The base model's
//! extra two points is one pair out of fifty trials — noise at that sample size —
//! for 3.2x the size and 3.5x the time.
//!
//! **`all-MiniLM-L6-v2` (Q) is 23 MB and was rejected**, and not for speed.
//! It is *dynamically* quantized: it refits its data range per batch, so the same
//! text embedded in different company comes back at cosine 0.992 rather than
//! 1.000, where this model returns 1.000000. A store that embeds incrementally
//! forever and compares vectors written months apart cannot use a model that
//! disagrees with itself — KAIROS-T-0187's `content_hash` exists precisely so
//! unchanged text can be skipped, and a nondeterministic model makes staleness
//! undetectable. Its 0.008 self-reproduction error also sits against a 0.12 gap
//! between related and unrelated pairs, so the noise is a real fraction of the
//! signal.
//!
//! # Where the weights come from
//!
//! **Not from the network at request time.** `fastembed` resolves models from a
//! cache directory and downloads into it on a miss, and a stateless container
//! that must reach huggingface.co before it can serve is not "local by default"
//! in any sense an operator would accept — it breaks air-gapped deployments
//! outright.
//!
//! So the cache directory is explicit ([`LocalConfig::cache_dir`]) and the
//! container image is built with it already populated. On a miss,
//! [`LocalProvider::new`] says which directory it looked in and how to fill it,
//! rather than quietly reaching for the internet: see
//! [`LocalConfig::allow_download`], which is **off by default** for exactly that
//! reason.

use std::path::PathBuf;
use std::sync::Mutex;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::{EmbedError, Embedding, EmbeddingProvider, ModelId, validate_batch};

/// The model this provider uses. One value, not a knob: a deployment that wants
/// a different model uses the remote provider, and a deployment that wants a
/// different *local* model is a change to this line plus a re-measurement, which
/// should be deliberate.
const MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15Q;

/// Stable name recorded with every vector. Deliberately not derived from the
/// `EmbeddingModel` enum's `Debug`: that is a Rust identifier which could be
/// renamed upstream, and this string is written into a database.
const MODEL_NAME: &str = "bge-small-en-v1.5-q";

/// Vector width, asserted against the model at startup rather than trusted.
const DIMENSION: usize = 384;

/// How many texts go to the model at once.
///
/// Measured at 42 texts/s on the corpus at this size. Batching is not optional
/// for throughput, and this model tolerates it because it is *statically*
/// quantized — see the module docs for the model that does not.
const BATCH: usize = 256;

/// How to start the local provider.
#[derive(Debug, Clone)]
pub struct LocalConfig {
    /// Directory holding the model files. The container image is built with this
    /// populated; a local developer's first run fills it if
    /// [`Self::allow_download`] is set.
    pub cache_dir: PathBuf,
    /// Whether a cache miss may fetch from HuggingFace.
    ///
    /// **Off by default.** An operator who deployed an image expecting it to be
    /// self-contained should get a clear error rather than an outbound
    /// connection they did not ask for and may not be able to make.
    pub allow_download: bool,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("/var/lib/kairos/models"),
            allow_download: false,
        }
    }
}

/// ONNX inference in-process.
pub struct LocalProvider {
    // fastembed's `embed` needs `&mut self` (ONNX sessions are not shareable),
    // while `EmbeddingProvider` is `&self` so callers can hold one behind an
    // `Arc`. A Mutex is the honest way to bridge that: inference is CPU-bound
    // and already batched, so serialising callers costs nothing that parallelism
    // would have won — the work inside is what saturates the cores.
    model: Mutex<TextEmbedding>,
    id: ModelId,
}

impl LocalProvider {
    /// Load the model from `config.cache_dir`.
    ///
    /// Fails rather than downloading unless `config.allow_download` is set, and
    /// the error names the directory and how to populate it.
    pub fn new(config: &LocalConfig) -> Result<Self, EmbedError> {
        if !config.allow_download && !Self::is_cached(config) {
            return Err(EmbedError::Unavailable(format!(
                "the {MODEL_NAME} model is not in {}, and downloading is disabled. \
                 Kairos images ship with this directory populated; if you are running \
                 the binary directly, set KAIROS_EMBED_ALLOW_DOWNLOAD=1 once to fetch \
                 it (~65 MB), point KAIROS_EMBED_CACHE at a populated directory, or \
                 configure an OpenAI-compatible endpoint instead.",
                config.cache_dir.display()
            )));
        }

        let options = TextInitOptions::new(MODEL)
            .with_cache_dir(config.cache_dir.clone())
            .with_show_download_progress(false);
        let model = TextEmbedding::try_new(options).map_err(|e| {
            EmbedError::Unavailable(format!(
                "could not load {MODEL_NAME} from {}: {e}",
                config.cache_dir.display()
            ))
        })?;

        let provider = Self {
            model: Mutex::new(model),
            id: ModelId::new("local", MODEL_NAME, DIMENSION),
        };

        // Assert the advertised width against the model rather than trusting the
        // constant. If an upstream model revision changed it, every stored
        // vector would be the wrong width and the first symptom would be a
        // retrieval that quietly returns nothing.
        let probe = provider.embed_one("dimension probe")?;
        if probe.len() != DIMENSION {
            return Err(EmbedError::WidthMismatch {
                advertised: DIMENSION,
                got: probe.len(),
            });
        }
        tracing::info!(
            model = MODEL_NAME,
            dimension = DIMENSION,
            cache_dir = %config.cache_dir.display(),
            "local embedding provider ready"
        );
        Ok(provider)
    }

    /// Whether `cache_dir` already holds something for this model.
    ///
    /// Deliberately shallow: it looks for a directory naming the model rather
    /// than validating the files, because fastembed owns that layout and
    /// duplicating its rules here would rot. Getting this wrong fails loudly in
    /// `try_new` a moment later; the check exists to turn "silently downloads
    /// 65 MB" into "says what is missing".
    fn is_cached(config: &LocalConfig) -> bool {
        let Ok(entries) = std::fs::read_dir(&config.cache_dir) else {
            return false;
        };
        entries.flatten().any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .replace(['-', '_'], "")
                .contains("bgesmallenv15")
        })
    }
}

impl EmbeddingProvider for LocalProvider {
    fn model_id(&self) -> &ModelId {
        &self.id
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let mut model = self
            .model
            .lock()
            .map_err(|_| EmbedError::Failed("embedding model mutex poisoned".into()))?;
        let vectors = model
            .embed(refs, Some(BATCH))
            .map_err(|e| EmbedError::Failed(e.to_string()))?;
        validate_batch(&self.id, texts, vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache-miss path is the one an operator meets, so its message is worth
    /// asserting. This needs no model and so runs in the unit tier.
    #[test]
    fn a_cache_miss_explains_itself_instead_of_downloading() {
        let config = LocalConfig {
            cache_dir: PathBuf::from("/nonexistent/kairos-embed-test"),
            allow_download: false,
        };
        // `expect_err` would need LocalProvider: Debug, and a loaded ONNX
        // session has no useful Debug; match instead.
        let said = match LocalProvider::new(&config) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("must not silently download"),
        };
        assert!(said.contains("/nonexistent/kairos-embed-test"), "{said}");
        assert!(said.contains("KAIROS_EMBED_ALLOW_DOWNLOAD"), "{said}");
        assert!(
            said.contains("OpenAI-compatible"),
            "offers the alternative: {said}"
        );
    }

    #[test]
    fn is_cached_is_false_for_a_missing_directory() {
        assert!(!LocalProvider::is_cached(&LocalConfig {
            cache_dir: PathBuf::from("/nonexistent/kairos-embed-test"),
            allow_download: false,
        }));
    }

    /// An empty directory is a miss, not a hit — otherwise an image whose model
    /// layer failed to copy would start and then embed nothing.
    #[test]
    fn is_cached_is_false_for_an_empty_directory() {
        let dir = std::env::temp_dir().join(format!("kairos-embed-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cached = LocalProvider::is_cached(&LocalConfig {
            cache_dir: dir.clone(),
            allow_download: false,
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!cached);
    }
}
