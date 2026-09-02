//! Git-forge connection services (KAIROS-T-0097, design in
//! KAIROS-I-0009): the CRUD behind `/api/forge-connections`, plus the
//! ordering-safe link upsert the webhook endpoint (KAIROS-T-0099) calls.
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
use crate::models::forge::{
    ForgeConnection, ForgeConnectionChangeset, ItemLink, NewForgeConnection, NewItemLink,
};

/// Errors from the forge-connection services.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    /// No live connection with this id.
    #[error("forge connection {0} does not exist")]
    ConnectionNotFound(Uuid),
    /// A live connection already covers this `(forge, repo)`.
    #[error("{forge} repository {repo:?} is already connected")]
    RepoAlreadyConnected { forge: Forge, repo: String },
    /// The named team does not exist (attribution target).
    #[error("team {0} does not exist")]
    TeamNotFound(Uuid),
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// Every live connection, newest first.
pub fn list_connections(conn: &mut PgConnection) -> Result<Vec<ForgeConnection>, ForgeError> {
    use crate::schema::forge_connections::dsl;
    Ok(dsl::forge_connections
        .filter(dsl::deleted_at.is_null())
        .order(dsl::created_at.desc())
        .select(ForgeConnection::as_select())
        .load(conn)?)
}

/// One live connection, or [`ForgeError::ConnectionNotFound`].
pub fn load_connection(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<ForgeConnection, ForgeError> {
    use crate::schema::forge_connections::dsl;
    dsl::forge_connections
        .filter(dsl::id.eq(id))
        .filter(dsl::deleted_at.is_null())
        .select(ForgeConnection::as_select())
        .first(conn)
        .optional()?
        .ok_or(ForgeError::ConnectionNotFound(id))
}

/// The live connection for a `(forge, repo_full_name)`, if any — the
/// webhook path's lookup after it has resolved the tenant.
pub fn find_connection_by_repo(
    conn: &mut PgConnection,
    forge: Forge,
    repo_full_name: &str,
) -> Result<Option<ForgeConnection>, ForgeError> {
    use crate::schema::forge_connections::dsl;
    Ok(dsl::forge_connections
        .filter(dsl::forge.eq(forge))
        .filter(dsl::repo_full_name.eq(repo_full_name))
        .filter(dsl::deleted_at.is_null())
        .select(ForgeConnection::as_select())
        .first(conn)
        .optional()?)
}

/// Register a repository. The `(forge, repo)` pair must be free among
/// live connections.
pub fn create_connection(
    conn: &mut PgConnection,
    input: NewForgeConnection,
) -> Result<ForgeConnection, ForgeError> {
    if let Some(team_id) = input.team_id {
        require_team(conn, team_id)?;
    }
    let forge = input.forge;
    let repo = input.repo_full_name.clone();
    diesel::insert_into(crate::schema::forge_connections::table)
        .values(input)
        .returning(ForgeConnection::as_returning())
        .get_result(conn)
        .map_err(|e| match &e {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                ForgeError::RepoAlreadyConnected { forge, repo }
            }
            _ => ForgeError::Database(e),
        })
}

/// Re-attribute a connection to a team (or clear it with `Some(None)`).
pub fn set_connection_team(
    conn: &mut PgConnection,
    id: Uuid,
    team_id: Option<Uuid>,
) -> Result<ForgeConnection, ForgeError> {
    load_connection(conn, id)?;
    if let Some(team_id) = team_id {
        require_team(conn, team_id)?;
    }
    use crate::schema::forge_connections::dsl;
    Ok(
        diesel::update(dsl::forge_connections.filter(dsl::id.eq(id)))
            .set(ForgeConnectionChangeset {
                team_id: Some(team_id),
                updated_at: Some(Utc::now()),
                ..Default::default()
            })
            .returning(ForgeConnection::as_returning())
            .get_result(conn)?,
    )
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

/// 422-worthy check that a live team exists.
fn require_team(conn: &mut PgConnection, team_id: Uuid) -> Result<(), ForgeError> {
    use crate::schema::teams::dsl;
    let found: Option<Uuid> = dsl::teams
        .filter(dsl::id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()?;
    found.map(|_| ()).ok_or(ForgeError::TeamNotFound(team_id))
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
    use crate::schema::{forge_connections, item_links};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table)
        .filter(item_links::item_id.eq(item_id))
        .filter(forge_connections::deleted_at.is_null())
        .order((
            item_links::kind.desc(),
            item_links::forge_updated_at.desc(),
        ))
        .select((
            ItemLink::as_select(),
            forge_connections::forge,
            forge_connections::repo_full_name,
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
    use crate::schema::{forge_connections, item_links};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table)
        .filter(item_links::item_id.eq_any(item_ids))
        .filter(item_links::state.eq_any(states))
        .filter(forge_connections::deleted_at.is_null())
        .order(item_links::forge_updated_at.desc())
        .limit(limit)
        .select((
            ItemLink::as_select(),
            forge_connections::forge,
            forge_connections::repo_full_name,
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

/// Links in the given states belonging to repositories attributed to a
/// team — the rollup's repo-level path (a repo can carry team attribution
/// even when an individual item does not).
pub fn links_for_connection_team(
    conn: &mut PgConnection,
    team_id: Uuid,
    states: &[LinkState],
    limit: i64,
) -> Result<Vec<LinkWithRepo>, ForgeError> {
    use crate::schema::{forge_connections, item_links};
    let rows: Vec<(ItemLink, Forge, String)> = item_links::table
        .inner_join(forge_connections::table)
        .filter(forge_connections::team_id.eq(team_id))
        .filter(item_links::state.eq_any(states))
        .filter(forge_connections::deleted_at.is_null())
        .order(item_links::forge_updated_at.desc())
        .limit(limit)
        .select((
            ItemLink::as_select(),
            forge_connections::forge,
            forge_connections::repo_full_name,
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
