//! The deterministic provider (KAIROS-T-0189): stable, free, instant.
//!
//! Not a convenience. Every test tier above unit needs embeddings, and needs the
//! same text to give the same vector every time; a real model in CI is slow, a
//! remote one is a network dependency inside a test suite, and neither is
//! reproducible enough to assert an *ordering* on.
//!
//! What it is not: meaningful. Two texts about the same subject get unrelated
//! vectors, because the vector is derived from a hash of the bytes. So it can
//! prove that ranking, storage, staleness and pagination work; it cannot prove
//! that retrieval finds anything. Tests that need real semantics need a real
//! model, and KAIROS-T-0189's own measurements are where that happens.

use sha2::{Digest, Sha256};

use crate::{EmbedError, Embedding, EmbeddingProvider, ModelId, validate_batch};

/// Hash-derived embeddings: same text, same vector, always.
#[derive(Debug, Clone)]
pub struct DeterministicProvider {
    id: ModelId,
}

impl DeterministicProvider {
    /// A provider producing `dimension`-wide vectors.
    ///
    /// The dimension is a parameter so a test can match whatever width the
    /// stored rows claim — including reproducing a width mismatch on purpose.
    pub fn new(dimension: usize) -> Self {
        Self {
            id: ModelId::new("deterministic", format!("sha256-{dimension}"), dimension),
        }
    }
}

impl Default for DeterministicProvider {
    /// 384 wide, matching the local default (`bge-small-en-v1.5-q`), so swapping
    /// the deterministic provider in does not also change the width and make a
    /// test pass or fail for the wrong reason.
    fn default() -> Self {
        Self::new(384)
    }
}

impl EmbeddingProvider for DeterministicProvider {
    fn model_id(&self) -> &ModelId {
        &self.id
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        let vectors = texts
            .iter()
            .map(|text| {
                // Expand SHA-256 by counter until the width is filled, so any
                // dimension is reachable from a 32-byte digest, then normalise:
                // real providers return unit vectors and cosine is what callers
                // use, so behaving differently here would let a bug hide.
                let mut out: Embedding = Vec::with_capacity(self.id.dimension);
                let mut counter: u32 = 0;
                while out.len() < self.id.dimension {
                    let mut hasher = Sha256::new();
                    hasher.update(text.as_bytes());
                    hasher.update(counter.to_le_bytes());
                    for chunk in hasher.finalize().chunks_exact(4) {
                        if out.len() == self.id.dimension {
                            break;
                        }
                        let bits = u32::from_le_bytes(chunk.try_into().unwrap());
                        // Into [-1, 1), evenly.
                        out.push((bits as f64 / u32::MAX as f64) as f32 * 2.0 - 1.0);
                    }
                    counter += 1;
                }
                let norm = out.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in out.iter_mut() {
                        *x /= norm;
                    }
                }
                out
            })
            .collect();
        validate_batch(&self.id, texts, vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosine;

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// The property the whole thing exists for.
    #[test]
    fn the_same_text_always_gives_the_same_vector() {
        let p = DeterministicProvider::default();
        let once = p
            .embed(&texts(&["ConfigMap watcher RBAC error spam"]))
            .unwrap();
        let again = p
            .embed(&texts(&["ConfigMap watcher RBAC error spam"]))
            .unwrap();
        assert_eq!(once, again);
    }

    /// And it does not depend on what else was in the batch — the failure that
    /// disqualified the dynamically quantized model in KAIROS-T-0189.
    #[test]
    fn a_vector_does_not_depend_on_its_company() {
        let p = DeterministicProvider::default();
        let alone = p.embed(&texts(&["probe"])).unwrap();
        let crowded = p
            .embed(&texts(&[
                "probe",
                "an unrelated and much longer companion text",
            ]))
            .unwrap();
        assert_eq!(alone[0], crowded[0]);
    }

    #[test]
    fn order_is_preserved() {
        let p = DeterministicProvider::default();
        let batch = p.embed(&texts(&["a", "b", "c"])).unwrap();
        for (i, t) in ["a", "b", "c"].iter().enumerate() {
            assert_eq!(batch[i], p.embed_one(t).unwrap(), "position {i} is {t}");
        }
    }

    #[test]
    fn different_texts_are_not_similar() {
        let p = DeterministicProvider::default();
        let v = p.embed(&texts(&["alpha", "beta"])).unwrap();
        let c = cosine(&v[0], &v[1]).unwrap();
        // Hash-derived vectors in 384 dimensions are near-orthogonal. This is
        // also the honest limit of this provider: it cannot express that two
        // texts are ABOUT the same thing.
        assert!(c.abs() < 0.2, "unrelated by construction, got {c}");
    }

    #[test]
    fn vectors_are_unit_length_like_a_real_provider() {
        let p = DeterministicProvider::default();
        for v in p.embed(&texts(&["x", "yy", "zzz"])).unwrap() {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((n - 1.0).abs() < 1e-5, "norm {n}");
        }
    }

    #[test]
    fn any_width_is_reachable() {
        for dim in [1, 7, 8, 384, 768, 1001] {
            let p = DeterministicProvider::new(dim);
            assert_eq!(p.embed_one("t").unwrap().len(), dim, "dimension {dim}");
            assert_eq!(p.model_id().dimension, dim);
        }
    }

    #[test]
    fn an_empty_batch_is_an_empty_result_not_an_error() {
        let p = DeterministicProvider::default();
        assert!(p.embed(&[]).unwrap().is_empty());
    }
}
