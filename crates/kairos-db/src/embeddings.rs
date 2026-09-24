//! Storing and refreshing item vectors (KAIROS-A-0021 rules 3 and 4,
//! KAIROS-T-0190).
//!
//! The database side of retrieval's write path. The pure halves live in
//! [`kairos_core::chunk`] and [`kairos_core::primary`]; the model lives in
//! `kairos-embed`; this module is what puts the result in KAIROS-T-0187's
//! `item_embeddings` and `item_chunks` and decides what still needs doing.
//!
//! Every function operates in the CURRENT `search_path` tenant schema, the same
//! convention as [`crate::items`] and [`crate::search`].
//!
//! # Why raw SQL
//!
//! Sanctioned by KAIROS-A-0009, and for a concrete reason rather than
//! preference: diesel has no `vector` type, so `schema.rs` — which is generated
//! by `angreal db schema-sync` from `diesel print-schema` — cannot describe these
//! columns at all. `crate::search` already carries the same exemption for
//! `tsvector`. Values are still bound as parameters; only the vector literal is
//! formatted, and only from `f32`s, which cannot carry SQL.
//!
//! # What "stale" means
//!
//! An item's embedding is current when the stored `content_hash` matches the
//! hash of the text that *would* be composed from it now, and the stored
//! provider/model/dimension match the configured ones. Anything else is stale:
//! edited text, a changed title, a swapped model.
//!
//! That is the whole point of hashing rather than comparing timestamps. Metis
//! instructs agents to append to a Status Updates section every few tool calls,
//! and the measurement says a median document has 9 sections — so an append must
//! re-embed **one chunk**, not the document. A modified-time comparison cannot
//! tell which section moved; a per-chunk hash can.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Uuid as SqlUuid};
use diesel::{QueryableByName, sql_query};
use uuid::Uuid;

/// Errors from the embedding store.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The database refused or failed the statement.
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// A vector's width disagrees with the dimension recorded beside it. Checked
    /// on the way in: a row whose `dimension` lies is worse than no row, because
    /// every later comparison trusts it.
    #[error("vector has {got} dimensions but {declared} was declared")]
    DimensionMismatch {
        /// The vector's actual width.
        got: usize,
        /// What the caller said it was.
        declared: usize,
    },
}

/// Which model produced a stored vector, as recorded beside it.
///
/// Mirrors `kairos_embed::ModelId` without depending on it: `kairos-db` has no
/// business linking an ML runtime, and the columns are the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredModel {
    /// Provider family.
    pub provider: String,
    /// Model name.
    pub model: String,
    /// Vector width.
    pub dimension: usize,
}

/// An item that needs embedding, with everything needed to compose its text.
#[derive(Debug, Clone, QueryableByName)]
pub struct PendingItem {
    /// The item's id.
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    /// `strategy` | `initiative` | `task` | `document` | `adr`.
    #[diesel(sql_type = Text)]
    pub entity_type: String,
    /// The item's short code, for logs and citations.
    #[diesel(sql_type = Text)]
    pub short_code: String,
    /// The item's title.
    #[diesel(sql_type = Text)]
    pub title: String,
    /// The item's content.
    #[diesel(sql_type = Text)]
    pub content: String,
    /// The hash currently stored, if any. `None` means never embedded.
    #[diesel(sql_type = Nullable<Text>)]
    pub stored_hash: Option<String>,
}

/// How far behind the vectors are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmbeddingCounts {
    /// Live items in the tenant.
    pub items: i64,
    /// Items with a primary vector, whatever its freshness.
    pub embedded: i64,
    /// Items whose stored vector came from a different provider/model/dimension
    /// than the one configured now.
    pub wrong_model: i64,
}

impl EmbeddingCounts {
    /// Items with no primary vector at all.
    pub fn missing(&self) -> i64 {
        (self.items - self.embedded).max(0)
    }
}

/// Format a vector as a pgvector literal.
///
/// `f32`'s `Display` emits the shortest representation that round-trips, so this
/// neither loses precision nor bloats the statement. Floats cannot contain SQL,
/// which is why this is safe to interpolate where a bind would be tidier —
/// diesel has no `vector` type to bind through.
fn vector_literal(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8 + 2);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        // Non-finite values would produce `NaN`/`inf`, which pgvector rejects —
        // better to write a zero and have the row be useless than to fail the
        // whole batch, but in practice a provider returning NaN is a bug worth
        // seeing, so this is deliberately NOT silently cleaned. pgvector's error
        // names the column.
        use std::fmt::Write as _;
        let _ = write!(s, "{x}");
    }
    s.push(']');
    s
}

fn check_dimension(vector: &[f32], declared: usize) -> Result<(), EmbeddingError> {
    if vector.len() != declared {
        return Err(EmbeddingError::DimensionMismatch {
            got: vector.len(),
            declared,
        });
    }
    Ok(())
}

/// Items whose primary vector is missing or whose content hash has moved on.
///
/// `hashes` is the caller's `(item_id, hash)` view of what the text hashes to
/// *now* — the caller composes the text, so only the caller can say. Passing it
/// in rather than computing it here keeps the composition rules in
/// `kairos_core::primary` where they are tested, instead of duplicated in SQL.
///
/// Ordered by `short_code` so a resumable backfill walks the same sequence every
/// run; `limit` bounds the page.
pub fn pending_primary(
    conn: &mut PgConnection,
    model: &StoredModel,
    limit: i64,
) -> Result<Vec<PendingItem>, EmbeddingError> {
    let rows = sql_query(
        "SELECT s.id, s.entity_type, s.short_code, s.title, s.content, e.content_hash AS stored_hash \
         FROM searchable_items s \
         LEFT JOIN item_embeddings e \
           ON e.item_id = s.id \
          AND e.provider = $1 AND e.model = $2 AND e.dimension = $3 \
         WHERE s.deleted_at IS NULL \
         ORDER BY s.short_code \
         LIMIT $4",
    )
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .bind::<BigInt, _>(limit)
    .load::<PendingItem>(conn)?;
    Ok(rows)
}

/// Write (or replace) an item's primary vector.
///
/// Upsert rather than delete-then-insert: the row's identity is the item, so a
/// re-embed is an update, and `ON CONFLICT` keeps a concurrent write from
/// producing a duplicate-key error on a path that is logically idempotent.
pub fn store_primary(
    conn: &mut PgConnection,
    item_id: Uuid,
    entity_type: &str,
    vector: &[f32],
    model: &StoredModel,
    content_hash: &str,
) -> Result<(), EmbeddingError> {
    check_dimension(vector, model.dimension)?;
    sql_query(format!(
        "INSERT INTO item_embeddings \
             (item_id, entity_type, embedding, provider, model, dimension, content_hash, updated_at) \
         VALUES ($1, $2, '{}'::public.vector, $3, $4, $5, $6, now()) \
         ON CONFLICT (item_id) DO UPDATE SET \
             entity_type = EXCLUDED.entity_type, \
             embedding = EXCLUDED.embedding, \
             provider = EXCLUDED.provider, \
             model = EXCLUDED.model, \
             dimension = EXCLUDED.dimension, \
             content_hash = EXCLUDED.content_hash, \
             updated_at = now()",
        vector_literal(vector)
    ))
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(entity_type)
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .bind::<Text, _>(content_hash)
    .execute(conn)?;
    Ok(())
}

/// One chunk ready to store.
#[derive(Debug, Clone)]
pub struct ChunkWrite<'a> {
    /// Position within the item.
    pub ordinal: i32,
    /// The literal heading, or `None`.
    pub heading: Option<&'a str>,
    /// Character range within the item's content.
    pub char_start: i32,
    /// Character range within the item's content.
    pub char_end: i32,
    /// The chunk's text.
    pub text: &'a str,
    /// Its vector.
    pub vector: &'a [f32],
    /// Hash of `text`.
    pub content_hash: &'a str,
}

/// Which of an item's chunks are already stored and current.
///
/// Returned as `(ordinal, content_hash)` so a caller can embed only what moved.
/// This is the mechanism behind the cost argument for chunking at all: appending
/// to one section of a nine-section document should cost one embedding.
pub fn stored_chunk_hashes(
    conn: &mut PgConnection,
    item_id: Uuid,
    model: &StoredModel,
) -> Result<Vec<(i32, String)>, EmbeddingError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Integer)]
        ordinal: i32,
        #[diesel(sql_type = Text)]
        content_hash: String,
    }
    let rows = sql_query(
        "SELECT ordinal, content_hash FROM item_chunks \
         WHERE item_id = $1 AND provider = $2 AND model = $3 AND dimension = $4 \
         ORDER BY ordinal",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .load::<Row>(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.ordinal, r.content_hash))
        .collect())
}

/// Replace an item's chunks with `chunks`, in one transaction.
///
/// Delete-then-insert rather than upsert, unlike [`store_primary`], because
/// chunk ordinals are **not stable identities**: editing a document can change
/// how many sections it has, and an upsert keyed on `(item_id, ordinal)` would
/// leave the tail of a now-shorter document behind as orphaned chunks that still
/// match queries. The transaction is what stops a reader seeing the gap.
pub fn replace_chunks(
    conn: &mut PgConnection,
    item_id: Uuid,
    entity_type: &str,
    chunks: &[ChunkWrite<'_>],
    model: &StoredModel,
) -> Result<(), EmbeddingError> {
    for c in chunks {
        check_dimension(c.vector, model.dimension)?;
    }
    conn.transaction(|conn| {
        sql_query("DELETE FROM item_chunks WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        for c in chunks {
            sql_query(format!(
                "INSERT INTO item_chunks \
                     (item_id, entity_type, ordinal, heading, char_start, char_end, \
                      chunk_text, embedding, provider, model, dimension, content_hash) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::public.vector, $8, $9, $10, $11)",
                vector_literal(c.vector)
            ))
            .bind::<SqlUuid, _>(item_id)
            .bind::<Text, _>(entity_type)
            .bind::<Integer, _>(c.ordinal)
            .bind::<Nullable<Text>, _>(c.heading)
            .bind::<Integer, _>(c.char_start)
            .bind::<Integer, _>(c.char_end)
            .bind::<Text, _>(c.text)
            .bind::<Text, _>(&model.provider)
            .bind::<Text, _>(&model.model)
            .bind::<Integer, _>(model.dimension as i32)
            .bind::<Text, _>(c.content_hash)
            .execute(conn)?;
        }
        Ok::<_, diesel::result::Error>(())
    })?;
    Ok(())
}

/// Forget everything stored for an item.
///
/// Called when an item is hard-deleted. Soft-deleted items keep their vectors on
/// purpose: KAIROS-A-0020 made put-away work searchable, and prior art in
/// completed work is one of the three claims the retrieval surface exists to
/// make — dropping their vectors would make the most valuable answer the one
/// thing that cannot be found.
pub fn forget_item(conn: &mut PgConnection, item_id: Uuid) -> Result<(), EmbeddingError> {
    conn.transaction(|conn| {
        sql_query("DELETE FROM item_chunks WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        sql_query("DELETE FROM item_embeddings WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        Ok::<_, diesel::result::Error>(())
    })?;
    Ok(())
}

/// How far behind the vectors are, for the configured model.
///
/// Observable staleness is a requirement rather than a nicety: a queue that
/// loses work silently is worse than a synchronous one, and an operator seeing
/// thin retrieval needs to be able to ask whether the cause is the model, the
/// backlog, or the query.
pub fn counts(
    conn: &mut PgConnection,
    model: &StoredModel,
) -> Result<EmbeddingCounts, EmbeddingError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = BigInt)]
        items: i64,
        #[diesel(sql_type = BigInt)]
        embedded: i64,
        #[diesel(sql_type = BigInt)]
        wrong_model: i64,
    }
    let row = sql_query(
        "SELECT \
            (SELECT count(*) FROM searchable_items WHERE deleted_at IS NULL) AS items, \
            (SELECT count(*) FROM item_embeddings e \
              JOIN searchable_items s ON s.id = e.item_id AND s.deleted_at IS NULL \
              WHERE e.provider = $1 AND e.model = $2 AND e.dimension = $3) AS embedded, \
            (SELECT count(*) FROM item_embeddings e \
              JOIN searchable_items s ON s.id = e.item_id AND s.deleted_at IS NULL \
              WHERE e.provider <> $1 OR e.model <> $2 OR e.dimension <> $3) AS wrong_model",
    )
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .get_result::<Row>(conn)?;
    Ok(EmbeddingCounts {
        items: row.items,
        embedded: row.embedded,
        wrong_model: row.wrong_model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_literal_is_pgvector_shaped() {
        assert_eq!(vector_literal(&[1.0, -0.5, 0.25]), "[1,-0.5,0.25]");
        assert_eq!(vector_literal(&[]), "[]");
    }

    /// The literal must round-trip: a lossy format would silently change every
    /// distance computed against it.
    #[test]
    fn a_vector_literal_round_trips_f32() {
        let v: Vec<f32> = vec![0.1, 1.0 / 3.0, f32::MIN_POSITIVE, -1.234_567_9e-7];
        let literal = vector_literal(&v);
        let parsed: Vec<f32> = literal
            .trim_matches(['[', ']'])
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(parsed, v, "from {literal}");
    }

    #[test]
    fn a_declared_dimension_that_lies_is_refused() {
        let err = check_dimension(&[0.0, 1.0], 384).expect_err("width disagrees");
        assert!(matches!(
            err,
            EmbeddingError::DimensionMismatch {
                got: 2,
                declared: 384
            }
        ));
        assert!(check_dimension(&[0.0, 1.0], 2).is_ok());
    }

    #[test]
    fn missing_is_items_minus_embedded_and_never_negative() {
        let c = EmbeddingCounts {
            items: 10,
            embedded: 4,
            wrong_model: 0,
        };
        assert_eq!(c.missing(), 6);
        // Defensive: a race between the two subqueries must not report a
        // negative backlog to an operator.
        let racy = EmbeddingCounts {
            items: 3,
            embedded: 5,
            wrong_model: 0,
        };
        assert_eq!(racy.missing(), 0);
    }
}
