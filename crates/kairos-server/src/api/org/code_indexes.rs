//! `/api/repositories/{slug}/code-indexes` (COLLIERY-T-1853,
//! COLLIERY-I-0264 "The flow"): the base code index of each indexed commit
//! of a repository.
//!
//! - `GET …/code-indexes`: each indexed commit.
//! - `GET …/code-indexes/commits?limit=&offset=`: one page of the indexed
//!   commits, newest first, with the total (KAIROS-T-0518).
//! - `PUT …/code-indexes/{commit}`: send the index file of a commit. The
//!   body is the bytes of a `kairos-index` SQLite file. Its summaries go to
//!   the pool of the repository, which the commits share; its structure is
//!   stored for the commit.
//! - `GET …/code-indexes/{commit}`: the index file of a commit, with the
//!   summaries that it uses.
//! - `GET …/code-indexes/nearest?commit=`: the nearest indexed commit at or
//!   below a commit. It reads the bare clone of the repository
//!   ([`crate::code_index`]).
//! - `GET …/code-indexes/builds`: the runs of the builder for the
//!   repository, newest first (KAIROS-T-0331). An upload is a run too.
//! - `POST …/code-indexes/rebuild`: ask the builder of the server for a
//!   full build of the head of the default branch (KAIROS-T-0332). The
//!   answer is 202 with the `running` run. Refused while a run is active
//!   (`CODE_INDEX_BUILD_RUNNING`), when the repository has the builder off
//!   (`CODE_INDEX_BUILD_OFF`), and when the deployment has no builder
//!   (`CODE_INDEX_BUILDER_OFF`). The gate is the gate of an upload.
//!
//! Gating: reads are open tenant-wide, like the repository itself. An
//! upload needs the right to change the repository (org admin, or
//! `manage_tasks` on the delivery board of its owner team), checked before
//! the body is read. Storage: [`kairos_db::code_indexes`].

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_code_index as dto;
use kairos_db::code_index_builds::{self, Build};
use kairos_db::code_indexes::{self, CodeIndexInfo, NewCodeIndex};
use kairos_db::models::repositories::Repository;
use kairos_db::repositories;
use serde_json::json;
use uuid::Uuid;

use super::repositories::{changeable, map_error};
use crate::app::AppState;
use crate::code_index::{MAX_DISTANCE, used_rows};
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The largest index file that an upload takes. An index of Kairos with
/// its summaries is about 22 MB.
pub const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

/// The media type of an index file.
const INDEX_FILE_TYPE: &str = "application/vnd.sqlite3";

/// The longest `ref` of an upload.
const MAX_REF_LEN: usize = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/repositories/{slug}/code-indexes",
            get(list_code_indexes),
        )
        .route(
            "/api/repositories/{slug}/code-indexes/commits",
            get(code_indexes_page),
        )
        .route(
            "/api/repositories/{slug}/code-indexes/nearest",
            get(nearest_code_index),
        )
        .route(
            "/api/repositories/{slug}/code-indexes/builds",
            get(list_code_index_builds),
        )
        .route(
            "/api/repositories/{slug}/code-indexes/rebuild",
            post(rebuild_code_index),
        )
        .route(
            "/api/repositories/{slug}/code-indexes/{commit}",
            get(download_code_index)
                .put(upload_code_index)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
}

/// An index file in the OpenAPI document: the bytes of a `kairos-index`
/// SQLite file.
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct IndexFile(#[allow(dead_code)] Vec<u8>);

/// Refuse a commit that is not a full commit id, and name it.
fn check_commit(commit: &str, field: &str) -> Result<(), ApiError> {
    let hex = commit
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if hex && (commit.len() == 40 || commit.len() == 64) {
        return Ok(());
    }
    Err(ApiError::validation(format!(
        "The {field} {commit:?} is not a full commit id. Send the 40 lower-case hex \
         characters of the commit, as `git rev-parse HEAD` gives them."
    ))
    .with_details(json!({ "field": field })))
}

fn check_ref(r#ref: &str) -> Result<(), ApiError> {
    let bad = r#ref.is_empty()
        || r#ref.len() > MAX_REF_LEN
        || r#ref.chars().any(|c| c.is_whitespace() || c.is_control());
    if !bad {
        return Ok(());
    }
    Err(ApiError::validation(format!(
        "The ref {ref:?} is not correct. Send a branch, a pull request or a tag, for \
         example main, pull/12 or v1.0.0. Use 1 to {MAX_REF_LEN} characters and no space."
    ))
    .with_details(json!({ "field": "ref" })))
}

/// The bytes at the start of a structure that give its schema version.
const VERSION_PREFIX: i32 = kairos_index::store::VERSION_PREFIX_BYTES as i32;

/// The schema version of a structure, from its first bytes (`prefix`).
fn version_of(prefix: &[u8]) -> Option<i64> {
    let end = prefix.len().min(kairos_index::store::VERSION_PREFIX_BYTES);
    kairos_index::store::structure_version(&prefix[..end])
}

/// The schema version of the stored structure of one commit.
fn stored_version(
    conn: &mut PgConnection,
    repository_id: Uuid,
    commit: &str,
) -> Result<Option<i64>, ApiError> {
    let prefix = code_indexes::structure_prefix(conn, repository_id, commit, VERSION_PREFIX)
        .map_err(ApiError::internal)?;
    Ok(prefix.as_deref().and_then(version_of))
}

fn dto_of(info: CodeIndexInfo, schema_version: Option<i64>) -> dto::CodeIndex {
    dto::CodeIndex {
        commit: info.commit_sha,
        r#ref: info.ref_name,
        source: info.source,
        structure_bytes: info.structure_bytes,
        summary_keys: info.summary_keys,
        schema_version,
        vector_model: info.vector_model,
        created_at: info.created_at.to_rfc3339(),
        updated_at: info.updated_at.to_rfc3339(),
    }
}

pub(crate) fn build_dto_of(run: Build) -> dto::CodeIndexBuild {
    dto::CodeIndexBuild {
        id: run.id.to_string(),
        commit: run.commit_sha,
        r#ref: run.ref_name,
        trigger: run.trigger,
        outcome: run.outcome,
        error: run.error,
        files: run.files,
        symbols: run.symbols,
        edges: run.edges,
        summaries_made: run.summaries_made,
        summaries_reused: run.summaries_reused,
        summaries_missing: run.summaries_missing,
        requested_by: run.requested_by.map(|u| u.to_string()),
        started_at: run.started_at.to_rfc3339(),
        finished_at: run.finished_at.map(|t| t.to_rfc3339()),
        model: run.model,
        // A row that is not a valid list gives no warning: the row of a run
        // is never refused for it.
        scip_warnings: run
            .scip_warnings
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_default(),
    }
}

/// The runs of the code index builder for a repository, newest first
/// (KAIROS-T-0331). The server keeps `limit` in the range 1 to 100, as it
/// does for each list (COLLIERY-T-0264). With no `limit`, it gives 20.
/// `offset` skips the newest runs: the pages (KAIROS-T-0584). `total` is
/// exact.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/code-indexes/builds",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        dto::CodeIndexBuildListQuery,
    ),
    responses(
        (status = 200, description = "The runs, newest first", body = dto::CodeIndexBuildList),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_code_index_builds(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(query): ApiQuery<dto::CodeIndexBuildListQuery>,
) -> Result<Json<dto::CodeIndexBuildList>, ApiError> {
    let limit = query
        .limit
        .unwrap_or(code_index_builds::DEFAULT_LIST)
        .clamp(1, code_index_builds::MAX_LIST);
    let offset = query.offset.unwrap_or(0).max(0);
    let role = state.config.role;
    let (runs, total, builder) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let id = repository_id(conn, &slug)?;
            let runs =
                code_index_builds::list(conn, id, limit, offset).map_err(ApiError::internal)?;
            let total = code_index_builds::count(conn, id).map_err(ApiError::internal)?;
            // KAIROS-T-0394: an `api` server with no worker says so.
            let builder = crate::code_index::jobs::builder_note(conn, role)?;
            Ok((runs, total, builder))
        })
        .await?;
    // KAIROS-T-0478: the disk use of the tools and the caches of this
    // server, for an organization admin, off the async threads (a walk of
    // the folders, kept a minute).
    let admin = tenant.role == kairos_db::models::enums::OrgRole::Admin;
    let disk = match state.code_index.clone().filter(|_| admin) {
        Some(service) => tokio::task::spawn_blocking(move || service.disk_use())
            .await
            .ok(),
        None => None,
    };
    Ok(Json(dto::CodeIndexBuildList {
        items: runs.into_iter().map(build_dto_of).collect(),
        total,
        limit,
        offset,
        builder,
        disk,
    }))
}

/// The ONE check and write of a rebuild request (KAIROS-T-0332), for REST
/// and for the MCP tool `rebuild_code_index`: the gate of a change of the
/// repository, the builder setting of the repository, no active run and no
/// build job that runs, then the `running` row with `trigger = request` of
/// the server `server_instance` (KAIROS-T-0354) and its build job
/// (KAIROS-T-0374). A build job of the repository that waits (an update or
/// a first build) becomes the requested build: it gets the priority of a
/// request and names the run. The request resets the breaker of the
/// repository (KAIROS-T-0399). The check and the writes are one
/// transaction, with the row of the repository locked. The caller wakes
/// the job worker after the write.
pub(crate) fn request_rebuild(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    reference: &str,
    server_instance: Uuid,
    max_tries: i32,
) -> Result<(Repository, Build), ApiError> {
    let repo = changeable(conn, tenant_slug, user, reference)?;
    if !repo.code_index_build_on() {
        return Err(ApiError::unprocessable(
            "CODE_INDEX_BUILD_OFF",
            format!(
                "The code index builder is off for the repository {:?}. Set code_index_build to \
                 on (PATCH /api/repositories/{}, `kairos repos update {} --code-index-build on`, \
                 or the page Admin, Repositories), then ask again.",
                repo.slug, repo.slug, repo.slug
            ),
        ));
    }
    super::run_in_transaction(conn, |conn| {
        code_index_builds::lock_repository(conn, repo.id).map_err(ApiError::internal)?;
        if let Some(active) =
            code_index_builds::running(conn, repo.id).map_err(ApiError::internal)?
        {
            return Err(ApiError::new(
            StatusCode::CONFLICT,
            "CODE_INDEX_BUILD_RUNNING",
            format!(
                "A run of the code index of the repository {:?} is active since {} (trigger {}). \
                 Wait for its end: GET /api/repositories/{}/code-indexes/builds.",
                repo.slug,
                active.started_at.to_rfc3339(),
                active.trigger,
                repo.slug
            ),
        )
        .with_details(json!({ "run": active.id })));
        }
        let key = crate::code_index::job_key(tenant_slug, repo.id);
        let open = kairos_db::jobs::open_by_key(conn, &key).map_err(ApiError::internal)?;
        if let Some(job) = &open
            && job.state != "queued"
        {
            return Err(job_runs(&repo, job));
        }
        let run = code_index_builds::start(
            conn,
            repo.id,
            "request",
            Some(&repo.default_branch),
            Some(user),
            Some(server_instance),
        )
        .map_err(ApiError::internal)?;
        match open {
            Some(job) => {
                let patch = json!({ "trigger": crate::code_index::jobs::REQUEST, "run": run.id });
                if kairos_db::jobs::raise(
                    conn,
                    job.id,
                    crate::jobs::priority::REQUESTED_BUILD,
                    &patch,
                )
                .map_err(ApiError::internal)?
                .is_none()
                {
                    return Err(job_runs(&repo, &job));
                }
            }
            None => {
                if let Err(job) = crate::code_index::jobs::enqueue_build(
                    conn,
                    tenant_slug,
                    &repo,
                    crate::code_index::jobs::REQUEST,
                    Some(run.id),
                    None,
                    max_tries,
                )? {
                    return Err(job_runs(&repo, &job));
                }
            }
        }
        // KAIROS-T-0399: a person asks for the build, so the breaker of the
        // repository resets, and the sweep works on it again.
        if kairos_db::code_index_breakers::reset(conn, repo.id).map_err(ApiError::internal)? {
            tracing::info!(tenant = tenant_slug, repository = %repo.slug, "code index breaker reset by a requested build");
        }
        crate::code_index::notify_run(conn, &repo.slug, Some(user))?;
        Ok((repo, run))
    })
}

/// The refusal of a request while a build job of the repository runs
/// (KAIROS-T-0374): its run row comes when the build starts.
fn job_runs(repo: &Repository, job: &kairos_db::jobs::Job) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "CODE_INDEX_BUILD_RUNNING",
        format!(
            "A build of the code index of the repository {:?} runs now (job {}, since {}). \
             Wait for its end: GET /api/repositories/{}/code-indexes/builds.",
            repo.slug,
            job.id,
            job.created_at.to_rfc3339(),
            repo.slug
        ),
    )
    .with_details(json!({ "job": job.id }))
}

/// The refusal when the deployment has no code index builder.
pub(crate) fn no_builder() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "CODE_INDEX_BUILDER_OFF",
        "This deployment has no code index builder: KAIROS_CODE_INDEX_DIR is not set. An \
         operator sets it, and Kairos then builds the indexes.",
    )
}

/// The id of the server that takes a rebuild request, for the `running`
/// row. A server that builds (KAIROS_ROLE all) needs its builder; a server
/// with KAIROS_ROLE=api queues the build for a worker, and needs none
/// (KAIROS-T-0466). The id is that of the job worker, which each role has.
pub(crate) fn rebuild_instance(state: &AppState) -> Result<Uuid, ApiError> {
    if state.config.role.builds() && state.code_index.is_none() {
        return Err(no_builder());
    }
    Ok(state.jobs.instance())
}

/// Ask the builder for a full build of the code index of a repository
/// (KAIROS-T-0332). The body is empty. The answer is 202 with the run.
/// The request resets the breaker of the code index builder of the
/// repository (`code_index_breaker`, KAIROS-T-0399).
#[utoipa::path(
    post,
    path = "/api/repositories/{slug}/code-indexes/rebuild",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 202, description = "The run, as `running`", body = dto::CodeIndexBuild),
        (status = 403, description = "No right to change the repository", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A run of the repository is active (CODE_INDEX_BUILD_RUNNING)", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The repository has the builder off (CODE_INDEX_BUILD_OFF), or the request has a body", body = kairos_client::types::ErrorEnvelope),
        (status = 503, description = "The deployment has no builder (CODE_INDEX_BUILDER_OFF)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn rebuild_code_index(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !body.is_empty() {
        return Err(ApiError::validation(
            "This route takes no body. Send the POST with no content.",
        )
        .with_details(json!({ "field": "body" })));
    }
    let instance = rebuild_instance(&state)?;
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let max_tries = state.config.jobs.max_tries;
    let (_repo, run) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            request_rebuild(conn, &tenant_slug, user, &slug, instance, max_tries)
        })
        .await?;
    state.jobs.wake();
    Ok((StatusCode::ACCEPTED, Json(build_dto_of(run))).into_response())
}

fn repository_id(conn: &mut PgConnection, slug: &str) -> Result<Uuid, ApiError> {
    Ok(repositories::resolve(conn, slug).map_err(map_error)?.id)
}

fn no_index(slug: &str, commit: &str) -> ApiError {
    ApiError::not_found(format!(
        "The repository {slug:?} has no code index of the commit {commit}."
    ))
    .with_details(json!({ "field": "commit" }))
}

/// Each indexed commit of a repository, newest write first.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/code-indexes",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The indexed commits", body = Vec<dto::CodeIndex>),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_code_indexes(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<Vec<dto::CodeIndex>>, ApiError> {
    let list = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let id = repository_id(conn, &slug)?;
            code_indexes::page(conn, id, None, 0, VERSION_PREFIX).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(
        list.into_iter()
            .map(|(info, prefix)| dto_of(info, version_of(&prefix)))
            .collect(),
    ))
}

/// One page of the indexed commits of a repository, newest write first
/// (KAIROS-T-0518). The server keeps `limit` in the range 1 to 200. The
/// default `limit` is 50. `offset` skips the newest rows. `total` is exact.
/// Each row has the schema version of its index file.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/code-indexes/commits",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        Pagination,
    ),
    responses(
        (status = 200, description = "A page of the indexed commits, newest first", body = ListEnvelope<dto::CodeIndex>),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn code_indexes_page(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(page): ApiQuery<Pagination>,
) -> Result<Json<ListEnvelope<dto::CodeIndex>>, ApiError> {
    let (limit, offset) = crate::api::clamp_pagination(&page);
    let (rows, total) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let id = repository_id(conn, &slug)?;
            let rows = code_indexes::page(conn, id, Some(limit), offset, VERSION_PREFIX)
                .map_err(ApiError::internal)?;
            let total = code_indexes::count(conn, id).map_err(ApiError::internal)?;
            Ok((rows, total))
        })
        .await?;
    Ok(Json(ListEnvelope {
        items: rows
            .into_iter()
            .map(|(info, prefix)| dto_of(info, version_of(&prefix)))
            .collect(),
        total,
        limit,
        offset,
    }))
}

/// Send the index file of a commit. Kairos adds its summaries to the pool
/// of the repository and keeps its structure for the commit. A commit that
/// has an index gets the new one.
#[utoipa::path(
    put,
    path = "/api/repositories/{slug}/code-indexes/{commit}",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        ("commit" = String, Path, description = "The full commit id"),
        dto::UploadCodeIndexQuery,
    ),
    request_body(content = IndexFile, content_type = "application/vnd.sqlite3", description = "A kairos-index SQLite file"),
    responses(
        (status = 201, description = "Stored; the commit had no index", body = dto::UploadedCodeIndex),
        (status = 200, description = "Stored; it replaces the index of the commit", body = dto::UploadedCodeIndex),
        (status = 403, description = "No right to change the repository", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "The vectors are of another model than the pool of the repository", body = kairos_client::types::ErrorEnvelope),
        (status = 413, description = "The file is larger than 256 MiB"),
        (status = 422, description = "Bad commit or ref, or a body that is not an index file", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn upload_code_index(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((slug, commit)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<dto::UploadCodeIndexQuery>,
    body: Bytes,
) -> Result<Response, ApiError> {
    check_commit(&commit, "commit")?;
    if let Some(r#ref) = &query.r#ref {
        check_ref(r#ref)?;
    }
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let repo = state
        .blocking
        .run(&tenant.slug, move |conn| {
            changeable(conn, &tenant_slug, user, &slug)
        })
        .await?;

    let split = tokio::task::spawn_blocking(move || {
        let file = tempfile::NamedTempFile::new().map_err(ApiError::internal)?;
        std::fs::write(file.path(), &body).map_err(ApiError::internal)?;
        kairos_index::store::split(file.path()).map_err(|e| {
            ApiError::validation(format!(
                "The body is not a code index file of this version of Kairos: {e} Send the \
                 file .kairos/index.db that `kairos index build` makes."
            ))
            .with_details(json!({ "field": "body" }))
        })
    })
    .await
    .map_err(ApiError::internal)??;

    let (created, uploaded) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let pool_model =
                code_indexes::vector_model(conn, repo.id).map_err(ApiError::internal)?;
            if let (Some(pool), Some(upload)) = (&pool_model, &split.vector_model)
                && pool != upload
            {
                return Err(ApiError::conflict(format!(
                    "The vectors of the upload are of the model {upload}. The pool of the \
                     repository has vectors of the model {pool}. Build the index with the \
                     model {pool}."
                )));
            }
            let pool = used_rows(split.pool, &split.keys);
            let keys = split.keys;
            let schema_version = version_of(&split.structure_gz);
            let outcome = code_indexes::put(
                conn,
                &NewCodeIndex {
                    repository_id: repo.id,
                    commit_sha: commit.clone(),
                    ref_name: query.r#ref,
                    source: "upload",
                    structure: split.structure_gz,
                    structure_bytes: split.structure_bytes as i64,
                    summary_keys: keys.len() as i32,
                    vector_model: split.vector_model,
                    created_by: Some(user),
                },
                &pool,
                &keys,
            )
            .map_err(ApiError::internal)?;
            let info = code_indexes::info(conn, repo.id, &commit)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("the stored index is not there"))?;
            // The upload is a run of the index (KAIROS-T-0331), and the
            // readers of the repository hear of it (KAIROS-T-0333).
            code_index_builds::record_finished(
                conn,
                repo.id,
                "upload",
                &commit,
                info.ref_name.as_deref(),
                Some(user),
                None,
            )
            .map_err(ApiError::internal)?;
            crate::code_index::notify_run(conn, &repo.slug, Some(user))?;
            let pool_size = code_indexes::pool_size(conn, repo.id).map_err(ApiError::internal)?;
            Ok((
                outcome.created,
                dto::UploadedCodeIndex {
                    index: dto_of(info, schema_version),
                    new_summaries: outcome.new_summaries,
                    pool_size,
                },
            ))
        })
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(uploaded)).into_response())
}

/// The index file of a commit, with the summaries that its structure uses.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/code-indexes/{commit}",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        ("commit" = String, Path, description = "The full commit id"),
    ),
    responses(
        (status = 200, description = "A kairos-index SQLite file", body = IndexFile, content_type = "application/vnd.sqlite3"),
        (status = 404, description = "Unknown repository, or no index of the commit", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad commit", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn download_code_index(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((slug, commit)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    check_commit(&commit, "commit")?;
    let name = format!("{slug}-{}.db", &commit[..12]);
    let (repo_id, structure) = {
        let commit = commit.clone();
        state
            .blocking
            .run(&tenant.slug, move |conn| {
                let id = repository_id(conn, &slug)?;
                let structure = code_indexes::structure(conn, id, &commit)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| no_index(&slug, &commit))?;
                Ok((id, structure))
            })
            .await?
    };
    // The keys come from the structure, so it is assembled 2 times: with
    // no pool, then with the rows of those keys.
    let work = tempfile::tempdir().map_err(ApiError::internal)?;
    let file = work.path().join("index.db");
    let (structure, keys) = {
        let file = file.clone();
        tokio::task::spawn_blocking(move || {
            kairos_index::store::assemble(&structure, [], &file).map_err(ApiError::internal)?;
            let keys = kairos_index::store::keys_of(&file).map_err(ApiError::internal)?;
            Ok::<_, ApiError>((structure, keys))
        })
        .await
        .map_err(ApiError::internal)??
    };
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            code_indexes::pool_rows(conn, repo_id, &keys).map_err(ApiError::internal)
        })
        .await?;
    let bytes = tokio::task::spawn_blocking(move || {
        kairos_index::store::assemble(
            &structure,
            rows.into_iter().map(crate::code_index::pool_row_of),
            &file,
        )
        .map_err(ApiError::internal)?;
        let bytes = std::fs::read(&file).map_err(ApiError::internal)?;
        drop(work);
        Ok::<_, ApiError>(bytes)
    })
    .await
    .map_err(ApiError::internal)??;
    Ok((
        [
            (header::CONTENT_TYPE, INDEX_FILE_TYPE.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// The nearest indexed commit at or below a commit, from the bare clone of
/// the repository. The clone fetches first when it does not have the
/// commit.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/code-indexes/nearest",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        dto::NearestCodeIndexQuery,
    ),
    responses(
        (status = 200, description = "The nearest indexed commit", body = dto::NearestCodeIndex),
        (status = 404, description = "Unknown repository or commit, or no indexed commit below it", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad commit", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "This deployment keeps no clones (KAIROS_CODE_INDEX_DIR)", body = kairos_client::types::ErrorEnvelope),
        (status = 502, description = "The fetch from the repository failed", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn nearest_code_index(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(query): ApiQuery<dto::NearestCodeIndexQuery>,
) -> Result<Json<dto::NearestCodeIndex>, ApiError> {
    let commit = query.commit;
    check_commit(&commit, "commit")?;
    let Some(service) = state.code_index.clone() else {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "CODE_INDEX_NOT_CONFIGURED",
            "This deployment keeps no clones of the repositories, so it cannot find the \
             nearest indexed commit. An operator sets KAIROS_CODE_INDEX_DIR to turn it on.",
        ));
    };
    let repo = {
        let slug = slug.clone();
        state
            .blocking
            .run(&tenant.slug, move |conn| {
                repositories::resolve(conn, &slug).map_err(map_error)
            })
            .await?
    };
    let repo_id = repo.id;
    // The token of the fetch (KAIROS-T-0431): the read token of the
    // organization for the host, else the token of the repository.
    let token = {
        let key = service.secrets_key().cloned();
        let tenant_slug = tenant.slug.clone();
        let row = repo.clone();
        state
            .blocking
            .run(&tenant.slug, move |conn| {
                crate::credentials::fetch_token(conn, &tenant_slug, &row, key.as_ref())
                    .map(|(token, _)| token)
                    .map_err(crate::credentials::token_refusal)
            })
            .await?
    };
    let ancestors = {
        let tenant = tenant.slug.clone();
        let wanted = commit.clone();
        tokio::task::spawn_blocking(move || {
            service.ancestors(&tenant, &repo, &wanted, token.as_ref())
        })
        .await
        .map_err(ApiError::internal)?
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GIT_FETCH_FAILED",
                format!(
                    "The server cannot fetch the repository {slug:?}. Make sure that it \
                         can read the repo_url of the repository."
                ),
            )
            .with_details(json!({ "error": e.to_string() }))
        })?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "The repository {slug:?} has no commit {commit}. Push the commit, then \
                     ask again."
            ))
            .with_details(json!({ "parameter": "commit" }))
        })?
    };
    let nearest = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let indexed = code_indexes::list(conn, repo_id).map_err(ApiError::internal)?;
            let found = ancestors.iter().enumerate().find_map(|(distance, c)| {
                indexed
                    .iter()
                    .find(|i| &i.commit_sha == c)
                    .map(|i| (distance, i.clone()))
            });
            match found {
                Some((distance, info)) => {
                    let version = stored_version(conn, repo_id, &info.commit_sha)?;
                    Ok(Some((distance, info, version)))
                }
                None => Ok(None),
            }
        })
        .await?;
    let Some((distance, info, schema_version)) = nearest else {
        return Err(ApiError::not_found(format!(
            "No indexed commit is within {MAX_DISTANCE} commits at or below {commit}. Build \
             the index with `kairos index build`."
        ))
        .with_details(json!({ "parameter": "commit" })));
    };
    Ok(Json(dto::NearestCodeIndex {
        from: commit,
        index: dto_of(info, schema_version),
        distance,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_is_40_or_64_lower_case_hex_characters() {
        assert!(check_commit(&"a".repeat(40), "commit").is_ok());
        assert!(check_commit(&"0".repeat(64), "commit").is_ok());
        for bad in ["", "abc", &"A".repeat(40), &"g".repeat(40), &"a".repeat(41)] {
            let refusal = check_commit(bad, "commit").unwrap_err();
            assert_eq!(refusal.code, "VALIDATION");
            assert_eq!(refusal.details["field"], "commit");
        }
    }

    #[test]
    fn a_ref_has_no_space() {
        assert!(check_ref("pull/12").is_ok());
        assert!(check_ref("main").is_ok());
        assert!(check_ref("").is_err());
        assert!(check_ref("my branch").is_err());
        assert!(check_ref(&"x".repeat(MAX_REF_LEN + 1)).is_err());
    }
}
