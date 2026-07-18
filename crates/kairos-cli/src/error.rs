//! The CLI-level error: every failure carries an actionable, user-facing
//! message and maps to the KAIROS-A-0015 exit-code contract —
//! 0 success · 1 API/validation error · 2 auth error.

use kairos_client::Error as ApiError;

/// Exit code for API/validation/transport failures (KAIROS-A-0015).
pub const EXIT_FAILURE: u8 = 1;
/// Exit code for authentication failures (KAIROS-A-0015): missing/expired
/// credentials, declined grants, 401s, refresh failures.
pub const EXIT_AUTH: u8 = 2;

/// A failed CLI invocation. `Auth` exits 2; `Failure` exits 1.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Authentication problem — the user must (re-)run `kairos login`.
    #[error("{0}")]
    Auth(String),

    /// API/validation/transport problem.
    #[error("{0}")]
    Failure(String),
}

impl CliError {
    /// The KAIROS-A-0015 process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Auth(_) => EXIT_AUTH,
            CliError::Failure(_) => EXIT_FAILURE,
        }
    }
}

impl From<ApiError> for CliError {
    /// Map the typed client error to the exit-code contract: 401s and
    /// token-provider failures (the refresh path) are auth errors (2);
    /// everything else — API rejections, transport, decoding — is an
    /// API/validation error (1). The structured S-0005 rejections render
    /// their actionable extras: 409 the server-current version, 422
    /// `INVALID_TRANSITION` the allowed target columns, 403 the required
    /// capability (KAIROS-T-0037).
    fn from(err: ApiError) -> Self {
        match err {
            ApiError::Unauthorized { message, .. } => CliError::Auth(format!(
                "the deployment rejected the token (401): {message}\n\
                 Run `kairos login --url <deployment>` to re-authenticate."
            )),
            ApiError::Token(message) => CliError::Auth(message),
            ApiError::Forbidden {
                message,
                capability,
                details,
                ..
            } => {
                let mut text = format!("access denied (403): {message}");
                if let Some(capability) = capability {
                    text.push_str(&format!(
                        "\nThis action requires the {capability:?} capability on the item's \
                         board; ask an org admin to grant it."
                    ));
                } else if details["required"] == "deployment_admin" {
                    text.push_str(
                        "\nThis is a deployment-admin operation: your OIDC subject must be \
                         listed in the server's KAIROS_DEPLOYMENT_ADMINS.",
                    );
                } else {
                    text.push_str(
                        "\nYou are authenticated, but this deployment has not granted you \
                         access. Ask an org admin to add you as a member.",
                    );
                }
                CliError::Failure(text)
            }
            ApiError::Conflict {
                code,
                message,
                current,
                ..
            } => {
                let mut text = format!("conflict (409 {code}): {message}");
                if let Some(version) = current["version"].as_i64() {
                    let title = current["title"].as_str().unwrap_or("?");
                    text.push_str(&format!(
                        "\nThe server-current version is {version} (title: {title:?}). \
                         Review it with `get`, then re-run the edit — the CLI re-bases on \
                         the latest version unless you pass --version explicitly."
                    ));
                }
                CliError::Failure(text)
            }
            ApiError::InvalidTransition {
                message,
                allowed_targets,
                ..
            } => {
                let mut text = format!("invalid transition (422): {message}");
                match allowed_targets.as_array() {
                    Some(targets) if !targets.is_empty() => {
                        text.push_str("\nAllowed target columns:");
                        for target in targets {
                            text.push_str(&format!(
                                "\n  - {} ({})",
                                target["name"].as_str().unwrap_or("?"),
                                target["id"].as_str().unwrap_or("?"),
                            ));
                        }
                    }
                    Some(_) => text
                        .push_str("\nNo transitions are allowed from the item's current column."),
                    None => {}
                }
                CliError::Failure(text)
            }
            ApiError::Other {
                status,
                code,
                message,
                ..
            } if code == "LAST_ADMIN" => CliError::Failure(format!(
                "{status} LAST_ADMIN: {message}\n\
                 An organization must keep at least one admin; promote another member \
                 to admin first."
            )),
            ApiError::Transport(err) => CliError::Failure(format!(
                "could not reach the deployment: {err}\n\
                 Check the URL and your network connection."
            )),
            other => CliError::Failure(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// 401s and token-provider failures exit 2; other API errors exit 1
    /// (KAIROS-A-0015).
    #[test]
    fn exit_code_contract() {
        let unauthorized: CliError = ApiError::Unauthorized {
            code: "UNAUTHORIZED".into(),
            message: "bad token".into(),
            details: Value::Null,
        }
        .into();
        assert_eq!(unauthorized.exit_code(), EXIT_AUTH);
        assert!(unauthorized.to_string().contains("kairos login"));

        let token: CliError = ApiError::Token("refresh failed; run `kairos login`".into()).into();
        assert_eq!(token.exit_code(), EXIT_AUTH);

        let not_found: CliError = ApiError::NotFound {
            code: "NOT_FOUND".into(),
            message: "gone".into(),
            details: Value::Null,
        }
        .into();
        assert_eq!(not_found.exit_code(), EXIT_FAILURE);

        let forbidden: CliError = ApiError::Forbidden {
            code: "MEMBERSHIP_REQUIRED".into(),
            message: "not a member".into(),
            capability: None,
            details: Value::Null,
        }
        .into();
        assert_eq!(forbidden.exit_code(), EXIT_FAILURE);
        assert!(forbidden.to_string().contains("org admin"));
    }

    /// 403 with a KAIROS-A-0006 capability check names the capability
    /// (KAIROS-T-0037).
    #[test]
    fn forbidden_names_the_capability() {
        let err: CliError = ApiError::Forbidden {
            code: "FORBIDDEN".into(),
            message: "denied".into(),
            capability: Some("manage_tasks".into()),
            details: serde_json::json!({"required_capability": "manage_tasks"}),
        }
        .into();
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("\"manage_tasks\""), "{err}");
    }

    /// The deployment-admin gate's 403 points at KAIROS_DEPLOYMENT_ADMINS
    /// instead of the org-admin membership hint.
    #[test]
    fn forbidden_deployment_admin_hint() {
        let err: CliError = ApiError::Forbidden {
            code: "FORBIDDEN".into(),
            message: "this action requires deployment-admin privileges".into(),
            capability: None,
            details: serde_json::json!({"required": "deployment_admin"}),
        }
        .into();
        assert!(
            err.to_string().contains("KAIROS_DEPLOYMENT_ADMINS"),
            "{err}"
        );
    }

    /// 409 CONFLICT renders the server-current version guidance
    /// (KAIROS-A-0004 optimistic concurrency).
    #[test]
    fn conflict_renders_current_version_guidance() {
        let err: CliError = ApiError::Conflict {
            code: "CONFLICT".into(),
            message: "version mismatch: expected 1, current is 4".into(),
            current: serde_json::json!({"version": 4, "title": "Newer title"}),
            details: serde_json::json!({"current": {"version": 4}}),
        }
        .into();
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        let text = err.to_string();
        assert!(text.contains("server-current version is 4"), "{text}");
        assert!(text.contains("Newer title"), "{text}");
        assert!(text.contains("--version"), "{text}");
    }

    /// 422 INVALID_TRANSITION lists the allowed target columns, name + id.
    #[test]
    fn invalid_transition_lists_allowed_targets() {
        let err: CliError = ApiError::InvalidTransition {
            message: "transition \"Backlog\" -> \"Active\" is not allowed".into(),
            allowed_targets: serde_json::json!([
                {"id": "0193-c1", "name": "Todo"},
                {"id": "0193-c2", "name": "Blocked"},
            ]),
            details: Value::Null,
        }
        .into();
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        let text = err.to_string();
        assert!(text.contains("Allowed target columns:"), "{text}");
        assert!(text.contains("Todo (0193-c1)"), "{text}");
        assert!(text.contains("Blocked (0193-c2)"), "{text}");

        let empty: CliError = ApiError::InvalidTransition {
            message: "no moves".into(),
            allowed_targets: serde_json::json!([]),
            details: Value::Null,
        }
        .into();
        assert!(
            empty
                .to_string()
                .contains("No transitions are allowed from the item's current column"),
            "{empty}"
        );
    }

    /// 422 LAST_ADMIN gets the keep-one-admin guidance (KAIROS-T-0037).
    #[test]
    fn last_admin_is_surfaced_with_guidance() {
        let err: CliError = ApiError::Other {
            status: 422,
            code: "LAST_ADMIN".into(),
            message: "cannot demote the last admin".into(),
            details: Value::Null,
        }
        .into();
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        let text = err.to_string();
        assert!(text.contains("LAST_ADMIN"), "{text}");
        assert!(text.contains("at least one admin"), "{text}");
    }
}
