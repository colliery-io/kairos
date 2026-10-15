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
    /// The browser URL on the forge. An older server does not send it.
    #[serde(default)]
    pub repo_url: String,
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
    /// The branch that the code index builder indexes (KAIROS-T-0430).
    pub default_branch: String,
    /// `forge` or `hand` (KAIROS-T-0430): where `default_branch` comes
    /// from.
    #[serde(default = "branch_from_forge")]
    pub default_branch_source: String,
    /// The default branch that the remote gave at the last fetch.
    #[serde(default)]
    pub forge_branch: Option<String>,
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
    /// The GitHub write-back (KAIROS-T-0386). Never the write token.
    #[serde(default)]
    pub write_back: RepositoryWriteBack,
    /// `on` or `off` (KAIROS-T-0318): whether the code index builder
    /// works on the repository.
    #[serde(default = "code_index_build_on")]
    pub code_index_build: String,
    /// `embedded` or `hosted` (KAIROS-T-0340): where the summaries of the
    /// code index of the repository are made.
    #[serde(default = "code_index_summaries_embedded")]
    pub code_index_summaries: String,
    /// `embedded` or `hosted`: where the summaries are made now
    /// (KAIROS-T-0358).
    #[serde(default = "code_index_summaries_embedded")]
    pub code_index_summaries_resolved: String,
    /// The breaker of the code index builder (KAIROS-T-0399).
    #[serde(default)]
    pub code_index_breaker: CodeIndexBreaker,
    /// The published site of the repository (KAIROS-T-0408).
    #[serde(default)]
    pub site: RepositorySite,
    /// The forge filters (KAIROS-T-0479): field name → rule. An older
    /// server does not send it: each field has the default.
    #[serde(default)]
    pub forge_filters: std::collections::BTreeMap<String, ForgeFilterRule>,
}

/// mirror of: `kairos_client::types_repositories::ForgeFilterRule`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ForgeFilterRule {
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub exclude: Option<String>,
}

/// The fields of the forge filter, in the order of the docs
/// (KAIROS-T-0479).
pub const FORGE_FILTER_FIELDS: [&str; 6] = [
    "branch_name",
    "pr_head_branch",
    "pr_title",
    "pr_author",
    "issue_title",
    "issue_author",
];

/// mirror of: `kairos_client::types_repositories::CodeIndexBreaker`: the
/// breaker of the code index builder of a repository (KAIROS-T-0399). A
/// build job that stopped its server or stalled trips it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CodeIndexBreaker {
    #[serde(default)]
    pub tripped: bool,
    #[serde(default)]
    pub tripped_at: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl CodeIndexBreaker {
    /// The text of the breaker on the admin pages. Pure, host-tested.
    pub fn summary(&self) -> String {
        if !self.tripped {
            return "Code index breaker: not tripped.".to_string();
        }
        format!(
            "Code index breaker: tripped at {}. {}. Kairos starts no build of this repository \
             until a person pushes \"Build again\" in the panel Code index.",
            self.tripped_at.as_deref().unwrap_or("-"),
            self.reason.as_deref().unwrap_or("-").trim_end_matches('.')
        )
    }
}

/// mirror of: `kairos_client::types_sites::RepositorySite`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RepositorySite {
    #[serde(default)]
    pub enabled: bool,
    /// Where Kairos serves the site.
    #[serde(default)]
    pub base_path: String,
    #[serde(default)]
    pub published: Option<SitePublished>,
    /// The live publish was built for another base path.
    #[serde(default)]
    pub needs_republish: bool,
}

/// mirror of: `kairos_client::types_sites::SitePublish` (the fields that the
/// GUI shows).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SitePublished {
    /// RFC 3339.
    pub published_at: String,
    pub md5: String,
    #[serde(default)]
    pub principal_name: Option<String>,
}

/// The default of [`Repository::code_index_summaries`]: an older server
/// does not send it, and the embedded model writes the summaries.
fn code_index_summaries_embedded() -> String {
    "embedded".to_string()
}

/// The default of [`Repository::code_index_build`]: an older server does not
/// send it, and the builder works on each repository.
fn branch_from_forge() -> String {
    "forge".to_string()
}

fn code_index_build_on() -> String {
    "on".to_string()
}

impl Repository {
    /// The text of the setting `code_index_summaries` (KAIROS-T-0340).
    pub fn code_index_summaries_summary(&self) -> &'static str {
        match (
            self.code_index_summaries.as_str(),
            self.code_index_summaries_resolved.as_str(),
        ) {
            ("organization", "hosted") => {
                "Summaries: the default of the organization, now the hosted provider. The code \
                 of each changed symbol leaves the host."
            }
            ("organization", _) => {
                "Summaries: the default of the organization, now the embedded model."
            }
            ("hosted", _) => {
                "Summaries: the hosted provider of the organization. The code of each changed \
                 symbol leaves the host."
            }
            _ => "Summaries: the embedded model.",
        }
    }

    /// The indexed branch and where it comes from (KAIROS-T-0430).
    pub fn default_branch_summary(&self) -> String {
        let branch = &self.default_branch;
        match (
            self.default_branch_source.as_str(),
            self.forge_branch.as_deref(),
        ) {
            ("hand", Some(forge)) if forge != branch => {
                format!("Indexed branch: {branch}, set by hand. The forge gives {forge}.")
            }
            ("hand", _) => format!("Indexed branch: {branch}, set by hand."),
            (_, Some(_)) => format!("Indexed branch: {branch}, from the forge."),
            (_, None) => format!(
                "Indexed branch: {branch}. Kairos reads the default branch of the forge at \
                 the next fetch."
            ),
        }
    }

    /// The include and exclude pattern of a forge filter field, with the
    /// defaults (KAIROS-T-0479).
    pub fn forge_filter(&self, field: &str) -> (String, String) {
        let rule = self.forge_filters.get(field).cloned().unwrap_or_default();
        (
            rule.include.unwrap_or_else(|| ".*".to_string()),
            rule.exclude.unwrap_or_default(),
        )
    }

    /// The text of the forge filters (KAIROS-T-0479). Pure, host-tested.
    pub fn forge_filters_summary(&self) -> String {
        let parts: Vec<String> = FORGE_FILTER_FIELDS
            .iter()
            .filter_map(|field| {
                let (include, exclude) = self.forge_filter(field);
                let include_default = include.is_empty() || include == ".*";
                match (include_default, exclude.is_empty()) {
                    (true, true) => None,
                    (true, false) => Some(format!("{field} exclude {exclude}")),
                    (false, true) => Some(format!("{field} include {include}")),
                    (false, false) => Some(format!("{field} include {include}, exclude {exclude}")),
                }
            })
            .collect();
        if parts.is_empty() {
            "Forge filters: the default. Kairos picks up each branch, pull request and issue."
                .to_string()
        } else {
            format!("Forge filters: {}.", parts.join("; "))
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

/// mirror of: `kairos_client::types_repositories::RepositoryWriteBack`:
/// the GitHub write-back of a repository (KAIROS-T-0386). The server never
/// sends the write token.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RepositoryWriteBack {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub set_by_name: Option<String>,
    #[serde(default)]
    pub set_by: Option<String>,
    #[serde(default)]
    pub set_at: Option<String>,
    #[serde(default)]
    pub last_write_at: Option<String>,
    #[serde(default)]
    pub last_write_ok: Option<bool>,
    #[serde(default)]
    pub last_write_error: Option<String>,
}

impl RepositoryWriteBack {
    /// The text of the status on the admin page. Pure, host-tested.
    pub fn summary(&self) -> String {
        if !self.enabled {
            return "GitHub write-back: off. The repository has no write token.".to_string();
        }
        let by = self
            .set_by_name
            .as_deref()
            .or(self.set_by.as_deref())
            .unwrap_or("-");
        let at = self.set_at.as_deref().unwrap_or("-");
        let last = match (self.last_write_ok, self.last_write_at.as_deref()) {
            (Some(true), Some(when)) => format!(" The last write at {when} worked."),
            (Some(false), Some(when)) => format!(
                " The last write at {when} failed: {}",
                self.last_write_error.as_deref().unwrap_or("-")
            ),
            _ => " No write yet.".to_string(),
        };
        format!("GitHub write-back: on. {by} set the write token at {at}.{last}")
    }
}

/// `GET /api/repositories[?team=]` — the directory, optionally one team's.
/// The route gives pages, as each other list (KAIROS-T-0268), and this
/// function reads each page.
pub async fn list_repositories(
    auth: Auth,
    team: Option<&str>,
) -> Result<Vec<Repository>, ApiError> {
    let path = match team {
        Some(team) => format!(
            "/api/repositories?team={}",
            crate::api::encode_component(team)
        ),
        None => "/api/repositories".to_string(),
    };
    crate::api::get_all(auth, &path).await
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

/// mirror of: `kairos_client::types_forge::ForgeRef` (KAIROS-T-0480): a
/// branch or a pull request of a repository, linked to an item or not.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ForgeRef {
    /// `branch|pull_request`.
    pub kind: String,
    pub external_id: String,
    pub title: String,
    pub url: String,
    /// `open|merged|closed|draft`.
    pub state: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub forge: String,
    #[serde(default)]
    pub repo_full_name: String,
    #[serde(default)]
    pub linked: bool,
    #[serde(default)]
    pub item_short_codes: Vec<String>,
}

impl ForgeRef {
    /// The label of the row: `#12 Title` for a pull request, the branch
    /// name for a branch. Pure, host-tested.
    pub fn label(&self) -> String {
        if self.kind == "pull_request" {
            format!("#{} {}", self.external_id, self.title)
        } else {
            self.title.clone()
        }
    }
}

/// mirror of: `kairos_client::types_repositories::RepositoryDetail`
/// (partial — the branches and pull requests, KAIROS-T-0480).
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct RepositoryRefs {
    #[serde(default)]
    refs: Vec<ForgeRef>,
}

/// `GET /api/repositories/{slug}` — the open branches and pull requests
/// that no item is linked to (KAIROS-T-0480).
pub async fn unlinked_refs(auth: Auth, slug: &str) -> Result<Vec<ForgeRef>, ApiError> {
    let detail: RepositoryRefs = get_json(
        auth,
        &format!("/api/repositories/{}", crate::api::encode_component(slug)),
    )
    .await?;
    Ok(detail.refs.into_iter().filter(|r| !r.linked).collect())
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

    /// KAIROS-T-0408: the site of a repository decodes, and an older
    /// server's answer with no site decodes as "off".
    #[test]
    fn repository_site_decodes() {
        let base = serde_json::json!({
            "id": "r-1", "slug": "payments", "forge": "other",
            "repo_full_name": "payments", "repo_url": "https://example.com/p",
            "default_branch": "main",
            "team": {"id": "t-1", "slug": "platform", "name": "Platform"},
        });
        let old: Repository = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(old.site, RepositorySite::default());
        let mut with_site = base;
        with_site["site"] = serde_json::json!({
            "enabled": true,
            "base_path": "/pages/acme/platform/repos/payments/",
            "needs_republish": true,
            "published": {
                "publish_id": "p-1", "repository_slug": "payments",
                "base_path": "/pages/acme/web/repos/payments/",
                "md5": "0123456789abcdef0123456789abcdef", "size": 10,
                "unpacked_size": 20, "files": 2, "principal_id": "u-1",
                "principal_name": "Robin", "agent_key_id": null,
                "published_at": "2026-10-13T02:14:26+00:00", "state": "live"
            }
        });
        let repo: Repository = serde_json::from_value(with_site).unwrap();
        assert!(repo.site.enabled && repo.site.needs_republish);
        let published = repo.site.published.unwrap();
        assert_eq!(published.principal_name.as_deref(), Some("Robin"));
        assert_eq!(published.published_at, "2026-10-13T02:14:26+00:00");
    }

    /// The repository mirrors decode a realistic `GET /api/repositories`
    /// row (field-name lock against the S-0005 wire shape; optional
    /// counts/flags default when absent).
    #[test]
    fn the_repository_ref_decodes_the_url_of_the_code() {
        let with_url: RepositoryRef = serde_json::from_value(serde_json::json!({
            "id": "r-1", "slug": "payments-api", "forge": "github",
            "repo_full_name": "acme/payments-api",
            "repo_url": "https://github.com/acme/payments-api", "team_id": "t-1"
        }))
        .expect("the mirror decodes");
        assert_eq!(with_url.repo_url, "https://github.com/acme/payments-api");
        // An older server sends no URL: the repository has no link.
        let older: RepositoryRef = serde_json::from_value(serde_json::json!({
            "id": "r-1", "slug": "payments-api", "forge": "github",
            "repo_full_name": "acme/payments-api", "team_id": "t-1"
        }))
        .expect("the mirror decodes");
        assert_eq!(older.repo_url, "");
    }

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
        // KAIROS-T-0386: an older server sends no write-back: off.
        assert!(!bare.write_back.enabled);
        assert_eq!(
            bare.write_back.summary(),
            "GitHub write-back: off. The repository has no write token."
        );
        let on = RepositoryWriteBack {
            enabled: true,
            set_by_name: Some("Ada".into()),
            set_at: Some("2026-10-12T00:00:00+00:00".into()),
            last_write_at: Some("2026-10-12T01:00:00+00:00".into()),
            last_write_ok: Some(false),
            last_write_error: Some("GitHub answered 502.".into()),
            ..Default::default()
        };
        assert_eq!(
            on.summary(),
            "GitHub write-back: on. Ada set the write token at 2026-10-12T00:00:00+00:00. \
             The last write at 2026-10-12T01:00:00+00:00 failed: GitHub answered 502."
        );

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

        // KAIROS-T-0479: an older server sends no forge filters: defaults.
        assert_eq!(
            bare.forge_filters_summary(),
            "Forge filters: the default. Kairos picks up each branch, pull request and issue."
        );
        let filtered: Repository = serde_json::from_value(serde_json::json!({
            "id": "r-4", "slug": "app", "forge": "github",
            "repo_full_name": "acme/app", "repo_url": "https://github.com/acme/app",
            "default_branch": "main",
            "team": {"id": "t-1", "slug": "platform", "name": "Platform"},
            "forge_filters": {
                "branch_name": {"include": ".*", "exclude": ""},
                "pr_author": {"include": ".*", "exclude": "^dependabot\\[bot\\]$"},
                "issue_title": {"include": "^Bug", "exclude": ""}
            }
        }))
        .expect("the forge filters decode");
        assert_eq!(
            filtered.forge_filter("pr_author"),
            (".*".to_string(), "^dependabot\\[bot\\]$".to_string())
        );
        assert_eq!(
            filtered.forge_filter("pr_title"),
            (".*".to_string(), String::new())
        );
        assert_eq!(
            filtered.forge_filters_summary(),
            "Forge filters: pr_author exclude ^dependabot\\[bot\\]$; issue_title include ^Bug."
        );

        let embedded: RepositoryRef =
            serde_json::from_value(serde_json::json!({"id": "r-1", "slug": "payments-api"}))
                .expect("ref mirror decodes without repo_full_name");
        assert_eq!(embedded.repo_full_name, "");
    }

    /// KAIROS-T-0480: the branches and pull requests of a repository decode;
    /// an older server sends none.
    #[test]
    fn the_refs_decode_server_shape() {
        let detail: RepositoryRefs = serde_json::from_value(serde_json::json!({
            "id": "r-1", "slug": "app", "refs": [
                {"kind": "pull_request", "external_id": "7", "title": "Fix crash",
                 "url": "https://github.com/acme/app/pull/7", "state": "open",
                 "author": "robin", "head_branch": "fix", "forge": "github",
                 "repo_full_name": "acme/app", "repository": "app", "linked": false,
                 "item_short_codes": [], "forge_updated_at": "x"},
                {"kind": "branch", "external_id": "sam/x", "title": "sam/x",
                 "url": "u", "state": "open", "linked": true,
                 "item_short_codes": ["APP-T-0001"]}
            ]
        }))
        .expect("refs decode");
        assert_eq!(detail.refs.len(), 2);
        assert_eq!(detail.refs[0].label(), "#7 Fix crash");
        assert_eq!(detail.refs[1].label(), "sam/x");
        assert!(!detail.refs[0].linked && detail.refs[1].linked);
        let old: RepositoryRefs =
            serde_json::from_value(serde_json::json!({"id": "r-1"})).expect("no refs");
        assert!(old.refs.is_empty());
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
