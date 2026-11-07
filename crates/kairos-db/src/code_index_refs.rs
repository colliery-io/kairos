//! The refs that point at a stored code index (KAIROS-T-0750,
//! COLLIERY-I-0671): the default branch, each open pull request and each
//! tag that the builder saw at an indexed commit. One commit can carry many
//! refs. `code_indexes.ref_name` stays the ref that the build was for.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::code_index_refs;

/// The kind of a ref: the value of the column `kind`.
pub const BRANCH: &str = "branch";
pub const PULL: &str = "pull";
pub const TAG: &str = "tag";

/// One ref that points at an indexed commit.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = code_index_refs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct IndexRef {
    pub repository_id: Uuid,
    /// `main`, `pull/12`, `merge-requests/12`, `v1.0.0`: no `refs/` prefix.
    pub ref_name: String,
    pub commit_sha: String,
    /// `branch`, `pull` or `tag`.
    pub kind: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = code_index_refs)]
struct NewRef<'a> {
    repository_id: Uuid,
    ref_name: &'a str,
    commit_sha: &'a str,
    kind: &'a str,
}

/// Record that `ref_name` of `repository` points at `commit`, which has an
/// index. A ref that is there moves to the commit. The caller makes sure
/// that the commit has an index: the foreign key refuses the write when it
/// has none.
pub fn record(
    conn: &mut PgConnection,
    repository: Uuid,
    ref_name: &str,
    kind: &str,
    commit: &str,
) -> QueryResult<()> {
    diesel::insert_into(code_index_refs::table)
        .values(NewRef {
            repository_id: repository,
            ref_name,
            commit_sha: commit,
            kind,
        })
        .on_conflict((code_index_refs::repository_id, code_index_refs::ref_name))
        .do_update()
        .set((
            code_index_refs::commit_sha.eq(commit),
            code_index_refs::kind.eq(kind),
            code_index_refs::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)
        .map(|_| ())
}

/// The refs of `repository`, by the commit they point at, each list in
/// the order branch, tag, pull, then by name.
pub fn by_commit(
    conn: &mut PgConnection,
    repository: Uuid,
) -> QueryResult<HashMap<String, Vec<IndexRef>>> {
    let rows = code_index_refs::table
        .filter(code_index_refs::repository_id.eq(repository))
        .order((code_index_refs::kind.asc(), code_index_refs::ref_name.asc()))
        .select(IndexRef::as_select())
        .load::<IndexRef>(conn)?;
    let mut out: HashMap<String, Vec<IndexRef>> = HashMap::new();
    for row in rows {
        out.entry(row.commit_sha.clone()).or_default().push(row);
    }
    // `kind` sorts branch, pull, tag; the reader wants the branch and the
    // tags before the pull requests.
    for refs in out.values_mut() {
        refs.sort_by_key(|r| (kind_rank(&r.kind), r.ref_name.clone()));
    }
    Ok(out)
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        BRANCH => 0,
        TAG => 1,
        _ => 2,
    }
}

/// The refs of `repository`.
pub fn list(conn: &mut PgConnection, repository: Uuid) -> QueryResult<Vec<IndexRef>> {
    code_index_refs::table
        .filter(code_index_refs::repository_id.eq(repository))
        .order(code_index_refs::ref_name.asc())
        .select(IndexRef::as_select())
        .load(conn)
}

/// Delete the refs of `repository` that are not in `keep`: the sweep saw
/// the remote, and these refs are gone or outside its window. The number
/// deleted.
pub fn delete_except(
    conn: &mut PgConnection,
    repository: Uuid,
    keep: &[String],
) -> QueryResult<usize> {
    diesel::delete(
        code_index_refs::table
            .filter(code_index_refs::repository_id.eq(repository))
            .filter(code_index_refs::ref_name.ne_all(keep)),
    )
    .execute(conn)
}
