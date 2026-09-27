//! Repository services (KAIROS-T-0103, design in KAIROS-I-0010, decision
//! KAIROS-A-0019): the CRUD behind `/api/repositories` and the one routing
//! decision every task-create path needs, [`route_task`]. A board or a team
//! chooses where a task goes; a repository is only the task's link to where
//! the code is (COLLIERY-T-0217, COLLIERY-A-0023).
//!
//! Every function operates in the CURRENT `search_path` tenant schema,
//! the same convention as [`crate::items`] and [`crate::forge`].
//!
//! # Why deletion is refused while referenced
//!
//! Tasks link to a repository and a webhook connection hangs off it. Soft-deleting it under them would leave
//! tasks pointing at nothing (`tasks.repository_id` has no `ON DELETE`)
//! and a live connection with a dead owner. The caller unbinds first;
//! the typed [`RepositoryError::InUse`] says what is still attached.

use chrono::Utc;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use uuid::Uuid;

use crate::models::enums::{ActivityAction, BoardLevel, Forge};
use crate::models::graph::NewActivityLogEntry;
use crate::models::repositories::{NewRepository, Repository, RepositoryChangeset};

/// Errors from the repository services.
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    /// No live repository with this id.
    #[error("repository {0} does not exist")]
    NotFound(Uuid),
    /// No live repository with this slug.
    #[error("repository {0:?} does not exist")]
    SlugNotFound(String),
    /// The slug fails [`kairos_core::repositories::is_valid_slug`].
    #[error("invalid repository slug {0:?}: expected ^[a-z0-9][a-z0-9-]{{1,62}}$")]
    InvalidSlug(String),
    /// A live repository already carries this slug.
    #[error("repository slug {0:?} is already taken")]
    SlugTaken(String),
    /// A live repository already covers this `(forge, full name)`.
    #[error("{forge} repository {repo:?} is already registered")]
    AlreadyRegistered { forge: Forge, repo: String },
    /// The owning team does not exist.
    #[error("team {0} does not exist")]
    TeamNotFound(Uuid),
    /// The team has no live delivery board to route tickets onto (or, in
    /// a misconfigured tenant, more than one).
    #[error("team {team} has {count} live delivery boards; exactly one is needed to route tasks")]
    NoDeliveryBoard { team: Uuid, count: usize },
    /// Deletion refused: live tasks and/or a live webhook connection still
    /// reference the repository.
    #[error(
        "repository {id} is still referenced by {tasks} live task(s) and {connections} live connection(s)"
    )]
    InUse {
        id: Uuid,
        tasks: i64,
        connections: i64,
    },
    /// `route_task`: the caller named neither a board nor a team
    /// (COLLIERY-T-0217, COLLIERY-A-0023). A repository beside them does not
    /// help: it is a link, and it does not choose a board. Until
    /// COLLIERY-T-0217 a repository alone was enough, and this error meant
    /// "no board and no repository".
    #[error(
        "name a board or a team (board_id or team_id); a repository is a link and does not \
         choose a board"
    )]
    NothingToRouteBy,
    /// The caller named a team that the board does not belong to
    /// (COLLIERY-T-0216): the board decides a task's team.
    #[error(
        "team {team} is not the team of board {board}; the board decides a task's team, \
         so name the board's own team or none"
    )]
    TeamNotBoardTeam {
        board: Uuid,
        board_team: Option<Uuid>,
        team: Uuid,
    },
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// Where a task write lands: the board, the team of that board, and the
/// repository the task links to.
///
/// The three are not equals (COLLIERY-T-0217, COLLIERY-A-0023). The board is
/// what the caller chose, by naming it or by naming its team. The team is
/// read from the board (COLLIERY-T-0216). The repository is an optional link
/// that says where the code is; it has no part in the other two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRoute {
    pub board_id: Uuid,
    pub team_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
}

/// THE routing decision for a task write (KAIROS-T-0112 moved it here from
/// the HTTP layer so HTTP, MCP and the board view share one implementation
/// in the crate that owns the data). The rule is COLLIERY-A-0023, which
/// amends KAIROS-A-0019 §2, and COLLIERY-T-0217 put it here:
///
/// 1. board → that board, and THAT BOARD'S team. A team named beside it
///    must be the board's own ([`RepositoryError::TeamNotBoardTeam`],
///    COLLIERY-T-0216);
/// 2. no board, a team → the team's ONE live delivery board
///    ([`delivery_board_for_team`], [`RepositoryError::NoDeliveryBoard`]);
/// 3. neither → [`RepositoryError::NothingToRouteBy`], with or without a
///    repository.
///
/// The repository takes no part in 1 to 3. When one is given it must be
/// live ([`RepositoryError::NotFound`] / [`RepositoryError::SlugNotFound`]),
/// and its id is carried into the route as the task's link. Its owning team
/// is NOT looked at: a task on any team's board may link to any live
/// repository.
///
/// # What this replaced
///
/// Until COLLIERY-T-0217 the repository chose the board (A-0019 §2): a
/// repository alone sent the task to its owner's delivery board, and a board
/// or a team named beside it had to agree with the owner (two mismatch
/// errors, both removed). That made one field do two jobs. Work that
/// one team does in another team's code could not be filed where the work is
/// done, and a change of owner left every bound task on a board the rule no
/// longer allowed.
pub fn route_task(
    conn: &mut PgConnection,
    board_id: Option<Uuid>,
    team_id: Option<Uuid>,
    repository: Option<&str>,
) -> Result<TaskRoute, RepositoryError> {
    let (board_id, board_team) = match (board_id, team_id) {
        (Some(board_id), team_id) => {
            // COLLIERY-T-0216: the board decides the team. A caller may
            // still NAME a team, and naming the board's own is harmless;
            // naming any other is refused rather than quietly ignored,
            // because the caller believes the task will be that team's work
            // and it will not be.
            let board_team: Option<Uuid> = {
                use crate::schema::boards;
                boards::table
                    .filter(boards::id.eq(board_id))
                    .select(boards::team_id)
                    .first::<Option<Uuid>>(conn)
                    .optional()?
                    .flatten()
            };
            if let Some(team_id) = team_id
                && Some(team_id) != board_team
            {
                return Err(RepositoryError::TeamNotBoardTeam {
                    board: board_id,
                    board_team,
                    team: team_id,
                });
            }
            (board_id, board_team)
        }
        // COLLIERY-T-0217: a team without a board names that team's delivery
        // board. Zero boards or several is refused, not guessed at.
        (None, Some(team_id)) => (delivery_board_for_team(conn, team_id)?, Some(team_id)),
        // Refused BEFORE the repository is resolved, so a caller that sent
        // only a repository is told what is missing, not that its repository
        // is unknown.
        (None, None) => return Err(RepositoryError::NothingToRouteBy),
    };
    // The link. Resolving proves the repository is live and turns a slug
    // into an id; the owner on the row is deliberately not read.
    let repository_id = match repository {
        Some(reference) => Some(resolve(conn, reference)?.id),
        None => None,
    };
    Ok(TaskRoute {
        board_id,
        team_id: board_team,
        repository_id,
    })
}

/// Every live repository, optionally one team's, by slug.
pub fn list(
    conn: &mut PgConnection,
    team_id: Option<Uuid>,
) -> Result<Vec<Repository>, RepositoryError> {
    use crate::schema::repositories::dsl;
    let mut query = dsl::repositories
        .filter(dsl::deleted_at.is_null())
        .into_boxed();
    if let Some(team) = team_id {
        query = query.filter(dsl::team_id.eq(team));
    }
    Ok(query
        .order(dsl::slug.asc())
        .select(Repository::as_select())
        .load(conn)?)
}

/// One live repository by id, or [`RepositoryError::NotFound`].
pub fn load(conn: &mut PgConnection, id: Uuid) -> Result<Repository, RepositoryError> {
    use crate::schema::repositories::dsl;
    dsl::repositories
        .filter(dsl::id.eq(id))
        .filter(dsl::deleted_at.is_null())
        .select(Repository::as_select())
        .first(conn)
        .optional()?
        .ok_or(RepositoryError::NotFound(id))
}

/// One live repository by slug, or [`RepositoryError::SlugNotFound`].
pub fn load_by_slug(conn: &mut PgConnection, slug: &str) -> Result<Repository, RepositoryError> {
    use crate::schema::repositories::dsl;
    dsl::repositories
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(Repository::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| RepositoryError::SlugNotFound(slug.to_string()))
}

/// Resolve a repository reference as clients write it: a slug, or a UUID.
pub fn resolve(conn: &mut PgConnection, reference: &str) -> Result<Repository, RepositoryError> {
    match reference.parse::<Uuid>() {
        Ok(id) => load(conn, id),
        Err(_) => load_by_slug(conn, reference),
    }
}

/// The live repository for a `(forge, full name)`, if any — what
/// bootstrap matches a git remote against.
pub fn find_by_forge_name(
    conn: &mut PgConnection,
    forge: Forge,
    repo_full_name: &str,
) -> Result<Option<Repository>, RepositoryError> {
    use crate::schema::repositories::dsl;
    Ok(dsl::repositories
        .filter(dsl::forge.eq(forge))
        .filter(dsl::repo_full_name.eq(repo_full_name))
        .filter(dsl::deleted_at.is_null())
        .select(Repository::as_select())
        .first(conn)
        .optional()?)
}

/// Register a repository. The slug must be valid and free, the
/// `(forge, full name)` pair free, and the owning team live.
pub fn create(
    conn: &mut PgConnection,
    input: NewRepository,
) -> Result<Repository, RepositoryError> {
    if !kairos_core::repositories::is_valid_slug(&input.slug) {
        return Err(RepositoryError::InvalidSlug(input.slug));
    }
    require_team(conn, input.team_id)?;
    let slug = input.slug.clone();
    let forge = input.forge;
    let repo = input.repo_full_name.clone();
    let actor = input.created_by;
    conn.transaction::<_, RepositoryError, _>(|conn| {
        let created: Repository = diesel::insert_into(crate::schema::repositories::table)
            .values(input)
            .returning(Repository::as_returning())
            .get_result(conn)
            .map_err(|e| map_unique(e, &slug, forge, &repo))?;
        log_activity(
            conn,
            actor,
            created.id,
            format!("repository_created:{}", created.slug),
        )?;
        Ok(created)
    })
}

/// Edit a repository's mutable fields (slug, URL, default branch, owning
/// team, description). Changing the team does NOT touch the tasks that
/// link to the repository, now or on their next write: each task stays on
/// its board and keeps its link (COLLIERY-T-0219, COLLIERY-A-0023). Until
/// COLLIERY-T-0217 the owner chose the board, and the next write of such a
/// task checked it against the new owner.
pub fn update(
    conn: &mut PgConnection,
    id: Uuid,
    mut changes: RepositoryChangeset,
    actor: Uuid,
) -> Result<Repository, RepositoryError> {
    let current = load(conn, id)?;
    if let Some(slug) = &changes.slug
        && !kairos_core::repositories::is_valid_slug(slug)
    {
        return Err(RepositoryError::InvalidSlug(slug.clone()));
    }
    if let Some(team) = changes.team_id {
        require_team(conn, team)?;
    }
    changes.updated_by = Some(actor);
    changes.updated_at = Some(Utc::now());
    let slug = changes.slug.clone().unwrap_or_else(|| current.slug.clone());
    conn.transaction::<_, RepositoryError, _>(|conn| {
        use crate::schema::repositories::dsl;
        let updated: Repository = diesel::update(dsl::repositories.filter(dsl::id.eq(id)))
            .set(changes)
            .returning(Repository::as_returning())
            .get_result(conn)
            .map_err(|e| map_unique(e, &slug, current.forge, &current.repo_full_name))?;
        log_activity(
            conn,
            actor,
            id,
            format!("repository_updated:{}", updated.slug),
        )?;
        Ok(updated)
    })
}

/// Soft-delete a repository. Refused with [`RepositoryError::InUse`] while
/// live tasks or a live webhook connection still reference it.
pub fn soft_delete(conn: &mut PgConnection, id: Uuid, actor: Uuid) -> Result<(), RepositoryError> {
    let current = load(conn, id)?;
    conn.transaction::<_, RepositoryError, _>(|conn| {
        use crate::schema::repositories::dsl;
        // The reference check runs INSIDE the transaction (KAIROS-T-0112) so
        // a concurrent bind cannot slip between check and delete.
        let (tasks, connections) = references(conn, id)?;
        if tasks > 0 || connections > 0 {
            return Err(RepositoryError::InUse {
                id,
                tasks,
                connections,
            });
        }
        diesel::update(dsl::repositories.filter(dsl::id.eq(id)))
            .set((
                dsl::deleted_at.eq(Some(Utc::now())),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(Utc::now()),
            ))
            .execute(conn)?;
        log_activity(
            conn,
            actor,
            id,
            format!("repository_deleted:{}", current.slug),
        )?;
        Ok(())
    })
}

/// How many live tasks and live connections reference the repository.
///
/// Until COLLIERY-T-0219 a second count stood beside this one: the "stale"
/// tasks, linked here but on a board of a team that does not own the
/// repository. COLLIERY-A-0023 makes that the normal case, because a task on
/// any team's board may link to any repository. The count reported normal
/// work as a fault, so it is removed.
pub fn references(conn: &mut PgConnection, id: Uuid) -> Result<(i64, i64), RepositoryError> {
    use crate::schema::{forge_connections, tasks};
    let task_count: i64 = tasks::table
        .filter(tasks::repository_id.eq(id))
        .filter(tasks::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    let connection_count: i64 = forge_connections::table
        .filter(forge_connections::repository_id.eq(id))
        .filter(forge_connections::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    Ok((task_count, connection_count))
}

/// THE routing helper (A-0019 §2): the one live delivery board of a team.
/// Zero or several is a tenant misconfiguration surfaced as
/// [`RepositoryError::NoDeliveryBoard`] rather than a silent pick.
pub fn delivery_board_for_team(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Uuid, RepositoryError> {
    use crate::schema::boards::dsl;
    let boards: Vec<Uuid> = dsl::boards
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::board_level.eq(BoardLevel::Delivery))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .load(conn)?;
    match boards.as_slice() {
        [one] => Ok(*one),
        other => Err(RepositoryError::NoDeliveryBoard {
            team: team_id,
            count: other.len(),
        }),
    }
}

/// 422-worthy check that a live team exists.
fn require_team(conn: &mut PgConnection, team_id: Uuid) -> Result<(), RepositoryError> {
    use crate::schema::teams::dsl;
    let found: Option<Uuid> = dsl::teams
        .filter(dsl::id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()?;
    found
        .map(|_| ())
        .ok_or(RepositoryError::TeamNotFound(team_id))
}

/// The two partial unique indexes → typed errors. Postgres names the
/// violated constraint; that is how the two are told apart.
fn map_unique(e: DieselError, slug: &str, forge: Forge, repo: &str) -> RepositoryError {
    match &e {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
            if info.constraint_name() == Some("idx_repositories_slug") {
                RepositoryError::SlugTaken(slug.to_string())
            } else {
                RepositoryError::AlreadyRegistered {
                    forge,
                    repo: repo.to_string(),
                }
            }
        }
        _ => RepositoryError::Database(e),
    }
}

fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    repository_id: Uuid,
    details: String,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action: ActivityAction::Repository,
            entity_id: Some(repository_id),
            entity_type: Some("repository".to_string()),
            details,
        })
        .execute(conn)?;
    Ok(())
}

/// Per-repository counts the directory renders (KAIROS-T-0106), ONE query
/// for a whole list: live tasks not in a done column, and whether a live
/// webhook connection exists.
///
/// `open_tasks` counts every open task that links to the repository, on ALL
/// boards. It does not look at the team or the board of the task, so work
/// that a different team does in this code is counted (COLLIERY-T-0219,
/// COLLIERY-A-0023).
#[derive(Debug, Clone, QueryableByName)]
pub struct RepositoryCounts {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub repository_id: Uuid,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub open_tasks: i64,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub has_webhook: bool,
}

/// [`RepositoryCounts`] for every id in `ids` (repositories with no tasks
/// and no connection still get a row).
pub fn counts(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<Vec<RepositoryCounts>, RepositoryError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(diesel::sql_query(
        "SELECT r.id AS repository_id, \
                (SELECT count(*) FROM tasks t \
                   JOIN board_columns c ON c.id = t.column_id \
                  WHERE t.repository_id = r.id AND t.deleted_at IS NULL \
                    AND NOT c.is_done) AS open_tasks, \
                EXISTS (SELECT 1 FROM forge_connections fc \
                         WHERE fc.repository_id = r.id AND fc.deleted_at IS NULL) AS has_webhook \
           FROM repositories r \
          WHERE r.id = ANY($1)",
    )
    .bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(ids.to_vec())
    .load(conn)?)
}
