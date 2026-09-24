//! Keeping item vectors current (KAIROS-A-0021 rules 3 and 4, KAIROS-T-0190).
//!
//! The piece that joins the three halves: [`kairos_core`] composes and chunks
//! the text, [`kairos_embed`] turns it into vectors, and
//! [`kairos_db::embeddings`] stores it. This module decides **what actually
//! needs doing**, which is almost always far less than "everything".
//!
//! # Skipping is the point
//!
//! Every text — the composed primary and each chunk — is hashed, and the hash is
//! compared with what is stored. Unchanged text is not sent to a model at all.
//!
//! That is not an optimisation bolted on afterwards; it is why chunking exists.
//! Metis instructs agents to append to a Status Updates section every few tool
//! calls, and the measured corpus has a median of nine sections per document, so
//! the difference between hashing per chunk and re-embedding per document is
//! roughly nine to one on the commonest write there is.
//!
//! # Batching
//!
//! Whatever does need embedding goes to the provider in **one call**, measured
//! at 42 texts/s batched against 4,927 documents. A per-chunk call would turn a
//! document refresh into nine round trips, which for the remote provider is nine
//! HTTP requests.
//!
//! # Blocking
//!
//! [`kairos_embed::EmbeddingProvider`] is synchronous and the remote
//! implementation **panics** if called from an async worker thread. Everything
//! here therefore runs under [`crate::blocking::BlockingTenantPool`], which is
//! already a `spawn_blocking` context. Nothing in this module may be awaited
//! directly from a handler.

use std::collections::HashMap;
use std::sync::Arc;

use kairos_core::chunk::chunk;
use kairos_core::primary::{PrimaryInputs, content_hash, primary_text};
use kairos_core::short_code::ItemType;
use kairos_db::embeddings::{
    ChunkWrite, EmbeddingError, PendingItem, StoredModel, metadata_for, pending_primary,
    store_primary, stored_chunk_hashes, sync_chunks,
};
use kairos_embed::{EmbedError, EmbeddingProvider};

/// Errors from a refresh.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] EmbeddingError),
    /// The provider refused or failed.
    #[error(transparent)]
    Provider(#[from] EmbedError),
    /// A stored row names an entity type this build does not know.
    #[error("unknown entity type {0:?}")]
    UnknownEntityType(String),
}

/// What a refresh actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefreshOutcome {
    /// Whether the primary vector was recomputed.
    pub primary_embedded: bool,
    /// How many chunks were recomputed.
    pub chunks_embedded: usize,
    /// How many chunks the item has in total.
    pub chunks_total: usize,
}

impl RefreshOutcome {
    /// Whether anything at all was sent to the model.
    pub fn did_work(&self) -> bool {
        self.primary_embedded || self.chunks_embedded > 0
    }

    /// How many texts were embedded.
    pub fn embedded(&self) -> usize {
        usize::from(self.primary_embedded) + self.chunks_embedded
    }
}

/// What a batch did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatchOutcome {
    /// Items considered.
    pub items_seen: usize,
    /// Items that needed any work.
    pub items_changed: usize,
    /// Texts sent to the model.
    pub texts_embedded: usize,
}

/// How many items a background sweep looks at per tenant per tick.
///
/// Small on purpose. The sweep runs every few seconds and is competing with real
/// request traffic for the same connection pool; a large page would turn a
/// catch-up into a latency spike on the boards. A deployment with a real backlog
/// runs `embed-backfill`, which exists to go fast under supervision.
pub const REFRESH_BATCH: i64 = 25;

/// Keeps an item's vectors in line with its text.
pub struct EmbeddingService {
    provider: Arc<dyn EmbeddingProvider>,
    model: StoredModel,
}

impl std::fmt::Debug for EmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingService")
            .field("model", &self.model)
            .finish()
    }
}

fn parse_entity_type(value: &str) -> Result<ItemType, RefreshError> {
    Ok(match value {
        "strategy" => ItemType::Strategy,
        "initiative" => ItemType::Initiative,
        "task" => ItemType::Task,
        "document" => ItemType::Document,
        "adr" => ItemType::Adr,
        other => return Err(RefreshError::UnknownEntityType(other.to_string())),
    })
}

impl EmbeddingService {
    /// Wrap a provider.
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Self {
        let id = provider.model_id();
        let model = StoredModel {
            provider: id.provider.clone(),
            model: id.model.clone(),
            dimension: id.dimension,
        };
        Self { provider, model }
    }

    /// The model every stored row is compared against.
    pub fn model(&self) -> &StoredModel {
        &self.model
    }

    /// The model, for a log line an operator will read.
    pub fn model_display(&self) -> String {
        format!(
            "{}/{} ({}d)",
            self.model.provider, self.model.model, self.model.dimension
        )
    }

    /// Bring one item's vectors up to date.
    ///
    /// `metadata` is the item's stamped `(label, value)` pairs, which the caller
    /// fetches for a whole batch at once — see [`metadata_for`].
    pub fn refresh_item(
        &self,
        conn: &mut diesel::pg::PgConnection,
        item: &PendingItem,
        metadata: &[(String, String)],
    ) -> Result<RefreshOutcome, RefreshError> {
        let item_type = parse_entity_type(&item.entity_type)?;
        let mut outcome = RefreshOutcome::default();

        // ---- the primary vector -------------------------------------------
        let inputs = PrimaryInputs {
            title: &item.title,
            repository: item.repository.as_deref(),
            team: item.team.as_deref(),
            parent_title: item.parent_title.as_deref(),
            metadata,
            content: &item.content,
        };
        let text = primary_text(item_type, &inputs);
        let hash = content_hash(&text);
        let primary_stale = item.stored_hash.as_deref() != Some(hash.as_str());

        // ---- the chunks ---------------------------------------------------
        let chunks = chunk(&item.content);
        outcome.chunks_total = chunks.len();
        let stored: HashMap<i32, String> = stored_chunk_hashes(conn, item.id, &self.model)?
            .into_iter()
            .collect();
        let hashes: Vec<String> = chunks.iter().map(|c| content_hash(&c.text)).collect();

        // Everything that needs the model, gathered before anything is sent, so
        // one item costs one call however many of its sections moved.
        let mut to_embed: Vec<String> = Vec::new();
        if primary_stale {
            to_embed.push(text);
        }
        let stale_chunks: Vec<usize> = (0..chunks.len())
            .filter(|&i| stored.get(&(i as i32)) != Some(&hashes[i]))
            .collect();
        to_embed.extend(stale_chunks.iter().map(|&i| chunks[i].text.clone()));

        if to_embed.is_empty() {
            return Ok(outcome);
        }
        let vectors = self.provider.embed(&to_embed)?;

        let mut next = 0usize;
        if primary_stale {
            store_primary(
                conn,
                item.id,
                &item.entity_type,
                &vectors[next],
                &self.model,
                &hash,
            )?;
            outcome.primary_embedded = true;
            next += 1;
        }

        // `None` for every chunk that did not move: the store leaves those rows
        // untouched rather than rewriting them with identical content.
        let mut fresh: HashMap<usize, &[f32]> = HashMap::new();
        for &i in &stale_chunks {
            fresh.insert(i, vectors[next].as_slice());
            next += 1;
        }
        outcome.chunks_embedded = stale_chunks.len();

        let writes: Vec<ChunkWrite<'_>> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| ChunkWrite {
                ordinal: i as i32,
                heading: c.heading.as_deref(),
                char_start: c.char_start as i32,
                char_end: c.char_end as i32,
                text: &c.text,
                content_hash: &hashes[i],
                vector: fresh.get(&i).copied(),
            })
            .collect();
        sync_chunks(conn, item.id, &item.entity_type, &writes, &self.model)?;

        Ok(outcome)
    }

    /// Refresh up to `limit` items, oldest short code first.
    ///
    /// The unit a backfill walks. It is deliberately a *page* rather than a loop
    /// over everything: a backfill has to be interruptible and rate-limitable,
    /// and the caller is what decides how fast to go.
    pub fn refresh_batch(
        &self,
        conn: &mut diesel::pg::PgConnection,
        limit: i64,
    ) -> Result<BatchOutcome, RefreshError> {
        let items = pending_primary(conn, &self.model, limit)?;
        let ids: Vec<uuid::Uuid> = items.iter().map(|i| i.id).collect();

        // One metadata query for the page, not one per item.
        let mut by_item: HashMap<uuid::Uuid, Vec<(String, String)>> = HashMap::new();
        for (id, label, value) in metadata_for(conn, &ids)? {
            by_item.entry(id).or_default().push((label, value));
        }

        let mut outcome = BatchOutcome {
            items_seen: items.len(),
            ..Default::default()
        };
        for item in &items {
            let metadata = by_item.get(&item.id).cloned().unwrap_or_default();
            let one = self.refresh_item(conn, item, &metadata)?;
            if one.did_work() {
                outcome.items_changed += 1;
                outcome.texts_embedded += one.embedded();
            }
        }
        Ok(outcome)
    }
}

/// Run a refresh sweep across every tenant, for ever.
///
/// # Why a sweep and not a queue
///
/// [[KAIROS-A-0021]] rule 7 requires embedding to happen **off** the write path:
/// `create_item` must not wait on a model, and a failed embedding must leave the
/// item created and retrievable lexically. The obvious implementation is a work
/// queue, and this is not one.
///
/// Staleness here is **derived**, not recorded: an item needs work exactly when
/// its stored content hash no longer matches the text it would compose now. A
/// queue would be a second statement of the same fact, and second statements
/// drift — a row enqueued and lost, an item edited twice while one entry sits
/// waiting, a retry counter that outlives the reason for it. A sweep asks the
/// only source of truth there is and is therefore self-healing: whatever went
/// wrong last time, the next pass sees the world as it actually is.
///
/// The cost is latency rather than correctness. An item is embedded within one
/// interval rather than immediately, and that is an acceptable trade for a
/// feature whose results are proposals.
///
/// # It never takes the server down
///
/// Every error is logged and the loop continues. An embedding provider that has
/// gone away is a reason for retrieval to degrade to lexical, which is what rule
/// 7 promises; it is not a reason to stop serving boards.
pub async fn run_refresher(
    blocking: crate::blocking::BlockingTenantPool,
    service: Arc<EmbeddingService>,
    interval: std::time::Duration,
    batch: i64,
) {
    tracing::info!(
        interval_secs = interval.as_secs(),
        batch,
        model = %service.model_display(),
        "embedding refresher started"
    );
    let mut ticker = tokio::time::interval(interval);
    // A slow sweep must not cause a burst of catch-up ticks afterwards.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        let tenants = match blocking
            .run_public(|conn| {
                kairos_db::list_tenants(conn).map_err(crate::error::ApiError::internal)
            })
            .await
        {
            Ok(tenants) => tenants,
            Err(e) => {
                tracing::warn!(error = ?e, "embedding refresher could not list tenants");
                continue;
            }
        };

        for tenant in tenants.into_iter().filter(|t| t.schema_exists) {
            let service = Arc::clone(&service);
            let slug = tenant.slug.clone();
            let outcome = blocking
                .run(&tenant.slug, move |conn| {
                    service
                        .refresh_batch(conn, batch)
                        .map_err(crate::error::ApiError::internal)
                })
                .await;
            match outcome {
                Ok(o) if o.items_changed > 0 => tracing::info!(
                    tenant = %slug,
                    items = o.items_changed,
                    texts = o.texts_embedded,
                    "embedded"
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!(tenant = %slug, error = ?e, "embedding refresh failed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entity_type_parses_and_nothing_else_does() {
        for (s, expected) in [
            ("strategy", ItemType::Strategy),
            ("initiative", ItemType::Initiative),
            ("task", ItemType::Task),
            ("document", ItemType::Document),
            ("adr", ItemType::Adr),
        ] {
            assert_eq!(parse_entity_type(s).unwrap(), expected);
        }
        let err = parse_entity_type("epic").expect_err("not a Kairos entity type");
        assert!(err.to_string().contains("epic"), "{err}");
    }

    #[test]
    fn an_outcome_reports_what_it_sent_to_the_model() {
        let none = RefreshOutcome {
            primary_embedded: false,
            chunks_embedded: 0,
            chunks_total: 9,
        };
        assert!(!none.did_work());
        assert_eq!(none.embedded(), 0);

        // The commonest real write: one section of nine appended to.
        let append = RefreshOutcome {
            primary_embedded: false,
            chunks_embedded: 1,
            chunks_total: 9,
        };
        assert!(append.did_work());
        assert_eq!(append.embedded(), 1, "one embedding, not nine");
    }
}
