//! Git-forge connection and link DTOs (KAIROS-T-0097, design in
//! KAIROS-I-0009).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One repository wired up to this tenant. The webhook secret is never
/// carried here — it is returned exactly once, by
/// [`CreatedForgeConnection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ForgeConnection {
    /// Connection id (UUID) — also the webhook URL's last segment.
    pub id: String,
    /// `github|gitlab`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/project` on GitLab.
    pub repo_full_name: String,
    pub repo_url: String,
    /// The owning team (UUID) of the connected repository. Always present
    /// since KAIROS-T-0103 (ownership lives on the repository, A-0019);
    /// kept optional on the wire for one release of client compatibility.
    pub team_id: Option<String>,
    /// The repository this connection delivers webhooks for (UUID,
    /// KAIROS-T-0103).
    pub repository_id: String,
    /// RFC 3339.
    pub created_at: String,
}

/// Body of `POST /api/forge-connections`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateForgeConnectionRequest {
    /// `github|gitlab`.
    pub forge: String,
    /// `owner/repo` — must match what the forge sends in its payloads.
    pub repo_full_name: String,
    /// Browser URL of the repository.
    pub repo_url: String,
    /// Owning team (UUID). REQUIRED since KAIROS-T-0103: connecting a
    /// repository that is not yet registered creates it, and every
    /// repository has exactly one owning team (A-0019). Ignored when the
    /// repository already exists.
    #[serde(default)]
    pub team_id: Option<String>,
}

/// Response of connection creation and rotation: the connection plus the
/// delivery URL and secret to paste into the forge. **The secret is shown
/// here and nowhere else** — it is derived, never stored, so it cannot be
/// re-read later (rotate to get a new one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreatedForgeConnection {
    #[serde(flatten)]
    pub connection: ForgeConnection,
    /// Paste into the forge's webhook "Payload URL".
    pub webhook_url: String,
    /// Paste into the forge's webhook "Secret" (GitHub) / "Secret token"
    /// (GitLab).
    pub webhook_secret: String,
}

/// Body of `PATCH /api/forge-connections/{id}` — re-homes the connected
/// REPOSITORY to another team (KAIROS-T-0103); repo identity is fixed for
/// a connection's lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateForgeConnectionRequest {
    /// New team (UUID). JSON `null` cannot distinguish "unchanged" from
    /// "clear", so set `clear_team` for the latter.
    #[serde(default)]
    pub team_id: Option<String>,
    /// Formerly dropped the team attribution. Since KAIROS-T-0103 a
    /// repository always has an owner, so this is rejected with 422.
    #[serde(default)]
    pub clear_team: bool,
}

/// One row of a team's in-flight rollup (KAIROS-T-0101): a link plus the
/// work item it belongs to, so the panel can link both to the forge and
/// back into Kairos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamLink {
    /// `branch|pull_request`.
    pub kind: String,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    /// Browser URL on the forge.
    pub url: String,
    /// `open|merged|closed|draft`.
    pub state: String,
    pub author: String,
    /// `github|gitlab`.
    pub forge: String,
    /// `owner/repo`.
    pub repo_full_name: String,
    /// The Kairos work item this link belongs to.
    pub item_short_code: String,
    pub item_title: String,
    /// RFC 3339.
    pub forge_updated_at: String,
}

/// One branch or pull/merge request linked to a work item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemLink {
    /// Link id (UUID).
    pub id: String,
    /// The linked work item (UUID).
    pub item_id: String,
    /// `branch|pull_request`.
    pub kind: String,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    /// Browser URL on the forge.
    pub url: String,
    /// `open|merged|closed|draft`.
    pub state: String,
    pub author: String,
    /// `github|gitlab` (from the owning connection).
    pub forge: String,
    /// `owner/repo` (from the owning connection).
    pub repo_full_name: String,
    /// The forge's own last-update time, RFC 3339.
    pub forge_updated_at: String,
}
