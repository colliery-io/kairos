//! Repository mirrors + calls (KAIROS-T-0109, A-0019), per the
//! conventions' data-layer rules (docs/gui-conventions.md §4 — partial
//! mirrors with the exact wire field names, a `mirror of:` line each, and
//! a decode test).

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api::{get_json, put_json};
use crate::auth::Auth;

/// mirror of: `kairos_client::types_repositories::RepositoryRef` (partial —
/// what cards and pickers show). Embedded on tasks by the server
/// (KAIROS-T-0104).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RepositoryRef {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub repo_full_name: String,
}

/// mirror of: `kairos_client::types_repositories::RepositoryTeam`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RepositoryTeam {
    pub id: String,
    pub slug: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_repositories::Repository`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Repository {
    pub id: String,
    pub slug: String,
    pub forge: String,
    pub repo_full_name: String,
    pub repo_url: String,
    pub default_branch: String,
    #[serde(default)]
    pub description: String,
    pub team: RepositoryTeam,
    #[serde(default)]
    pub delivery_board_id: Option<String>,
    #[serde(default)]
    pub open_tasks: i64,
    #[serde(default)]
    pub has_webhook: bool,
}

/// `GET /api/repositories[?team=]` — the directory, optionally one team's.
pub async fn list_repositories(
    auth: Auth,
    team: Option<&str>,
) -> Result<Vec<Repository>, ApiError> {
    let path = match team {
        Some(team) => format!("/api/repositories?team={team}"),
        None => "/api/repositories".to_string(),
    };
    get_json(auth, &path).await
}

/// mirror of: `kairos_client::types_repositories::SetTaskRepositoryRequest`.
#[derive(Debug, Serialize)]
struct SetTaskRepositoryRequest<'a> {
    repository: Option<&'a str>,
}

/// `PUT /api/tasks/{code}/repository` — bind, re-home, or clear (`None`).
pub async fn set_repository(
    auth: Auth,
    short_code: &str,
    repository: Option<&str>,
) -> Result<(), ApiError> {
    let path = format!("/api/tasks/{short_code}/repository");
    let _: serde_json::Value =
        put_json(auth, &path, &SetTaskRepositoryRequest { repository }).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository mirrors decode a realistic `GET /api/repositories`
    /// row (field-name lock against the S-0005 wire shape; optional
    /// counts/flags default when absent).
    #[test]
    fn repository_mirrors_decode_server_shape() {
        let repo: Repository = serde_json::from_value(serde_json::json!({
            "id": "r-1", "slug": "payments-api", "forge": "github",
            "repo_full_name": "acme/payments-api",
            "repo_url": "https://github.com/acme/payments-api",
            "default_branch": "main", "description": "",
            "team": {"id": "t-1", "slug": "platform", "name": "Platform"},
            "delivery_board_id": "b-1", "open_tasks": 3, "has_webhook": true,
            "created_at": "x", "updated_at": "x"
        }))
        .expect("repository mirror decodes");
        assert_eq!(repo.team.slug, "platform");
        assert_eq!((repo.open_tasks, repo.has_webhook), (3, true));

        let bare: Repository = serde_json::from_value(serde_json::json!({
            "id": "r-2", "slug": "infra", "forge": "other",
            "repo_full_name": "acme/infra", "repo_url": "https://x/acme/infra",
            "default_branch": "main",
            "team": {"id": "t-1", "slug": "platform", "name": "Platform"}
        }))
        .expect("defaults fill absent optionals");
        assert_eq!((bare.open_tasks, bare.has_webhook), (0, false));
        assert!(bare.delivery_board_id.is_none());

        let embedded: RepositoryRef =
            serde_json::from_value(serde_json::json!({"id": "r-1", "slug": "payments-api"}))
                .expect("ref mirror decodes without repo_full_name");
        assert_eq!(embedded.repo_full_name, "");
    }

    /// The bind request serializes `null` for a clear (the server reads
    /// the key's presence, not its absence).
    #[test]
    fn set_repository_request_serializes_wire_shape() {
        let bind = serde_json::to_value(SetTaskRepositoryRequest {
            repository: Some("payments-api"),
        })
        .expect("serializes");
        assert_eq!(bind, serde_json::json!({"repository": "payments-api"}));
        let clear = serde_json::to_value(SetTaskRepositoryRequest { repository: None })
            .expect("serializes");
        assert_eq!(clear, serde_json::json!({"repository": null}));
    }
}
