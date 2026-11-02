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
    #[error("The forge connection {0} does not exist.")]
    ConnectionNotFound(Uuid),
    /// A live connection already covers this repository.
    #[error("The repository {repo:?} has a live connection already.")]
    RepoAlreadyConnected { repo: String },
    /// The repository lookup failed (not found, or a database error) —
    /// carried whole so the API maps it precisely (KAIROS-T-0116).
    #[error(transparent)]
    Repository(#[from] crate::repositories::RepositoryError),
    /// The connection's forge must match its repository's (`other` repos
    /// have no webhook dialect at all).
    #[error(
        "The forge {connection} of the connection is not the forge {repository} of the repository."
    )]
    ForgeMismatch {
        connection: Forge,
        repository: Forge,
    },
    #[error("The database gave an error: {0}.")]
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
    let repository = crate::repositories::load(conn, input.repository_id)?;
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
/// ordering lives here so every client agrees). A link with a connection
/// shows while the connection lives; a link of the cadence
/// (KAIROS-T-0670) has a repository and no connection.
pub fn links_for_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<LinkWithRepo>, ForgeError> {
    use diesel::sql_types::{Nullable, Text, Timestamptz, Uuid as SqlUuid};

    /// One row of the one statement below.
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
        #[diesel(sql_type = SqlUuid)]
        item_id: Uuid,
        #[diesel(sql_type = Nullable<SqlUuid>)]
        connection_id: Option<Uuid>,
        #[diesel(sql_type = Text)]
        kind: LinkKind,
        #[diesel(sql_type = Text)]
        external_id: String,
        #[diesel(sql_type = Text)]
        title: String,
        #[diesel(sql_type = Text)]
        url: String,
        #[diesel(sql_type = Text)]
        state: LinkState,
        #[diesel(sql_type = Text)]
        author: String,
        #[diesel(sql_type = Timestamptz)]
        forge_updated_at: DateTime<Utc>,
        #[diesel(sql_type = Timestamptz)]
        created_at: DateTime<Utc>,
        #[diesel(sql_type = Timestamptz)]
        updated_at: DateTime<Utc>,
        #[diesel(sql_type = Nullable<SqlUuid>)]
        repository_id: Option<Uuid>,
        #[diesel(sql_type = Text)]
        forge: Forge,
        #[diesel(sql_type = Text)]
        repo_full_name: String,
    }

    // ONE statement: a link with a connection shows while the connection
    // lives, and its repository is the one of the connection; a link
    // without a connection has its own repository.
    let rows: Vec<Row> = diesel::sql_query(
        "SELECT l.id, l.item_id, l.connection_id, l.kind, l.external_id, l.title, l.url, \
                l.state, l.author, l.forge_updated_at, l.created_at, l.updated_at, \
                l.repository_id, r.forge, r.repo_full_name \
           FROM item_links l \
           LEFT JOIN forge_connections c ON c.id = l.connection_id \
           JOIN repositories r ON r.id = COALESCE(c.repository_id, l.repository_id) \
          WHERE l.item_id = $1 AND (l.connection_id IS NULL OR c.deleted_at IS NULL) \
          ORDER BY l.kind DESC, l.forge_updated_at DESC",
    )
    .bind::<SqlUuid, _>(item_id)
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|row| LinkWithRepo {
            link: ItemLink {
                id: row.id,
                item_id: row.item_id,
                connection_id: row.connection_id,
                kind: row.kind,
                external_id: row.external_id,
                title: row.title,
                url: row.url,
                state: row.state,
                author: row.author,
                forge_updated_at: row.forge_updated_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
                repository_id: row.repository_id,
            },
            forge: row.forge,
            repo_full_name: row.repo_full_name,
        })
        .collect())
}

/// The link of an issue that the cadence took (KAIROS-T-0670): it has the
/// repository and no webhook connection. The caller makes it once, for the
/// task that it just made.
pub fn add_repository_link(
    conn: &mut PgConnection,
    repository_id: Uuid,
    link: NewItemLink,
) -> Result<(), ForgeError> {
    diesel::sql_query(
        "INSERT INTO item_links
             (item_id, repository_id, kind, external_id, title, url, state,
              author, forge_updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind::<diesel::sql_types::Uuid, _>(link.item_id)
    .bind::<diesel::sql_types::Uuid, _>(repository_id)
    .bind::<diesel::sql_types::Text, _>(link.kind)
    .bind::<diesel::sql_types::Text, _>(link.external_id)
    .bind::<diesel::sql_types::Text, _>(link.title)
    .bind::<diesel::sql_types::Text, _>(link.url)
    .bind::<diesel::sql_types::Text, _>(link.state)
    .bind::<diesel::sql_types::Text, _>(link.author)
    .bind::<diesel::sql_types::Timestamptz, _>(link.forge_updated_at)
    .execute(conn)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Repository-level branches and pull requests (KAIROS-T-0480)
// ---------------------------------------------------------------------------

/// One branch or pull request of a repository, as the forge reported it
/// (`forge_refs`). The webhook writes one for each event that the filter of
/// the repository admits, with or without a short code.
#[derive(Debug, Clone)]
pub struct NewForgeRef {
    pub repository_id: Uuid,
    pub kind: LinkKind,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub state: LinkState,
    pub author: String,
    pub head_branch: String,
    pub forge_updated_at: DateTime<Utc>,
}

/// Insert or advance one `forge_refs` row, ordering-safely: the row
/// changes only when the event is at least as recent as the stored one
/// (the guard of [`upsert_link`]). Returns `true` when the row was created
/// or advanced.
pub fn upsert_ref(conn: &mut PgConnection, row: NewForgeRef) -> Result<bool, ForgeError> {
    // Sanctioned raw SQL (A-0009), for the same reason as `upsert_link`.
    let affected = diesel::sql_query(
        "INSERT INTO forge_refs
             (repository_id, kind, external_id, title, url, state, author,
              head_branch, forge_updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (repository_id, kind, external_id) DO UPDATE
            SET title = EXCLUDED.title,
                url = EXCLUDED.url,
                state = EXCLUDED.state,
                author = EXCLUDED.author,
                head_branch = EXCLUDED.head_branch,
                forge_updated_at = EXCLUDED.forge_updated_at,
                updated_at = now()
          WHERE forge_refs.forge_updated_at <= EXCLUDED.forge_updated_at",
    )
    .bind::<diesel::sql_types::Uuid, _>(row.repository_id)
    .bind::<diesel::sql_types::Text, _>(row.kind)
    .bind::<diesel::sql_types::Text, _>(row.external_id)
    .bind::<diesel::sql_types::Text, _>(row.title)
    .bind::<diesel::sql_types::Text, _>(row.url)
    .bind::<diesel::sql_types::Text, _>(row.state)
    .bind::<diesel::sql_types::Text, _>(row.author)
    .bind::<diesel::sql_types::Text, _>(row.head_branch)
    .bind::<diesel::sql_types::Timestamptz, _>(row.forge_updated_at)
    .execute(conn)?;
    Ok(affected > 0)
}

/// A `forge_refs` row with its repository and the live items linked to it
/// (none: the row is not linked).
#[derive(Debug, Clone, PartialEq, Eq, diesel::QueryableByName)]
pub struct ForgeRefRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub kind: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub external_id: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub title: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub url: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub state: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub author: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub head_branch: String,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    pub forge_updated_at: DateTime<Utc>,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub forge: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub repo_full_name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub repository_slug: String,
    #[diesel(sql_type = diesel::sql_types::Array<diesel::sql_types::Text>)]
    pub item_short_codes: Vec<String>,
}

/// The select list and joins of [`ForgeRefRow`]. `f` is `forge_refs`, `r`
/// the live repository.
const REF_SELECT: &str = "SELECT f.kind, f.external_id, f.title, f.url, f.state, f.author, \
        f.head_branch, f.forge_updated_at, r.forge, r.repo_full_name, \
        r.slug AS repository_slug, \
        ARRAY(SELECT d.short_code FROM item_links l \
                JOIN forge_connections c ON c.id = l.connection_id AND c.deleted_at IS NULL \
                JOIN entity_directory d ON d.id = l.item_id AND d.deleted_at IS NULL \
               WHERE c.repository_id = f.repository_id AND l.kind = f.kind \
                 AND l.external_id = f.external_id \
               ORDER BY d.short_code) AS item_short_codes \
     FROM forge_refs f \
     JOIN repositories r ON r.id = f.repository_id AND r.deleted_at IS NULL";

/// The branches and pull requests of one repository in the given states,
/// newest first, each with its linked items (KAIROS-T-0480).
pub fn repository_refs(
    conn: &mut PgConnection,
    repository_id: Uuid,
    states: &[&str],
    limit: i64,
) -> Result<Vec<ForgeRefRow>, ForgeError> {
    Ok(diesel::sql_query(format!(
        "{REF_SELECT} WHERE f.repository_id = $1 AND f.state = ANY($2) \
         ORDER BY f.forge_updated_at DESC LIMIT $3"
    ))
    .bind::<diesel::sql_types::Uuid, _>(repository_id)
    .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(
        states.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)?)
}

/// A row of [`repository_unlinked_refs`].
#[derive(diesel::QueryableByName)]
struct PagedRefRow {
    #[diesel(embed)]
    item: ForgeRefRow,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    total: i64,
}

/// The branches and pull requests of one repository in the given states
/// that are NOT linked to an item, newest first: at most `limit` rows after
/// `offset`, and the count of all of them (KAIROS-T-0561). The filter is in
/// the query, so that a page has `limit` rows when there are. A page past
/// the end runs the query once more for the count.
pub fn repository_unlinked_refs(
    conn: &mut PgConnection,
    repository_id: Uuid,
    states: &[&str],
    limit: i64,
    offset: i64,
) -> Result<(Vec<ForgeRefRow>, i64), ForgeError> {
    let states: Vec<String> = states.iter().map(|s| s.to_string()).collect();
    let sql = format!(
        "SELECT refs.*, count(*) OVER () AS total \
           FROM ({REF_SELECT} WHERE f.repository_id = $1 AND f.state = ANY($2)) AS refs \
          WHERE cardinality(refs.item_short_codes) = 0 \
          ORDER BY refs.forge_updated_at DESC, refs.kind, refs.external_id \
          LIMIT $3 OFFSET $4"
    );
    let mut page = |limit: i64, offset: i64| -> Result<Vec<PagedRefRow>, ForgeError> {
        Ok(diesel::sql_query(&sql)
            .bind::<diesel::sql_types::Uuid, _>(repository_id)
            .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(&states)
            .bind::<diesel::sql_types::BigInt, _>(limit)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn)?)
    };
    let rows = page(limit, offset)?;
    let total = match rows.first() {
        Some(row) => row.total,
        None if offset > 0 => page(1, 0)?.first().map_or(0, |row| row.total),
        None => 0,
    };
    Ok((rows.into_iter().map(|r| r.item).collect(), total))
}

/// The branches and pull requests in the given states that are NOT linked
/// to an item, in the repositories that the team owns, newest first
/// (KAIROS-T-0480: the unlinked half of the team rollup).
pub fn team_unlinked_refs(
    conn: &mut PgConnection,
    team_id: Uuid,
    states: &[&str],
    limit: i64,
) -> Result<Vec<ForgeRefRow>, ForgeError> {
    Ok(diesel::sql_query(format!(
        "SELECT * FROM ({REF_SELECT} WHERE r.team_id = $1 AND f.state = ANY($2)) AS refs \
         WHERE cardinality(refs.item_short_codes) = 0 \
         ORDER BY refs.forge_updated_at DESC LIMIT $3"
    ))
    .bind::<diesel::sql_types::Uuid, _>(team_id)
    .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(
        states.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)?)
}
