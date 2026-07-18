//! `kairos-db` model → `kairos_client::types` DTO conversions
//! (KAIROS-T-0018). The DTO crate stays dependency-light (no uuid/chrono),
//! so ids render as canonical UUID strings and timestamps as RFC 3339 —
//! this module is the ONE place that encoding is decided.
//!
//! Both sides are foreign types, so the orphan rule forbids `From` impls
//! here; [`IntoDto`] is the local conversion trait instead.

use chrono::{DateTime, Utc};
use kairos_client::types as dto;
use kairos_db::models::items::{Adr, Document, Initiative, Strategy, Task};

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
            team_id: self.team_id.map(|id| id.to_string()),
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
