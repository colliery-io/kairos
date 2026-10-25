//! `/api/repositories/{slug}/site` (KAIROS-T-0406, KAIROS-D-0012): CI
//! publishes the built site of a repository, and Kairos serves it.
//!
//! - `PUT` takes one `.tar.gz`, streamed to a temp file with its MD5. The
//!   archive is checked ([`crate::site_archive`]), each file goes to the
//!   site store under a new publish id, and the switch to the new publish is
//!   one transaction. The files of the publish it replaced are deleted by a
//!   job after [`REPLACED_GRACE`].
//! - `DELETE` unpublishes; its files are deleted by a job now.
//! - `GET .../site/publishes` is the history, newest first.
//!
//! Rights: a publish needs the capability `pages` on the delivery board of
//! the owner team (an organization admin has each capability), and the site
//! switch of the repository on. An unpublish needs `pages` or the right to
//! change the repository. The history is open to each member, like the
//! repository.
//!
//! [`REPLACED_GRACE`]: crate::site_store::REPLACED_GRACE

use std::path::{Path as FsPath, PathBuf};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, put};
use axum::{Json, Router};
use base64::Engine as _;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use futures_util::{StreamExt as _, TryStreamExt as _};
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_sites as dto;
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::repositories::Repository;
use kairos_db::repositories;
use kairos_db::sites::{self, NewSitePublish, SiteFile, SitePublish};
use md5::{Digest as _, Md5};
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;

use super::super::clamp_pagination;
use super::repositories::{delivery_board_of, map_error, require_manage_for_team};
use crate::app::AppState;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::site_archive::{self, ArchiveError, Limits, Unpacked};
use crate::site_store::{PublishKey, REPLACED_GRACE, SiteStore, cleanup_job};

/// The files that go to the store at a time.
const UPLOAD_CONCURRENCY: usize = 8;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/repositories/{slug}/site",
            put(publish_site)
                .delete(unpublish_site)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/repositories/{slug}/site/publishes",
            get(list_site_publishes),
        )
}

/// A site archive in the OpenAPI document: the bytes of a `.tar.gz`.
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct SiteArchive(#[allow(dead_code)] Vec<u8>);

/// [`SitePublish`] → the wire, with the display name of its principal.
pub(crate) fn publish_dto_named(
    p: &SitePublish,
    names: &std::collections::HashMap<Uuid, String>,
) -> dto::SitePublish {
    dto::SitePublish {
        principal_name: names.get(&p.principal_id).cloned(),
        ..publish_dto(p)
    }
}

/// [`SitePublish`] → the wire.
pub(crate) fn publish_dto(p: &SitePublish) -> dto::SitePublish {
    dto::SitePublish {
        principal_name: None,
        publish_id: p.id.to_string(),
        repository_slug: p.repository_slug.clone(),
        base_path: p.base_path.clone(),
        md5: p.md5.clone(),
        size: p.size,
        unpacked_size: p.unpacked_size,
        files: p.files,
        principal_id: p.principal_id.to_string(),
        agent_key_id: p.agent_key_id.map(|id| id.to_string()),
        published_at: p.published_at.to_rfc3339(),
        state: p.state.clone(),
    }
}

/// `pages` on the delivery board of the owner team. An organization admin
/// has each capability; a team with no delivery board is admin-only.
fn require_pages(
    conn: &mut PgConnection,
    tenant_slug: &str,
    user: Uuid,
    team_id: Uuid,
) -> Result<(), ApiError> {
    let board = delivery_board_of(conn, team_id)?;
    super::super::require_capability(conn, tenant_slug, board, user, kairos_core::abac::PAGES)
}

fn team_slug(conn: &mut PgConnection, team_id: Uuid) -> Result<String, ApiError> {
    use kairos_db::schema::teams::dsl;
    dsl::teams
        .filter(dsl::id.eq(team_id))
        .select(dsl::slug)
        .first(conn)
        .map_err(ApiError::internal)
}

fn no_store() -> ApiError {
    ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "NO_SITE_STORE",
        "This deployment has no site store, so it cannot keep a site. An operator sets \
         KAIROS_PAGES_STORE (disk or s3).",
    )
}

fn site_off(slug: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "SITE_OFF",
        format!(
            "The site of the repository {slug:?} is off. Turn it on first: PATCH the \
             repository with site_enabled true."
        ),
    )
}

fn archive_refused(e: ArchiveError) -> ApiError {
    match e {
        ArchiveError::Io(text) => ApiError::internal(text),
        e => ApiError::unprocessable("ARCHIVE_REFUSED", e.to_string()),
    }
}

/// The MD5 that the `Content-MD5` header gives, if any.
fn content_md5(headers: &HeaderMap) -> Result<Option<[u8; 16]>, ApiError> {
    let Some(value) = headers.get("content-md5") else {
        return Ok(None);
    };
    let bad = || {
        ApiError::validation(
            "The header Content-MD5 is not correct. Send the MD5 of the body in base64 (24 \
             characters), or do not send the header.",
        )
        .with_details(serde_json::json!({ "field": "Content-MD5" }))
    };
    let text = value.to_str().map_err(|_| bad())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| bad())?;
    let digest: [u8; 16] = bytes.try_into().map_err(|_| bad())?;
    Ok(Some(digest))
}

/// The body of an upload, in a temp folder.
struct Received {
    dir: tempfile::TempDir,
    archive: PathBuf,
    md5: [u8; 16],
    size: u64,
}

/// Stream `body` to a temp file, with its MD5. Stops with 413 when the body
/// is larger than `max_size`.
async fn receive(body: Body, max_size: u64) -> Result<Received, ApiError> {
    let dir = tempfile::tempdir().map_err(ApiError::internal)?;
    let archive = dir.path().join("site.tar.gz");
    let mut file = tokio::fs::File::create(&archive)
        .await
        .map_err(ApiError::internal)?;
    let mut hasher = Md5::new();
    let mut size: u64 = 0;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            ApiError::validation(format!(
                "Kairos cannot read the body of the request: {e}. Send it again."
            ))
            .with_details(serde_json::json!({ "field": "body" }))
        })?;
        size += chunk.len() as u64;
        if size > max_size {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "TOO_LARGE",
                format!(
                    "The archive is larger than {max_size} bytes. The limit is {max_size} bytes."
                ),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(ApiError::internal)?;
    }
    file.flush().await.map_err(ApiError::internal)?;
    Ok(Received {
        dir,
        archive,
        md5: hasher.finalize().into(),
        size,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write each unpacked file to the store.
async fn store_files(
    store: &SiteStore,
    key: &PublishKey,
    root: &FsPath,
    unpacked: &Unpacked,
) -> Result<(), crate::site_store::StoreError> {
    futures_util::stream::iter(unpacked.files.iter().map(Ok))
        .try_for_each_concurrent(UPLOAD_CONCURRENCY, |file| async move {
            store
                .put_file(key, &file.path, &root.join(&file.path))
                .await
        })
        .await
}

fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    repository_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(kairos_db::models::NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(repository_id),
            entity_type: Some("repository".to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// Queue the deletion of the stored files of `key` now. A failure here is
/// logged: the files stay, and they are not served.
async fn clean_up_now(state: &AppState, key: PublishKey) {
    let job = cleanup_job(&key, std::time::Duration::ZERO);
    let queued = state
        .blocking
        .run_public(move |conn| kairos_db::jobs::enqueue(conn, &job).map_err(ApiError::internal))
        .await;
    if let Err(e) = queued {
        tracing::warn!(publish_id = %key.publish_id, error = %e.message, "cannot queue the cleanup of a site publish");
    }
}

/// Publish the built site of a repository: one `.tar.gz` of the build
/// folder, with `index.html` at its root.
#[utoipa::path(
    put,
    path = "/api/repositories/{slug}/site",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        ("Content-MD5" = Option<String>, Header, description = "MD5 of the body, base64. When sent, it must match"),
    ),
    request_body(content = SiteArchive, content_type = "application/gzip", description = "A .tar.gz of the built site"),
    responses(
        (status = 201, description = "Published; it is the live site now", body = dto::SitePublish),
        (status = 403, description = "No capability pages on the delivery board of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "The site of the repository is off (SITE_OFF)", body = kairos_client::types::ErrorEnvelope),
        (status = 413, description = "The archive is larger than the limit (TOO_LARGE)", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The archive is refused (ARCHIVE_REFUSED), or Content-MD5 does not match (MD5_MISMATCH)", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "The deployment has no site store (NO_SITE_STORE)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn publish_site(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<dto::SitePublish>), ApiError> {
    let store = state.site_store.clone().ok_or_else(no_store)?;
    let expected_md5 = content_md5(&headers)?;
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let (repo, base_path): (Repository, String) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            require_pages(conn, &tenant_slug, user, repo.team_id)?;
            if !repo.site_enabled {
                return Err(site_off(&repo.slug));
            }
            let team = team_slug(conn, repo.team_id)?;
            let base_path = sites::base_path(&tenant_slug, &team, &repo.slug);
            Ok((repo, base_path))
        })
        .await?;

    let settings = &state.config.pages;
    let received = receive(body, settings.max_size).await?;
    if let Some(expected) = expected_md5
        && expected != received.md5
    {
        return Err(ApiError::unprocessable(
            "MD5_MISMATCH",
            format!(
                "The MD5 of the body is {}. The header Content-MD5 gives {}. Kairos keeps \
                 nothing. Send the archive again.",
                hex(&received.md5),
                hex(&expected)
            ),
        ));
    }
    let limits = Limits {
        max_size: settings.max_size,
        max_files: settings.max_files,
    };
    let out = received.dir.path().join("site");
    let archive = received.archive.clone();
    let unpack_out = out.clone();
    let unpacked =
        tokio::task::spawn_blocking(move || site_archive::unpack(&archive, &unpack_out, limits))
            .await
            .map_err(ApiError::internal)?
            .map_err(archive_refused)?;

    let key = PublishKey {
        org_id: tenant.org_id,
        repository_id: repo.id,
        publish_id: Uuid::new_v4(),
    };
    if let Err(e) = store_files(&store, &key, &out, &unpacked).await {
        clean_up_now(&state, key).await;
        return Err(ApiError::internal(e));
    }

    let files: Vec<SiteFile> = unpacked
        .files
        .iter()
        .map(|f| SiteFile {
            publish_id: key.publish_id,
            path: f.path.clone(),
            size: f.size as i64,
            sha256: f.sha256.clone(),
            content_type: f.content_type.clone(),
        })
        .collect();
    let new = NewSitePublish {
        id: key.publish_id,
        repository_id: repo.id,
        repository_slug: repo.slug.clone(),
        base_path,
        md5: hex(&received.md5),
        size: received.size as i64,
        unpacked_size: unpacked.unpacked_size as i64,
        files: unpacked.entries as i32,
        principal_id: user,
        agent_key_id: auth.agent_key,
        state: sites::state::LIVE.to_string(),
    };
    let org_id = tenant.org_id;
    let switched = state
        .blocking
        .run(&tenant.slug, move |conn| {
            super::run_in_transaction(conn, |conn| {
                let replaced = sites::switch(conn, new, &files).map_err(ApiError::internal)?;
                log_activity(
                    conn,
                    user,
                    ActivityAction::Publish,
                    key.repository_id,
                    format!("site publish:{} md5:{}", key.publish_id, hex(&received.md5)),
                )?;
                if let Some(old) = &replaced {
                    let old_key = PublishKey {
                        org_id,
                        repository_id: old.repository_id,
                        publish_id: old.id,
                    };
                    kairos_db::jobs::enqueue(conn, &cleanup_job(&old_key, REPLACED_GRACE))
                        .map_err(ApiError::internal)?;
                }
                let live = sites::live(conn, &[key.repository_id])
                    .map_err(ApiError::internal)?
                    .remove(&key.repository_id)
                    .ok_or_else(|| ApiError::internal("the new publish is not live"))?;
                let names = sites::names(conn, &[live.principal_id]).map_err(ApiError::internal)?;
                Ok(publish_dto_named(&live, &names))
            })
        })
        .await;
    match switched {
        Ok(published) => Ok((StatusCode::CREATED, Json(published))),
        Err(e) => {
            clean_up_now(&state, key).await;
            Err(e)
        }
    }
}

/// Unpublish the site of a repository. Its files are deleted from the
/// store; the history stays.
#[utoipa::path(
    delete,
    path = "/api/repositories/{slug}/site",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 204, description = "Unpublished"),
        (status = 403, description = "No capability pages, and no right to change the repository", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository, or no published site", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn unpublish_site(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let org_id = tenant.org_id;
    state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            if require_pages(conn, &tenant_slug, user, repo.team_id).is_err() {
                require_manage_for_team(conn, &tenant_slug, user, repo.team_id)?;
            }
            super::run_in_transaction(conn, |conn| {
                let removed = sites::unpublish(conn, repo.id)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!(
                            "The repository {:?} has no published site.",
                            repo.slug
                        ))
                        .with_details(serde_json::json!({ "field": "repository" }))
                    })?;
                log_activity(
                    conn,
                    user,
                    ActivityAction::Unpublish,
                    repo.id,
                    format!("site publish:{}", removed.id),
                )?;
                let key = PublishKey {
                    org_id,
                    repository_id: repo.id,
                    publish_id: removed.id,
                };
                kairos_db::jobs::enqueue(conn, &cleanup_job(&key, std::time::Duration::ZERO))
                    .map_err(ApiError::internal)?;
                Ok(())
            })
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The publishes of the site of a repository, newest first.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/site/publishes",
    tag = "repositories",
    params(
        ("slug" = String, Path, description = "Repository slug (or UUID)"),
        Pagination,
    ),
    responses(
        (status = 200, description = "The publishes, newest first", body = ListEnvelope<dto::SitePublish>),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_site_publishes(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiQuery(pagination): ApiQuery<Pagination>,
) -> Result<Json<ListEnvelope<dto::SitePublish>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let page = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            let (rows, total) =
                sites::history(conn, repo.id, limit, offset).map_err(ApiError::internal)?;
            let ids: Vec<Uuid> = rows.iter().map(|p| p.principal_id).collect();
            let names = sites::names(conn, &ids).map_err(ApiError::internal)?;
            Ok(ListEnvelope {
                items: rows.iter().map(|p| publish_dto_named(p, &names)).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(page))
}
