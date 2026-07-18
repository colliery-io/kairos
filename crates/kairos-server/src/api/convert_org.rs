//! `kairos-db` model → `kairos_client::types_org` DTO conversions for the
//! KAIROS-T-0019 organizational families. Same encoding rules as
//! [`super::convert`] (canonical UUID strings, RFC 3339 timestamps); kept in
//! its own module so T-0018's `convert.rs` stays untouched.

use chrono::{DateTime, Utc};
use kairos_client::types_org as dto;
use kairos_db::models::boards::{Board, BoardColumn, BoardTransition};
use kairos_db::models::teams::{DeliveryStream, Team};

use super::convert::IntoDto;

/// RFC 3339 with microsecond precision (same as [`super::convert`]).
fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

impl IntoDto<dto::Board> for Board {
    fn into_dto(self) -> dto::Board {
        dto::Board {
            id: self.id.to_string(),
            name: self.name,
            slug: self.slug,
            board_level: self.board_level.to_string(),
            team_id: self.team_id.map(|id| id.to_string()),
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::BoardColumn> for BoardColumn {
    fn into_dto(self) -> dto::BoardColumn {
        dto::BoardColumn {
            id: self.id.to_string(),
            board_id: self.board_id.to_string(),
            name: self.name,
            position: self.position,
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::BoardTransition> for BoardTransition {
    fn into_dto(self) -> dto::BoardTransition {
        dto::BoardTransition {
            id: self.id.to_string(),
            board_id: self.board_id.to_string(),
            from_column_id: self.from_column_id.to_string(),
            to_column_id: self.to_column_id.to_string(),
        }
    }
}

impl IntoDto<dto::DeliveryStream> for DeliveryStream {
    fn into_dto(self) -> dto::DeliveryStream {
        dto::DeliveryStream {
            id: self.id.to_string(),
            name: self.name,
            slug: self.slug,
            description: self.description,
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

/// [`Team`] → DTO. Not an [`IntoDto`] impl because the wire type carries
/// `delivery_board_id`, which lives on `boards`, not on the team row.
pub fn team_to_dto(team: Team, delivery_board_id: Option<uuid::Uuid>) -> dto::Team {
    dto::Team {
        id: team.id.to_string(),
        name: team.name,
        slug: team.slug,
        team_type: team.team_type.to_string(),
        delivery_board_id: delivery_board_id.map(|id| id.to_string()),
        created_at: timestamp(team.created_at),
        updated_at: timestamp(team.updated_at),
    }
}
