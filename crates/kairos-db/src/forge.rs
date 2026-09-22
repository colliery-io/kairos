//! Git-forge connection services (KAIROS-T-0097, design in
//! KAIROS-I-0009): the CRUD behind `/api/forge-connections`, plus the
//! ordering-safe link upsert the webhook endpoint (KAIROS-T-0099) calls.
//!
//! Since KAIROS-T-0103 (A-0019) a connection is the webhook wiring OF a
//! [`Repository`]: repo identity and the owning team live there, and
//! every read that needs a repo name or team joins through it. One live
//! connection per repository.
//!
//! Every function operates in the CURRENT `search_path` tenant schema,
//! the same convention as [`crate::items`] and [`crate::team_pages`].
//!
//! # Why links have no version
//!
//! [`crate::models::forge::ItemLink`] rows mirror forge state; Kairos
//! never authors them, so A-0004 optimistic concurrency does not apply.
//! What DOES matter is that webhooks retry and arrive out of order, so
//! [`upsert_link`] only advances a row when the incoming event is at
//! least as recent as the stored one — see its docs.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use uuid::Uuid;

use crate::models::enums::{Forge, LinkKind, LinkState};
use crate::models::forge::{ForgeConnection, ItemLink, NewForgeConnection, NewItemLink};
use crate::models::repositories::Repository;

/// Errors from the forge-connection services.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    /// No live connection with this id.
    #[error("forge connection {0} does not exist")]
    ConnectionNotFound(Uuid),
    /// A live connection already covers this repository.
    #[error("repository {repo:?} already has a live connection")]
    RepoAlreadyConnected { repo: String },
    /// No live repository with this id (connection target).
    #[error("repository {0} does not exist")]
    RepositoryNotFound(Uuid),
    /// The connection's forge must match its repository's (`other` repos
    /// have no webhook dialect at all).
    #[error("connection forge {connection} does not match repository forge {repository}")]
    ForgeMismatch {
        connection: Forge,
        repository: Forge,
    },
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// A connection joined to its repository — the read shape the API renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionWithRepo {
    pub connection: ForgeConnection,
    pub repository: Repository,
}

/// Every live connection with its repository, newest first.
pub fn list_connections(conn: &mut PgConnection) -> Result<Vec<ConnectionWithRepo>, ForgeError> {
    use crate::schema::{forge_connections, repositories};
    let rows: Vec<(ForgeConnection, Repository)> = forge_connections::table
        .inner_join(repositories::table)
        .filter(forge_connections::deleted_at.is_null())
        .order(forge_connections::created_at.desc())
        .select((ForgeConnection::as_select(), Repository::as_select()))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(connection, repository)| ConnectionWithRepo {
            connection,
            repository,
        })
        .collect())
}

/// One live connection, or [`ForgeError::ConnectionNotFound`].
pub fn load_connection(conn: &mut PgConnection, id: Uuid) -> Result<ForgeConnection, ForgeError> {
    use crate::schema::forge_connections::dsl;
    dsl::forge_connections
        .filter(dsl::id.eq(id))
        .filter(dsl::deleted_at.is_null())
        .select(ForgeConnection::as_select())
        .first(conn)
        .optional()?
        .ok_or(ForgeError::ConnectionNotFound(id))
}

/// One live connection with its repository.
pub fn load_connection_with_repo(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<ConnectionWithRepo, ForgeError> {
    use crate::schema::{forge_connections, repositories};
    let row: Option<(ForgeConnection, Repository)> = forge_connections::table
        .inner_join(repositories::table)
        .filter(forge_connections::id.eq(id))
        .filter(forge_connections::deleted_at.is_null())
        .select((ForgeConnection::as_select(), Repository::as_select()))
        .first(conn)
        .optional()?;
    row.map(|(connection, repository)| ConnectionWithRepo {
        connection,
        repository,
    })
    .ok_or(ForgeError::ConnectionNotFound(id))
}

/// The live connection for a `(forge, repo_full_name)`, if any — resolved
/// through the repository.
pub fn find_connection_by_repo(
    conn: &mut PgConnection,
    forge: Forge,
    repo_full_name: &str,
) -> Result<Option<ForgeConnection>, ForgeError> {
    use crate::schema::{forge_connections, repositories};
    Ok(forge_connections::table
        .inner_join(repositories::table)
        .filter(repositories::forge.eq(forge))
        .filter(repositories::repo_full_name.eq(repo_full_name))
        .filter(repositories::deleted_at.is_null())
        .filter(forge_connections::deleted_at.is_null())
        .select(ForgeConnection::as_select())
        .first(conn)
        .optional()?)
}

/// The live connection of one repository, if any.
pub fn find_connection_for_repository(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<Option<ForgeConnection>, ForgeError> {
    use crate::schema::forge_connections::dsl;
    Ok(dsl::forge_connections
        .filter(dsl::repository_id.eq(repository_id))
        .filter(dsl::deleted_at.is_null())
        .select(ForgeConnection::as_select())
        .first(conn)
        .optional()?)
}

/// Wire a webhook connection onto a repository. The repository must be
/// live, carry the same forge, and have no live connection yet.
pub fn create_connection(
    conn: &mut PgConnection,
    input: NewForgeConnection,
) -> Result<ForgeConnection, ForgeError> {
    let repository = crate::repositories::load(conn, input.repository_id)
        .map_err(|_| ForgeError::RepositoryNotFound(input.repository_id))?;
    if repository.forge != input.forge {
        return Err(ForgeError::ForgeMismatch {
            connection: input.forge,
            repository: repository.forge,
        });
    }
    diesel::insert_into(crate::schema::forge_connections::table)
        .values(input)
        .returning(ForgeConnection::as_returning())
        .get_result(conn)
        .map_err(|e| match &e {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                ForgeError::RepoAlreadyConnected {
                    repo: repository.repo_full_name.clone(),
                }
            }
            _ => ForgeError::Database(e),
        })
}

/// Soft-delete a connection. Its links go with it (`ON DELETE CASCADE`
/// covers hard deletes; here the links simply stop being reachable
/// because every read joins a live connection).
pub fn delete_connection(conn: &mut PgConnection, id: Uuid) -> Result<(), ForgeError> {
    load_connection(conn, id)?;
    use crate::schema::forge_connections::dsl;
    diesel::update(dsl::forge_connections.filter(dsl::id.eq(id)))
        .set((
            dsl::deleted_at.eq(Some(Utc::now())),
            dsl::updated_at.eq(Utc::now()),
        ))
        .execute(conn)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// Insert or advance one link, ORDERING-SAFELY.
///
/// Webhook deliveries retry and are not ordered, so the update only fires
/// when the incoming `forge_updated_at` is at least as recent as the
/// stored one. Without that guard a redelivered "opened" event would
/// silently regress a merged pull request back to open — the failure this
/// whole design is shaped around.
///
/// Returns `true` when the row was created or advanced, `false` when a
/// stale event was correctly ignored.
pub fn upsert_link(conn: &mut PgConnection, link: NewItemLink) -> Result<bool, ForgeError> {
    // Sanctioned raw SQL (A-0009): diesel's builder cannot express a
    // `DO UPDATE ... WHERE` guard, and doing this as select-then-write
    // would open a race between concurrent deliveries for the same PR.
    // One statement keeps it atomic under the unique index.
    let affected = diesel::sql_query(
        "INSERT INTO item_links
             (item_id, connection_id, kind, external_id, title, url, state,
              author, forge_updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (connection_id, kind, external_id, item_id) DO UPDATE
            SET title = EXCLUDED.title,
                url = EXCLUDED.url,
                state = EXCLUDED.state,
                author = EXCLUDED.author,
                forge_updated_at = EXCLUDED.forge_updated_at,
                updated_at = now()
          WHERE item_links.forge_updated_at <= EXCLUDED.forge_updated_at",
    )
    .bind::<diesel::sql_types::Uuid, _>(link.item_id)
    .bind::<diesel::sql_types::Uuid, _>(link.connection_id)
    .bind::<diesel::sql_types::Text, _>(link.kind)
    .bind::<diesel::sql_types::Text, _>(link.external_id)
    .bind::<diesel::sql_types::Text, _>(link.title)
    .bind::<diesel::sql_types::Text, _>(link.url)
    .bind::<diesel::sql_types::Text, _>(link.state)
    .bind::<diesel::sql_types::Text, _>(link.author)
    .bind::<diesel::sql_types::Timestamptz, _>(link.forge_updated_at)
    .execute(conn)?;
    Ok(affected > 0)
}

/// Drop a link between one item and one PR/branch — used when a pull
/// request is edited to no longer mention a short code (links are
/// derived, so removing a stale association is correct, not lossy).
pub fn delete_link(
    conn: &mut PgConnection,
    connection_id: Uuid,
    kind: LinkKind,
    external_id: &str,
    item_id: Uuid,
) -> Result<(), ForgeError> {
    use crate::schema::item_links::dsl;
    diesel::delete(
        dsl::item_links
            .filter(dsl::connection_id.eq(connection_id))
            .filter(dsl::kind.eq(kind))
            .filter(dsl::external_id.eq(external_id))
            .filter(dsl::item_id.eq(item_id)),
    )
    .execute(conn)?;
    Ok(())
}

/// The item ids currently linked to one PR/branch — the "which links
/// should be removed" input when a re-edited PR drops a short code.
pub fn linked_items(
    conn: &mut PgConnection,
    connection_id: Uuid,
    kind: LinkKind,
    external_id: &str,
) -> Result<Vec<Uuid>, ForgeError> {
    use crate::schema::item_links::dsl;
    Ok(dsl::item_links
        .filter(dsl::connection_id.eq(connection_id))
        .filter(dsl::kind.eq(kind))
        .filter(dsl::external_id.eq(external_id))
        .select(dsl::item_id)
        .load(conn)?)
}

/// One link joined to its repository — the read shape both the item panel
/// (KAIROS-T-0100) and the team rollup (KAIROS-T-0101) render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkWithRepo {
    pub link: ItemLink,
    pub forge: Forge,
    pub repo_full_name: String,
}

/// Every link on one item, PRs before branches then newest first (the
/// ordering lives here so every client agrees).
pub fn links_for_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<LinkWithRepo>, ForgeError> {
    use crate::schema::{forge_connections, item_links, repositories};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table.inner_join(repositories::table))
        .filter(item_links::item_id.eq(item_id))
        .filter(forge_connections::deleted_at.is_null())
        .order((item_links::kind.desc(), item_links::forge_updated_at.desc()))
        .select((
            ItemLink::as_select(),
            repositories::forge,
            repositories::repo_full_name,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(link, forge, repo_full_name)| LinkWithRepo {
            link,
            forge,
            repo_full_name,
        })
        .collect())
}

/// Links in the given states across a set of items — the team rollup's
/// second half (KAIROS-T-0101 supplies the item ids).
pub fn links_for_items(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
    states: &[LinkState],
    limit: i64,
) -> Result<Vec<LinkWithRepo>, ForgeError> {
    use crate::schema::{forge_connections, item_links, repositories};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table.inner_join(repositories::table))
        .filter(item_links::item_id.eq_any(item_ids))
        .filter(item_links::state.eq_any(states))
        .filter(forge_connections::deleted_at.is_null())
        .order(item_links::forge_updated_at.desc())
        .limit(limit)
        .select((
            ItemLink::as_select(),
            repositories::forge,
            repositories::repo_full_name,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(link, forge, repo_full_name)| LinkWithRepo {
            link,
            forge,
            repo_full_name,
        })
        .collect())
}

/// Links in the given states belonging to repositories OWNED by a team —
/// the rollup's repo-level path (a repo carries its owner even when an
/// individual item carries no team).
pub fn links_for_connection_team(
    conn: &mut PgConnection,
    team_id: Uuid,
    states: &[LinkState],
    limit: i64,
) -> Result<Vec<LinkWithRepo>, ForgeError> {
    use crate::schema::{forge_connections, item_links, repositories};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table.inner_join(repositories::table))
        .filter(repositories::team_id.eq(team_id))
        .filter(item_links::state.eq_any(states))
        .filter(forge_connections::deleted_at.is_null())
        .order(item_links::forge_updated_at.desc())
        .limit(limit)
        .select((
            ItemLink::as_select(),
            repositories::forge,
            repositories::repo_full_name,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(link, forge, repo_full_name)| LinkWithRepo {
            link,
            forge,
            repo_full_name,
        })
        .collect())
}

/// Convenience for callers building a link from a normalized event.
#[allow(clippy::too_many_arguments)]
pub fn new_link(
    item_id: Uuid,
    connection_id: Uuid,
    kind: LinkKind,
    external_id: String,
    title: String,
    url: String,
    state: LinkState,
    author: String,
    forge_updated_at: DateTime<Utc>,
) -> NewItemLink {
    NewItemLink {
        item_id,
        connection_id,
        kind,
        external_id,
        title,
        url,
        state,
        author,
        forge_updated_at,
    }
}
