//! The real model, end to end (KAIROS-T-0189).
//!
//! Everything in the unit tier deliberately avoids loading 65 MB of weights, so
//! nothing there proves the local provider actually embeds anything. This does,
//! and it asserts the two properties the model was *chosen* for — because a
//! future model change that broke either would otherwise be found by retrieval
//! quietly getting worse.
//!
//! The model is cached under `target/embed-cache`, and downloading is allowed
//! here: the integration tier already needs Docker images and a network, and
//! `target/` is what CI caches, so it is usually warm. That is also why this is
//! an integration test rather than a unit one.

use std::path::PathBuf;
use std::sync::LazyLock;

use kairos_embed::local::{LocalConfig, LocalProvider};
use kairos_embed::{EmbeddingProvider, cosine};

/// ONE provider, shared by every test in this file.
///
/// Not just an optimisation. Cargo runs these tests in parallel threads, and the
/// first version of this file built a provider per test: four of the five then
/// failed with `Failed to retrieve model file 'model_optimized.onnx'` because
/// they were all downloading into the same cache directory at once and racing
/// each other. `LazyLock` serialises the initialisation, which is also how the
/// server holds it — one provider behind an `Arc` for the process.
///
/// The product avoids the race for a different reason: the image is built with
/// the cache already populated, so nothing downloads at runtime at all.
static PROVIDER: LazyLock<LocalProvider> = LazyLock::new(|| {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/embed-cache");
    std::fs::create_dir_all(&cache).expect("creating the model cache directory");
    LocalProvider::new(&LocalConfig {
        cache_dir: cache,
        allow_download: true,
    })
    .expect("loading the local model")
});

fn provider() -> &'static LocalProvider {
    &PROVIDER
}

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_local_model_embeds_and_advertises_itself_honestly() {
    let p = provider();
    assert_eq!(p.model_id().provider, "local");
    assert_eq!(p.model_id().model, "bge-small-en-v1.5-q");
    assert_eq!(p.model_id().dimension, 384);

    let v = p
        .embed(&texts(&["a ticket about refund rounding"]))
        .unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].len(), 384, "the advertised width is the real width");
    let norm = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-3,
        "bge returns unit vectors: {norm}"
    );
}

/// **The property the model was chosen for.** A dynamically quantized model
/// returns cos 0.992 here rather than 1.0, which is why one was rejected: this
/// store skips unchanged text by content hash, so a model that disagrees with
/// itself makes staleness undetectable and identical queries unstable between
/// days. If a future model change breaks this, it must be a deliberate decision,
/// not a discovery.
#[test]
fn the_same_text_gives_the_same_vector_whatever_it_travels_with() {
    let p = provider();
    let probe = "ConfigMap watcher RBAC error spam in broker logs";

    let alone = p.embed(&texts(&[probe])).unwrap();
    let with_short = p.embed(&texts(&[probe, "a"])).unwrap();
    let with_long = p
        .embed(&texts(&[
            probe,
            "an altogether longer companion text, present only to change what \
             else is in the batch, describing at some length a wholly unrelated \
             matter so that any per-batch adaptation has something to adapt to",
        ]))
        .unwrap();

    for (name, other) in [
        ("short company", &with_short[0]),
        ("long company", &with_long[0]),
    ] {
        let c = cosine(&alone[0], other).unwrap();
        assert!(
            c > 0.99999,
            "the same text must embed identically regardless of {name}: cos {c}"
        );
    }
}

/// And the vectors mean something, which the deterministic provider cannot show.
/// Two ways of describing one defect should be closer to each other than either
/// is to an unrelated ticket.
#[test]
fn related_text_is_closer_than_unrelated_text() {
    let p = provider();
    let v = p
        .embed(&texts(&[
            "the refund total is rounded before tax instead of after",
            "rounding is applied too early when calculating a refund",
            "add a dark mode toggle to the settings page",
        ]))
        .unwrap();

    let related = cosine(&v[0], &v[1]).unwrap();
    let unrelated = cosine(&v[0], &v[2]).unwrap();
    assert!(
        related > unrelated,
        "two descriptions of one defect ({related:.3}) must beat an unrelated \
         ticket ({unrelated:.3})"
    );
    // The gap, not just the ordering: KAIROS-T-0190 measured related pairs at
    // ~0.80 mean against ~0.68 for unrelated, a gap of about 0.12. Requiring
    // clearly more than nothing here would pass on noise.
    assert!(
        related - unrelated > 0.05,
        "the gap must be real, not marginal: {related:.3} vs {unrelated:.3}"
    );
}

/// Batching must not change the answer. This is what makes a backfill's vectors
/// comparable with vectors written one at a time on the write path.
#[test]
fn batched_and_unbatched_agree() {
    let p = provider();
    let items = texts(&[
        "first ticket about pagination",
        "second ticket about caching",
        "third ticket about retries",
    ]);
    let batched = p.embed(&items).unwrap();
    for (i, text) in items.iter().enumerate() {
        let alone = p.embed_one(text).unwrap();
        let c = cosine(&batched[i], &alone).unwrap();
        assert!(
            c > 0.99999,
            "item {i} differs between batched and alone: {c}"
        );
    }
}

/// The empty batch is a real case — a backfill whose page happens to be fully
/// up to date — and it must not be an error or a round trip.
#[test]
fn an_empty_batch_costs_nothing() {
    let p = provider();
    assert!(p.embed(&[]).unwrap().is_empty());
}
