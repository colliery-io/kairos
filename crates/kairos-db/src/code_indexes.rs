//! The base code indexes of a repository (COLLIERY-T-1853, COLLIERY-I-0264
//! "The flow"): the summary pool that its commits share, and the structure
//! of each indexed commit.
//!
//! - `code_index_summaries`: one row for each summary key of a repository.
//!   The same key always means the same input (COLLIERY-T-1851), so a key
//!   that is there keeps its row, and a write of it changes nothing. A
//!   vector that the row does not have is added.
//! - `code_indexes`: one row for each commit, with the structure as the
//!   gzip of the SQLite index file with no summaries. A second write for a
//!   commit replaces the row.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
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
}

#[derive(Insertable)]
#[diesel(table_name = code_index_summaries)]
struct NewPoolRow<'a> {
    repository_id: Uuid,
    key: &'a str,
    level: &'a str,
    summary: &'a str,
    vector: Option<&'a [u8]>,
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
/// pool of the repository does not have, in one transaction.
pub fn put(
    conn: &mut PgConnection,
    index: &NewCodeIndex,
    pool: &[PoolRow],
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
                })
                .collect();
            new_summaries += diesel::insert_into(code_index_summaries::table)
                .values(&rows)
                .on_conflict_do_nothing()
                .execute(conn)?;
            // A key that is there gets the vector of the upload if it has
            // none.
            let with_vector: Vec<(&str, &[u8])> = chunk
                .iter()
                .filter_map(|r| r.vector.as_deref().map(|v| (r.key.as_str(), v)))
                .collect();
            for (key, vector) in with_vector {
                diesel::update(
                    code_index_summaries::table
                        .filter(code_index_summaries::repository_id.eq(index.repository_id))
                        .filter(code_index_summaries::key.eq(key))
                        .filter(code_index_summaries::vector.is_null()),
                )
                .set(code_index_summaries::vector.eq(vector))
                .execute(conn)?;
            }
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
        Ok(PutOutcome {
            created: !existed,
            new_summaries,
        })
    })
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

/// Each indexed commit of a repository, newest write first.
pub fn list(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Vec<CodeIndexInfo>> {
    code_indexes::table
        .filter(code_indexes::repository_id.eq(repository_id))
        .order(code_indexes::updated_at.desc())
        .select(CodeIndexInfo::as_select())
        .load(conn)
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
