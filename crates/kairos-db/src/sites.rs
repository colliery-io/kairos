//! The published sites of the repositories (KAIROS-T-0406, KAIROS-D-0012).
//!
//! A publish is a row of `site_publishes` with its manifest in `site_files`.
//! `repositories.site_publish_id` names the live one; [`switch`] makes a new
//! publish live, in one transaction, and gives back the publish it replaced
//! so the caller can queue the deletion of its stored files.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::schema::{repositories, site_files, site_publishes};

/// The states of a publish (the CHECK of `site_publishes.state`).
pub mod state {
    /// The publish that the repository serves.
    pub const LIVE: &str = "live";
    /// A newer publish took its place.
    pub const REPLACED: &str = "replaced";
    /// The site was unpublished.
    pub const UNPUBLISHED: &str = "unpublished";
}

/// One publish (`site_publishes`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = site_publishes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SitePublish {
    pub id: Uuid,
    pub repository_id: Uuid,
    /// The slug of the repository at the time of the publish.
    pub repository_slug: String,
    /// The base path the site was built for.
    pub base_path: String,
    /// MD5 of the uploaded archive, lower-case hex.
    pub md5: String,
    /// Bytes of the archive.
    pub size: i64,
    /// Bytes of the files.
    pub unpacked_size: i64,
    /// Entries of the archive (files and folders).
    pub files: i32,
    /// The user or service account that authenticated the upload.
    pub principal_id: Uuid,
    /// The agent key, when one was used.
    pub agent_key_id: Option<Uuid>,
    pub published_at: DateTime<Utc>,
    /// One of [`state`].
    pub state: String,
}

/// Insert for [`SitePublish`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = site_publishes)]
pub struct NewSitePublish {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub repository_slug: String,
    pub base_path: String,
    pub md5: String,
    pub size: i64,
    pub unpacked_size: i64,
    pub files: i32,
    pub principal_id: Uuid,
    pub agent_key_id: Option<Uuid>,
    pub state: String,
}

/// One file of a publish (`site_files`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable)]
#[diesel(table_name = site_files)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SiteFile {
    pub publish_id: Uuid,
    /// The path in the site, relative, `/`-separated (`guide/index.html`).
    pub path: String,
    pub size: i64,
    /// SHA-256 of the file, lower-case hex: the ETag.
    pub sha256: String,
    pub content_type: String,
}

/// The rows of `site_files` in one INSERT.
const INSERT_CHUNK: usize = 1000;

/// Make `publish` the live publish of its repository, with its manifest
/// `files`, in one transaction. Returns the publish it replaced, now in
/// [`state::REPLACED`], if there was one.
pub fn switch(
    conn: &mut PgConnection,
    publish: NewSitePublish,
    files: &[SiteFile],
) -> Result<Option<SitePublish>, DieselError> {
    conn.transaction(|conn| {
        let repository_id = publish.repository_id;
        let previous: Option<Uuid> = repositories::table
            .filter(repositories::id.eq(repository_id))
            .select(repositories::site_publish_id)
            .for_update()
            .first(conn)?;
        let mut publish = publish;
        publish.state = state::LIVE.to_string();
        let id = publish.id;
        diesel::insert_into(site_publishes::table)
            .values(&publish)
            .execute(conn)?;
        for chunk in files.chunks(INSERT_CHUNK) {
            diesel::insert_into(site_files::table)
                .values(chunk)
                .execute(conn)?;
        }
        let replaced = match previous {
            Some(old) => Some(
                diesel::update(site_publishes::table.filter(site_publishes::id.eq(old)))
                    .set(site_publishes::state.eq(state::REPLACED))
                    .returning(SitePublish::as_returning())
                    .get_result(conn)?,
            ),
            None => None,
        };
        diesel::update(repositories::table.filter(repositories::id.eq(repository_id)))
            .set(repositories::site_publish_id.eq(Some(id)))
            .execute(conn)?;
        Ok(replaced)
    })
}

/// Remove the live publish of `repository_id`: it becomes
/// [`state::UNPUBLISHED`]. Returns it, or `None` when the repository has no
/// live publish.
pub fn unpublish(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<Option<SitePublish>, DieselError> {
    conn.transaction(|conn| {
        let live: Option<Uuid> = repositories::table
            .filter(repositories::id.eq(repository_id))
            .select(repositories::site_publish_id)
            .for_update()
            .first(conn)?;
        let Some(live) = live else {
            return Ok(None);
        };
        diesel::update(repositories::table.filter(repositories::id.eq(repository_id)))
            .set(repositories::site_publish_id.eq(None::<Uuid>))
            .execute(conn)?;
        let publish = diesel::update(site_publishes::table.filter(site_publishes::id.eq(live)))
            .set(site_publishes::state.eq(state::UNPUBLISHED))
            .returning(SitePublish::as_returning())
            .get_result(conn)?;
        Ok(Some(publish))
    })
}

/// The live publishes of `repository_ids`, by repository.
pub fn live(
    conn: &mut PgConnection,
    repository_ids: &[Uuid],
) -> Result<HashMap<Uuid, SitePublish>, DieselError> {
    let rows: Vec<SitePublish> = site_publishes::table
        .inner_join(
            repositories::table.on(repositories::site_publish_id.eq(site_publishes::id.nullable())),
        )
        .filter(repositories::id.eq_any(repository_ids))
        .select(SitePublish::as_select())
        .load(conn)?;
    Ok(rows.into_iter().map(|p| (p.repository_id, p)).collect())
}

/// The live publish served at `base_path`: the live publish of a repository
/// with its site on, built for that base path (KAIROS-T-0407).
pub fn live_at(
    conn: &mut PgConnection,
    base_path: &str,
) -> Result<Option<SitePublish>, DieselError> {
    site_publishes::table
        .inner_join(
            repositories::table.on(repositories::site_publish_id.eq(site_publishes::id.nullable())),
        )
        .filter(site_publishes::base_path.eq(base_path))
        .filter(repositories::site_enabled.eq(true))
        .filter(repositories::deleted_at.is_null())
        .select(SitePublish::as_select())
        .first(conn)
        .optional()
}

/// The live publish of the repository that `team`/`repository` names now,
/// with its site on, whatever base path it was built for.
pub fn live_of(
    conn: &mut PgConnection,
    team: &str,
    repository: &str,
) -> Result<Option<SitePublish>, DieselError> {
    use crate::schema::teams;
    site_publishes::table
        .inner_join(
            repositories::table.on(repositories::site_publish_id.eq(site_publishes::id.nullable())),
        )
        .inner_join(teams::table.on(teams::id.eq(repositories::team_id)))
        .filter(teams::slug.eq(team))
        .filter(teams::deleted_at.is_null())
        .filter(repositories::slug.eq(repository))
        .filter(repositories::site_enabled.eq(true))
        .filter(repositories::deleted_at.is_null())
        .select(SitePublish::as_select())
        .first(conn)
        .optional()
}

/// A base path that a repository's site had before it was published again
/// under a new one: the base path of its live publish now.
pub fn moved_to(
    conn: &mut PgConnection,
    old_base_path: &str,
) -> Result<Option<String>, DieselError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        base_path: String,
    }
    let row: Option<Row> = diesel::sql_query(
        "SELECT live.base_path FROM site_publishes old \
         JOIN repositories r ON r.id = old.repository_id \
         JOIN site_publishes live ON live.id = r.site_publish_id \
         WHERE old.base_path = $1 AND live.base_path <> $1 \
           AND r.site_enabled AND r.deleted_at IS NULL \
         ORDER BY old.published_at DESC LIMIT 1",
    )
    .bind::<diesel::sql_types::Text, _>(old_base_path)
    .get_result(conn)
    .optional()?;
    Ok(row.map(|r| r.base_path))
}

/// The file `path` of a publish, from its manifest.
pub fn file(
    conn: &mut PgConnection,
    publish_id: Uuid,
    path: &str,
) -> Result<Option<SiteFile>, DieselError> {
    site_files::table
        .filter(site_files::publish_id.eq(publish_id))
        .filter(site_files::path.eq(path))
        .select(SiteFile::as_select())
        .first(conn)
        .optional()
}

/// The display names of `user_ids` (KAIROS-T-0408).
pub fn names(
    conn: &mut PgConnection,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, DieselError> {
    use crate::schema::users;
    let rows: Vec<(Uuid, String)> = users::table
        .filter(users::id.eq_any(user_ids))
        .select((users::id, users::display_name))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// The tenant slug of the connection: its `search_path` is `org_{slug}`.
pub fn tenant_slug(conn: &mut PgConnection) -> Result<String, DieselError> {
    #[derive(QueryableByName)]
    struct Schema {
        #[diesel(sql_type = diesel::sql_types::Text)]
        name: String,
    }
    let schema: Schema =
        diesel::sql_query("SELECT COALESCE(current_schema()::text, 'public') AS name")
            .get_result(conn)?;
    Ok(schema
        .name
        .strip_prefix("org_")
        .unwrap_or(schema.name.as_str())
        .to_string())
}

/// The base path of the site of a repository: where Kairos serves it, and
/// what its build must use (KAIROS-D-0012).
pub fn base_path(tenant: &str, team: &str, repository: &str) -> String {
    format!("/pages/{tenant}/{team}/repos/{repository}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_path() {
        assert_eq!(
            base_path("acme", "kairos", "kairos-server"),
            "/pages/acme/kairos/repos/kairos-server/"
        );
    }
}
