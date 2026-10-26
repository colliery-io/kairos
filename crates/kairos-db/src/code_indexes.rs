//! The base code indexes of a repository (COLLIERY-T-1853, COLLIERY-I-0264
//! "The flow"): the summary pool that its commits share, and the structure
//! of each indexed commit.
//!
//! - `code_index_summaries`: one row for each summary key of a repository.
//!   The same key always means the same input (COLLIERY-T-1851), so a key
//!   that is there keeps its row, and a write of it changes nothing. A
//!   vector that the row does not have is added. A build writes each
//!   summary when the model makes it ([`add_made`], KAIROS-T-0378), so a
//!   row can be there before an index uses it. `last_used_at` is the time
//!   of the last stored index that uses the row; [`delete_unused`] removes
//!   the rows that no index has used.
//! - `code_indexes`: one row for each commit, with the structure as the
//!   gzip of the SQLite index file with no summaries. A second write for a
//!   commit replaces the row. The retention pass keeps the newest indexes
//!   of each repository and ref, and deletes the others (KAIROS-T-0638).
//!
//! - `code_index_citations`: the ADRs that the code of each indexed commit
//!   cites (KAIROS-T-0703), with the count of their places and of their
//!   files. [`put`] replaces the rows of the commit; a delete of the index
//!   deletes them.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bytea, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel::upsert::excluded;
use uuid::Uuid;

use crate::schema::{code_index_summaries, code_indexes};

/// The most rows in one insert statement (5 parameters each, well below
/// the 65,535 parameters of a Postgres statement).
const INSERT_CHUNK: usize = 1000;

/// One row of a summary pool.
#[derive(Debug, Clone, PartialEq, Queryable, Selectable)]
#[diesel(table_name = code_index_summaries)]
pub struct PoolRow {
    pub key: String,
    pub level: String,
    pub summary: String,
    pub vector: Option<Vec<u8>>,
    /// The model that wrote the summary, as `provider/model`
    /// (KAIROS-T-0338).
    pub model: String,
}

#[derive(Insertable)]
#[diesel(table_name = code_index_summaries)]
struct NewPoolRow<'a> {
    repository_id: Uuid,
    key: &'a str,
    level: &'a str,
    summary: &'a str,
    vector: Option<&'a [u8]>,
    model: &'a str,
}

/// The structure of one commit, to write.
#[derive(Debug, Clone)]
pub struct NewCodeIndex {
    pub repository_id: Uuid,
    pub commit_sha: String,
    pub ref_name: Option<String>,
    /// `upload` or `build`.
    pub source: &'static str,
    pub structure: Vec<u8>,
    pub structure_bytes: i64,
    pub summary_keys: i32,
    pub vector_model: Option<String>,
    /// `None` for the builder of the server.
    pub created_by: Option<Uuid>,
    /// The ADRs that the code cites (KAIROS-T-0703), one row for each code.
    pub citations: Vec<CitedCode>,
}

/// An ADR that the code of a commit cites (KAIROS-T-0703).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedCode {
    /// The ADR short code, as `KAIROS-A-0065`.
    pub code: String,
    /// The count of the places that cite it.
    pub places: i32,
    /// The count of the files that cite it.
    pub files: i32,
}

/// An ADR that the newest index of a branch cites, with the item of the
/// code when the organization has one (KAIROS-T-0703).
#[derive(Debug, Clone, PartialEq, QueryableByName)]
pub struct CitedAdr {
    #[diesel(sql_type = Text)]
    pub code: String,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    pub places: i32,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    pub files: i32,
    /// `None` when no ADR has the code.
    #[diesel(sql_type = Nullable<Text>)]
    pub title: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub column_name: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub archived_at: Option<DateTime<Utc>>,
}

/// The newest index of a branch, and the ADRs that its code cites.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchCitations {
    pub commit_sha: String,
    /// The first bytes of the structure, for the schema version.
    pub prefix: Vec<u8>,
    /// By code.
    pub adrs: Vec<CitedAdr>,
}

#[derive(Insertable)]
#[diesel(table_name = code_indexes)]
struct InsertCodeIndex<'a> {
    repository_id: Uuid,
    commit_sha: &'a str,
    ref_name: Option<&'a str>,
    source: &'a str,
    structure: &'a [u8],
    structure_bytes: i64,
    summary_keys: i32,
    vector_model: Option<&'a str>,
    created_by: Option<Uuid>,
}

/// One indexed commit, with no structure.
#[derive(Debug, Clone, PartialEq, Queryable, Selectable)]
#[diesel(table_name = code_indexes)]
pub struct CodeIndexInfo {
    pub commit_sha: String,
    pub ref_name: Option<String>,
    pub source: String,
    pub structure_bytes: i64,
    pub summary_keys: i32,
    pub vector_model: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What [`put`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutOutcome {
    /// The commit had no index before.
    pub created: bool,
    /// The pool rows that were new.
    pub new_summaries: usize,
}

/// Write the structure of one commit and add the rows of `pool` that the
/// pool of the repository does not have, in one transaction. Each row of
/// the pool whose key is in `keys` (the summary keys that the structure
/// uses) gets the time of this write as its `last_used_at`
/// (KAIROS-T-0378).
pub fn put(
    conn: &mut PgConnection,
    index: &NewCodeIndex,
    pool: &[PoolRow],
    keys: &[String],
) -> QueryResult<PutOutcome> {
    conn.transaction(|conn| {
        let mut new_summaries = 0;
        for chunk in pool.chunks(INSERT_CHUNK) {
            let rows: Vec<NewPoolRow<'_>> = chunk
                .iter()
                .map(|r| NewPoolRow {
                    repository_id: index.repository_id,
                    key: &r.key,
                    level: &r.level,
                    summary: &r.summary,
                    vector: r.vector.as_deref(),
                    model: &r.model,
                })
                .collect();
            new_summaries += diesel::insert_into(code_index_summaries::table)
                .values(&rows)
                .on_conflict_do_nothing()
                .execute(conn)?;
            // A key that is there gets the vector of the upload if it has
            // none: one statement for the chunk (KAIROS-T-0637).
            let (with_keys, vectors): (Vec<&str>, Vec<&[u8]>) = chunk
                .iter()
                .filter_map(|r| r.vector.as_deref().map(|v| (r.key.as_str(), v)))
                .unzip();
            if !with_keys.is_empty() {
                sql_query(
                    "UPDATE code_index_summaries s SET vector = u.vector \
                       FROM unnest($2::text[], $3::bytea[]) AS u(key, vector) \
                      WHERE s.repository_id = $1 AND s.key = u.key AND s.vector IS NULL",
                )
                .bind::<SqlUuid, _>(index.repository_id)
                .bind::<Array<Text>, _>(&with_keys)
                .bind::<Array<Bytea>, _>(&vectors)
                .execute(conn)?;
            }
        }
        for chunk in keys.chunks(INSERT_CHUNK) {
            diesel::update(
                code_index_summaries::table
                    .filter(code_index_summaries::repository_id.eq(index.repository_id))
                    .filter(code_index_summaries::key.eq_any(chunk)),
            )
            .set(code_index_summaries::last_used_at.eq(diesel::dsl::now))
            .execute(conn)?;
        }
        let existed = code_indexes::table
            .filter(code_indexes::repository_id.eq(index.repository_id))
            .filter(code_indexes::commit_sha.eq(&index.commit_sha))
            .select(code_indexes::commit_sha)
            .first::<String>(conn)
            .optional()?
            .is_some();
        diesel::insert_into(code_indexes::table)
            .values(InsertCodeIndex {
                repository_id: index.repository_id,
                commit_sha: &index.commit_sha,
                ref_name: index.ref_name.as_deref(),
                source: index.source,
                structure: &index.structure,
                structure_bytes: index.structure_bytes,
                summary_keys: index.summary_keys,
                vector_model: index.vector_model.as_deref(),
                created_by: index.created_by,
            })
            .on_conflict((code_indexes::repository_id, code_indexes::commit_sha))
            .do_update()
            .set((
                code_indexes::ref_name.eq(excluded(code_indexes::ref_name)),
                code_indexes::source.eq(excluded(code_indexes::source)),
                code_indexes::structure.eq(excluded(code_indexes::structure)),
                code_indexes::structure_bytes.eq(excluded(code_indexes::structure_bytes)),
                code_indexes::summary_keys.eq(excluded(code_indexes::summary_keys)),
                code_indexes::vector_model.eq(excluded(code_indexes::vector_model)),
                code_indexes::created_by.eq(excluded(code_indexes::created_by)),
                code_indexes::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;
        // KAIROS-T-0703: the cited ADRs of the commit, replaced.
        sql_query("DELETE FROM code_index_citations WHERE repository_id = $1 AND commit_sha = $2")
            .bind::<SqlUuid, _>(index.repository_id)
            .bind::<Text, _>(&index.commit_sha)
            .execute(conn)?;
        for chunk in index.citations.chunks(INSERT_CHUNK) {
            let codes: Vec<&str> = chunk.iter().map(|c| c.code.as_str()).collect();
            let places: Vec<i32> = chunk.iter().map(|c| c.places).collect();
            let files: Vec<i32> = chunk.iter().map(|c| c.files).collect();
            sql_query(
                "INSERT INTO code_index_citations (repository_id, commit_sha, code, places, files) \
                 SELECT $1, $2, u.code, u.places, u.files \
                   FROM unnest($3::text[], $4::int[], $5::int[]) AS u(code, places, files)",
            )
            .bind::<SqlUuid, _>(index.repository_id)
            .bind::<Text, _>(&index.commit_sha)
            .bind::<Array<Text>, _>(&codes)
            .bind::<Array<diesel::sql_types::Integer>, _>(&places)
            .bind::<Array<diesel::sql_types::Integer>, _>(&files)
            .execute(conn)?;
        }
        Ok(PutOutcome {
            created: !existed,
            new_summaries,
        })
    })
}

/// One summary that a build made (KAIROS-T-0378): the pool gets it at once,
/// before an index uses it, with no vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeSummary {
    pub key: String,
    pub level: String,
    pub summary: String,
    pub model: String,
}

/// Add the summaries that a build of `repository_id` made to its pool, at
/// the time they are made (KAIROS-T-0378). A key that is there keeps its
/// row. The new rows have no `last_used_at`: no stored index uses them
/// yet. So a build that stops keeps its summaries in Postgres, and the
/// next build of the repository, on any server, links them. Returns the
/// rows added.
pub fn add_made(
    conn: &mut PgConnection,
    repository_id: Uuid,
    made: &[MadeSummary],
) -> QueryResult<usize> {
    let mut added = 0;
    for chunk in made.chunks(INSERT_CHUNK) {
        let rows: Vec<NewPoolRow<'_>> = chunk
            .iter()
            .map(|m| NewPoolRow {
                repository_id,
                key: &m.key,
                level: &m.level,
                summary: &m.summary,
                vector: None,
                model: &m.model,
            })
            .collect();
        added += diesel::insert_into(code_index_summaries::table)
            .values(&rows)
            .on_conflict_do_nothing()
            .execute(conn)?;
    }
    Ok(added)
}

/// Delete at most `limit` rows of the pool that no stored index has used
/// and that were made before `before` (KAIROS-T-0378): the summaries of
/// builds that did not end, or of code that went away before the build
/// ended. A row that a stored index used stays. Returns the count; the
/// retention pass calls it until a page deletes fewer than `limit`
/// (KAIROS-T-0638).
pub fn delete_unused(
    conn: &mut PgConnection,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<usize> {
    sql_query(
        "DELETE FROM code_index_summaries WHERE (repository_id, key) IN ( \
             SELECT repository_id, key FROM code_index_summaries \
             WHERE last_used_at IS NULL AND created_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(before)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}

/// Delete at most `limit` rows of the pool of `repository_id` whose last
/// use is before `before` and before the last write of every stored index
/// of the repository (KAIROS-T-0638). A stored index marks each row that
/// it uses with the time of its write ([`put`]), so no stored index uses
/// such a row: only indexes that retention removed used it. With no stored
/// index, each row used before `before` goes. Returns the count.
pub fn delete_used_by_no_stored_index(
    conn: &mut PgConnection,
    repository_id: Uuid,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<usize> {
    sql_query(
        "DELETE FROM code_index_summaries WHERE repository_id = $1 AND key IN ( \
             SELECT key FROM code_index_summaries \
             WHERE repository_id = $1 AND last_used_at < $2 \
               AND last_used_at < COALESCE( \
                   (SELECT min(updated_at) FROM code_indexes WHERE repository_id = $1), \
                   'infinity'::timestamptz) \
             LIMIT $3)",
    )
    .bind::<SqlUuid, _>(repository_id)
    .bind::<Timestamptz, _>(before)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}

/// At most `limit` repositories that have rows in `code_indexes` (when
/// `indexes`) or in `code_index_summaries`, after `after`, in id order
/// (KAIROS-T-0638): the pages of the retention pass.
pub fn repositories_page(
    conn: &mut PgConnection,
    indexes: bool,
    after: Option<Uuid>,
    limit: i64,
) -> QueryResult<Vec<Uuid>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        repository_id: Uuid,
    }
    let table = if indexes {
        "code_indexes"
    } else {
        "code_index_summaries"
    };
    let rows: Vec<Row> = sql_query(format!(
        "SELECT DISTINCT repository_id FROM {table} \
         WHERE ($1::uuid IS NULL OR repository_id > $1) \
         ORDER BY repository_id LIMIT $2"
    ))
    .bind::<Nullable<SqlUuid>, _>(after)
    .bind::<BigInt, _>(limit)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.repository_id).collect())
}

/// The key, the ref and the last write of each stored index of
/// `repositories`, with no structure (KAIROS-T-0638).
pub fn index_meta(
    conn: &mut PgConnection,
    repositories: &[Uuid],
) -> QueryResult<Vec<kairos_core::retention::CodeIndexMeta>> {
    let rows: Vec<(Uuid, String, Option<String>, DateTime<Utc>)> = code_indexes::table
        .filter(code_indexes::repository_id.eq_any(repositories))
        .select((
            code_indexes::repository_id,
            code_indexes::commit_sha,
            code_indexes::ref_name,
            code_indexes::updated_at,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(repository_id, commit_sha, ref_name, updated_at)| {
            kairos_core::retention::CodeIndexMeta {
                repository_id,
                commit_sha,
                ref_name,
                updated_at,
            }
        })
        .collect())
}

/// Delete the indexes of `repository_id` for `commits` (KAIROS-T-0638).
/// Returns the count.
pub fn delete_indexes(
    conn: &mut PgConnection,
    repository_id: Uuid,
    commits: &[String],
) -> QueryResult<usize> {
    diesel::delete(
        code_indexes::table
            .filter(code_indexes::repository_id.eq(repository_id))
            .filter(code_indexes::commit_sha.eq_any(commits)),
    )
    .execute(conn)
}

/// The structure of one commit (the gzip of the SQLite file), if the
/// repository has an index of it.
pub fn structure(
    conn: &mut PgConnection,
    repository_id: Uuid,
    commit_sha: &str,
) -> QueryResult<Option<Vec<u8>>> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .filter(code_indexes::commit_sha.eq(commit_sha))
        .select(code_indexes::structure)
        .first(conn)
        .optional()
}

/// The first `bytes` bytes of the structure of one commit: enough for the
/// schema version check of the builder, without the whole blob
/// (KAIROS-T-0347).
pub fn structure_prefix(
    conn: &mut PgConnection,
    repository_id: Uuid,
    commit_sha: &str,
    bytes: i32,
) -> QueryResult<Option<Vec<u8>>> {
    #[derive(QueryableByName)]
    struct Prefix {
        #[diesel(sql_type = diesel::sql_types::Bytea)]
        prefix: Vec<u8>,
    }
    diesel::sql_query(
        "SELECT substring(structure from 1 for $1) AS prefix
         FROM code_indexes
         WHERE repository_id = $2 AND commit_sha = $3",
    )
    .bind::<diesel::sql_types::Integer, _>(bytes)
    .bind::<diesel::sql_types::Uuid, _>(repository_id)
    .bind::<diesel::sql_types::Text, _>(commit_sha)
    .get_result::<Prefix>(conn)
    .optional()
    .map(|row| row.map(|row| row.prefix))
}

/// The rows of the pool of a repository with these keys.
pub fn pool_rows(
    conn: &mut PgConnection,
    repository_id: Uuid,
    keys: &[String],
) -> QueryResult<Vec<PoolRow>> {
    let mut rows = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(INSERT_CHUNK) {
        rows.extend(
            code_index_summaries::table
                .filter(code_index_summaries::repository_id.eq(repository_id))
                .filter(code_index_summaries::key.eq_any(chunk))
                .select(PoolRow::as_select())
                .load(conn)?,
        );
    }
    Ok(rows)
}

/// Each row of the pool of a repository: the input of a build, so that the
/// model runs only for keys that no commit has.
pub fn whole_pool(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Vec<PoolRow>> {
    code_index_summaries::table
        .filter(code_index_summaries::repository_id.eq(repository_id))
        .select(PoolRow::as_select())
        .load(conn)
}

/// The count of the rows of the pool of a repository.
pub fn pool_size(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<i64> {
    code_index_summaries::table
        .filter(code_index_summaries::repository_id.eq(repository_id))
        .count()
        .get_result(conn)
}

/// The newest index of the branch `ref_name` of a repository, and the ADRs
/// that its code cites (KAIROS-T-0703), by code. Each code has the title,
/// the column and the archive time of the ADR with that short code, when
/// there is one. `prefix_bytes` of the structure come with it, for the
/// schema version: an index from before the citations has none. `None`
/// when the branch has no index.
pub fn branch_citations(
    conn: &mut PgConnection,
    repository_id: Uuid,
    ref_name: &str,
    prefix_bytes: i32,
) -> QueryResult<Option<BranchCitations>> {
    // `prefix_bytes` is a number, so the text of the SQL is safe.
    let prefix =
        diesel::dsl::sql::<Bytea>(&format!("substring(structure from 1 for {prefix_bytes})"));
    let newest: Option<(String, Vec<u8>)> = code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .filter(code_indexes::ref_name.eq(ref_name))
        .order((
            code_indexes::updated_at.desc(),
            code_indexes::commit_sha.asc(),
        ))
        .select((code_indexes::commit_sha, prefix))
        .first(conn)
        .optional()?;
    let Some((commit_sha, prefix)) = newest else {
        return Ok(None);
    };
    let adrs = sql_query(
        "SELECT c.code, c.places, c.files, a.title, col.name AS column_name, \
                a.deleted_at AS archived_at \
           FROM code_index_citations c \
           LEFT JOIN adrs a ON a.short_code = c.code \
           LEFT JOIN board_columns col ON col.id = a.column_id \
          WHERE c.repository_id = $1 AND c.commit_sha = $2 \
          ORDER BY c.code",
    )
    .bind::<SqlUuid, _>(repository_id)
    .bind::<Text, _>(&commit_sha)
    .load(conn)?;
    Ok(Some(BranchCitations {
        commit_sha,
        prefix,
        adrs,
    }))
}

/// Each indexed commit of a repository, newest write first.
pub fn list(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Vec<CodeIndexInfo>> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .order(code_indexes::updated_at.desc())
        .select(CodeIndexInfo::as_select())
        .load(conn)
}

/// One page of the indexed commits of a repository, newest first: the
/// time of the last write, then the commit (KAIROS-T-0518). Each row has
/// the first `prefix_bytes` bytes of its structure, for the schema version.
/// With no `limit`, the page has each row from `offset`.
pub fn page(
    conn: &mut PgConnection,
    repository_id: Uuid,
    limit: Option<i64>,
    offset: i64,
    prefix_bytes: i32,
) -> QueryResult<Vec<(CodeIndexInfo, Vec<u8>)>> {
    // `prefix_bytes` is a number, so the text of the SQL is safe.
    let prefix = diesel::dsl::sql::<diesel::sql_types::Bytea>(&format!(
        "substring(structure from 1 for {prefix_bytes})"
    ));
    let mut query = code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .order((
            code_indexes::updated_at.desc(),
            code_indexes::commit_sha.asc(),
        ))
        .select((CodeIndexInfo::as_select(), prefix))
        .offset(offset)
        .into_boxed();
    if let Some(limit) = limit {
        query = query.limit(limit);
    }
    query.load(conn)
}

/// The count of the indexed commits of a repository.
pub fn count(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<i64> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .count()
        .get_result(conn)
}

/// One indexed commit, with no structure.
pub fn info(
    conn: &mut PgConnection,
    repository_id: Uuid,
    commit_sha: &str,
) -> QueryResult<Option<CodeIndexInfo>> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .filter(code_indexes::commit_sha.eq(commit_sha))
        .select(CodeIndexInfo::as_select())
        .first(conn)
        .optional()
}

/// The model of the vectors of the pool of a repository: the model of an
/// index that has one.
pub fn vector_model(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Option<String>> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .filter(code_indexes::vector_model.is_not_null())
        .select(code_indexes::vector_model)
        .first::<Option<String>>(conn)
        .optional()
        .map(Option::flatten)
}

/// The time of the newest index write of each repository that has an
/// index (KAIROS-T-0334): the newest `updated_at` of its indexes. The
/// update lane serves the repository with the oldest time first.
pub fn newest_writes(conn: &mut PgConnection) -> QueryResult<Vec<(Uuid, DateTime<Utc>)>> {
    code_indexes::table
        .group_by(code_indexes::repository_id)
        .select((
            code_indexes::repository_id,
            diesel::dsl::max(code_indexes::updated_at),
        ))
        .load::<(Uuid, Option<DateTime<Utc>>)>(conn)
        .map(|rows| {
            rows.into_iter()
                .filter_map(|(id, at)| at.map(|at| (id, at)))
                .collect()
        })
}

/// The ids of the live repositories that have one index or more: the
/// repositories that the builder follows.
pub fn indexed_repositories(conn: &mut PgConnection) -> QueryResult<Vec<Uuid>> {
    use crate::schema::repositories;
    repositories::table
        .filter(repositories::deleted_at.is_null())
        .filter(repositories::id.eq_any(code_indexes::table.select(code_indexes::repository_id)))
        .order(repositories::slug)
        .select(repositories::id)
        .load(conn)
}

/// The ids of the live repositories that have no index: the repositories
/// whose first index the builder makes (KAIROS-T-0318). By slug. A
/// repository with `code_index_build = 'off'` is in the list too, so that
/// the builder can log why it does not build it.
pub fn unindexed_repositories(conn: &mut PgConnection) -> QueryResult<Vec<Uuid>> {
    use crate::schema::repositories;
    repositories::table
        .filter(repositories::deleted_at.is_null())
        .filter(diesel::dsl::not(repositories::id.eq_any(
            code_indexes::table.select(code_indexes::repository_id),
        )))
        .order(repositories::slug)
        .select(repositories::id)
        .load(conn)
}
