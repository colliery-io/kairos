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
//!
//! # The marks (KAIROS-T-0636)
//!
//! The hash decides what goes to the model. A mark in `embedding_dirty`
//! decides which items the background sweep looks at. The write transaction
//! of an item marks it ([`mark_dirty`], through the item event of the
//! write), and a write that changes the text of other items marks them too
//! ([`mark_children_dirty`], [`mark_repository_dirty`],
//! [`mark_definition_dirty`]). The sweep reads the oldest marks
//! ([`dirty_page`]) and deletes a mark when it is still the mark that it read
//! ([`clear_dirty`]). With no mark, a run reads nothing else.

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
    #[error(
        "The vector has {got} dimensions, and the call gave {declared} as the number of dimensions."
    )]
    DimensionMismatch {
        /// The vector's actual width.
        got: usize,
        /// What the caller said it was.
        declared: usize,
    },
    /// No item carries this short code.
    #[error("No item has the short code {0:?}.")]
    ItemNotFound(String),
    /// Stored rows disagree with the width the columns are being pinned to.
    #[error(
        "The table {table} has {rows} rows with vectors that do not have {wanted} dimensions. \
         Thus the column cannot have the type of {wanted} dimensions. First make the \
         vectors again with the model of the configuration. Use \
         `angreal db backfill-embeddings` or `kairos-server embed-backfill`."
    )]
    MixedDimensions {
        /// Which table.
        table: &'static str,
        /// The width being pinned to.
        wanted: usize,
        /// How many rows disagree.
        rows: i64,
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
    /// The item's content, and then its notes as `## Note` sections
    /// (KAIROS-T-0630: the SQL function `item_notes_markdown`). So a note
    /// is embedded as it was when it was part of the content.
    #[diesel(sql_type = Text)]
    pub content: String,
    /// The repository the item is issued against, if any (tasks only).
    #[diesel(sql_type = Nullable<Text>)]
    pub repository: Option<String>,
    /// The title of the item's parent, if it has one, live or archived
    /// (COLLIERY-T-1840: most imported parents are archived, and their title is
    /// still the context of the child).
    #[diesel(sql_type = Nullable<Text>)]
    pub parent_title: Option<String>,
    /// The hash currently stored, if any. `None` means never embedded.
    #[diesel(sql_type = Nullable<Text>)]
    pub stored_hash: Option<String>,
}

/// How far behind the vectors are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmbeddingCounts {
    /// Items in the tenant, archived ones included — they carry vectors too,
    /// because prior art is one of the claims retrieval makes.
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
/// **Never-embedded items first**, then by `short_code`, with `limit`/`offset`
/// paging the rest. Both halves of that ordering are load-bearing, and the
/// second was added after a bug that only a full-scale run could find.
///
/// `embed-backfill` walks these pages; the background sweep reads the marks
/// of [`dirty_page`] (KAIROS-T-0636). A sweep took one page per tick before
/// the marks. Ordered by `short_code` alone and
/// always from offset 0, it took *the same* page every time — so once the first
/// 25 items were current it did no work for ever, and item 26 was never embedded
/// at all. The caller pages through and wraps; putting never-embedded rows first
/// means genuinely new work is still picked up promptly rather than waiting for
/// the cursor to come round to it.
///
/// **Archived items are included.** Not an oversight: prior art in finished work
/// is one of the three claims retrieval exists to make (KAIROS-T-0191), and
/// KAIROS-A-0020 kept put-away work searchable precisely so it is possible. If
/// this filtered on `deleted_at IS NULL`, an item archived before it was ever
/// embedded would never get a vector — and *that* item, the one somebody already
/// finished, is exactly the one worth finding. Archived rows are written once and
/// then never change, so the cost is a single pass.
pub fn pending_primary(
    conn: &mut PgConnection,
    model: &StoredModel,
    limit: i64,
    offset: i64,
) -> Result<Vec<PendingItem>, EmbeddingError> {
    let rows = sql_query(format!(
        "{PENDING_SELECT} \
         ORDER BY (e.item_id IS NULL) DESC, s.short_code \
         LIMIT $4 OFFSET $5"
    ))
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .bind::<BigInt, _>(limit)
    .bind::<BigInt, _>(offset)
    .load::<PendingItem>(conn)?;
    Ok(rows)
}

/// The columns of a [`PendingItem`] and their joins: the one text of what
/// [`pending_primary`] and [`pending_items`] read, so the backfill and the
/// sweep compose the same text. `$1`, `$2` and `$3` are the model.
///
/// The 1:1 structural inputs rule 4 asks for. `repository` is a task-level
/// attribute, so it LEFT JOINs through `tasks` and is NULL for every other
/// type — which is correct rather than missing: a document has no
/// repository, and composing a blank `repository:` line for it would put the
/// same token into every document in the tenant. The team is not read
/// (COLLIERY-T-1840): see `kairos_core::primary`.
///
/// The parent comes from `item_relationships`, where the edge points
/// parent -> child, so the item is the TARGET and the parent is the source.
/// An archived parent counts (COLLIERY-T-1840): `entity_directory` has
/// archived rows, and the join no longer filters them out.
///
/// KAIROS-T-0630: the content is the content and then the notes as `## Note`
/// sections (the SQL function `item_notes_markdown`), so a note is embedded
/// as it was when it was part of the content.
const PENDING_SELECT: &str = "SELECT s.id, s.entity_type, s.short_code, s.title, \
                coalesce(rtrim(s.content, E' \\t\\r\\n\\f\\v') || item_notes_markdown(s.id), \
                         s.content) AS content, \
                r.slug AS repository, p.title AS parent_title, \
                e.content_hash AS stored_hash \
         FROM searchable_items s \
         LEFT JOIN item_embeddings e \
           ON e.item_id = s.id \
          AND e.provider = $1 AND e.model = $2 AND e.dimension = $3 \
         LEFT JOIN tasks t ON t.id = s.id \
         LEFT JOIN repositories r ON r.id = t.repository_id AND r.deleted_at IS NULL \
         LEFT JOIN item_relationships rel \
           ON rel.target_id = s.id AND rel.relationship = 'parent' \
         LEFT JOIN entity_directory p \
           ON p.id = rel.source_id";

/// The items `ids`, with what [`pending_primary`] reads for each
/// (KAIROS-T-0636: the items of one page of [`dirty_page`]). An id of no
/// item gives no row. No order.
pub fn pending_items(
    conn: &mut PgConnection,
    model: &StoredModel,
    ids: &[Uuid],
) -> Result<Vec<PendingItem>, EmbeddingError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sql_query(format!("{PENDING_SELECT} WHERE s.id = ANY($4)"))
        .bind::<Text, _>(&model.provider)
        .bind::<Text, _>(&model.model)
        .bind::<Integer, _>(model.dimension as i32)
        .bind::<diesel::sql_types::Array<SqlUuid>, _>(ids)
        .load::<PendingItem>(conn)?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// The marks of the sweep (KAIROS-T-0636)
// ---------------------------------------------------------------------------

/// The mark of an item whose vectors can be behind its text: a row of
/// `embedding_dirty`. `mark` is new at each write, so the sweep can tell
/// that the item changed after it read it ([`clear_dirty`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, QueryableByName)]
pub struct DirtyMark {
    /// The item.
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    /// The mark that the last write gave.
    #[diesel(sql_type = BigInt)]
    pub mark: i64,
}

/// The conflict clause of a mark: a marked item gets a new mark, the
/// microseconds of the clock and at least the old mark plus 1, and keeps its
/// place in the order (`dirty_at`).
const NEW_MARK: &str = "ON CONFLICT (item_id) DO UPDATE SET mark = greatest( \
     (extract(epoch FROM clock_timestamp()) * 1000000)::bigint, embedding_dirty.mark + 1)";

/// A `WITH` item that marks the item `$1` (KAIROS-T-0636), for a statement
/// of the write that does more: the outbox row of the item event
/// ([`crate::events::emit_item_change_by_id`]). So the mark costs no
/// statement of its own, and it is in the transaction of the write.
pub(crate) fn mark_item_cte() -> String {
    format!("embedding_mark AS (INSERT INTO embedding_dirty (item_id) VALUES ($1) {NEW_MARK})")
}

/// Mark the items that `select` gives (one column of item ids, with `$1`
/// bound to `id`). A marked item gets a new mark and keeps its place in
/// the order. The ids go in in their order, so two writes lock the rows in
/// one order.
fn mark_selected(
    conn: &mut PgConnection,
    select: &str,
    id: Uuid,
) -> Result<usize, diesel::result::Error> {
    sql_query(format!(
        "INSERT INTO embedding_dirty (item_id) \
         SELECT DISTINCT ids.item_id FROM ({select}) AS ids(item_id) ORDER BY 1 \
         {NEW_MARK}"
    ))
    .bind::<SqlUuid, _>(id)
    .execute(conn)
}

/// Mark the item `item_id`: its text, its metadata or its parent changed
/// (KAIROS-T-0636). The item event of a write marks its item in the outbox
/// statement (`mark_item_cte`); this is the same mark as a statement of
/// its own.
pub fn mark_dirty(conn: &mut PgConnection, item_id: Uuid) -> Result<(), diesel::result::Error> {
    mark_selected(conn, "SELECT $1::uuid", item_id).map(|_| ())
}

/// Mark the children of `parent_id`: the title of the parent is part of the
/// text of each child (COLLIERY-T-1840). The number marked.
pub fn mark_children_dirty(
    conn: &mut PgConnection,
    parent_id: Uuid,
) -> Result<usize, diesel::result::Error> {
    mark_selected(
        conn,
        "SELECT target_id FROM item_relationships \
         WHERE source_id = $1 AND relationship = 'parent'",
        parent_id,
    )
}

/// Mark the tasks of the repository `repository_id`: its slug is part of
/// the text of each task. The number marked.
pub fn mark_repository_dirty(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<usize, diesel::result::Error> {
    mark_selected(
        conn,
        "SELECT id FROM tasks WHERE repository_id = $1",
        repository_id,
    )
}

/// Mark the items with a value of the metadata definition `definition_id`:
/// its name is a label in the text of each ([`metadata_for`]). The number
/// marked.
pub fn mark_definition_dirty(
    conn: &mut PgConnection,
    definition_id: Uuid,
) -> Result<usize, diesel::result::Error> {
    mark_selected(
        conn,
        "SELECT item_id FROM item_metadata WHERE metadata_definition_id = $1",
        definition_id,
    )
}

/// Mark each item that has no primary vector of `model`: a new model, or an
/// item that a write outside the write path made (KAIROS-T-0636). The sweep
/// runs it once for each tenant when its server starts. One read of each
/// item; the number marked.
pub fn mark_missing(conn: &mut PgConnection, model: &StoredModel) -> Result<usize, EmbeddingError> {
    Ok(sql_query(
        "INSERT INTO embedding_dirty (item_id) \
         SELECT s.id FROM searchable_items s \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM item_embeddings e \
             WHERE e.item_id = s.id \
               AND e.provider = $1 AND e.model = $2 AND e.dimension = $3) \
         ORDER BY 1 \
         ON CONFLICT (item_id) DO NOTHING",
    )
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .execute(conn)?)
}

/// The oldest `limit` marks (KAIROS-T-0636): the page of one run of the
/// sweep. One read of `embedding_dirty`, and of nothing else, so a run with
/// no mark costs one small statement.
pub fn dirty_page(conn: &mut PgConnection, limit: i64) -> Result<Vec<DirtyMark>, EmbeddingError> {
    Ok(
        sql_query("SELECT item_id, mark FROM embedding_dirty ORDER BY dirty_at, item_id LIMIT $1")
            .bind::<BigInt, _>(limit)
            .load(conn)?,
    )
}

/// Delete each of `marks` that is still the mark of its item
/// (KAIROS-T-0636). A write after the read gave the item a new mark, so the
/// item stays dirty, and the next run reads its new text. The number
/// deleted.
pub fn clear_dirty(conn: &mut PgConnection, marks: &[DirtyMark]) -> Result<usize, EmbeddingError> {
    if marks.is_empty() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = marks.iter().map(|m| m.item_id).collect();
    let values: Vec<i64> = marks.iter().map(|m| m.mark).collect();
    Ok(sql_query(
        "DELETE FROM embedding_dirty d \
         USING unnest($1::uuid[], $2::bigint[]) AS m(item_id, mark) \
         WHERE d.item_id = m.item_id AND d.mark = m.mark",
    )
    .bind::<diesel::sql_types::Array<SqlUuid>, _>(ids)
    .bind::<diesel::sql_types::Array<BigInt>, _>(values)
    .execute(conn)?)
}

/// How many items a sweep has to walk to cover a tenant once.
pub fn item_count(conn: &mut PgConnection) -> Result<i64, EmbeddingError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }
    Ok(
        sql_query("SELECT count(*)::bigint AS count FROM searchable_items")
            .get_result::<Row>(conn)?
            .count,
    )
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
    /// The chunk's text as it stands in the content. What was embedded can
    /// be more (COLLIERY-T-1840 adds the item's title and the heading).
    pub text: &'a str,
    /// Hash of the text that was embedded for this chunk.
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

/// Forget everything stored for an item: its chunks, its vectors and its
/// mark for the sweep (`embedding_dirty`).
///
/// The purge of a task calls it (`crate::purge::purge_task`, KAIROS-T-0743). Soft-deleted items keep their vectors on
/// purpose: KAIROS-A-0020 made put-away work searchable, and prior art in
/// completed work is one of the three claims the retrieval surface exists to
/// make — dropping their vectors would make the most valuable answer the one
/// thing that cannot be found.
pub fn forget_item(conn: &mut PgConnection, item_id: Uuid) -> Result<(), diesel::result::Error> {
    conn.transaction(|conn| {
        sql_query("DELETE FROM item_chunks WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        sql_query("DELETE FROM item_embeddings WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        sql_query("DELETE FROM embedding_dirty WHERE item_id = $1")
            .bind::<SqlUuid, _>(item_id)
            .execute(conn)?;
        Ok(())
    })
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
            (SELECT count(*) FROM searchable_items) AS items, \
            (SELECT count(*) FROM item_embeddings e \
              JOIN searchable_items s ON s.id = e.item_id \
              WHERE e.provider = $1 AND e.model = $2 AND e.dimension = $3) AS embedded, \
            (SELECT count(*) FROM item_embeddings e \
              JOIN searchable_items s ON s.id = e.item_id \
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
    fn a_title_query_matches_any_of_its_words() {
        assert_eq!(
            any_word_query("broker serves the brokkr-web wasm bundle + SPA fallback").as_deref(),
            Some("broker OR serves OR the OR brokkr OR web OR wasm OR bundle OR spa OR fallback")
        );
        // Nothing of the title acts as an operator.
        assert_eq!(
            any_word_query("-not \"a phrase\" or this").as_deref(),
            Some("not OR a OR phrase OR this")
        );
        assert_eq!(any_word_query(" — + "), None);
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

/// What [`pin_and_index`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexReport {
    /// The width the columns were pinned to.
    pub dimension: usize,
    /// Whether this call changed the column types (false if already pinned).
    pub pinned: bool,
    /// Indexes created by this call, by name. Empty on a second run.
    pub created: Vec<String>,
    /// How long the whole thing took, in milliseconds — the number worth
    /// recording, because it is what an operator is deciding about when they
    /// choose a maintenance window.
    pub elapsed_ms: u128,
}

/// Pin the vector columns to `dimension` and build approximate-search indexes.
///
/// # Why this is a command and not a migration
///
/// pgvector refuses to index a column of unspecified width — `CREATE INDEX …
/// USING hnsw` on one fails with *"column does not have dimensions"* — so the
/// columns must be pinned before they can be indexed.
///
/// But the width is a **deployment** fact, not a schema constant. It is decided
/// by which provider a deployment configured: 384 for the bundled local model,
/// 1536 for OpenAI's `text-embedding-3-small`, something else again for whatever
/// an operator runs on their own GPU. A migration pinning 384 would refuse every
/// write on any deployment that brought its own endpoint — which is the option
/// [[KAIROS-A-0021]] rule 1 exists to preserve.
///
/// So this runs when a deployment knows its own answer, and the migration
/// (KAIROS-T-0187) deliberately left `vector` unmodified.
///
/// # Why it refuses rather than converting
///
/// If any stored row disagrees with `dimension`, this stops. The alternative is
/// an `ALTER` that fails halfway through a table-rewrite, or worse, succeeds
/// after silently dropping rows. Re-embedding under the configured model is the
/// fix, and it is what the backfill already does.
///
/// # Index choice
///
/// HNSW rather than IVFFlat: IVFFlat picks its lists from whatever data is
/// present when it is built, so it degrades as a tenant grows and needs
/// rebuilding; HNSW does not. Cosine ops because cosine is the measure used
/// everywhere else in this initiative — an index built for a different distance
/// would simply not be used by the query.
///
/// Idempotent: a second run reports no work.
pub fn pin_and_index(
    conn: &mut PgConnection,
    dimension: usize,
) -> Result<IndexReport, EmbeddingError> {
    let started = std::time::Instant::now();

    // Refuse before touching anything if the stored rows disagree.
    for table in ["item_embeddings", "item_chunks"] {
        let rows = sql_query(format!(
            "SELECT count(*)::bigint AS count FROM {table} WHERE dimension <> $1"
        ))
        .bind::<Integer, _>(dimension as i32)
        .get_result::<CountRow>(conn)?;
        if rows.count > 0 {
            return Err(EmbeddingError::MixedDimensions {
                table,
                wanted: dimension,
                rows: rows.count,
            });
        }
    }

    let mut report = IndexReport {
        dimension,
        pinned: false,
        created: Vec::new(),
        elapsed_ms: 0,
    };

    for (table, index) in [
        ("item_embeddings", "idx_item_embeddings_hnsw"),
        ("item_chunks", "idx_item_chunks_hnsw"),
    ] {
        if !is_pinned(conn, table, dimension)? {
            sql_query(format!(
                "ALTER TABLE {table} ALTER COLUMN embedding \
                 TYPE public.vector({dimension}) USING embedding::public.vector({dimension})"
            ))
            .execute(conn)?;
            report.pinned = true;
        }
        let existed = sql_query(
            "SELECT count(*)::bigint AS count FROM pg_indexes \
                                 WHERE schemaname = current_schema() AND indexname = $1",
        )
        .bind::<Text, _>(index)
        .get_result::<CountRow>(conn)?
        .count
            > 0;
        if !existed {
            sql_query(format!(
                "CREATE INDEX {index} ON {table} \
                 USING hnsw (embedding public.vector_cosine_ops)"
            ))
            .execute(conn)?;
            report.created.push(index.to_string());
        }
    }

    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

/// Whether `table`'s `embedding` column is already `vector(dimension)`.
fn is_pinned(
    conn: &mut PgConnection,
    table: &str,
    dimension: usize,
) -> Result<bool, EmbeddingError> {
    // `atttypmod` carries pgvector's width, offset by the usual 4-byte header
    // convention PostgreSQL uses for type modifiers; -1 means unspecified.
    let row = sql_query(
        "SELECT a.atttypmod::bigint AS count \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = current_schema() AND c.relname = $1 AND a.attname = 'embedding'",
    )
    .bind::<Text, _>(table)
    .get_result::<CountRow>(conn)?;
    Ok(row.count == dimension as i64)
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

// ---------------------------------------------------------------------------
// Retrieval (KAIROS-T-0191)
// ---------------------------------------------------------------------------

/// One neighbour of an item, as the database sees it.
///
/// Deliberately carries **ranks and facts, not similarities**. The fusion in
/// `kairos_core::retrieval` works on position, and handing it a cosine would
/// invite someone to compare one to a constant — which the measurements say
/// cannot work.
#[derive(Debug, Clone, QueryableByName)]
pub struct Neighbour {
    /// The item's id.
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    /// Its short code.
    #[diesel(sql_type = Text)]
    pub short_code: String,
    /// Its title.
    #[diesel(sql_type = Text)]
    pub title: String,
    /// Its entity type.
    #[diesel(sql_type = Text)]
    pub entity_type: String,
    /// Whether it is soft-deleted (put away).
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub archived: bool,
    /// The literal heading of the best-matching chunk, when the match came from
    /// one. Echoed for citation; never interpreted.
    #[diesel(sql_type = Nullable<Text>)]
    pub heading: Option<String>,
}

/// The nearest items by vector, excluding the item itself.
///
/// The probe is the item's **primary** vector. It is compared with the other
/// items' primary vectors **and** with their chunks, and each item appears
/// once, at its best match:
///
/// - the chunks are there because a document whose opening is unremarkable can
///   still have one section that is exactly the thing, and a chunk is what
///   lets the answer cite where it matched;
/// - the primaries are there because a probe is a summary, and the nearest
///   thing to a summary is another summary. Before COLLIERY-T-1840 only chunks
///   were compared, so an item with many chunks had many chances, and an item
///   whose sections were all long and specific had none that looked like a
///   summary.
///
/// **A short chunk cannot be the best match** (COLLIERY-T-1840). A chunk whose
/// text is below [`kairos_core::embed_text::MIN_MATCH_CHARS`] — "None.", a line
/// of codes, a template marker — is near to anything short, and it made the
/// heading of a template the reason for a proposal. Such chunks are stored, and
/// skipped here. A match on a primary vector has no heading.
///
/// Archived items are included on purpose. Prior art in finished work is one of
/// the three claims retrieval exists to make, and KAIROS-A-0020 kept put-away
/// work searchable precisely so this is possible.
///
/// Returns items in rank order. No threshold — see
/// `kairos_core::retrieval` for why there cannot be one.
pub fn vector_neighbours(
    conn: &mut PgConnection,
    item_id: Uuid,
    model: &StoredModel,
    limit: i64,
) -> Result<Vec<Neighbour>, EmbeddingError> {
    let rows = sql_query(
        "WITH probe AS ( \
             SELECT embedding FROM item_embeddings \
              WHERE item_id = $1 AND provider = $2 AND model = $3 AND dimension = $4 \
         ), \
         hits AS ( \
             SELECT e.item_id, NULL::text AS heading, \
                    (e.embedding <=> (SELECT embedding FROM probe)) AS d \
               FROM item_embeddings e \
              WHERE e.item_id <> $1 \
                AND e.provider = $2 AND e.model = $3 AND e.dimension = $4 \
                AND EXISTS (SELECT 1 FROM probe) \
             UNION ALL \
             SELECT c.item_id, c.heading, \
                    (c.embedding <=> (SELECT embedding FROM probe)) AS d \
               FROM item_chunks c \
              WHERE c.item_id <> $1 \
                AND c.provider = $2 AND c.model = $3 AND c.dimension = $4 \
                AND char_length(btrim(c.chunk_text, E' \\t\\r\\n')) >= $6 \
                AND EXISTS (SELECT 1 FROM probe) \
         ), \
         best AS ( \
             SELECT item_id, heading, d, \
                    row_number() OVER (PARTITION BY item_id ORDER BY d, heading NULLS FIRST) AS rn \
               FROM hits \
         ) \
         SELECT d.id, d.short_code, d.entity_type, d.title, \
                (d.deleted_at IS NOT NULL) AS archived, b.heading \
           FROM best b \
           JOIN entity_directory d ON d.id = b.item_id \
          WHERE b.rn = 1 \
          ORDER BY b.d, d.short_code \
          LIMIT $5",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(&model.provider)
    .bind::<Text, _>(&model.model)
    .bind::<Integer, _>(model.dimension as i32)
    .bind::<BigInt, _>(limit)
    .bind::<Integer, _>(kairos_core::embed_text::MIN_MATCH_CHARS as i32)
    .load::<Neighbour>(conn)?;
    Ok(rows)
}

/// The nearest items by text, excluding the item itself.
///
/// The fallback everything degrades to (KAIROS-A-0021 rule 7), and half the
/// hybrid when it does not. Uses the same weighted `tsv` the unified search
/// ranks on (KAIROS-T-0186), so a title match outranks a passing mention here
/// too.
///
/// `title` is the item's title, and an item matches when it has **any** of its
/// words ([`any_word_query`]); `ts_rank_cd` puts the items with more of them,
/// and closer together, first. Before COLLIERY-T-1840 the title went to
/// `websearch_to_tsquery` as it was, which needs **each** word: for the twelve
/// words of COLLIERY-T-1757 no item matched, and in a sample of 120 items 31%
/// got no row at all. The answer then came from the vectors alone while it said
/// "Ranked across text and meaning".
pub fn lexical_neighbours(
    conn: &mut PgConnection,
    item_id: Uuid,
    title: &str,
    limit: i64,
) -> Result<Vec<Neighbour>, EmbeddingError> {
    let Some(query) = any_word_query(title) else {
        return Ok(Vec::new());
    };
    let rows = sql_query(
        "SELECT s.id, s.short_code, s.entity_type, s.title, \
                (s.deleted_at IS NOT NULL) AS archived, NULL::text AS heading \
           FROM searchable_items s \
          WHERE s.id <> $1 \
            AND s.tsv @@ websearch_to_tsquery('english', $2) \
          ORDER BY ts_rank_cd(s.tsv, websearch_to_tsquery('english', $2)) DESC, s.short_code \
          LIMIT $3",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(&query)
    .bind::<BigInt, _>(limit)
    .load::<Neighbour>(conn)?;
    Ok(rows)
}

/// A neighbour by text with its text score (KAIROS-T-0562).
#[derive(Debug, Clone, QueryableByName)]
pub struct ScoredNeighbour {
    /// The item.
    #[diesel(embed)]
    pub neighbour: Neighbour,
    /// The share of the title's word weight that the item matches, 0 to 1.
    #[diesel(sql_type = diesel::sql_types::Double)]
    pub text_score: f64,
}

/// The nearest items by text, each with a text score, for an answer with no
/// vectors (KAIROS-T-0562).
///
/// The words are the lexemes of `title`. Each word weighs by how rare it is
/// in the tenant: `ln((N + 1) / (df + 0.5))`, where `N` counts the items and
/// `df` the items that have the word. A word in most items weighs almost
/// nothing. An item gets the full weight of a word in its title (weight A of
/// `item_search.tsv`) and half the weight of a word only in its other text.
/// The score is the sum of these, divided by the weight of all the words. So
/// a match on common words alone gives a low score, and the same title gives
/// 1.
///
/// An item is a candidate when it has any of the words. The rows come best
/// first, then by short code. The caller applies the floor and the limit
/// (`kairos_core::retrieval::propose`). Archived items are included, as in
/// [`lexical_neighbours`].
pub fn text_neighbours(
    conn: &mut PgConnection,
    item_id: Uuid,
    title: &str,
    limit: i64,
) -> Result<Vec<ScoredNeighbour>, EmbeddingError> {
    // A lexeme goes into a tsquery as a quoted literal. A lexeme with a quote
    // or a backslash is left out, so that no lexeme acts as tsquery syntax.
    let rows = sql_query(
        "WITH words AS ( \
             SELECT DISTINCT w \
               FROM unnest(tsvector_to_array(to_tsvector('english', $2))) AS w \
              WHERE w !~ '[''\\\\]' \
         ), \
         total AS (SELECT count(*)::float8 AS n FROM item_search), \
         weighted AS ( \
             SELECT quote_literal(w)::tsquery AS q, \
                    (quote_literal(w) || ':A')::tsquery AS qa, \
                    ln(((SELECT n FROM total) + 1) / \
                       ((SELECT count(*) FROM item_search s \
                          WHERE s.tsv @@ quote_literal(w)::tsquery) + 0.5)) AS idf \
               FROM words \
         ), \
         any_word AS ( \
             SELECT string_agg(quote_literal(w), ' | ')::tsquery AS q FROM words \
         ), \
         scored AS ( \
             SELECT s.item_id, \
                    sum(CASE WHEN s.tsv @@ x.qa THEN x.idf \
                             WHEN s.tsv @@ x.q THEN x.idf * 0.5 \
                             ELSE 0 END) \
                    / NULLIF((SELECT sum(idf) FROM weighted), 0) AS score \
               FROM item_search s CROSS JOIN weighted x \
              WHERE s.item_id <> $1 \
                AND s.tsv @@ (SELECT q FROM any_word) \
              GROUP BY s.item_id \
         ) \
         SELECT d.id, d.short_code, d.entity_type, d.title, \
                (d.deleted_at IS NOT NULL) AS archived, NULL::text AS heading, \
                COALESCE(sc.score, 0)::float8 AS text_score \
           FROM scored sc \
           JOIN entity_directory d ON d.id = sc.item_id \
          ORDER BY sc.score DESC NULLS LAST, d.short_code \
          LIMIT $3",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(title)
    .bind::<BigInt, _>(limit)
    .load::<ScoredNeighbour>(conn)?;
    Ok(rows)
}

/// A `websearch_to_tsquery` query that matches any word of `title`.
///
/// The words are the runs of letters and digits, joined by `OR`. So nothing of
/// the title can act as an operator: a `-` that would negate, a `"` that would
/// start a phrase, and the words `or` and `and` (stop words, which the English
/// configuration drops anyway) are gone. `None` when the title has no word.
pub fn any_word_query(title: &str) -> Option<String> {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| w != "or" && w != "and")
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

/// What the graph says about each of `others`, relative to `item_id`.
///
/// Three facts, not a path length: a direct edge, a shared parent, the same
/// repository or board. `item_relationships` is sparse — 18 rows in the seeded
/// tenant — so a depth-bounded traversal would report "no path at depth 4" and
/// sound like a finding while meaning almost nothing. Checking only what the
/// claims actually rest on is the version that can be described honestly.
///
/// This is also why KAIROS-A-0021 defers Apache AGE: nothing here needs a graph
/// engine.
pub fn graph_facts(
    conn: &mut PgConnection,
    item_id: Uuid,
    others: &[Uuid],
) -> Result<Vec<GraphFacts>, EmbeddingError> {
    if others.is_empty() {
        return Ok(Vec::new());
    }
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        other: Uuid,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        directly_linked: bool,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        shared_parent: bool,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        same_repository: bool,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        same_board: bool,
    }
    let rows = sql_query(
        "WITH me AS ( \
            SELECT t.repository_id, d.board_id \
              FROM entity_directory d \
              LEFT JOIN tasks t ON t.id = d.id \
             WHERE d.id = $1 \
         ), \
         my_parents AS ( \
            SELECT source_id FROM item_relationships \
             WHERE target_id = $1 AND relationship = 'parent' \
         ) \
         SELECT d.id AS other, \
                EXISTS ( \
                    SELECT 1 FROM item_relationships r \
                     WHERE (r.source_id = $1 AND r.target_id = d.id) \
                        OR (r.source_id = d.id AND r.target_id = $1) \
                ) AS directly_linked, \
                EXISTS ( \
                    SELECT 1 FROM item_relationships r \
                     WHERE r.target_id = d.id AND r.relationship = 'parent' \
                       AND r.source_id IN (SELECT source_id FROM my_parents) \
                ) AS shared_parent, \
                COALESCE(t.repository_id IS NOT NULL \
                         AND t.repository_id = (SELECT repository_id FROM me), false) \
                    AS same_repository, \
                COALESCE(d.board_id IS NOT NULL \
                         AND d.board_id = (SELECT board_id FROM me), false) AS same_board \
           FROM entity_directory d \
           LEFT JOIN tasks t ON t.id = d.id \
          WHERE d.id = ANY($2)",
    )
    .bind::<SqlUuid, _>(item_id)
    .bind::<diesel::sql_types::Array<SqlUuid>, _>(others)
    .load::<Row>(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| GraphFacts {
            item_id: r.other,
            directly_linked: r.directly_linked,
            shared_parent: r.shared_parent,
            same_repository: r.same_repository,
            same_board: r.same_board,
        })
        .collect())
}

/// What the graph knows about one candidate, relative to the item asked about.
///
/// Three facts and two priors, not a path length — see [`graph_facts`] for why
/// a traversal would sound more informative than it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphFacts {
    /// The candidate.
    pub item_id: Uuid,
    /// An edge joins them, in either direction.
    pub directly_linked: bool,
    /// They hang off the same parent.
    pub shared_parent: bool,
    /// Same repository (KAIROS-A-0019: a strong prior for real coupling).
    pub same_repository: bool,
    /// Same board.
    pub same_board: bool,
}

/// Resolve a short code to the item it names, live or archived.
///
/// Archived is deliberate: asking what relates to a piece of finished work is a
/// reasonable question, and refusing it would make the audit view a second-class
/// one (KAIROS-A-0020).
pub fn item_by_short_code(
    conn: &mut PgConnection,
    short_code: &str,
) -> Result<Neighbour, EmbeddingError> {
    sql_query(
        "SELECT id, short_code, entity_type, title, \
                (deleted_at IS NOT NULL) AS archived, NULL::text AS heading \
           FROM entity_directory WHERE short_code = $1",
    )
    .bind::<Text, _>(short_code)
    .get_result::<Neighbour>(conn)
    .map_err(|e| match e {
        diesel::result::Error::NotFound => EmbeddingError::ItemNotFound(short_code.to_string()),
        other => EmbeddingError::Database(other),
    })
}

/// Resolve an item id to its directory row.
///
/// Used to turn a proposal's two ids back into short codes: a person deciding
/// whether two things are related needs to see what they are, and a UUID pair
/// tells them nothing.
pub fn item_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Neighbour, EmbeddingError> {
    sql_query(
        "SELECT id, short_code, entity_type, title, \
                (deleted_at IS NOT NULL) AS archived, NULL::text AS heading \
           FROM entity_directory WHERE id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result::<Neighbour>(conn)
    .map_err(|e| match e {
        diesel::result::Error::NotFound => EmbeddingError::ItemNotFound(id.to_string()),
        other => EmbeddingError::Database(other),
    })
}
