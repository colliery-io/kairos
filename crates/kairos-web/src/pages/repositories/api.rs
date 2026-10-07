//! Repository mirrors + calls (KAIROS-T-0109, A-0019), per the
//! conventions' data-layer rules (docs/gui-conventions.md §4 — partial
//! mirrors with the exact wire field names, a `mirror of:` line each, and
//! a decode test).

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api::{get_json, put_json};
use crate::auth::Auth;

/// The repository pickers' "no repository" option value (item page and
/// the board's New task modal, KAIROS-T-0124 #6b).
pub const NO_REPOSITORY: &str = "(none)";

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
    /// The status of the read token (COLLIERY-T-3105). Never the token.
    #[serde(default)]
    pub credential: RepositoryCredential,
    /// `on` or `off` (KAIROS-T-0318): whether the code index builder
    /// works on the repository.
    #[serde(default = "code_index_build_on")]
    pub code_index_build: String,
    /// `embedded` or `hosted` (KAIROS-T-0340): where the summaries of the
    /// code index of the repository are made.
    #[serde(default = "code_index_summaries_embedded")]
    pub code_index_summaries: String,
}

/// The default of [`Repository::code_index_summaries`]: an older server
/// does not send it, and the embedded model writes the summaries.
fn code_index_summaries_embedded() -> String {
    "embedded".to_string()
}

/// The default of [`Repository::code_index_build`]: an older server does not
/// send it, and the builder works on each repository.
fn code_index_build_on() -> String {
    "on".to_string()
}

impl Repository {
    /// The text of the setting `code_index_summaries` (KAIROS-T-0340).
    pub fn code_index_summaries_summary(&self) -> &'static str {
        if self.code_index_summaries == "hosted" {
            "Summaries: the hosted provider of the organization. The code of each changed \
             symbol leaves the host."
        } else {
            "Summaries: the embedded model."
        }
    }

    /// The text of the setting `code_index_build` (KAIROS-T-0318).
    pub fn code_index_build_summary(&self) -> &'static str {
        if self.code_index_build == "off" {
            "Code index builder: off. Kairos makes no index of this repository."
        } else {
            "Code index builder: on."
        }
    }
}

/// mirror of: `kairos_client::types_repositories::RepositoryCredential`:
/// the status of the read token of a repository (COLLIERY-T-3105). The
/// server never sends the token.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RepositoryCredential {
    #[serde(default)]
    pub set: bool,
    #[serde(default)]
    pub set_by_name: Option<String>,
    #[serde(default)]
    pub set_by: Option<String>,
    #[serde(default)]
    pub set_at: Option<String>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    #[serde(default)]
    pub last_check_ok: Option<bool>,
    #[serde(default)]
    pub last_check_error: Option<String>,
}

impl RepositoryCredential {
    /// The text of the status on the admin page.
    pub fn summary(&self) -> String {
        if !self.set {
            return "Read token: not set.".to_string();
        }
        let by = self
            .set_by_name
            .as_deref()
            .or(self.set_by.as_deref())
            .unwrap_or("-");
        let at = self.set_at.as_deref().unwrap_or("-");
        let check = match (self.last_check_ok, self.last_checked_at.as_deref()) {
            (Some(true), Some(when)) => format!(" The last check at {when} passed."),
            (Some(false), Some(when)) => format!(
                " The last check at {when} failed: {}",
                self.last_check_error.as_deref().unwrap_or("-")
            ),
            _ => " Not checked.".to_string(),
        };
        format!("Read token: set by {by} at {at}.{check}")
    }
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

/// mirror of: `kairos_client::types_repositories::ImpactingItem`: one
/// document or ADR that impacts a repository (COLLIERY-T-0269).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ImpactingItem {
    pub short_code: String,
    pub title: String,
    /// `document` or `adr`.
    pub entity_type: String,
    /// The document type (`vision`, `architecture`, ...), if the document
    /// has one.
    #[serde(default)]
    pub document_type: Option<String>,
    /// The editorial lifecycle of a document.
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// The name of the column of an ADR.
    #[serde(default)]
    pub column: Option<String>,
}

/// mirror of: `kairos_client::types_repositories::RepositoryDetail`
/// (partial — the documents and the ADRs that impact the repository).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct RepositoryImpacts {
    #[serde(default)]
    impacted_by: Vec<ImpactingItem>,
}

/// `GET /api/repositories/{slug}` — the live documents and ADRs that
/// impact the repository (COLLIERY-T-0269).
pub async fn impacted_by(auth: Auth, slug: &str) -> Result<Vec<ImpactingItem>, ApiError> {
    let detail: RepositoryImpacts = get_json(
        auth,
        &format!("/api/repositories/{}", crate::api::encode_component(slug)),
    )
    .await?;
    Ok(detail.impacted_by)
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
        assert!(!bare.credential.set);
        assert_eq!(bare.credential.summary(), "Read token: not set.");

        // COLLIERY-T-3105: the status of the read token.
        let with_token: Repository = serde_json::from_value(serde_json::json!({
            "id": "r-3", "slug": "skadi", "forge": "github",
            "repo_full_name": "skadi-media/skadi",
            "repo_url": "https://github.com/skadi-media/skadi",
            "default_branch": "main",
            "team": {"id": "t-1", "slug": "platform", "name": "Platform"},
            "credential": {"set": true, "set_by": "u-1", "set_by_name": "Ada",
                           "set_at": "2026-10-03T00:00:00+00:00", "last_checked_at": null,
                           "last_check_ok": null, "last_check_error": null}
        }))
        .expect("the credential decodes");
        assert_eq!(
            with_token.credential.summary(),
            "Read token: set by Ada at 2026-10-03T00:00:00+00:00. Not checked."
        );

        let embedded: RepositoryRef =
            serde_json::from_value(serde_json::json!({"id": "r-1", "slug": "payments-api"}))
                .expect("ref mirror decodes without repo_full_name");
        assert_eq!(embedded.repo_full_name, "");
    }

    /// COLLIERY-T-0269: the detail of a repository has the documents and
    /// the ADRs that impact it. A body of an older server has none.
    #[test]
    fn the_impacting_items_decode_server_shape() {
        let detail: RepositoryImpacts = serde_json::from_value(serde_json::json!({
            "id": "r-1", "slug": "fidius", "connection_id": null, "in_flight": [],
            "impacted_by": [
                {"short_code": "ACME-D-0004", "title": "The vision of fidius",
                 "entity_type": "document", "document_type": "vision",
                 "lifecycle": "published", "column": null},
                {"short_code": "ACME-A-0002", "title": "Plugins are dynamic libraries",
                 "entity_type": "adr", "document_type": null, "lifecycle": null,
                 "column": "Decided"}
            ]
        }))
        .expect("the mirror decodes");
        assert_eq!(detail.impacted_by.len(), 2);
        assert_eq!(
            detail.impacted_by[0].document_type.as_deref(),
            Some("vision")
        );
        assert_eq!(detail.impacted_by[1].column.as_deref(), Some("Decided"));
        let old: RepositoryImpacts =
            serde_json::from_value(serde_json::json!({"id": "r-1", "slug": "fidius"}))
                .expect("a body with no impacted_by decodes");
        assert!(old.impacted_by.is_empty());
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
