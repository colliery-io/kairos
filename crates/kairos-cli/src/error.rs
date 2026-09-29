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
                "The deployment did not accept the token (401): {message}\n\
                 Run `kairos login --url <deployment>` to log in again."
            )),
            ApiError::Token(message) => CliError::Auth(message),
            ApiError::Forbidden {
                message,
                capability,
                details,
                ..
            } => {
                let mut text = format!("The deployment refused the request (403): {message}");
                if let Some(capability) = capability {
                    // COLLIERY-T-0267: the message of the server says what
                    // the action requires, so the CLI adds only what to do
                    // next, and only when the message does not say it. The
                    // CLI does not name "the board of the item": a
                    // repository and a board have no item.
                    let advice = if details["board_id"].is_null() {
                        format!(
                            "\nThe capability {capability:?} is not sufficient here. Ask an \
                             organization admin to do this."
                        )
                    } else {
                        format!(
                            "\nAsk an organization admin for the capability {capability:?} on \
                             the board."
                        )
                    };
                    if !says_who_to_ask(&message) {
                        text.push_str(&advice);
                    }
                } else if details["required"] == "deployment_admin" {
                    text.push_str(
                        "\nOnly a deployment admin can do this. Your OIDC subject must be in \
                         KAIROS_DEPLOYMENT_ADMINS of the server.",
                    );
                } else {
                    text.push_str(
                        "\nYou are logged in, but you are not a member of this organization. \
                         Ask an organization admin to add you as a member.",
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
                let mut text = format!("The request is in conflict (409 {code}): {message}");
                if let Some(version) = current["version"].as_i64() {
                    let title = current["title"].as_str().unwrap_or("?");
                    text.push_str(&format!(
                        "\nThe current version on the server is {version} (title: {title:?}). \
                         Read the item with `get`. Then run the edit again. The CLI makes the \
                         edit on the current version, unless you use --version."
                    ));
                }
                CliError::Failure(text)
            }
            ApiError::InvalidTransition {
                message,
                allowed_targets,
                ..
            } => {
                let mut text = format!("The transition is not correct (422): {message}");
                match allowed_targets.as_array() {
                    Some(targets) if !targets.is_empty() => {
                        text.push_str("\nThe item can move to these columns:");
                        for target in targets {
                            text.push_str(&format!(
                                "\n  - {} ({})",
                                target["name"].as_str().unwrap_or("?"),
                                target["id"].as_str().unwrap_or("?"),
                            ));
                        }
                    }
                    Some(_) => {
                        text.push_str("\nNo transition starts at the current column of the item.")
                    }
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
                "{status} LAST_ADMIN: {message}\nMake a different member an admin first."
            )),
            ApiError::Transport(err) => CliError::Failure(format!(
                "The CLI cannot connect to the deployment: {err}\n\
                 Make sure that the URL is correct and that the network connection works."
            )),
            other => CliError::Failure(other.to_string()),
        }
    }
}

/// Whether the message of a 403 tells the user who can help
/// (COLLIERY-T-0267): it has a sentence that starts with `Ask`. The server
/// writes such a sentence when it knows more than the capability, for
/// example the owner team of a repository.
fn says_who_to_ask(message: &str) -> bool {
    message.starts_with("Ask ") || message.contains(". Ask ")
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
        assert!(forbidden.to_string().contains("organization admin"));
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

    /// COLLIERY-T-0267: the CLI adds what to do next, and no more. It does
    /// not name "the board of the item".
    #[test]
    fn forbidden_adds_only_what_to_do_next() {
        let board = "0193a1c2-0000-7000-8000-000000000003";
        let err: CliError = ApiError::Forbidden {
            code: "FORBIDDEN".into(),
            message: format!(
                "This action requires the capability \"manage_tasks\" on the board {board}."
            ),
            capability: Some("manage_tasks".into()),
            details: serde_json::json!({"required_capability": "manage_tasks", "board_id": board}),
        }
        .into();
        assert_eq!(
            err.to_string(),
            format!(
                "The deployment refused the request (403): This action requires the \
                 capability \"manage_tasks\" on the board {board}.\nAsk an organization admin \
                 for the capability \"manage_tasks\" on the board."
            )
        );

        // No board: the action requires the organization admin role (the
        // delete of a repository).
        let err: CliError = ApiError::Forbidden {
            code: "FORBIDDEN".into(),
            message: "This action requires the organization admin role.".into(),
            capability: Some("manage_tasks".into()),
            details: serde_json::json!({"required_capability": "manage_tasks", "board_id": null}),
        }
        .into();
        assert_eq!(
            err.to_string(),
            "The deployment refused the request (403): This action requires the organization \
             admin role.\nThe capability \"manage_tasks\" is not sufficient here. Ask an \
             organization admin to do this."
        );
    }

    /// COLLIERY-T-0267: the refusal of `kairos repos create` and of `kairos
    /// repos update` says who can do the action (COLLIERY-T-0266). The CLI
    /// adds nothing to it.
    #[test]
    fn forbidden_with_advice_of_the_server_gets_no_more() {
        let message = "This action requires the capability \"manage_tasks\" on the delivery \
                       board of the team \"platform\", the owner team of the repository. You \
                       do not have that capability. Each member of the team has it, and an \
                       organization admin has each capability. Ask a member of the team \
                       \"platform\" or an organization admin to do this.";
        for board in [
            serde_json::json!("0193a1c2-0000-7000-8000-000000000003"),
            Value::Null,
        ] {
            let err: CliError = ApiError::Forbidden {
                code: "FORBIDDEN".into(),
                message: message.into(),
                capability: Some("manage_tasks".into()),
                details: serde_json::json!({"required_capability": "manage_tasks", "board_id": board}),
            }
            .into();
            let text = err.to_string();
            assert_eq!(
                text,
                format!("The deployment refused the request (403): {message}")
            );
            assert!(!text.contains("the board of the item"), "{text}");
        }
    }

    /// The deployment-admin gate's 403 points at KAIROS_DEPLOYMENT_ADMINS
    /// instead of the org-admin membership hint.
    #[test]
    fn forbidden_deployment_admin_hint() {
        let err: CliError = ApiError::Forbidden {
            code: "FORBIDDEN".into(),
            message: "This action requires a deployment admin.".into(),
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
            message: "The request has the version 1, and the current version is 4.".into(),
            current: serde_json::json!({"version": 4, "title": "Newer title"}),
            details: serde_json::json!({"current": {"version": 4}}),
        }
        .into();
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        let text = err.to_string();
        assert!(
            text.contains("current version on the server is 4"),
            "{text}"
        );
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
        assert!(
            text.contains("The item can move to these columns:"),
            "{text}"
        );
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
                .contains("No transition starts at the current column of the item"),
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
        assert!(
            text.contains("Make a different member an admin first"),
            "{text}"
        );
    }
}
