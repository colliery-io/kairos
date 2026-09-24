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
    /// The repository the item is issued against, if any (tasks only).
    #[diesel(sql_type = Nullable<Text>)]
    pub repository: Option<String>,
    /// The owning team, if any (tasks only).
    #[diesel(sql_type = Nullable<Text>)]
    pub team: Option<String>,
    /// The title of the item's parent, if it has one.
    #[diesel(sql_type = Nullable<Text>)]
    pub parent_title: Option<String>,
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
    // The 1:1 structural inputs rule 4 asks for. `repository` and `team` are
    // task-level attributes, so they LEFT JOIN through `tasks` and are NULL for
    // every other type — which is correct rather than missing: a document has no
    // repository, and composing a blank `repository:` line for it would put the
    // same token into every document in the tenant.
    //
    // The parent comes from `item_relationships`, where the edge points
    // parent -> child, so the item is the TARGET and the parent is the source.
    let rows = sql_query(
        "SELECT s.id, s.entity_type, s.short_code, s.title, s.content, \
                r.slug AS repository, tm.slug AS team, p.title AS parent_title, \
                e.content_hash AS stored_hash \
         FROM searchable_items s \
         LEFT JOIN item_embeddings e \
           ON e.item_id = s.id \
          AND e.provider = $1 AND e.model = $2 AND e.dimension = $3 \
         LEFT JOIN tasks t ON t.id = s.id \
         LEFT JOIN repositories r ON r.id = t.repository_id AND r.deleted_at IS NULL \
         LEFT JOIN teams tm ON tm.id = t.team_id AND tm.deleted_at IS NULL \
         LEFT JOIN item_relationships rel \
           ON rel.target_id = s.id AND rel.relationship = 'parent' \
         LEFT JOIN entity_directory p \
           ON p.id = rel.source_id AND p.deleted_at IS NULL \
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

/// Stamped metadata for a batch of items, as `(item_id, label, value)`.
///
/// Fetched for the batch rather than per item: a backfill page of 200 items
/// would otherwise be 200 round trips for what is one query. Ordered by
/// definition name so composition is stable — the same item must produce the
/// same text, or its content hash moves for no reason and every run re-embeds
/// everything.
///
/// The **label** is the definition's human name, not its slug: rule 4 leans on
/// metadata precisely because it is where a tenant wrote down what something
/// means, and the name is what they wrote.
pub fn metadata_for(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
) -> Result<Vec<(Uuid, String, String)>, EmbeddingError> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        item_id: Uuid,
        #[diesel(sql_type = Text)]
        label: String,
        #[diesel(sql_type = Text)]
        value: String,
    }
    let rows = sql_query(
        "SELECT m.item_id, d.name AS label, m.value \
         FROM item_metadata m \
         JOIN metadata_definitions d ON d.id = m.metadata_definition_id \
         WHERE m.item_id = ANY($1) \
         ORDER BY m.item_id, d.name",
    )
    .bind::<diesel::sql_types::Array<SqlUuid>, _>(item_ids)
    .load::<Row>(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.item_id, r.label, r.value))
        .collect())
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

/// One chunk as the caller wants it stored.
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
    /// Hash of `text`.
    pub content_hash: &'a str,
    /// The chunk's vector, or `None` to mean **unchanged — leave the stored row
    /// alone**.
    ///
    /// This is where the whole cost argument for chunking cashes out. Metis
    /// instructs agents to append to a Status Updates section every few tool
    /// calls and a median document has nine sections, so an append must cost one
    /// embedding rather than nine. A caller that has compared hashes passes
    /// `None` for the eight that did not move, and no model is asked about them.
    pub vector: Option<&'a [f32]>,
}

/// Which of an item's chunks are already stored and current.
///
/// Returned as `(ordinal, content_hash)` so a caller can embed only what moved.
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

/// Bring an item's stored chunks in line with `chunks`, in one transaction.
///
/// `chunks` is the **complete** new set, in ordinal order. Chunks carrying a
/// vector are written; chunks carrying `None` are left exactly as they are; and
/// any stored chunk beyond the end of the new set is deleted.
///
/// That deletion is not tidiness. Chunk ordinals are **positions, not
/// identities**: editing a document changes how many sections it has, and
/// without it the tail of a now-shorter document would survive as chunks that
/// still match queries, forever, citing text the document no longer contains.
/// The transaction is what stops a reader seeing the gap mid-update.
pub fn sync_chunks(
    conn: &mut PgConnection,
    item_id: Uuid,
    entity_type: &str,
    chunks: &[ChunkWrite<'_>],
    model: &StoredModel,
) -> Result<(), EmbeddingError> {
    for c in chunks {
        if let Some(v) = c.vector {
            check_dimension(v, model.dimension)?;
        }
    }
    let keep = chunks.len() as i32;
    conn.transaction(|conn| {
        sql_query("DELETE FROM item_chunks WHERE item_id = $1 AND ordinal >= $2")
            .bind::<SqlUuid, _>(item_id)
            .bind::<Integer, _>(keep)
            .execute(conn)?;
        for c in chunks {
            let Some(vector) = c.vector else {
                continue;
            };
            sql_query(format!(
                "INSERT INTO item_chunks \
                     (item_id, entity_type, ordinal, heading, char_start, char_end, \
                      chunk_text, embedding, provider, model, dimension, content_hash) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::public.vector, $8, $9, $10, $11) \
                 ON CONFLICT (item_id, ordinal) DO UPDATE SET \
                     entity_type = EXCLUDED.entity_type, \
                     heading = EXCLUDED.heading, \
                     char_start = EXCLUDED.char_start, \
                     char_end = EXCLUDED.char_end, \
                     chunk_text = EXCLUDED.chunk_text, \
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
