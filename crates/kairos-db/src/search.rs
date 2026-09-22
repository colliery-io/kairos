//! The unified search/traverse pipeline (KAIROS-A-0007, KAIROS-T-0014):
//! `execute_search` runs a validated [`SearchRequest`] through the A-0007
//! execution order and returns fully typed results grouped by entity type.
//!
//! The pure request model and validation live in [`kairos_core::search`];
//! this module is the SQL side of the contract. Per KAIROS-A-0009 the
//! recursive-CTE traversal, the `searchable_items` query, and the metadata
//! pre-pass are sanctioned `diesel::sql_query` raw SQL; per-type hydration
//! is typed diesel.
//!
//! # Pipeline (A-0007 execution order)
//!
//! 1. **Traverse** (if present): resolve the root through
//!    `entity_directory` (unknown or soft-deleted roots are the typed
//!    [`SearchError::TraverseRootNotFound`]), then walk
//!    `item_relationships` with a recursive CTE — parameterized
//!    relationship types, direction (`outbound`/`inbound`/`both`), and
//!    depth. Cycle safety: `UNION` deduplicates `(id, depth)` rows and
//!    depth strictly increases toward the validated cap
//!    ([`kairos_core::search::MAX_TRAVERSE_DEPTH`]), so the walk always
//!    terminates; the root itself is excluded from the results. The walk
//!    runs over the raw edge graph — edges through soft-deleted items are
//!    followed (their live descendants stay reachable); visibility is
//!    enforced at type resolution and hydration.
//! 2. **Full-text `q`** (if present): match ids via the `searchable_items`
//!    view using `websearch_to_tsquery('english', $q)` — chosen over
//!    `plainto_tsquery` because it accepts arbitrary end-user input without
//!    ever erroring and supports quoted phrases, `OR`, and `-negation`.
//!    The view is the search surface (S-0004) and filters `deleted_at IS
//!    NULL`, so full-text search only ever matches live items — even with
//!    `include_deleted` (which governs filter/traverse/hydration
//!    visibility, not the text index).
//! 3. **Metadata filter** (if present): one pre-pass query over
//!    `item_metadata`/`metadata_definitions` (pairs unnested server-side)
//!    returning ids that satisfy EVERY entry; values use the T-0011 LIKE
//!    translation ([`kairos_core::search::metadata_like_pattern`]).
//! 4. **Candidate set**: the intersection of the sets above (whichever are
//!    present). No set ⇒ pure structural filtering.
//! 5. **Type resolution + partition**: candidates resolve to `(id,
//!    entity_type)` via `entity_directory`; with `include_deleted` the same
//!    UNION shape runs against the base tables without the `deleted_at`
//!    filter (the view would silently drop deleted candidates).
//! 6. **Hydrate**: at most one typed `SELECT … WHERE id = ANY(…)` per
//!    entity type — **bounded at 5 queries regardless of result size**
//!    ([`SearchStats::hydration_queries`] proves it) — carrying the
//!    structural filter predicates (`board_id`, `column_id`, `team_id`,
//!    `task_type`, `is_bucket`, `created_after`/`before`) and the
//!    `deleted_at IS NULL` default. Filters on attributes a type does not
//!    have exclude the type outright (e.g. `task_type` ⇒ tasks only,
//!    `board_id` ⇒ no documents), so those types cost no query at all.
//! 7. **Sort, count, paginate**: the combined cross-type rows are sorted
//!    in memory (`created_at`/`updated_at`/`title`, tie-broken by
//!    `short_code` for determinism), `total` is counted **before**
//!    `limit`/`offset` are applied, and the page is regrouped by type.
//!    Pagination over a mixed-type result set is applied in memory over the
//!    hydrated rows (A-0007's combined-result-set semantics); the ≤5-query
//!    bound holds regardless.
//!
//! Every public function operates in the CURRENT `search_path` tenant
//! schema (same conventions as [`crate::items`]); the pipeline is
//! read-only, so no transaction is opened.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Integer, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::search::{
    self as core_search, Direction, SearchFilter, SearchRequest, SearchTaskType,
    SearchValidationError, SearchWorkClass, Sort, SortField, SortOrder, Traverse, TraverseFrom,
};
use kairos_core::short_code::ItemType;

use crate::models::enums::{TaskType, UnknownEnumValue, WorkClass};
use crate::models::items::{Adr, Document, Initiative, Strategy, Task};

/// Errors from the search pipeline.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// The request failed [`kairos_core::search::validate`] (HTTP 400).
    #[error(transparent)]
    Invalid(#[from] SearchValidationError),
    /// `traverse.from` names no live entity (unknown, or soft-deleted —
    /// roots resolve through `entity_directory`).
    #[error("traverse root {reference} does not exist")]
    TraverseRootNotFound {
        /// The submitted `short_code` or `id`, for the error envelope.
        reference: String,
    },
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// Search results grouped by entity type (A-0007 response shape), each
/// group fully typed — no lossy common-denominator projection. Group order
/// within each vector follows the request's combined sort.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResults {
    /// Matching strategies.
    pub strategies: Vec<Strategy>,
    /// Matching initiatives.
    pub initiatives: Vec<Initiative>,
    /// Matching tasks.
    pub tasks: Vec<Task>,
    /// Matching documents.
    pub documents: Vec<Document>,
    /// Matching ADRs.
    pub adrs: Vec<Adr>,
    /// Total matches BEFORE pagination.
    pub total: i64,
    /// The applied page size.
    pub limit: i64,
    /// The applied offset.
    pub offset: i64,
}

/// Query-count instrumentation for one `execute_search` run — the proof
/// behind A-0007's "bounded at 5 hydration queries regardless of result
/// size".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchStats {
    /// Per-type hydration queries executed (step 6) — always `<= 5`.
    pub hydration_queries: usize,
    /// Every query executed by the pipeline, hydration included.
    pub total_queries: usize,
}

/// Run the A-0007 search pipeline (module docs) for `request` in the
/// current tenant schema.
pub fn execute_search(
    conn: &mut PgConnection,
    request: &SearchRequest,
) -> Result<SearchResults, SearchError> {
    execute_search_with_stats(conn, request).map(|(results, _)| results)
}

/// [`execute_search`], also returning the [`SearchStats`] query counters
/// (integration tests assert the ≤5-hydration-query bound through this).
pub fn execute_search_with_stats(
    conn: &mut PgConnection,
    request: &SearchRequest,
) -> Result<(SearchResults, SearchStats), SearchError> {
    core_search::validate(request)?;
    let mut stats = SearchStats::default();

    let filter = request.filter.as_ref();
    let include_deleted = filter.is_some_and(|f| f.include_deleted);
    let limit = request.effective_limit();
    let offset = request.effective_offset();

    // -- steps 1-4: candidate id sets ---------------------------------------
    let mut candidates: Option<HashSet<Uuid>> = None;
    let intersect = |candidates: &mut Option<HashSet<Uuid>>, ids: HashSet<Uuid>| {
        *candidates = Some(match candidates.take() {
            Some(existing) => existing.intersection(&ids).copied().collect(),
            None => ids,
        });
    };

    if let Some(traverse) = &request.traverse {
        let root = resolve_root(conn, &traverse.from, &mut stats)?;
        let ids = traverse_ids(conn, root, traverse, &mut stats)?;
        intersect(&mut candidates, ids);
    }
    if let Some(q) = &request.q {
        let ids = text_match_ids(conn, q, &mut stats)?;
        intersect(&mut candidates, ids);
    }
    if let Some(metadata) = filter
        .and_then(|f| f.metadata.as_ref())
        .filter(|m| !m.is_empty())
    {
        let ids = metadata_match_ids(conn, metadata, &mut stats)?;
        intersect(&mut candidates, ids);
    }

    // -- step 5: type resolution + partition ---------------------------------
    let types = applicable_types(filter);
    let per_type: HashMap<ItemType, Option<Vec<Uuid>>> = match candidates {
        Some(ids) => {
            let mut partitioned = partition_by_type(conn, &ids, include_deleted, &mut stats)?;
            types
                .iter()
                .filter_map(|t| partitioned.remove(t).map(|ids| (*t, Some(ids))))
                .collect()
        }
        // No candidate restriction: every applicable type, unrestricted.
        None => types.iter().map(|t| (*t, None)).collect(),
    };

    // -- step 6: bounded per-type hydration ----------------------------------
    let mut combined: Vec<AnyItem> = Vec::new();
    for (item_type, ids) in per_type {
        match item_type {
            ItemType::Strategy => combined.extend(
                hydrate_strategies(conn, ids, filter, &mut stats)?
                    .into_iter()
                    .map(AnyItem::Strategy),
            ),
            ItemType::Initiative => combined.extend(
                hydrate_initiatives(conn, ids, filter, &mut stats)?
                    .into_iter()
                    .map(AnyItem::Initiative),
            ),
            ItemType::Task => combined.extend(
                hydrate_tasks(conn, ids, filter, &mut stats)?
                    .into_iter()
                    .map(AnyItem::Task),
            ),
            ItemType::Document => combined.extend(
                hydrate_documents(conn, ids, filter, &mut stats)?
                    .into_iter()
                    .map(AnyItem::Document),
            ),
            ItemType::Adr => combined.extend(
                hydrate_adrs(conn, ids, filter, &mut stats)?
                    .into_iter()
                    .map(AnyItem::Adr),
            ),
        }
    }

    // -- step 7: sort, count, paginate, regroup by type -----------------------
    sort_items(&mut combined, request.effective_sort());
    let total = combined.len() as i64;

    let mut results = SearchResults {
        total,
        limit,
        offset,
        ..Default::default()
    };
    for item in combined
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
    {
        match item {
            AnyItem::Strategy(row) => results.strategies.push(row),
            AnyItem::Initiative(row) => results.initiatives.push(row),
            AnyItem::Task(row) => results.tasks.push(row),
            AnyItem::Document(row) => results.documents.push(row),
            AnyItem::Adr(row) => results.adrs.push(row),
        }
    }
    Ok((results, stats))
}

// ---------------------------------------------------------------------------
// Steps 1-3: candidate id sets (sanctioned raw SQL, A-0009)
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// Resolve `traverse.from` to a live entity id via `entity_directory`.
fn resolve_root(
    conn: &mut PgConnection,
    from: &TraverseFrom,
    stats: &mut SearchStats,
) -> Result<Uuid, SearchError> {
    stats.total_queries += 1;
    let row: Option<IdRow> = match (&from.short_code, from.id) {
        (Some(short_code), None) => {
            sql_query("SELECT id FROM entity_directory WHERE short_code = $1")
                .bind::<Text, _>(short_code)
                .get_result(conn)
                .optional()?
        }
        (None, Some(id)) => sql_query("SELECT id FROM entity_directory WHERE id = $1")
            .bind::<SqlUuid, _>(id)
            .get_result(conn)
            .optional()?,
        // validate() enforces exactly-one before we get here.
        _ => unreachable!("validated traverse.from names exactly one reference"),
    };
    row.map(|r| r.id).ok_or_else(|| {
        let reference = from
            .short_code
            .clone()
            .unwrap_or_else(|| from.id.map(|id| id.to_string()).unwrap_or_default());
        SearchError::TraverseRootNotFound { reference }
    })
}

/// Recursive-CTE walk from `root` (module docs, step 1). The `UNION`
/// deduplicates `(id, depth)` rows and `w.depth < $3` bounds the recursion
/// at the validated depth, so cycles cannot loop it; `id <> $1` keeps the
/// root out even if a cycle re-reaches it.
fn traverse_ids(
    conn: &mut PgConnection,
    root: Uuid,
    traverse: &Traverse,
    stats: &mut SearchStats,
) -> Result<HashSet<Uuid>, SearchError> {
    let step = match traverse.direction {
        Direction::Outbound => {
            "SELECT r.target_id, w.depth + 1
             FROM walk w
             JOIN item_relationships r ON r.source_id = w.id
             WHERE r.relationship = ANY($2) AND w.depth < $3"
        }
        Direction::Inbound => {
            "SELECT r.source_id, w.depth + 1
             FROM walk w
             JOIN item_relationships r ON r.target_id = w.id
             WHERE r.relationship = ANY($2) AND w.depth < $3"
        }
        Direction::Both => {
            "SELECT CASE WHEN r.source_id = w.id THEN r.target_id ELSE r.source_id END,
                    w.depth + 1
             FROM walk w
             JOIN item_relationships r ON (r.source_id = w.id OR r.target_id = w.id)
             WHERE r.relationship = ANY($2) AND w.depth < $3"
        }
    };
    let sql = format!(
        "WITH RECURSIVE walk(id, depth) AS (
             SELECT $1::uuid, 0
             UNION
             {step}
         )
         SELECT DISTINCT id FROM walk WHERE depth > 0 AND id <> $1"
    );
    let relationships: Vec<String> = traverse
        .relationships
        .iter()
        .map(|r| r.as_str().to_string())
        .collect();
    let depth = traverse.depth.expect("validated traverse carries a depth");

    stats.total_queries += 1;
    let rows: Vec<IdRow> = sql_query(sql)
        .bind::<SqlUuid, _>(root)
        .bind::<Array<Text>, _>(relationships)
        .bind::<Integer, _>(depth as i32)
        .load(conn)?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Full-text match via the `searchable_items` view (module docs, step 2).
fn text_match_ids(
    conn: &mut PgConnection,
    q: &str,
    stats: &mut SearchStats,
) -> Result<HashSet<Uuid>, SearchError> {
    stats.total_queries += 1;
    let rows: Vec<IdRow> = sql_query(
        "SELECT id FROM searchable_items WHERE tsv @@ websearch_to_tsquery('english', $1)",
    )
    .bind::<Text, _>(q)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Ids satisfying EVERY metadata entry (module docs, step 3): one query,
/// pairs unnested server-side, `HAVING` requiring all keys matched. Values
/// go through the T-0011 LIKE translation; PostgreSQL's default LIKE escape
/// (`\`) applies.
fn metadata_match_ids(
    conn: &mut PgConnection,
    metadata: &std::collections::BTreeMap<String, String>,
    stats: &mut SearchStats,
) -> Result<HashSet<Uuid>, SearchError> {
    let slugs: Vec<String> = metadata.keys().cloned().collect();
    let patterns: Vec<String> = metadata
        .values()
        .map(|value| core_search::metadata_like_pattern(value))
        .collect();

    stats.total_queries += 1;
    let rows: Vec<IdRow> = sql_query(
        "SELECT im.item_id AS id
         FROM item_metadata im
         JOIN metadata_definitions md ON md.id = im.metadata_definition_id
         JOIN unnest($1::text[], $2::text[]) AS m(slug, pattern)
           ON md.slug = m.slug AND im.value LIKE m.pattern
         GROUP BY im.item_id
         HAVING COUNT(DISTINCT md.slug) = $3",
    )
    .bind::<Array<Text>, _>(slugs)
    .bind::<Array<Text>, _>(patterns)
    .bind::<BigInt, _>(metadata.len() as i64)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

// ---------------------------------------------------------------------------
// Step 5: type resolution + partition
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct TypedIdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    entity_type: String,
}

/// Parse an `entity_type` literal from type resolution. Only the five
/// S-0004 values exist; anything else is schema drift and fails loudly
/// (same convention as `crate::graph`).
fn parse_entity_type(value: &str) -> Result<ItemType, SearchError> {
    ItemType::ALL
        .iter()
        .copied()
        .find(|t| t.entity_type() == value)
        .ok_or_else(|| {
            SearchError::Database(DieselError::DeserializationError(Box::new(
                UnknownEnumValue {
                    enum_name: "ItemType",
                    value: value.to_string(),
                },
            )))
        })
}

/// Resolve candidate ids to `(id, entity_type)` and partition by type
/// (module docs, step 5). Live-only via `entity_directory`; with
/// `include_deleted` the same UNION shape runs directly against the base
/// tables so soft-deleted candidates keep their type.
fn partition_by_type(
    conn: &mut PgConnection,
    ids: &HashSet<Uuid>,
    include_deleted: bool,
    stats: &mut SearchStats,
) -> Result<HashMap<ItemType, Vec<Uuid>>, SearchError> {
    let mut partitioned: HashMap<ItemType, Vec<Uuid>> = HashMap::new();
    if ids.is_empty() {
        return Ok(partitioned);
    }

    let sql = if include_deleted {
        "SELECT id, 'strategy' AS entity_type FROM strategies WHERE id = ANY($1)
         UNION ALL
         SELECT id, 'initiative' FROM initiatives WHERE id = ANY($1)
         UNION ALL
         SELECT id, 'task' FROM tasks WHERE id = ANY($1)
         UNION ALL
         SELECT id, 'document' FROM documents WHERE id = ANY($1)
         UNION ALL
         SELECT id, 'adr' FROM adrs WHERE id = ANY($1)"
    } else {
        "SELECT id, entity_type FROM entity_directory WHERE id = ANY($1)"
    };

    stats.total_queries += 1;
    let rows: Vec<TypedIdRow> = sql_query(sql)
        .bind::<Array<SqlUuid>, _>(ids.iter().copied().collect::<Vec<_>>())
        .load(conn)?;
    for row in rows {
        partitioned
            .entry(parse_entity_type(&row.entity_type)?)
            .or_default()
            .push(row.id);
    }
    Ok(partitioned)
}

/// Which entity types can match the structural filter at all (module docs,
/// step 6): the `entity_type` restriction intersected with the types that
/// carry every filtered attribute. Types ruled out here cost no hydration
/// query.
fn applicable_types(filter: Option<&SearchFilter>) -> Vec<ItemType> {
    let mut types: Vec<ItemType> = match filter.and_then(|f| f.entity_type.as_ref()) {
        Some(requested) => {
            let set: HashSet<ItemType> = requested.iter().map(|t| t.item_type()).collect();
            ItemType::ALL
                .iter()
                .copied()
                .filter(|t| set.contains(t))
                .collect()
        }
        None => ItemType::ALL.to_vec(),
    };
    if let Some(filter) = filter {
        if filter.board_id.is_some() || filter.column_id.is_some() {
            // Documents do not live on boards.
            types.retain(|t| *t != ItemType::Document);
        }
        if filter.team_id.is_some() || filter.task_type.is_some() || filter.work_class.is_some() {
            // team_id, task_type, and work_class are task-level attributes.
            types.retain(|t| *t == ItemType::Task);
        }
        if filter.is_bucket.is_some() {
            // is_bucket is an initiative-level attribute.
            types.retain(|t| *t == ItemType::Initiative);
        }
    }
    types
}

// ---------------------------------------------------------------------------
// Step 6: typed per-type hydration (≤5 queries by construction — these five
// functions are the only query sites that increment `hydration_queries`,
// and each runs at most once per execute_search)
// ---------------------------------------------------------------------------

/// The stored counterpart of a [`SearchTaskType`].
fn model_task_type(task_type: SearchTaskType) -> TaskType {
    match task_type {
        SearchTaskType::Task => TaskType::Task,
        SearchTaskType::Bug => TaskType::Bug,
        SearchTaskType::TechDebt => TaskType::TechDebt,
        SearchTaskType::Support => TaskType::Support,
    }
}

/// The stored counterpart of a [`SearchWorkClass`] (KAIROS-T-0077).
fn model_work_class(work_class: SearchWorkClass) -> WorkClass {
    match work_class {
        SearchWorkClass::Planned => WorkClass::Planned,
        SearchWorkClass::Support => WorkClass::Support,
    }
}

fn hydrate_strategies(
    conn: &mut PgConnection,
    ids: Option<Vec<Uuid>>,
    filter: Option<&SearchFilter>,
    stats: &mut SearchStats,
) -> Result<Vec<Strategy>, SearchError> {
    use crate::schema::strategies::dsl;

    let mut query = dsl::strategies.select(Strategy::as_select()).into_boxed();
    if let Some(ids) = ids {
        query = query.filter(dsl::id.eq_any(ids));
    }
    if let Some(filter) = filter {
        if !filter.include_deleted {
            query = query.filter(dsl::deleted_at.is_null());
        }
        if let Some(board_id) = filter.board_id {
            query = query.filter(dsl::board_id.eq(board_id));
        }
        if let Some(column_id) = filter.column_id {
            query = query.filter(dsl::column_id.eq(column_id));
        }
        if let Some(after) = filter.created_after {
            query = query.filter(dsl::created_at.gt(after));
        }
        if let Some(before) = filter.created_before {
            query = query.filter(dsl::created_at.lt(before));
        }
    } else {
        query = query.filter(dsl::deleted_at.is_null());
    }
    stats.hydration_queries += 1;
    stats.total_queries += 1;
    Ok(query.load(conn)?)
}

fn hydrate_initiatives(
    conn: &mut PgConnection,
    ids: Option<Vec<Uuid>>,
    filter: Option<&SearchFilter>,
    stats: &mut SearchStats,
) -> Result<Vec<Initiative>, SearchError> {
    use crate::schema::initiatives::dsl;

    let mut query = dsl::initiatives
        .select(Initiative::as_select())
        .into_boxed();
    if let Some(ids) = ids {
        query = query.filter(dsl::id.eq_any(ids));
    }
    if let Some(filter) = filter {
        if !filter.include_deleted {
            query = query.filter(dsl::deleted_at.is_null());
        }
        if let Some(board_id) = filter.board_id {
            query = query.filter(dsl::board_id.eq(board_id));
        }
        if let Some(column_id) = filter.column_id {
            query = query.filter(dsl::column_id.eq(column_id));
        }
        if let Some(is_bucket) = filter.is_bucket {
            query = query.filter(dsl::is_bucket.eq(is_bucket));
        }
        if let Some(after) = filter.created_after {
            query = query.filter(dsl::created_at.gt(after));
        }
        if let Some(before) = filter.created_before {
            query = query.filter(dsl::created_at.lt(before));
        }
    } else {
        query = query.filter(dsl::deleted_at.is_null());
    }
    stats.hydration_queries += 1;
    stats.total_queries += 1;
    Ok(query.load(conn)?)
}

fn hydrate_tasks(
    conn: &mut PgConnection,
    ids: Option<Vec<Uuid>>,
    filter: Option<&SearchFilter>,
    stats: &mut SearchStats,
) -> Result<Vec<Task>, SearchError> {
    use crate::schema::tasks::dsl;

    let mut query = dsl::tasks.select(Task::as_select()).into_boxed();
    if let Some(ids) = ids {
        query = query.filter(dsl::id.eq_any(ids));
    }
    if let Some(filter) = filter {
        if !filter.include_deleted {
            query = query.filter(dsl::deleted_at.is_null());
        }
        if let Some(board_id) = filter.board_id {
            query = query.filter(dsl::board_id.eq(board_id));
        }
        if let Some(column_id) = filter.column_id {
            query = query.filter(dsl::column_id.eq(column_id));
        }
        if let Some(team_id) = filter.team_id {
            query = query.filter(dsl::team_id.eq(team_id));
        }
        if let Some(task_types) = &filter.task_type {
            let stored: Vec<TaskType> = task_types.iter().copied().map(model_task_type).collect();
            query = query.filter(dsl::task_type.eq_any(stored));
        }
        if let Some(work_classes) = &filter.work_class {
            let stored: Vec<WorkClass> =
                work_classes.iter().copied().map(model_work_class).collect();
            query = query.filter(dsl::work_class.eq_any(stored));
        }
        if let Some(after) = filter.created_after {
            query = query.filter(dsl::created_at.gt(after));
        }
        if let Some(before) = filter.created_before {
            query = query.filter(dsl::created_at.lt(before));
        }
    } else {
        query = query.filter(dsl::deleted_at.is_null());
    }
    stats.hydration_queries += 1;
    stats.total_queries += 1;
    Ok(query.load(conn)?)
}

fn hydrate_documents(
    conn: &mut PgConnection,
    ids: Option<Vec<Uuid>>,
    filter: Option<&SearchFilter>,
    stats: &mut SearchStats,
) -> Result<Vec<Document>, SearchError> {
    use crate::schema::documents::dsl;

    let mut query = dsl::documents.select(Document::as_select()).into_boxed();
    if let Some(ids) = ids {
        query = query.filter(dsl::id.eq_any(ids));
    }
    if let Some(filter) = filter {
        if !filter.include_deleted {
            query = query.filter(dsl::deleted_at.is_null());
        }
        if let Some(after) = filter.created_after {
            query = query.filter(dsl::created_at.gt(after));
        }
        if let Some(before) = filter.created_before {
            query = query.filter(dsl::created_at.lt(before));
        }
    } else {
        query = query.filter(dsl::deleted_at.is_null());
    }
    stats.hydration_queries += 1;
    stats.total_queries += 1;
    Ok(query.load(conn)?)
}

fn hydrate_adrs(
    conn: &mut PgConnection,
    ids: Option<Vec<Uuid>>,
    filter: Option<&SearchFilter>,
    stats: &mut SearchStats,
) -> Result<Vec<Adr>, SearchError> {
    use crate::schema::adrs::dsl;

    let mut query = dsl::adrs.select(Adr::as_select()).into_boxed();
    if let Some(ids) = ids {
        query = query.filter(dsl::id.eq_any(ids));
    }
    if let Some(filter) = filter {
        if !filter.include_deleted {
            query = query.filter(dsl::deleted_at.is_null());
        }
        if let Some(board_id) = filter.board_id {
            query = query.filter(dsl::board_id.eq(board_id));
        }
        if let Some(column_id) = filter.column_id {
            query = query.filter(dsl::column_id.eq(column_id));
        }
        if let Some(after) = filter.created_after {
            query = query.filter(dsl::created_at.gt(after));
        }
        if let Some(before) = filter.created_before {
            query = query.filter(dsl::created_at.lt(before));
        }
    } else {
        query = query.filter(dsl::deleted_at.is_null());
    }
    stats.hydration_queries += 1;
    stats.total_queries += 1;
    Ok(query.load(conn)?)
}

// ---------------------------------------------------------------------------
// Step 7: combined sort + pagination
// ---------------------------------------------------------------------------

/// One hydrated row of any entity type, for the combined sort.
enum AnyItem {
    Strategy(Strategy),
    Initiative(Initiative),
    Task(Task),
    Document(Document),
    Adr(Adr),
}

impl AnyItem {
    fn created_at(&self) -> DateTime<Utc> {
        match self {
            AnyItem::Strategy(row) => row.created_at,
            AnyItem::Initiative(row) => row.created_at,
            AnyItem::Task(row) => row.created_at,
            AnyItem::Document(row) => row.created_at,
            AnyItem::Adr(row) => row.created_at,
        }
    }

    fn updated_at(&self) -> DateTime<Utc> {
        match self {
            AnyItem::Strategy(row) => row.updated_at,
            AnyItem::Initiative(row) => row.updated_at,
            AnyItem::Task(row) => row.updated_at,
            AnyItem::Document(row) => row.updated_at,
            AnyItem::Adr(row) => row.updated_at,
        }
    }

    fn title(&self) -> &str {
        match self {
            AnyItem::Strategy(row) => &row.title,
            AnyItem::Initiative(row) => &row.title,
            AnyItem::Task(row) => &row.title,
            AnyItem::Document(row) => &row.title,
            AnyItem::Adr(row) => &row.title,
        }
    }

    fn short_code(&self) -> &str {
        match self {
            AnyItem::Strategy(row) => &row.short_code,
            AnyItem::Initiative(row) => &row.short_code,
            AnyItem::Task(row) => &row.short_code,
            AnyItem::Document(row) => &row.short_code,
            AnyItem::Adr(row) => &row.short_code,
        }
    }
}

/// Sort the combined rows by the requested field/order, tie-broken by
/// `short_code` ascending for a deterministic total order.
fn sort_items(items: &mut [AnyItem], sort: Sort) {
    items.sort_by(|a, b| {
        let primary = match sort.field {
            SortField::CreatedAt => a.created_at().cmp(&b.created_at()),
            SortField::UpdatedAt => a.updated_at().cmp(&b.updated_at()),
            SortField::Title => a.title().cmp(b.title()),
        };
        let primary = match sort.order {
            SortOrder::Asc => primary,
            SortOrder::Desc => primary.reverse(),
        };
        primary.then_with(|| a.short_code().cmp(b.short_code()))
    });
}
