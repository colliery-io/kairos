//! Wire types of the webhook targets, the agent dispatch rules and the
//! delivery log of a board (KAIROS-T-0384, KAIROS-T-0385, COLLIERY-I-0614).
//!
//! Ids travel as UUID strings and times as RFC 3339 strings, as in
//! [`crate::types_org`].

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// A webhook target of a board: a URL. Kairos sends signed deliveries to
/// it. The secret is never in this type: Kairos shows it one time, in
/// [`CreatedWebhookTarget`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookTarget {
    pub id: String,
    pub board_id: String,
    pub name: String,
    /// The URL. A person who cannot configure the board gets it with no
    /// query and no fragment.
    pub url: String,
    /// False: Kairos turned the target off after failed tries, and sends
    /// nothing to it until a board manager turns it on again. Then Kairos
    /// sends the deliveries that it skipped (KAIROS-T-0577).
    pub enabled: bool,
    /// The failed tries in a row.
    pub consecutive_failures: i32,
    pub disabled_at: Option<String>,
    pub disabled_reason: Option<String>,
    pub created_by: String,
    pub created_at: String,
    /// Only in the answer of `enable` and `redeliver` (KAIROS-T-0577): the
    /// number of `skipped` deliveries that Kairos queued again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redelivered: Option<i64>,
}

/// `POST /api/boards/{id}/webhook-targets/{target_id}/redeliver` query
/// (KAIROS-T-0577).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct RedeliverQuery {
    /// Only the deliveries of events at or after this time (RFC 3339).
    /// With no value: each skipped delivery of the last 30 days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// `POST /api/boards/{id}/webhook-targets` body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWebhookTargetRequest {
    /// A name for people, for example `Agent runner`.
    pub name: String,
    /// An `http://` or `https://` URL. A loopback, private, link-local or
    /// shared address is refused unless the deployment sets
    /// `KAIROS_WEBHOOK_ALLOW_PRIVATE`.
    pub url: String,
}

/// `POST /api/boards/{id}/webhook-targets` response: the target, and its
/// secret. Kairos shows the secret this one time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreatedWebhookTarget {
    pub target: WebhookTarget,
    /// The key of the HMAC-SHA256 of `X-Kairos-Signature`. Keep it in the
    /// receiver. Kairos does not show it again.
    pub secret: String,
}

/// A dispatch rule of a board. With the trigger `entered_column`, the rule
/// fires when a task that matches the filter enters `column`. With
/// `unblocked` (KAIROS-T-0427), it fires when the last open blocker of a task
/// that matches closes. Then the task must be in `column`, if the rule names
/// one. Kairos sends one signed delivery to the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DispatchRule {
    pub id: String,
    pub board_id: String,
    /// `entered_column` or `unblocked`.
    pub trigger: String,
    /// `null`: an `unblocked` rule with no column.
    pub column_id: Option<String>,
    /// The name of the column.
    pub column: Option<String>,
    pub target_id: String,
    /// The name of the target.
    pub target: String,
    /// `task|bug|tech_debt|support`; `null`: each type.
    pub task_type: Option<String>,
    /// The slug of a metadata definition; `null`: no metadata filter.
    pub metadata_slug: Option<String>,
    /// The value of the metadata that matches (no case, no space at the
    /// ends).
    pub metadata_value: Option<String>,
    /// The slug of the repository; `null`: each repository, and none.
    pub repository: Option<String>,
    pub created_by: String,
    pub created_at: String,
}

/// `POST /api/boards/{id}/dispatch-rules` body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDispatchRuleRequest {
    /// `entered_column` (the default) or `unblocked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// The name or the id of a column of the board. Required for
    /// `entered_column`; for `unblocked`, the column the task must be in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// The id of a webhook target of the board.
    pub target_id: String,
    /// Only tasks of this type: `task|bug|tech_debt|support`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    /// Only tasks with this metadata: the slug of a definition. Send it
    /// with `metadata_value`. KAIROS-T-0696: the server also accepts the
    /// name `definition`, the name on MCP.
    #[serde(default, alias = "definition", skip_serializing_if = "Option::is_none")]
    pub metadata_slug: Option<String>,
    /// The value of `metadata_slug` that matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_value: Option<String>,
    /// Only tasks of this repository: its slug or its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

/// One try of a delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookDeliveryTry {
    /// 1 for the first try.
    pub try_number: i32,
    pub at: String,
    /// True: the target answered with a 2xx code.
    pub ok: bool,
    pub response_code: Option<i32>,
    pub error: Option<String>,
    pub duration_ms: i32,
}

/// One delivery of the log, with its tries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookDelivery {
    /// The id; the value of `X-Kairos-Delivery`.
    pub id: String,
    pub target_id: String,
    /// The rule that sent it; `null` for a test delivery.
    pub rule_id: Option<String>,
    /// `task.entered_column`, `task.unblocked` or `ping`.
    pub event: String,
    /// The code of the task now; `null` when there is no task.
    pub short_code: Option<String>,
    /// `pending|delivered|failed|skipped`.
    pub status: String,
    pub tries: i32,
    pub response_code: Option<i32>,
    pub last_error: Option<String>,
    pub next_try_at: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// Each try, oldest first.
    pub log: Vec<WebhookDeliveryTry>,
}

/// `GET /api/boards/{id}/webhook-deliveries` query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct WebhookDeliveriesQuery {
    /// Only the deliveries of this target (id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    /// Only the deliveries of this rule (id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// The newest deliveries to give, 1 to 200 (50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// The id of a delivery of the log: the page has the deliveries older
    /// than it (KAIROS-T-0584).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

/// Who decides the triage of intake on a board (KAIROS-T-0428). Intake is a
/// task in the support lane. Off (the default): an agent proposes with a
/// note, and a person decides. On: an agent moves intake itself, and the
/// activity log names the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AgentTriage {
    pub board_id: String,
    pub agents_decide: bool,
}

/// `PUT /api/boards/{id}/agent-triage` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAgentTriageRequest {
    pub agents_decide: bool,
}
