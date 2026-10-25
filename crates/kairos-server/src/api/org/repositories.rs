//! `/api/repositories` (KAIROS-T-0106, design in KAIROS-I-0010 §D4,
//! decision KAIROS-A-0019): the repository directory — where the code is.
//! Every repository has exactly one owning team. A task on any team's board
//! may link to any repository; the owner does not choose the board
//! (COLLIERY-A-0023).
//!
//! The detail does not report "stale tasks" (COLLIERY-T-0219). That count
//! was the linked tasks on a board of a team that does not own the
//! repository, which COLLIERY-A-0023 makes normal work. `open_tasks` counts
//! the open tasks that link to the repository on all boards.
//!
//! Gating (A-0006, with one deliberate widening recorded in I-0010):
//! - reads are open tenant-wide, like teams and boards — an agent in one
//!   repo needs to find who owns another;
//! - **create and edit** are org admin OR a `manage_tasks` holder on the
//!   owning team's delivery board (which team membership implies,
//!   KAIROS-T-0072), so a team — or its agent during `/kairos:bootstrap` —
//!   can register its own repositories without an admin;
//! - **delete** is org admin only, and refused while live tasks or a live
//!   webhook connection still reference the repository.
//!
//! A document or an ADR can IMPACT a repository (COLLIERY-T-0269): the
//! link says what the item is about. The detail of a repository gives
//! those items. The link gives no right: the owner team of a repository
//! gets no right on a document that impacts it. The delete of a repository
//! does not look at the links, and they stay.
//!
//! Webhook wiring is a separate resource (`/api/forge-connections`,
//! [`super::forge`]) that hangs off a repository.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_forge::TeamLink;
use kairos_client::types_repositories as dto;
use kairos_db::models::enums::{BoardLevel, Forge};
use kairos_db::models::repositories::{NewRepository, Repository, RepositoryChangeset};
use kairos_db::models::teams::Team;
use kairos_db::repositories::{self, RepositoryError};
use serde_json::json;
use uuid::Uuid;

use super::super::{clamp_pagination, parse_enum, require_capability, require_structure};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 capability that lets a team manage its own repositories
/// (held implicitly by every member of the owning team).
const MANAGE: &str = "manage_tasks";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/api/repositories/{slug}",
            get(get_repository)
                .patch(update_repository)
                .delete(delete_repository),
        )
        .route(
            "/api/repositories/{slug}/import-issues",
            axum::routing::post(import_issues).get(issue_import),
        )
}

/// Start the import of the open issues of a repository (KAIROS-T-0483).
///
/// Organization admin only. The import is a job of the job queue
/// (KAIROS-T-0487).
///
/// Each open issue goes through the path of an opened-issue event. The
/// forge filter decides. Then Kairos makes one support task for each issue
/// id on the delivery board of the team. The import skips an issue that a
/// webhook or an earlier import took.
///
/// The repository needs a read token with Issues: read (GitHub) or
/// `read_api` (GitLab). It needs no webhook connection.
///
/// The answer is the job, at once. While an import of the repository waits
/// or runs, the answer is that import: one runs at a time. Read the counts
/// with `GET /api/repositories/{slug}/import-issues`.
///
/// The interval of the repository (`issue_import_interval`, KAIROS-T-0670)
/// runs the same import on a cadence.
#[utoipa::path(
    post,
    path = "/api/repositories/{slug}/import-issues",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 202, description = "The import job: a new one, or the one that waits or runs", body = dto::IssueImport),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The repository has no read token, or the forge `other`", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn import_issues(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    use crate::forge::import;
    use axum::response::IntoResponse;
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let key = state.config.secrets_key.clone();
    let github_api = state.config.writeback.api_url.clone();
    let max_tries = state.config.jobs.max_tries;
    let job = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &tenant_slug, None, user, "manage_teams")?;
            let target = import::target(conn, &tenant_slug, &slug, key.as_ref(), &github_api)?;
            let new = import::new_job(
                &tenant_slug,
                &target.repository,
                max_tries,
                import::Trigger::Manual,
            );
            let dedupe_key = new.dedupe_key.clone().unwrap_or_default();
            // The open job of the repository, or a new one. A job that ends
            // between the two reads leaves room for a new one: look again.
            for _ in 0..3 {
                if let Some(job) =
                    kairos_db::jobs::enqueue(conn, &new).map_err(ApiError::internal)?
                {
                    return Ok(job);
                }
                if let Some(job) =
                    kairos_db::jobs::open_by_key(conn, &dedupe_key).map_err(ApiError::internal)?
                {
                    return Ok(job);
                }
            }
            Err(ApiError::internal(
                "the import job was neither added nor found",
            ))
        })
        .await?;
    state.jobs.wake();
    Ok((StatusCode::ACCEPTED, Json(import::import_dto(&job))).into_response())
}

/// The last import of the open issues of a repository (KAIROS-T-0487).
///
/// Organization admin only. The answer gives the state of the import and
/// its counts. It also gives the interval of the repository and its sync
/// state (KAIROS-T-0670). The sync state has the last complete run, the next
/// run and the count of the record. It also has the counts of the last run
/// and the last error. A repository with an interval answers before its
/// first job, with the state `idle`.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/import-issues",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The last import", body = dto::IssueImport),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository, or no import ran", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn issue_import(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::IssueImport>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let job = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &tenant_slug, None, user, "manage_teams")?;
            let repository = repositories::resolve(conn, &slug).map_err(map_error)?;
            let key = crate::forge::import::job_key(&tenant_slug, repository.id);
            let job = kairos_db::jobs::last_by_key(conn, &key).map_err(ApiError::internal)?;
            let sync =
                kairos_db::issue_sync::get(conn, repository.id).map_err(ApiError::internal)?;
            // KAIROS-T-0670: a repository with the cadence on, or a sync
            // state, answers before its first job too.
            if job.is_none() && sync.is_none() && repository.issue_import_interval_secs.is_none() {
                return Err(ApiError::not_found(format!(
                    "The repository {} has no import of open issues. Start one: POST \
                     /api/repositories/{}/import-issues, or set issue_import_interval.",
                    repository.slug, repository.slug
                )));
            }
            let record = kairos_db::issue_sync::record_count(conn, repository.id)
                .map_err(ApiError::internal)?;
            Ok((job, repository, sync, record))
        })
        .await?;
    let (job, repository, sync, record) = job;
    let mut out = match job {
        Some(job) => crate::forge::import::import_dto(&job),
        None => crate::forge::import::idle_dto(&repository),
    };
    out.interval = kairos_core::forge_sync::format_interval(
        repository.issue_import_interval_secs.map(|s| s as u32),
    );
    out.sync = crate::forge::import::sync_dto(sync.as_ref(), record);
    if repository.issue_import_interval_secs.is_none() {
        out.sync.next_run_at = None;
    }
    Ok(Json(out))
}

/// [`RepositoryError`] → HTTP.
pub(crate) fn map_error(e: RepositoryError) -> ApiError {
    match e {
        RepositoryError::NotFound(id) => {
            ApiError::not_found(format!("No live repository has the id {id}."))
        }
        RepositoryError::SlugNotFound(slug) => {
            ApiError::not_found(format!("No live repository has the slug {slug:?}."))
        }
        // COLLIERY-T-0265: the refusal names the field, so that a form
        // shows it below the field `Slug`. KAIROS-T-0268: the text (of
        // `RepositoryError`) says what to do.
        e @ RepositoryError::InvalidSlug(_) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "slug" }))
        }
        // COLLIERY-T-0267: the text and the name of the field come from
        // the function that has the rule of the field.
        RepositoryError::InvalidField(fault) => {
            let field = fault.field;
            ApiError::validation(fault.message).with_details(json!({ "field": field }))
        }
        RepositoryError::SlugTaken(slug) => {
            ApiError::conflict(format!("A repository has the slug {slug:?} already."))
        }
        RepositoryError::AlreadyRegistered { forge, repo } => ApiError::conflict(format!(
            "The {forge} repository {repo:?} is in the directory already."
        )),
        RepositoryError::TeamNotFound(id) => {
            ApiError::validation(format!("The team {id} is not in the organization."))
        }
        RepositoryError::NoDeliveryBoard { team, count } => ApiError::validation(format!(
            "The team {team} has {count} live delivery boards. A team must have one live \
             delivery board."
        )),
        RepositoryError::InUse {
            id,
            tasks,
            connections,
        } => ApiError::conflict(format!(
            "The repository {id} is in use. The number of live tasks that link to it is \
             {tasks}, and the number of its live webhook connections is {connections}. \
             Remove each link and each connection. Then delete the repository."
        )),
        RepositoryError::Database(e) => ApiError::internal(e),
        routing
        @ (RepositoryError::TeamNotBoardTeam { .. } | RepositoryError::NothingToRouteBy) => {
            ApiError::validation(routing.to_string())
        }
    }
}

/// Resolve a team reference ([`crate::references::TEAM_FORMS`]) to its live
/// row, or 422.
pub(crate) fn resolve_team(conn: &mut PgConnection, reference: &str) -> Result<Team, ApiError> {
    use kairos_db::schema::teams::dsl;
    if let Some(team) = crate::references::team(conn, reference)? {
        return Ok(team);
    }
    let reference = crate::references::text(reference);
    // KAIROS-T-0685: the refusal lists the slugs of the teams. Each member
    // can read the teams of the organization (the A-0006 read rule).
    let slugs: Vec<String> = dsl::teams
        .filter(dsl::deleted_at.is_null())
        .order(dsl::slug.asc())
        .select(dsl::slug)
        .limit(TEAM_SLUGS_IN_REFUSAL + 1)
        .load(conn)
        .map_err(ApiError::internal)?;
    Err(ApiError::validation(unknown_team_text(reference, &slugs)))
}

/// How many team slugs the refusal of an unknown team lists at most.
pub(crate) const TEAM_SLUGS_IN_REFUSAL: i64 = 50;

/// The refusal of a team reference that names no team (KAIROS-T-0685).
/// `slugs` has at most [`TEAM_SLUGS_IN_REFUSAL`] + 1 entries; one more
/// than the limit means that there are more.
pub(crate) fn unknown_team_text(reference: &str, slugs: &[String]) -> String {
    let mut text = format!(
        "The team {reference:?} is not in the organization. Send the id or the slug of a \
         team of the organization."
    );
    let limit = TEAM_SLUGS_IN_REFUSAL as usize;
    if slugs.is_empty() {
        text.push_str(" The organization has no team.");
    } else if slugs.len() > limit {
        text.push_str(&format!(
            " The first {limit} slugs are: {}. GET /api/teams gives each team.",
            slugs[..limit].join(", ")
        ));
    } else {
        text.push_str(&format!(" The slugs are: {}.", slugs.join(", ")));
    }
    text
}

/// The team's ONE live delivery board, or `None` when it has none or
/// several — the same exactly-one semantics routing uses
/// ([`repositories::delivery_board_for_team`]), so the directory, the
/// manage gate and routing never disagree (KAIROS-T-0112).
pub(crate) fn delivery_board_of(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Option<Uuid>, ApiError> {
    match repositories::delivery_board_for_team(conn, team_id) {
        Ok(board) => Ok(Some(board)),
        Err(RepositoryError::NoDeliveryBoard { .. }) => Ok(None),
        Err(e) => Err(map_error(e)),
    }
}

/// Org admin, or `manage_tasks` on the team's delivery board (team
/// membership implies it), or `manage_structure` at the organization
/// (KAIROS-T-0337). A team with no delivery board is admin-only, or for a
/// holder of `manage_structure` at the organization.
///
/// COLLIERY-T-0266: the refusal names the team, and it says who can do the
/// action. Until then it was the refusal of [`require_capability`], which
/// gives the id of a board and no more. The code and the `details` are the
/// same as before.
pub(crate) fn require_manage_for_team(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    team_id: Uuid,
) -> Result<(), ApiError> {
    let board = delivery_board_of(conn, team_id)?;
    let refusal = match require_capability(conn, slug, board, user, MANAGE) {
        Ok(()) => return Ok(()),
        Err(refusal) => refusal,
    };
    if refusal.code != "FORBIDDEN" {
        return Err(refusal);
    }
    // KAIROS-T-0337: the repositories are part of the structure of the
    // organization. A grant of `manage_structure` at the organization
    // covers them (a grant on one board does not).
    if kairos_db::abac::holds_structure(conn, user, None).map_err(crate::api::map_abac_error)? {
        return Ok(());
    }
    let team: String = {
        use kairos_db::schema::teams::dsl;
        dsl::teams
            .filter(dsl::id.eq(team_id))
            .select(dsl::slug)
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?
            .unwrap_or_else(|| team_id.to_string())
    };
    let message = match board {
        Some(_) => format!(
            "This action requires the capability {MANAGE:?} on the delivery board of the \
             team {team:?}, the owner team of the repository. You do not have that \
             capability. Each member of the team has it, and an organization admin has \
             each capability. Ask a member of the team {team:?} or an organization admin \
             to do this."
        ),
        None => format!(
            "The team {team:?}, the owner team of the repository, does not have one live \
             delivery board. Thus this action requires the organization admin role. Ask \
             an organization admin to do this."
        ),
    };
    Err(ApiError::forbidden(message).with_details(refusal.details))
}

/// Add a repository to the directory as `user`: the ONE implementation of
/// `POST /api/repositories` and of the MCP tool `add_repository`
/// (COLLIERY-T-0266). The rule, the defaults and the refusals are here, so
/// the two surfaces cannot disagree. The activity row comes from
/// [`repositories::create`], with `user` as the actor.
pub(crate) fn add(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    body: dto::CreateRepositoryRequest,
) -> Result<dto::Repository, ApiError> {
    let forge: Forge = parse_enum(&body.forge, "forge", Forge::ALL)?;
    let team = resolve_team(conn, &body.team)?;
    require_manage_for_team(conn, tenant_slug, user, team.id)?;
    // KAIROS-T-0430: a branch in the body is set by hand. With no branch,
    // the builder follows the default branch of the forge, and `main` is
    // the value until its first fetch.
    let create = if body.default_branch.is_some() {
        repositories::create
    } else {
        repositories::create_following_forge
    };
    let created = create(
        conn,
        NewRepository {
            slug: body.slug.unwrap_or_else(|| {
                kairos_core::repositories::slug_from_full_name(&body.repo_full_name)
            }),
            forge,
            repo_full_name: body.repo_full_name,
            repo_url: body.repo_url,
            default_branch: body.default_branch.unwrap_or_else(|| "main".to_string()),
            team_id: team.id,
            description: body.description.unwrap_or_default(),
            created_by: user,
            updated_by: user,
        },
    )
    .map_err(map_error)?;
    render_one(conn, created)
}

/// The repository that `reference` names (slug or UUID), when `user` can
/// change it: the gate of [`change`], evaluated against the CURRENT owner
/// (COLLIERY-T-0266).
pub(crate) fn changeable(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
) -> Result<Repository, ApiError> {
    let current = repositories::resolve(conn, reference).map_err(map_error)?;
    require_manage_for_team(conn, tenant_slug, user, current.team_id)?;
    Ok(current)
}

/// What [`change`] did (COLLIERY-T-0267).
pub(crate) struct Change {
    /// The repository after the call.
    pub repository: dto::Repository,
    /// The names of the fields that have a new value, in the sequence of
    /// the body. It is empty when the call changed nothing.
    pub changed: Vec<&'static str>,
    /// The code index build jobs that the call cancelled: it set
    /// `code_index_build` to off (KAIROS-T-0398).
    pub cancelled_builds: usize,
}

/// Change a repository as `user`: the ONE implementation of
/// `PATCH /api/repositories/{slug}` and of the MCP tool `update_repository`
/// (COLLIERY-T-0266). The activity row comes from [`repositories::update`],
/// with `user` as the actor.
///
/// COLLIERY-T-0267: only a value that is different from the value of the
/// row goes to the write. When no value is different, the function writes
/// nothing: `updated_at` stays, and the activity log gets no row. Until
/// then the MCP tool made the comparison and REST made none, so a PATCH
/// with the values of the row wrote a new `updated_at` and an activity row.
pub(crate) fn change(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
    body: dto::UpdateRepositoryRequest,
) -> Result<Change, ApiError> {
    let current = changeable(conn, tenant_slug, user, reference)?;
    // COLLIERY-T-0267: a body with no field is an input that does nothing.
    // The routes of boards, teams and streams refuse it, and this route
    // does the same. A body whose values are equal to the stored values is
    // a success that writes nothing: a client can send back what it read.
    if body.slug.is_none()
        && body.repo_url.is_none()
        && body.default_branch.is_none()
        && body.default_branch_source.is_none()
        && body.team.is_none()
        && body.description.is_none()
        && body.code_index_build.is_none()
        && body.code_index_summaries.is_none()
        && body.site_enabled.is_none()
        && body.issue_import_interval.is_none()
        && body
            .forge_filters
            .as_ref()
            .is_none_or(dto::ForgeFilters::is_empty)
    {
        return Err(ApiError::validation(
            "The request has no field to change. Send one or more of slug, repo_url, \
             default_branch, default_branch_source, team, description, code_index_build, \
             code_index_summaries, site_enabled, issue_import_interval and forge_filters.",
        ));
    }
    // KAIROS-T-0670: the interval of the issue import. Org admin only, as
    // the manual import. The form and the limits are checked before
    // anything is written.
    let interval = match body.issue_import_interval.as_deref() {
        Some(text) => {
            require_capability(conn, tenant_slug, None, user, "manage_teams")?;
            let parsed = kairos_core::forge_sync::parse_interval(text).map_err(|e| {
                ApiError::validation(e.to_string())
                    .with_details(json!({ "field": "issue_import_interval" }))
            })?;
            if parsed.is_some() && matches!(current.forge, kairos_db::models::enums::Forge::Other) {
                return Err(ApiError::validation(format!(
                    "The repository {} has the forge `other`. Only a github repository or a \
                     gitlab repository has issues to import.",
                    current.slug
                ))
                .with_details(json!({ "field": "issue_import_interval" })));
            }
            Some(parsed.map(|secs| secs as i32))
        }
        None => None,
    };
    // KAIROS-T-0479: the forge filters are set by an org admin only. The
    // patterns are checked before anything is written.
    let filter_changes = match body.forge_filters.as_ref().filter(|f| !f.is_empty()) {
        Some(sent) => {
            require_capability(conn, tenant_slug, None, user, "manage_teams")?;
            let changes = filter_changes(conn, current.id, sent)?;
            for (field, rule) in &changes {
                kairos_core::forge_filter::validate(*field, rule)
                    .map_err(|e| map_filter_error(e.into()))?;
            }
            changes
        }
        None => Vec::new(),
    };
    // KAIROS-T-0430: a branch is set by hand, so it cannot also follow the
    // forge.
    if body.default_branch.is_some()
        && body.default_branch_source == Some(dto::DefaultBranchSource::Forge)
    {
        return Err(ApiError::validation(
            "The request has a default_branch and default_branch_source forge. A \
             default_branch is set by hand. Send default_branch to set the branch by hand, \
             or default_branch_source forge to follow the forge.",
        )
        .with_details(json!({ "field": "default_branch_source" })));
    }
    // KAIROS-T-0340: `hosted` needs a hosted provider on the tenant.
    if body.code_index_summaries == Some(dto::CodeIndexSummaries::Hosted) {
        let settings =
            kairos_db::code_index_settings::load_or_default(conn).map_err(ApiError::internal)?;
        if settings.summary_provider == "embedded" {
            return Err(ApiError::unprocessable(
                "CODE_INDEX_NO_HOSTED_PROVIDER",
                "The organization has no hosted provider of the summaries, so a repository cannot \
                 opt in. An organization admin sets one first: `kairos admin code-index-settings \
                 set --summary-provider ollama-cloud ...`, or the page Admin, Code index.",
            )
            .with_details(json!({ "field": "code_index_summaries" })));
        }
    }
    let team_id = body
        .team
        .as_deref()
        .map(|reference| resolve_team(conn, reference).map(|t| t.id))
        .transpose()?
        .filter(|team| *team != current.team_id);
    // KAIROS-T-0112: re-homing needs the NEW owner's consent too —
    // manage on both delivery boards (org admin bypasses both).
    if let Some(new_team) = team_id {
        require_manage_for_team(conn, tenant_slug, user, new_team)?;
    }
    let different = |new: Option<String>, old: &str| new.filter(|new| new != old);
    // KAIROS-T-0430: a default_branch sets the branch by hand, also when
    // it has the value of the row. `forge` takes the branch of the forge
    // at the last fetch, when the builder has read one.
    let branch_sent = body.default_branch.is_some();
    let (default_branch, default_branch_source) =
        match (body.default_branch, body.default_branch_source) {
            (Some(branch), _) => (
                different(Some(branch), &current.default_branch),
                Some(dto::DefaultBranchSource::Hand),
            ),
            (None, Some(dto::DefaultBranchSource::Forge)) => (
                current
                    .forge_branch
                    .clone()
                    .filter(|branch| branch != &current.default_branch),
                Some(dto::DefaultBranchSource::Forge),
            ),
            (None, source) => (None, source),
        };
    let default_branch_source = different(
        default_branch_source.map(|v| v.as_str().to_string()),
        &current.default_branch_source,
    );
    let changes = RepositoryChangeset {
        slug: different(body.slug, &current.slug),
        repo_url: different(body.repo_url, &current.repo_url),
        default_branch,
        default_branch_source,
        team_id,
        description: different(body.description, &current.description),
        code_index_build: different(
            body.code_index_build.map(|v| v.as_str().to_string()),
            &current.code_index_build,
        ),
        code_index_summaries: different(
            body.code_index_summaries.map(|v| v.as_str().to_string()),
            &current.code_index_summaries,
        ),
        site_enabled: body
            .site_enabled
            .filter(|enabled| *enabled != current.site_enabled),
        issue_import_interval_secs: interval
            .filter(|new| *new != current.issue_import_interval_secs),
        ..Default::default()
    };
    let changed: Vec<&'static str> = [
        ("slug", changes.slug.is_some()),
        ("repo_url", changes.repo_url.is_some()),
        // A branch that is now set by hand is a change of the branch,
        // also with the value of the row.
        (
            "default_branch",
            changes.default_branch.is_some()
                || (branch_sent && changes.default_branch_source.is_some()),
        ),
        (
            "default_branch_source",
            !branch_sent && changes.default_branch_source.is_some(),
        ),
        ("team", changes.team_id.is_some()),
        ("description", changes.description.is_some()),
        ("code_index_build", changes.code_index_build.is_some()),
        (
            "code_index_summaries",
            changes.code_index_summaries.is_some(),
        ),
        ("site_enabled", changes.site_enabled.is_some()),
        (
            "issue_import_interval",
            changes.issue_import_interval_secs.is_some(),
        ),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect();
    // KAIROS-T-0398: off cancels the build jobs that wait, in the
    // transaction of the switch. The sweep adds no new job for a
    // repository that is off, and a build that runs now goes on.
    let turned_off = changes.code_index_build.as_deref() == Some("off");
    // KAIROS-T-0670: a new interval makes the next run due now.
    let interval_on = matches!(changes.issue_import_interval_secs, Some(Some(_)));
    // KAIROS-T-0479: the forge filters, in the same transaction.
    let filters_changed = if filter_changes.is_empty() {
        false
    } else {
        let current_slug = current.slug.clone();
        let current_id = current.id;
        let names = crate::api::org::run_in_transaction(conn, |conn| {
            kairos_db::forge_filters::set(conn, current_id, &current_slug, &filter_changes, user)
                .map_err(map_filter_error)
        })?;
        !names.is_empty()
    };
    let mut changed = changed;
    if filters_changed {
        changed.push("forge_filters");
    }
    let (repository, cancelled_builds) = if changed.iter().all(|c| *c == "forge_filters") {
        (current, 0)
    } else {
        crate::api::org::run_in_transaction(conn, |conn| {
            let repository =
                repositories::update(conn, current.id, changes, user).map_err(map_error)?;
            let cancelled = if turned_off {
                crate::code_index::jobs::cancel_builds(conn, tenant_slug, &repository)?
            } else {
                0
            };
            if interval_on {
                kairos_db::issue_sync::schedule(conn, repository.id, chrono::Utc::now())
                    .map_err(ApiError::internal)?;
            }
            Ok((repository, cancelled))
        })?
    };
    Ok(Change {
        repository: render_one(conn, repository)?,
        changed,
        cancelled_builds,
    })
}

/// Render repositories with their team, delivery board and counts — two
/// batched queries for the whole list, never per row.
pub(crate) fn render(
    conn: &mut PgConnection,
    rows: Vec<Repository>,
) -> Result<Vec<dto::Repository>, ApiError> {
    use kairos_db::schema::{boards, teams};
    use std::collections::HashMap;

    let team_ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = rows.iter().map(|r| r.team_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let team_rows: Vec<Team> = teams::table
        .filter(teams::id.eq_any(&team_ids))
        .filter(teams::deleted_at.is_null())
        .select(Team::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    let by_team: HashMap<Uuid, Team> = team_rows.into_iter().map(|t| (t.id, t)).collect();
    // One delivery board per team, with the SAME exactly-one semantics as
    // routing: a team with two delivery boards shows none here rather than
    // an arbitrary one (KAIROS-T-0112).
    let board_rows: Vec<(Uuid, Uuid, String)> = boards::table
        .filter(boards::team_id.eq_any(team_ids.iter().map(|id| Some(*id)).collect::<Vec<_>>()))
        .filter(boards::board_level.eq(BoardLevel::Delivery))
        .filter(boards::deleted_at.is_null())
        .select((boards::team_id.assume_not_null(), boards::id, boards::slug))
        .load(conn)
        .map_err(ApiError::internal)?;
    let mut board_count: HashMap<Uuid, usize> = HashMap::new();
    for (team, _, _) in &board_rows {
        *board_count.entry(*team).or_default() += 1;
    }
    // KAIROS-T-0561: the slug too, so that `list_repositories` names the
    // board as `board_items` takes it.
    let board_of: HashMap<Uuid, (Uuid, String)> = board_rows
        .into_iter()
        .filter(|(team, _, _)| board_count.get(team) == Some(&1))
        .map(|(team, id, slug)| (team, (id, slug)))
        .collect();
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let counts: HashMap<Uuid, (i64, bool)> = repositories::counts(conn, &ids)
        .map_err(map_error)?
        .into_iter()
        .map(|c| (c.repository_id, (c.open_tasks, c.has_webhook)))
        .collect();
    // COLLIERY-T-3105: the status of the read token, never the token.
    let credentials =
        kairos_db::repository_credentials::statuses(conn, &ids).map_err(ApiError::internal)?;
    // KAIROS-T-0386: the write-back, never the write token.
    let write_tokens =
        kairos_db::writeback::token_statuses(conn, &ids).map_err(ApiError::internal)?;
    // KAIROS-T-0358: where the summaries of each repository are made now.
    let code_index_settings =
        kairos_db::code_index_settings::load_or_default(conn).map_err(ApiError::internal)?;
    // KAIROS-T-0399: the tripped breakers of the code index builder.
    let breakers: HashMap<Uuid, kairos_db::code_index_breakers::Breaker> =
        kairos_db::code_index_breakers::of(conn, &ids)
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|b| (b.repository_id, b))
            .collect();
    // KAIROS-T-0668: the languages that the newest ok build indexed by name.
    let scip_warnings: HashMap<Uuid, String> =
        kairos_db::code_index_builds::newest_scip_warnings(conn, &ids)
            .map_err(ApiError::internal)?;
    // KAIROS-T-0406: the live publish of each site, and the tenant of the
    // base path.
    let live_sites = kairos_db::sites::live(conn, &ids).map_err(ApiError::internal)?;
    let publishers: Vec<Uuid> = live_sites.values().map(|p| p.principal_id).collect();
    let publisher_names = kairos_db::sites::names(conn, &publishers).map_err(ApiError::internal)?;
    let tenant = kairos_db::sites::tenant_slug(conn).map_err(ApiError::internal)?;
    // KAIROS-T-0479: the forge filters of each repository.
    let filters = kairos_db::forge_filters::stored_for(conn, &ids).map_err(ApiError::internal)?;
    // KAIROS-T-0670: the sync state of the issue import, and the record.
    let syncs: HashMap<Uuid, kairos_db::issue_sync::IssueSync> =
        kairos_db::issue_sync::get_many(conn, &ids)
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|s| (s.repository_id, s))
            .collect();
    let record_counts =
        kairos_db::issue_sync::record_counts(conn, &ids).map_err(ApiError::internal)?;

    rows.into_iter()
        .map(|repo| {
            let team = by_team.get(&repo.team_id).ok_or_else(|| {
                ApiError::internal(format!("repository {} references missing team", repo.id))
            })?;
            let (open_tasks, has_webhook) = counts.get(&repo.id).copied().unwrap_or((0, false));
            let hosted = crate::code_index::hosted_summaries(&repo, &code_index_settings);
            let base_path = kairos_db::sites::base_path(&tenant, &team.slug, &repo.slug);
            let published = live_sites
                .get(&repo.id)
                .map(|p| super::sites::publish_dto_named(p, &publisher_names));
            let site = kairos_client::types_sites::RepositorySite {
                enabled: repo.site_enabled,
                needs_republish: published.as_ref().is_some_and(|p| p.base_path != base_path),
                base_path,
                published,
            };
            Ok(dto::Repository {
                id: repo.id.to_string(),
                slug: repo.slug,
                forge: repo.forge.to_string(),
                repo_full_name: repo.repo_full_name,
                repo_url: repo.repo_url,
                default_branch: repo.default_branch,
                default_branch_source: dto::DefaultBranchSource::parse(&repo.default_branch_source)
                    .unwrap_or_default(),
                forge_branch: repo.forge_branch,
                description: repo.description,
                team: dto::RepositoryTeam {
                    id: team.id.to_string(),
                    slug: team.slug.clone(),
                    name: team.name.clone(),
                },
                delivery_board_id: board_of.get(&repo.team_id).map(|(id, _)| id.to_string()),
                delivery_board_slug: board_of.get(&repo.team_id).map(|(_, slug)| slug.clone()),
                open_tasks,
                has_webhook,
                credential: crate::credentials::status_dto(credentials.get(&repo.id)),
                write_back: crate::credentials::write_back_dto(write_tokens.get(&repo.id)),
                code_index_build: dto::CodeIndexBuild::parse(&repo.code_index_build)
                    .unwrap_or_default(),
                code_index_summaries: dto::CodeIndexSummaries::parse(&repo.code_index_summaries)
                    .unwrap_or_default(),
                code_index_summaries_resolved: if hosted {
                    dto::CodeIndexSummaries::Hosted
                } else {
                    dto::CodeIndexSummaries::Embedded
                },
                code_index_breaker: match breakers.get(&repo.id) {
                    Some(breaker) => dto::CodeIndexBreaker {
                        tripped: true,
                        tripped_at: Some(breaker.tripped_at.to_rfc3339()),
                        reason: Some(breaker.reason.clone()),
                    },
                    None => dto::CodeIndexBreaker::default(),
                },
                // A row that is not a valid list gives no warning.
                code_index_scip_warnings: scip_warnings
                    .get(&repo.id)
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_default(),
                site,
                forge_filters: filters_dto(filters.get(&repo.id).map(Vec::as_slice).unwrap_or(&[])),
                issue_import_interval: kairos_core::forge_sync::format_interval(
                    repo.issue_import_interval_secs.map(|s| s as u32),
                ),
                issue_import_interval_secs: repo.issue_import_interval_secs.map(|s| s as u32),
                issue_sync: {
                    let mut sync = crate::forge::import::sync_dto(
                        syncs.get(&repo.id),
                        record_counts.get(&repo.id).copied().unwrap_or(0),
                    );
                    // An off cadence has no next run.
                    if repo.issue_import_interval_secs.is_none() {
                        sync.next_run_at = None;
                    }
                    sync
                },
                created_at: repo.created_at.to_rfc3339(),
                updated_at: repo.updated_at.to_rfc3339(),
            })
        })
        .collect()
}

/// A `forge_refs` row as the DTO (KAIROS-T-0480).
pub(crate) fn ref_dto(row: kairos_db::forge::ForgeRefRow) -> kairos_client::types_forge::ForgeRef {
    kairos_client::types_forge::ForgeRef {
        linked: !row.item_short_codes.is_empty(),
        kind: row.kind,
        external_id: row.external_id,
        title: row.title,
        url: row.url,
        state: row.state,
        author: row.author,
        head_branch: row.head_branch,
        forge: row.forge,
        repo_full_name: row.repo_full_name,
        repository: row.repository_slug,
        item_short_codes: row.item_short_codes,
        forge_updated_at: row.forge_updated_at.to_rfc3339(),
    }
}

/// The forge filters of a repository as the DTO: every field, with the
/// default where no row is stored (KAIROS-T-0479).
pub(crate) fn filters_dto(
    stored: &[(
        kairos_core::forge_filter::FilterField,
        kairos_core::forge_filter::FilterRule,
    )],
) -> dto::ForgeFilters {
    let mut out = dto::ForgeFilters::default();
    for field in kairos_core::forge_filter::FilterField::ALL {
        let rule = stored
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        out.set(
            field.as_str(),
            dto::ForgeFilterRule {
                include: Some(rule.include),
                exclude: Some(rule.exclude),
            },
        );
    }
    out
}

/// The changes of the forge filters in a request, merged with the stored
/// rules: a pattern that the request does not send stays.
fn filter_changes(
    conn: &mut PgConnection,
    repository_id: Uuid,
    sent: &dto::ForgeFilters,
) -> Result<
    Vec<(
        kairos_core::forge_filter::FilterField,
        kairos_core::forge_filter::FilterRule,
    )>,
    ApiError,
> {
    let current =
        kairos_db::forge_filters::rules(conn, repository_id).map_err(ApiError::internal)?;
    let mut out = Vec::new();
    for (field, rule) in current {
        if let Some(patch) = sent.get(field.as_str()) {
            out.push((
                field,
                kairos_core::forge_filter::FilterRule {
                    include: patch.include.clone().unwrap_or(rule.include),
                    exclude: patch.exclude.clone().unwrap_or(rule.exclude),
                },
            ));
        }
    }
    Ok(out)
}

/// A refused pattern → 422 that names it.
fn map_filter_error(e: kairos_db::forge_filters::FilterStoreError) -> ApiError {
    match e {
        kairos_db::forge_filters::FilterStoreError::Pattern(e) => {
            let field = format!("forge_filters.{}", e.name());
            ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
        }
        kairos_db::forge_filters::FilterStoreError::Database(e) => ApiError::internal(e),
    }
}

fn render_one(conn: &mut PgConnection, repo: Repository) -> Result<dto::Repository, ApiError> {
    let mut rendered = render(conn, vec![repo])?;
    Ok(rendered.remove(0))
}

/// Query of `GET /api/repositories`.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRepositoriesQuery {
    /// Only this team's repositories (UUID or slug).
    pub team: Option<String>,
    /// With `name`: the ONE repository registered under this forge
    /// (`github|gitlab|other`) and full name — how a checkout matches its
    /// git remote (KAIROS-T-0116). Returns an empty list when unknown.
    pub forge: Option<String>,
    /// `owner/repo`, paired with `forge`.
    pub name: Option<String>,
    /// Page size (default 50, max 200).
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    pub offset: Option<i64>,
}

/// The repository directory (open tenant-wide), by slug. `?team=` narrows
/// to one owning team.
///
/// The response is a page, `{items, total, limit, offset}`, as for each
/// other list (KAIROS-T-0268). Until 0.11 it was an array of each
/// repository.
#[utoipa::path(
    get,
    path = "/api/repositories",
    tag = "repositories",
    params(ListRepositoriesQuery),
    responses(
        (status = 200, description = "Page of repositories, by slug", body = ListEnvelope<dto::Repository>),
        (status = 422, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_repositories(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(query): ApiQuery<ListRepositoriesQuery>,
) -> Result<Json<ListEnvelope<dto::Repository>>, ApiError> {
    let (limit, offset) = clamp_pagination(&Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let page = state
        .blocking
        .run(&tenant.slug, move |conn| {
            if let (Some(forge), Some(name)) = (query.forge.as_deref(), query.name.as_deref()) {
                let forge: Forge = parse_enum(forge, "forge", Forge::ALL)?;
                let found =
                    repositories::find_by_forge_name(conn, forge, name).map_err(map_error)?;
                return render_page(conn, found.into_iter().collect(), limit, offset);
            }
            if query.forge.is_some() != query.name.is_some() {
                return Err(ApiError::validation(
                    "The parameters forge and name go together. Send the two, or send \
                     none of them.",
                ));
            }
            let team_id = query
                .team
                .as_deref()
                .map(|reference| resolve_team(conn, reference).map(|t| t.id))
                .transpose()?;
            let rows = repositories::list(conn, team_id).map_err(map_error)?;
            render_page(conn, rows, limit, offset)
        })
        .await?;
    Ok(Json(page))
}

/// One page of the directory: the rows from `offset`, `limit` at most, and
/// the count of each row. The directory of a tenant is small, so the page
/// is cut after the read.
pub(crate) fn render_page(
    conn: &mut PgConnection,
    rows: Vec<Repository>,
    limit: i64,
    offset: i64,
) -> Result<ListEnvelope<dto::Repository>, ApiError> {
    let total = rows.len() as i64;
    let rows: Vec<Repository> = rows
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok(ListEnvelope {
        items: render(conn, rows)?,
        total,
        limit,
        offset,
    })
}

/// The documents and the ADRs that impact a repository, as the wire type
/// (COLLIERY-T-0269): the ONE read for `GET /api/repositories/{slug}` and
/// for the MCP tool `get_repository`.
pub(crate) fn impacted_by(
    conn: &mut PgConnection,
    repository_id: Uuid,
    include_archived: bool,
) -> Result<Vec<dto::ImpactingItem>, ApiError> {
    Ok(
        kairos_db::impacts::items_of_repository(conn, repository_id, include_archived)
            .map_err(ApiError::internal)?
            .into_iter()
            .map(impacting_item_dto)
            .collect(),
    )
}

/// The most ADRs of the owner team that a repository read lists
/// (KAIROS-T-0599). The total says how many more there are.
pub(crate) const OWNER_TEAM_ADRS_LIMIT: i64 = 50;

/// The ADRs of the owner team of a repository that no `impacts` link
/// names (KAIROS-T-0599), newest first, [`OWNER_TEAM_ADRS_LIMIT`] at
/// most, and the count of all of them: the ONE read for
/// `GET /api/repositories/{slug}` and for the MCP tool `get_repository`.
/// An ADR with a link is in [`impacted_by`] of the repository it names,
/// so no ADR shows twice. A superseded ADR is not in the list.
pub(crate) fn owner_team_adrs(
    conn: &mut PgConnection,
    team_id: Uuid,
    include_archived: bool,
) -> Result<(Vec<dto::ImpactingItem>, i64), ApiError> {
    let (items, total) = kairos_db::impacts::team_adrs_without_impacts(
        conn,
        team_id,
        include_archived,
        OWNER_TEAM_ADRS_LIMIT,
    )
    .map_err(ApiError::internal)?;
    Ok((items.into_iter().map(impacting_item_dto).collect(), total))
}

fn impacting_item_dto(item: kairos_db::impacts::ImpactingItem) -> dto::ImpactingItem {
    dto::ImpactingItem {
        short_code: item.short_code,
        title: item.title,
        entity_type: item.entity_type,
        document_type: item.document_type,
        lifecycle: item.lifecycle,
        column: item.column_name,
        archived_at: item.archived_at.map(|at| at.to_rfc3339()),
    }
}

/// One repository (open tenant-wide): the directory row plus its webhook
/// connection id and in-flight links — everything an agent reads before
/// working in, or filing against, a codebase.
///
/// `impacted_by` has the documents and the ADRs that impact the
/// repository (COLLIERY-T-0269): its vision, its architecture, the
/// decisions about it. It has the live items only.
/// `?include_deleted=true` adds the archived items, each marked with
/// `archived_at`.
///
/// `owner_team_adrs` has the other ADRs of the owner team (KAIROS-T-0599).
/// No `impacts` link names them. The newest come first, 50 at most.
/// A superseded ADR is not there. `owner_team_adrs_total` counts them all.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        dto::RepositoryDetailQuery,
    ),
    responses(
        (status = 200, description = "The repository", body = dto::RepositoryDetail),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_repository(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(query): ApiQuery<dto::RepositoryDetailQuery>,
) -> Result<Json<dto::RepositoryDetail>, ApiError> {
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            let repo_id = repo.id;
            let (owner_team_adrs, owner_team_adrs_total) =
                owner_team_adrs(conn, repo.team_id, query.include_deleted)?;
            let connection_id = kairos_db::forge::find_connection_for_repository(conn, repo_id)
                .map_err(super::forge::map_error)?
                .map(|c| c.id.to_string());
            let in_flight =
                kairos_db::graph::repository_link_rollup(conn, repo_id, &["open", "draft"], 100)
                    .map_err(ApiError::internal)?
                    .into_iter()
                    .map(|row| TeamLink {
                        kind: row.kind,
                        external_id: row.external_id,
                        title: row.title,
                        url: row.url,
                        state: row.state,
                        author: row.author,
                        forge: row.forge,
                        repo_full_name: row.repo_full_name,
                        item_short_code: row.item_short_code,
                        item_title: row.item_title,
                        forge_updated_at: row.forge_updated_at.to_rfc3339(),
                    })
                    .collect();
            // KAIROS-T-0480: every admitted branch and pull request, linked
            // or not.
            let refs = kairos_db::forge::repository_refs(conn, repo_id, &["open", "draft"], 100)
                .map_err(super::forge::map_error)?
                .into_iter()
                .map(ref_dto)
                .collect();
            Ok(dto::RepositoryDetail {
                repository: render_one(conn, repo)?,
                connection_id,
                in_flight,
                refs,
                impacted_by: impacted_by(conn, repo_id, query.include_deleted)?,
                owner_team_adrs,
                owner_team_adrs_total,
            })
        })
        .await?;
    Ok(Json(detail))
}

/// Register a repository under its owning team (org admin, or a
/// `manage_tasks` holder on that team's delivery board — self-serve for
/// the team and its agents).
#[utoipa::path(
    post,
    path = "/api/repositories",
    tag = "repositories",
    request_body = dto::CreateRepositoryRequest,
    responses(
        (status = 201, description = "Registered", body = dto::Repository),
        (status = 403, description = "Neither org admin nor the owning team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug or (forge, name) already registered", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad slug/forge, unknown team, or a field that does not have its form", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<dto::Repository>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| add(conn, &slug, user, body))
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Edit a repository (same gate as create, evaluated against the CURRENT
/// owner). Re-homing to another team does not touch the tasks that link to
/// the repository.
///
/// A body can have only the values that the repository has. That request
/// gets 200 with the repository, and the server writes nothing
/// (COLLIERY-T-0267).
///
/// To stop the code index builds of the repository, send
/// `{"code_index_build": "off"}` (KAIROS-T-0398). Kairos cancels the
/// build jobs of the repository that wait for a server, and their runs end
/// as failed. The builder adds no new job. Kairos does not stop a build
/// that runs now, but the build gets no further try. Send `"on"` to start
/// the builds again. The MCP tool `update_repository` does the same.
#[utoipa::path(
    patch,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    request_body = dto::UpdateRepositoryRequest,
    responses(
        (status = 200, description = "Updated", body = dto::Repository),
        (status = 403, description = "Neither org admin nor the owning team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already taken", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad slug, unknown team, or a field that does not have its form", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateRepositoryRequest>,
) -> Result<Json<dto::Repository>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            change(conn, &tenant_slug, user, &slug, body)
        })
        .await?;
    Ok(Json(updated.repository))
}

/// Remove a repository (org admin, or `manage_structure` at the
/// organization, KAIROS-T-0337). Refused with 409 while live tasks or a
/// live webhook connection still reference it.
#[utoipa::path(
    delete,
    path = "/api/repositories/{slug}",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "Removed", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Still referenced by tasks or a connection", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_repository(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let id = state
        .blocking
        .run(&tenant.slug, move |conn| {
            delete_as(conn, &tenant_slug, user, &slug).map(|repository| repository.id.to_string())
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id,
        deleted: true,
    }))
}

/// Remove the repository `reference` (slug or UUID) as `user`: the ONE
/// implementation of `DELETE /api/repositories/{slug}` and of the MCP tool
/// `delete_repository` (KAIROS-T-0443). The gate is [`require_structure`].
/// Returns the repository as it was.
pub(crate) fn delete_as(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
) -> Result<Repository, ApiError> {
    let current = repositories::resolve(conn, reference).map_err(map_error)?;
    require_structure(conn, tenant_slug, user)?;
    repositories::soft_delete(conn, current.id, user).map_err(map_error)?;
    Ok(current)
}
