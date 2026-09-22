//! `kairos-db` model → `kairos_client::types` DTO conversions
//! (KAIROS-T-0018). The DTO crate stays dependency-light (no uuid/chrono),
//! so ids render as canonical UUID strings and timestamps as RFC 3339 —
//! this module is the ONE place that encoding is decided.
//!
//! Both sides are foreign types, so the orphan rule forbids `From` impls
//! here; [`IntoDto`] is the local conversion trait instead.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_client::types_repositories::RepositoryRef;
use kairos_db::models::items::{Adr, Document, Initiative, Strategy, Task};
use kairos_db::models::repositories::Repository;
use uuid::Uuid;

/// Local conversion into a shared wire type (`model.into_dto()`).
pub trait IntoDto<T> {
    fn into_dto(self) -> T;
}

/// RFC 3339 with microsecond precision (stable wire format for
/// `Timestamptz`).
fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

impl IntoDto<dto::Strategy> for Strategy {
    fn into_dto(self) -> dto::Strategy {
        dto::Strategy {
            id: self.id.to_string(),
            short_code: self.short_code,
            title: self.title,
            content: self.content,
            board_id: self.board_id.to_string(),
            column_id: self.column_id.to_string(),
            hypothesis: self.hypothesis,
            version: self.version,
            created_by: self.created_by.to_string(),
            updated_by: self.updated_by.to_string(),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::Initiative> for Initiative {
    fn into_dto(self) -> dto::Initiative {
        dto::Initiative {
            id: self.id.to_string(),
            short_code: self.short_code,
            title: self.title,
            content: self.content,
            board_id: self.board_id.to_string(),
            column_id: self.column_id.to_string(),
            complexity: self.complexity.map(|c| c.to_string()),
            is_bucket: self.is_bucket,
            bucket_type: self.bucket_type.map(|b| b.to_string()),
            version: self.version,
            created_by: self.created_by.to_string(),
            updated_by: self.updated_by.to_string(),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::Task> for Task {
    fn into_dto(self) -> dto::Task {
        dto::Task {
            id: self.id.to_string(),
            short_code: self.short_code,
            title: self.title,
            content: self.content,
            board_id: self.board_id.to_string(),
            column_id: self.column_id.to_string(),
            task_type: self.task_type.to_string(),
            work_class: self.work_class.to_string(),
            team_id: self.team_id.map(|id| id.to_string()),
            repository_id: self.repository_id.map(|id| id.to_string()),
            // Pure conversion carries the id only; [`attach_repositories`]
            // fills the embedded ref in one batched query where it matters.
            repository: None,
            version: self.version,
            created_by: self.created_by.to_string(),
            updated_by: self.updated_by.to_string(),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::Document> for Document {
    fn into_dto(self) -> dto::Document {
        dto::Document {
            id: self.id.to_string(),
            short_code: self.short_code,
            title: self.title,
            content: self.content,
            template_id: self.template_id.map(|id| id.to_string()),
            lifecycle: self.lifecycle.to_string(),
            version: self.version,
            created_by: self.created_by.to_string(),
            updated_by: self.updated_by.to_string(),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::Adr> for Adr {
    fn into_dto(self) -> dto::Adr {
        dto::Adr {
            id: self.id.to_string(),
            short_code: self.short_code,
            title: self.title,
            content: self.content,
            board_id: self.board_id.map(|id| id.to_string()),
            column_id: self.column_id.map(|id| id.to_string()),
            decision_maker: self.decision_maker,
            decision_date: self.decision_date.map(|d| d.to_string()),
            version: self.version,
            created_by: self.created_by.to_string(),
            updated_by: self.updated_by.to_string(),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

/// The embedded repository ref (KAIROS-T-0104).
pub fn repository_ref(repo: &Repository) -> RepositoryRef {
    RepositoryRef {
        id: repo.id.to_string(),
        slug: repo.slug.clone(),
        forge: repo.forge.to_string(),
        repo_full_name: repo.repo_full_name.clone(),
        team_id: repo.team_id.to_string(),
    }
}

/// Fill `repository` on a batch of task DTOs with ONE query over the
/// distinct `repository_id`s present (KAIROS-T-0104). Tasks bound to a
/// soft-deleted repository keep `repository_id` and get no ref — the id
/// is a fact, the ref is a live lookup. Call at every endpoint that
/// renders tasks; the pure [`IntoDto`] conversion never touches the DB.
pub fn attach_repositories(
    conn: &mut PgConnection,
    tasks: &mut [dto::Task],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = tasks
            .iter()
            .filter_map(|t| t.repository_id.as_deref())
            .filter_map(|id| id.parse().ok())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if ids.is_empty() {
        return Ok(());
    }
    use kairos_db::schema::repositories::dsl;
    let repos: Vec<Repository> = dsl::repositories
        .filter(dsl::id.eq_any(&ids))
        .filter(dsl::deleted_at.is_null())
        .select(Repository::as_select())
        .load(conn)?;
    let by_id: HashMap<String, RepositoryRef> = repos
        .iter()
        .map(|r| (r.id.to_string(), repository_ref(r)))
        .collect();
    for task in tasks.iter_mut() {
        task.repository = task
            .repository_id
            .as_deref()
            .and_then(|id| by_id.get(id).cloned());
    }
    Ok(())
}

/// Single-task convenience over [`attach_repositories`].
pub fn attach_repository(
    conn: &mut PgConnection,
    task: dto::Task,
) -> Result<dto::Task, diesel::result::Error> {
    let mut tasks = [task];
    attach_repositories(conn, &mut tasks)?;
    let [task] = tasks;
    Ok(task)
}
