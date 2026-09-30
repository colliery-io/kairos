//! The `impacts` links (COLLIERY-T-0269, COLLIERY-I-0019): a document or an
//! ADR says which repository it is ABOUT.
//!
//! The owner decided the model on 2026-09-29. A document has two links,
//! and they say two different things:
//!
//! | Link | What it says |
//! |---|---|
//! | document -> board | The OWNER. The board gives the right to edit. |
//! | document -> repository, `impacts` | What the document is about. It gives NO right. |
//!
//! So nothing in [`crate::abac`] reads this module. A member of the team
//! that owns a repository gets no right on a document that impacts the
//! repository, and a person who may edit the document needs no right on
//! the repository to write the link.
//!
//! # Why a table of its own
//!
//! The links are rows of `item_impacts` and not of `item_relationships`.
//! Each end of an `item_relationships` row is an item, and each reader of
//! that table has that rule: the graph view and the neighbours
//! ([`crate::graph`]), the traversal of the search ([`crate::search`]),
//! the cascade of an archive ([`crate::items`]). A repository is not an
//! item: it has no short code, no version and no row in
//! `entity_directory`. The API and the tools show the link as the
//! relationship `impacts`.
//!
//! # The ends
//!
//! The SUBJECT is a document or an ADR ([`is_subject`]). A task links to a
//! repository with `tasks.repository_id` (`set_repository`), which says
//! where the code of the task is.
//!
//! The TARGET is a repository. `target_kind` is in each row, so a later
//! target kind (a team, the organization) is a new value and no change of
//! the rows that a tenant has.
//!
//! # Liveness
//!
//! A write needs a live subject and, for a new link, a live repository,
//! as each write of [`crate::graph`] does. A link stays when its
//! repository or its subject is archived: a read gives it with
//! `archived_at`, and the restore of the subject needs no repair. A link
//! to an archived repository can be removed.
//!
//! # Audit rows and events
//!
//! The add writes `action = 'relationship_add'`, and the remove
//! `action = 'relationship_remove'`, as the edges of [`crate::graph`] do.
//! `details` is `relationship:impacts:{short_code}->repository:{slug}`.
//! `entity_id` is the subject: a repository is not an item, so the subject
//! is the one item of the entry. Each write emits `relationship_changed`
//! for the subject.
//!
//! # Duplicate links
//!
//! `UNIQUE (item_id, target_kind, target_id)` is the enforcement. The add
//! of a link that is there is the typed [`ImpactError::AlreadyLinked`],
//! the convention of [`crate::graph::link_items`].
//!
//! Every public function operates in the CURRENT `search_path` tenant
//! schema.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Array, Bool, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;

use crate::events::{self, EventKind};
use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;

/// The name of the relationship, as the API and the tools show it.
pub const RELATIONSHIP: &str = "impacts";

/// The one target kind that a link can have.
pub const TARGET_REPOSITORY: &str = "repository";

/// Can an item of this type be the subject of an `impacts` link? A
/// document and an ADR can.
pub fn is_subject(item_type: ItemType) -> bool {
    matches!(item_type, ItemType::Document | ItemType::Adr)
}

/// Errors from the `impacts` services.
#[derive(Debug, thiserror::Error)]
pub enum ImpactError {
    /// No live item has this id.
    #[error("The item {0} does not exist.")]
    ItemNotFound(Uuid),
    /// The item is not a document and not an ADR.
    #[error("{short_code} is a {item_type}. Only a document or an ADR can impact a repository.")]
    SubjectType {
        item_type: ItemType,
        short_code: String,
    },
    /// No live repository has this id.
    #[error("No live repository has the id {0}.")]
    RepositoryNotFound(Uuid),
    /// The link is there already: nothing changed, and no activity row
    /// was written.
    #[error("{short_code} impacts the repository {repository:?} already.")]
    AlreadyLinked {
        short_code: String,
        repository: String,
    },
    /// No such link exists to remove.
    #[error("{short_code} does not impact the repository {repository:?}.")]
    NotLinked {
        short_code: String,
        repository: String,
    },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// One repository that an item impacts.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ImpactedRepository {
    /// The id of the link.
    #[diesel(sql_type = SqlUuid)]
    pub impact_id: Uuid,
    /// The subject of the link.
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub repository_id: Uuid,
    #[diesel(sql_type = Text)]
    pub slug: String,
    /// `github|gitlab|other`.
    #[diesel(sql_type = Text)]
    pub forge: String,
    #[diesel(sql_type = Text)]
    pub repo_full_name: String,
    /// Set when the repository is archived. The link stays.
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub archived_at: Option<DateTime<Utc>>,
    /// When the link was made.
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// One document or ADR that impacts a repository.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ImpactingItem {
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    /// `document` or `adr`.
    #[diesel(sql_type = Text)]
    pub entity_type: String,
    #[diesel(sql_type = Text)]
    pub short_code: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    /// The value of the metadata `document_type` (documents only, and only
    /// when the document has one).
    #[diesel(sql_type = Nullable<Text>)]
    pub document_type: Option<String>,
    /// The editorial lifecycle (documents only).
    #[diesel(sql_type = Nullable<Text>)]
    pub lifecycle: Option<String>,
    /// The name of the column (ADRs on a board only).
    #[diesel(sql_type = Nullable<Text>)]
    pub column_name: Option<String>,
    /// Set when the item is archived.
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(QueryableByName)]
struct SubjectRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    short_code: String,
}

/// The type and the short code of a LIVE item that can be a subject.
fn live_subject(conn: &mut PgConnection, item_id: Uuid) -> Result<(ItemType, String), ImpactError> {
    let row: Option<SubjectRow> = sql_query(
        "SELECT entity_type, short_code FROM entity_directory \
          WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind::<SqlUuid, _>(item_id)
    .get_result(conn)
    .optional()?;
    let row = row.ok_or(ImpactError::ItemNotFound(item_id))?;
    let item_type = ItemType::ALL
        .iter()
        .copied()
        .find(|t| t.entity_type() == row.entity_type)
        .ok_or(ImpactError::ItemNotFound(item_id))?;
    if !is_subject(item_type) {
        return Err(ImpactError::SubjectType {
            item_type,
            short_code: row.short_code,
        });
    }
    Ok((item_type, row.short_code))
}

/// The slug of a repository, archived or not.
fn repository_slug(
    conn: &mut PgConnection,
    repository_id: Uuid,
    live_only: bool,
) -> Result<Option<String>, DieselError> {
    use crate::schema::repositories::dsl;
    let mut query = dsl::repositories
        .filter(dsl::id.eq(repository_id))
        .select(dsl::slug)
        .into_boxed();
    if live_only {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query.first(conn).optional()
}

fn log_impact_activity(
    conn: &mut PgConnection,
    actor: Uuid,
    action: ActivityAction,
    (item_id, item_type, short_code): (Uuid, ItemType, &str),
    repository: &str,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: actor,
            action,
            entity_id: Some(item_id),
            entity_type: Some(item_type.entity_type().to_string()),
            details: format!(
                "relationship:{RELATIONSHIP}:{short_code}->{TARGET_REPOSITORY}:{repository}"
            ),
        })
        .execute(conn)?;
    Ok(())
}

/// Make the link "`item_id` impacts `repository_id`", in ONE transaction:
///
/// 1. the item is live, and it is a document or an ADR
///    ([`ImpactError::ItemNotFound`], [`ImpactError::SubjectType`]);
/// 2. the repository is live ([`ImpactError::RepositoryNotFound`]);
/// 3. insert (a link that is there is [`ImpactError::AlreadyLinked`]),
///    write the `relationship_add` activity row, and emit
///    `relationship_changed` for the item.
///
/// NO PERMISSION RULE, as the services of [`crate::graph`]: the caller
/// asks the edit rule of the item first.
pub fn link(
    conn: &mut PgConnection,
    item_id: Uuid,
    repository_id: Uuid,
    actor: Uuid,
) -> Result<ImpactedRepository, ImpactError> {
    conn.transaction::<_, ImpactError, _>(|conn| {
        let (item_type, short_code) = live_subject(conn, item_id)?;
        let slug = repository_slug(conn, repository_id, true)?
            .ok_or(ImpactError::RepositoryNotFound(repository_id))?;

        let inserted = sql_query(
            "INSERT INTO item_impacts (item_id, item_type, target_kind, target_id, created_by) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind::<SqlUuid, _>(item_id)
        .bind::<Text, _>(item_type.entity_type())
        .bind::<Text, _>(TARGET_REPOSITORY)
        .bind::<SqlUuid, _>(repository_id)
        .bind::<SqlUuid, _>(actor)
        .execute(conn);
        match inserted {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(ImpactError::AlreadyLinked {
                    short_code,
                    repository: slug,
                });
            }
            other => {
                other?;
            }
        }

        log_impact_activity(
            conn,
            actor,
            ActivityAction::RelationshipAdd,
            (item_id, item_type, &short_code),
            &slug,
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::RelationshipChanged,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        repositories_of(conn, item_id)?
            .into_iter()
            .find(|repository| repository.repository_id == repository_id)
            .ok_or(ImpactError::Database(DieselError::NotFound))
    })
}

/// Remove the link "`item_id` impacts `repository_id`", in ONE
/// transaction, with the `relationship_remove` activity row and the
/// `relationship_changed` event. The item must be live. The repository can
/// be archived. A link that is not there is [`ImpactError::NotLinked`]:
/// nothing changed, and no activity row was written.
pub fn unlink(
    conn: &mut PgConnection,
    item_id: Uuid,
    repository_id: Uuid,
    actor: Uuid,
) -> Result<(), ImpactError> {
    conn.transaction::<_, ImpactError, _>(|conn| {
        let (item_type, short_code) = live_subject(conn, item_id)?;
        let slug = repository_slug(conn, repository_id, false)?
            .ok_or(ImpactError::RepositoryNotFound(repository_id))?;

        let deleted = sql_query(
            "DELETE FROM item_impacts \
              WHERE item_id = $1 AND target_kind = $2 AND target_id = $3",
        )
        .bind::<SqlUuid, _>(item_id)
        .bind::<Text, _>(TARGET_REPOSITORY)
        .bind::<SqlUuid, _>(repository_id)
        .execute(conn)?;
        if deleted == 0 {
            return Err(ImpactError::NotLinked {
                short_code,
                repository: slug,
            });
        }

        log_impact_activity(
            conn,
            actor,
            ActivityAction::RelationshipRemove,
            (item_id, item_type, &short_code),
            &slug,
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::RelationshipChanged,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        Ok(())
    })
}

const REPOSITORIES_OF_SQL: &str = "\
    SELECT i.id AS impact_id, i.item_id, r.id AS repository_id, r.slug, \
           r.forge::text AS forge, r.repo_full_name, r.deleted_at AS archived_at, \
           i.created_at \
      FROM item_impacts i \
      JOIN repositories r ON r.id = i.target_id \
     WHERE i.target_kind = 'repository' AND i.item_id = ANY($1) \
     ORDER BY r.slug, r.deleted_at NULLS FIRST, i.created_at";

/// The repositories that one item impacts, by slug. An archived repository
/// is in the list, with `archived_at`. The item can be archived too: the
/// links are a property of the item, as its edges are.
pub fn repositories_of(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<ImpactedRepository>, ImpactError> {
    Ok(sql_query(REPOSITORIES_OF_SQL)
        .bind::<Array<SqlUuid>, _>(vec![item_id])
        .load(conn)?)
}

/// [`repositories_of`] for MANY items, in one query: what a list of
/// documents needs. An item with no link has no entry.
pub fn repositories_of_items(
    conn: &mut PgConnection,
    item_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<ImpactedRepository>>, ImpactError> {
    let mut by_item: HashMap<Uuid, Vec<ImpactedRepository>> = HashMap::new();
    if item_ids.is_empty() {
        return Ok(by_item);
    }
    let rows: Vec<ImpactedRepository> = sql_query(REPOSITORIES_OF_SQL)
        .bind::<Array<SqlUuid>, _>(item_ids)
        .load(conn)?;
    for row in rows {
        by_item.entry(row.item_id).or_default().push(row);
    }
    Ok(by_item)
}

/// The repository that `reference` names (a slug or a UUID) among the
/// repositories that the item IMPACTS, archived or not. `None` = the item
/// has no link to a repository with that name.
///
/// WHY not [`crate::repositories::resolve`]. That function gives live
/// repositories only, so a link to an archived repository could not be
/// removed. A slug is unique among the live repositories only: with a
/// live one and an archived one of one slug in the links of the item, the
/// live one is the answer.
pub fn linked_repository(
    conn: &mut PgConnection,
    item_id: Uuid,
    reference: &str,
) -> Result<Option<ImpactedRepository>, ImpactError> {
    let id = reference.parse::<Uuid>().ok();
    Ok(repositories_of(conn, item_id)?
        .into_iter()
        .find(|repository| match id {
            Some(id) => repository.repository_id == id,
            None => repository.slug == reference,
        }))
}

/// The documents and the ADRs that impact one repository, by short code.
/// `include_archived: false` gives the live ones only, which is the
/// default of each list (KAIROS-A-0020). With `true`, an archived one is
/// in the list, with `archived_at`.
///
/// The repository can be archived: the links stay.
pub fn items_of_repository(
    conn: &mut PgConnection,
    repository_id: Uuid,
    include_archived: bool,
) -> Result<Vec<ImpactingItem>, ImpactError> {
    Ok(sql_query(
        "SELECT * FROM ( \
            SELECT d.id AS item_id, 'document' AS entity_type, d.short_code, d.title, \
                   (SELECT m.value FROM item_metadata m \
                      JOIN metadata_definitions md ON md.id = m.metadata_definition_id \
                     WHERE m.item_id = d.id AND md.slug = 'document_type' \
                     LIMIT 1) AS document_type, \
                   d.lifecycle::text AS lifecycle, \
                   NULL::text AS column_name, \
                   d.deleted_at AS archived_at \
              FROM item_impacts i \
              JOIN documents d ON d.id = i.item_id \
             WHERE i.target_kind = 'repository' AND i.target_id = $1 \
            UNION ALL \
            SELECT a.id, 'adr', a.short_code, a.title, \
                   NULL::text, NULL::text, c.name, a.deleted_at \
              FROM item_impacts i \
              JOIN adrs a ON a.id = i.item_id \
              LEFT JOIN board_columns c ON c.id = a.column_id \
             WHERE i.target_kind = 'repository' AND i.target_id = $1 \
         ) items \
         WHERE ($2 OR archived_at IS NULL) \
         ORDER BY entity_type DESC, short_code",
    )
    .bind::<SqlUuid, _>(repository_id)
    .bind::<Bool, _>(include_archived)
    .load(conn)?)
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    item_id: Uuid,
}

/// The ids of the items that impact one repository, archived or not: the
/// set that a filter by repository keeps. The caller applies its own
/// liveness.
pub fn item_ids_of_repository(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<Vec<Uuid>, ImpactError> {
    let rows: Vec<IdRow> = sql_query(
        "SELECT item_id FROM item_impacts \
          WHERE target_kind = 'repository' AND target_id = $1",
    )
    .bind::<SqlUuid, _>(repository_id)
    .load(conn)?;
    Ok(rows.into_iter().map(|row| row.item_id).collect())
}
