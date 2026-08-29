//! Team-page write services (KAIROS-T-0082, design in KAIROS-I-0007):
//! the seeded scaffold every team gets. Page/announcement CRUD arrives
//! with KAIROS-T-0083.
//!
//! The scaffold here and the SQL backfill in migration
//! `2026-08-29-000000_team_pages` MUST stay in step: new teams scaffold
//! through this function (inside the create-team transaction, the
//! delivery-board precedent), pre-existing teams through the migration.
//! Both are guarded per (team, parent, slug), so re-runs and partially
//! scaffolded teams converge.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::models::enums::TeamPageKind;
use crate::models::team_pages::{NewTeamPage, NewTeamPageHistory, TeamPage};

/// The charter's templated skeleton (mirrors the `team_charter` system
/// template's headings).
const CHARTER_CONTENT: &str =
    "# Team Charter\n\n## Mission\n\n## Scope\n\n## Ways of Working\n\n## Success Measures\n";

/// The Support Processes index page's starter content.
const SUPPORT_OVERVIEW_CONTENT: &str =
    "# Support Processes\n\nHow to reach this team and how support work is handled.\n";

/// The Documentation section folders, in display order (the diataxis
/// buckets plus planning and design docs — KAIROS-I-0007 requirements).
const DOC_SECTIONS: &[(&str, &str)] = &[
    ("planning", "Planning"),
    ("tutorials", "Tutorials"),
    ("how-to-guides", "How-to Guides"),
    ("reference", "Reference"),
    ("explanation", "Explanation"),
    ("design-docs", "Design Docs"),
];

/// Insert one scaffold node if no live-or-deleted sibling with the slug
/// exists yet; returns the node's id either way. Pages get a v1 history
/// baseline (the T-0012 "complete from birth" posture).
#[allow(clippy::too_many_arguments)]
fn ensure_node(
    conn: &mut PgConnection,
    team_id: Uuid,
    parent_id: Option<Uuid>,
    kind: TeamPageKind,
    slug: &str,
    title: &str,
    content: &str,
    position: i32,
    is_protected: bool,
    actor: Uuid,
) -> Result<Uuid, DieselError> {
    use crate::schema::team_pages::dsl;
    let base = dsl::team_pages
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::slug.eq(slug))
        .select(dsl::id);
    let existing: Option<Uuid> = match parent_id {
        Some(parent) => base
            .filter(dsl::parent_id.eq(parent))
            .first(conn)
            .optional()?,
        None => base
            .filter(dsl::parent_id.is_null())
            .first(conn)
            .optional()?,
    };
    if let Some(id) = existing {
        return Ok(id);
    }
    let created: TeamPage = diesel::insert_into(dsl::team_pages)
        .values(NewTeamPage {
            team_id,
            parent_id,
            kind,
            slug: slug.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            position,
            is_protected,
            created_by: actor,
            updated_by: actor,
        })
        .returning(TeamPage::as_returning())
        .get_result(conn)?;
    if kind == TeamPageKind::Page {
        diesel::insert_into(crate::schema::team_page_history::table)
            .values(NewTeamPageHistory {
                page_id: created.id,
                version: 1,
                title: created.title.clone(),
                content: created.content.clone(),
                edited_by: actor,
            })
            .execute(conn)?;
    }
    Ok(created.id)
}

/// Seed a team's opinionated scaffold (idempotent): the protected Team
/// Charter, Support Processes (folder + Overview page), and the
/// Documentation folder with its section subfolders. Runs in the
/// caller's transaction — team creation calls this alongside the
/// delivery-board creation so a team is never born bare.
pub fn seed_team_scaffold(
    conn: &mut PgConnection,
    team_id: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    ensure_node(
        conn,
        team_id,
        None,
        TeamPageKind::Page,
        "charter",
        "Team Charter",
        CHARTER_CONTENT,
        0,
        true,
        actor,
    )?;
    let support = ensure_node(
        conn,
        team_id,
        None,
        TeamPageKind::Folder,
        "support-processes",
        "Support Processes",
        "",
        1,
        false,
        actor,
    )?;
    ensure_node(
        conn,
        team_id,
        Some(support),
        TeamPageKind::Page,
        "overview",
        "Overview",
        SUPPORT_OVERVIEW_CONTENT,
        0,
        false,
        actor,
    )?;
    let docs = ensure_node(
        conn,
        team_id,
        None,
        TeamPageKind::Folder,
        "documentation",
        "Documentation",
        "",
        2,
        false,
        actor,
    )?;
    for (position, (slug, title)) in DOC_SECTIONS.iter().enumerate() {
        ensure_node(
            conn,
            team_id,
            Some(docs),
            TeamPageKind::Folder,
            slug,
            title,
            "",
            position as i32,
            false,
            actor,
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Page write services (KAIROS-T-0083)
// ---------------------------------------------------------------------------

use crate::models::team_pages::{TeamAnnouncement, TeamPageChangeset};

/// Content bytes cap for team pages and announcements (KAIROS-I-0007
/// decision: the tenant's first server-side content cap).
pub const MAX_CONTENT_BYTES: usize = 256 * 1024;

/// Typed rejections of the team-page write paths.
#[derive(Debug, thiserror::Error)]
pub enum TeamPageError {
    /// No live page with this id exists on this team.
    #[error("team page {0} does not exist")]
    PageNotFound(Uuid),
    /// A sibling with this slug already exists.
    #[error("a sibling with slug {0:?} already exists")]
    SlugConflict(String),
    /// The referenced parent is missing, deleted, on another team, or not
    /// a folder.
    #[error("parent {0} is not a live folder of this team")]
    BadParent(Uuid),
    /// The page is protected (the Charter): rename/move/delete refused.
    #[error("page {0} is protected; it cannot be renamed, moved, or deleted")]
    ProtectedPage(Uuid),
    /// A folder with live children cannot be deleted.
    #[error("folder {folder} still contains {children} live page(s)")]
    FolderNotEmpty { folder: Uuid, children: i64 },
    /// KAIROS-A-0004-style optimistic-concurrency conflict on content.
    #[error(
        "version conflict on {page_id}: submitted base version \
         {expected_version}, current version {current_version}"
    )]
    VersionConflict {
        page_id: Uuid,
        expected_version: i32,
        current_version: i32,
        current_title: String,
        current_content: String,
    },
    /// Content exceeds [`MAX_CONTENT_BYTES`].
    #[error("content is {actual} bytes; the limit is {MAX_CONTENT_BYTES}")]
    ContentTooLarge { actual: usize },
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// Load a live page of a team, or the typed not-found.
pub fn load_page(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
) -> Result<TeamPage, TeamPageError> {
    use crate::schema::team_pages::dsl;
    dsl::team_pages
        .filter(dsl::id.eq(page_id))
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .select(TeamPage::as_select())
        .first(conn)
        .optional()?
        .ok_or(TeamPageError::PageNotFound(page_id))
}

/// A team's live page tree, parents-with-position order (flat list; the
/// client nests by parent_id).
pub fn list_pages(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<TeamPage>, DieselError> {
    use crate::schema::team_pages::dsl;
    dsl::team_pages
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .order((dsl::position.asc(), dsl::title.asc()))
        .select(TeamPage::as_select())
        .load(conn)
}

/// Validate a prospective parent: a live folder of the same team.
fn check_parent(
    conn: &mut PgConnection,
    team_id: Uuid,
    parent_id: Uuid,
) -> Result<(), TeamPageError> {
    let parent = load_page(conn, team_id, parent_id).map_err(|_| TeamPageError::BadParent(parent_id))?;
    if parent.kind != TeamPageKind::Folder {
        return Err(TeamPageError::BadParent(parent_id));
    }
    Ok(())
}

fn check_size(content: &str) -> Result<(), TeamPageError> {
    if content.len() > MAX_CONTENT_BYTES {
        return Err(TeamPageError::ContentTooLarge {
            actual: content.len(),
        });
    }
    Ok(())
}

/// Input of [`create_page`].
#[derive(Debug, Clone)]
pub struct CreatePage<'a> {
    pub parent_id: Option<Uuid>,
    pub kind: TeamPageKind,
    pub slug: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub position: i32,
}

/// Create a page or folder; pages get the v1 history baseline. One
/// transaction.
pub fn create_page(
    conn: &mut PgConnection,
    team_id: Uuid,
    input: CreatePage<'_>,
    actor: Uuid,
) -> Result<TeamPage, TeamPageError> {
    conn.transaction::<_, TeamPageError, _>(|conn| {
        check_size(input.content)?;
        if let Some(parent) = input.parent_id {
            check_parent(conn, team_id, parent)?;
        }
        let created: TeamPage = diesel::insert_into(crate::schema::team_pages::table)
            .values(NewTeamPage {
                team_id,
                parent_id: input.parent_id,
                kind: input.kind,
                slug: input.slug.to_string(),
                title: input.title.to_string(),
                content: input.content.to_string(),
                position: input.position,
                is_protected: false,
                created_by: actor,
                updated_by: actor,
            })
            .returning(TeamPage::as_returning())
            .get_result(conn)
            .map_err(|e| match &e {
                DieselError::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                ) => TeamPageError::SlugConflict(input.slug.to_string()),
                _ => TeamPageError::Database(e),
            })?;
        if created.kind == TeamPageKind::Page {
            diesel::insert_into(crate::schema::team_page_history::table)
                .values(NewTeamPageHistory {
                    page_id: created.id,
                    version: 1,
                    title: created.title.clone(),
                    content: created.content.clone(),
                    edited_by: actor,
                })
                .execute(conn)?;
        }
        Ok(created)
    })
}

/// Version-checked title/content edit (the A-0004 pattern): the UPDATE
/// guards on the expected version; zero rows means conflict and the
/// current row rides in the error for the merge UI. Every save writes a
/// history row. One transaction.
pub fn update_page_content(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
    new_title: Option<&str>,
    new_content: &str,
    expected_version: i32,
    actor: Uuid,
) -> Result<TeamPage, TeamPageError> {
    conn.transaction::<_, TeamPageError, _>(|conn| {
        use crate::schema::team_pages::dsl;
        check_size(new_content)?;
        let current = load_page(conn, team_id, page_id)?;
        let title = new_title.unwrap_or(&current.title).to_string();
        let updated: Option<TeamPage> = diesel::update(
            dsl::team_pages
                .filter(dsl::id.eq(page_id))
                .filter(dsl::version.eq(expected_version)),
        )
        .set((
            dsl::title.eq(&title),
            dsl::content.eq(new_content),
            dsl::version.eq(dsl::version + 1),
            dsl::updated_by.eq(actor),
            dsl::updated_at.eq(diesel::dsl::now),
        ))
        .returning(TeamPage::as_returning())
        .get_result(conn)
        .optional()?;
        let Some(updated) = updated else {
            return Err(TeamPageError::VersionConflict {
                page_id,
                expected_version,
                current_version: current.version,
                current_title: current.title,
                current_content: current.content,
            });
        };
        diesel::insert_into(crate::schema::team_page_history::table)
            .values(NewTeamPageHistory {
                page_id,
                version: updated.version,
                title: updated.title.clone(),
                content: updated.content.clone(),
                edited_by: actor,
            })
            .execute(conn)?;
        Ok(updated)
    })
}

/// Rename and/or move a page (slug, parent, position). Refused for
/// protected pages; not version-checked (structure, not content).
pub fn rename_move_page(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
    new_slug: Option<&str>,
    new_parent: Option<Option<Uuid>>,
    new_position: Option<i32>,
    actor: Uuid,
) -> Result<TeamPage, TeamPageError> {
    conn.transaction::<_, TeamPageError, _>(|conn| {
        use crate::schema::team_pages::dsl;
        let current = load_page(conn, team_id, page_id)?;
        if current.is_protected {
            return Err(TeamPageError::ProtectedPage(page_id));
        }
        if let Some(Some(parent)) = new_parent {
            if parent == page_id {
                return Err(TeamPageError::BadParent(parent));
            }
            check_parent(conn, team_id, parent)?;
        }
        let updated: TeamPage = diesel::update(dsl::team_pages.filter(dsl::id.eq(page_id)))
            .set(TeamPageChangeset {
                slug: new_slug.map(str::to_string),
                parent_id: new_parent,
                position: new_position,
                updated_by: Some(actor),
                updated_at: Some(chrono::Utc::now()),
                ..Default::default()
            })
            .returning(TeamPage::as_returning())
            .get_result(conn)
            .map_err(|e| match &e {
                DieselError::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                ) => TeamPageError::SlugConflict(new_slug.unwrap_or(&current.slug).to_string()),
                _ => TeamPageError::Database(e),
            })?;
        Ok(updated)
    })
}

/// Soft-delete a page; folders must be empty of live children. Refused
/// for protected pages.
pub fn soft_delete_page(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
    actor: Uuid,
) -> Result<(), TeamPageError> {
    conn.transaction::<_, TeamPageError, _>(|conn| {
        use crate::schema::team_pages::dsl;
        let current = load_page(conn, team_id, page_id)?;
        if current.is_protected {
            return Err(TeamPageError::ProtectedPage(page_id));
        }
        let children: i64 = dsl::team_pages
            .filter(dsl::parent_id.eq(page_id))
            .filter(dsl::deleted_at.is_null())
            .count()
            .get_result(conn)?;
        if children > 0 {
            return Err(TeamPageError::FolderNotEmpty {
                folder: page_id,
                children,
            });
        }
        diesel::update(dsl::team_pages.filter(dsl::id.eq(page_id)))
            .set((
                dsl::deleted_at.eq(diesel::dsl::now),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;
        Ok(())
    })
}

/// A team's announcements, pinned first then newest first.
pub fn list_announcements(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Vec<TeamAnnouncement>, DieselError> {
    use crate::schema::team_announcements::dsl;
    dsl::team_announcements
        .filter(dsl::team_id.eq(team_id))
        .order((dsl::pinned.desc(), dsl::created_at.desc()))
        .select(TeamAnnouncement::as_select())
        .load(conn)
}
