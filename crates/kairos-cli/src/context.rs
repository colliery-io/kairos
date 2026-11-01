//! Shared per-invocation context for the command tree (KAIROS-T-0037): the
//! `--url/--tenant/--json` flags every API-touching leaf command carries,
//! and the `KairosClient` builder over the T-0036 credential cache +
//! refreshing token provider.

use std::sync::Arc;

use serde::Serialize;

use crate::auth::{self, Source};
use crate::credentials;
use crate::error::CliError;
use crate::project_config;
use crate::provider::CachedTokenProvider;
use kairos_client::KairosClient;

/// Flags shared by every command that talks to the API.
#[derive(clap::Args, Debug, Default)]
pub struct Common {
    /// Deployment base URL (defaults to KAIROS_URL, then the project config, then the only cached deployment)
    #[arg(long)]
    pub url: Option<String>,
    /// Tenant slug override (defaults to the project config, then the tenant cached at login)
    #[arg(long)]
    pub tenant: Option<String>,
    /// Print JSON instead of the human-readable rendering. A list prints
    /// items, total and next_offset
    #[arg(long)]
    pub json: bool,
}

/// A `KairosClient` for the resolved deployment ([`auth::resolve`]): the
/// key in the environment, else (auto-refreshing) tokens from the
/// credential cache. Missing credentials are an auth error (exit 2) with
/// the `kairos login` instruction. `KAIROS_TIMEOUT` sets the request
/// timeout (KAIROS-T-0607); a value that is not a positive integer is a
/// failure (exit 1) that names the variable.
pub fn client(common: &Common) -> Result<KairosClient, CliError> {
    client_with_credential(common).map(|(client, _)| client)
}

/// [`client`], and the `credential:` line of `kairos whoami`
/// (KAIROS-T-0594). A key that hides a login for the same deployment gives
/// one warning line on stderr, unless `KAIROS_QUIET=1`. A `config.toml`
/// that does not parse is refused (KAIROS-T-0706).
pub fn client_with_credential(common: &Common) -> Result<(KairosClient, String), CliError> {
    client_for(common, &project_config::binding_here()?)
}

/// [`client_with_credential`] for the binding of a folder (#270). The
/// deployment: `--url`, then `KAIROS_URL`, then the binding (`[[project]]`,
/// `[default]`), then the only deployment of a login. The tenant of a login: `--tenant`, then the binding, then the
/// tenant of the login. A key carries its tenant: only `--tenant` changes it.
pub fn client_for(
    common: &Common,
    binding: &project_config::Binding,
) -> Result<(KairosClient, String), CliError> {
    let timeout = kairos_client::timeout_from_env()?;
    let path = credentials::credentials_path()?;
    let env = auth::Env::from_process();
    let url = common.url.clone().or_else(|| {
        env.url
            .is_none()
            .then(|| binding.get("deployment").map(str::to_string))
            .flatten()
    });
    let source = auth::resolve(url.as_deref(), &env, || credentials::load(&path))?;
    if let Some(warning) = auth::override_warning(&source, &env, || credentials::load(&path)) {
        eprintln!("{warning}");
    }
    let credential = source.credential_line();
    let (mut client, cached_tenant) = match source {
        Source::Key {
            deployment,
            var,
            key,
        } => {
            auth::note_deployment(&deployment);
            (auth::key_client(&deployment, var, key), None)
        }
        Source::Cache { deployment, store } => {
            let entry = credentials::entry_for(&store, &deployment)?;
            auth::note_deployment(&deployment);
            let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
            // The tenant of the binding is for its own deployment: not for
            // a `--url` or a KAIROS_URL of another one.
            let same = binding.get("deployment").is_none_or(|bound| {
                credentials::normalize_url(bound) == credentials::normalize_url(&deployment)
            });
            let bound = binding.get("tenant").filter(|_| same).map(str::to_string);
            let tenant = bound.or(entry.tenant);
            (KairosClient::new(&deployment, provider), tenant)
        }
    };
    if let Some(tenant) = common.tenant.clone().or(cached_tenant) {
        client = client.with_tenant(tenant);
    }
    if let Some(timeout) = timeout {
        client = client.with_timeout(timeout);
    }
    Ok((client, credential))
}

/// Pretty-print a DTO as JSON (the `--json` scripting surface).
pub fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|err| CliError::Failure(format!("The CLI cannot write the JSON: {err}.")))?
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// References: a name or a slug, or an id (KAIROS-T-0589)
// ---------------------------------------------------------------------------
//
// Each verb that names a column, a team, a template, a service account or a
// stream takes a name or a slug, or the id (UUID). A route of the server that
// takes a slug gets the reference as it is (boards, teams, templates, the
// members of the organization). Else the CLI finds the id with one lookup
// below. A reference with the form of a UUID is an id, as on the server.

/// True when `value` has the form of a UUID. The CLI then sends it as an id.
pub fn is_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The column of `detail` with the id or the name `column` (the case of the
/// name does not count). An unknown column is an error that names each
/// column of the board.
pub fn find_column<'a>(
    detail: &'a kairos_client::types_org::BoardDetail,
    column: &str,
) -> Result<&'a kairos_client::types_org::BoardColumn, CliError> {
    detail
        .columns
        .iter()
        .find(|c| c.id == column || c.name.eq_ignore_ascii_case(column))
        .ok_or_else(|| {
            CliError::Failure(format!(
                "The board {} has no column {column:?}. The columns are: {}.",
                detail.board.slug,
                detail
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

/// The id of the column `column` (a name or an id) of the board `board` (a
/// slug or an id). One call: the board with its columns.
pub async fn column_id(
    client: &KairosClient,
    board: &str,
    column: &str,
) -> Result<String, CliError> {
    let detail = client.get_board(board).await?;
    Ok(find_column(&detail, column)?.id.clone())
}

/// The id of the team `team` (a slug or an id). The server finds a team by
/// its slug, so a slug costs one call.
pub async fn team_id(client: &KairosClient, team: &str) -> Result<String, CliError> {
    if is_uuid(team) {
        return Ok(team.to_string());
    }
    Ok(client.get_team(team).await?.id)
}

/// The id of the template `template` (a slug or an id). The server finds a
/// template by its slug (KAIROS-T-0644), so a slug costs one call.
pub async fn template_id(client: &KairosClient, template: &str) -> Result<String, CliError> {
    if is_uuid(template) {
        return Ok(template.to_string());
    }
    Ok(client.get_template(template).await?.id)
}

/// The id of the service account `account` (a name or an id). One call: the
/// list of the service accounts.
pub async fn service_account_id(client: &KairosClient, account: &str) -> Result<String, CliError> {
    if is_uuid(account) {
        return Ok(account.to_string());
    }
    let list = client.list_service_accounts().await?;
    let named: Vec<(&str, &str)> = list
        .items
        .iter()
        .map(|a| (a.id.as_str(), a.name.as_str()))
        .collect();
    pick_by_name("service account", account, &named)
}

/// The id of the delivery stream `stream` (a slug, a name or an id). The
/// CLI reads the pages of the streams: an organization has few.
pub async fn stream_id(client: &KairosClient, stream: &str) -> Result<String, CliError> {
    if is_uuid(stream) {
        return Ok(stream.to_string());
    }
    let paging = crate::listing::Paging {
        all: true,
        ..Default::default()
    };
    let streams = crate::listing::fetch(paging, |page| client.list_streams(page)).await?;
    if let Some(found) = streams.items.iter().find(|s| s.slug == stream) {
        return Ok(found.id.clone());
    }
    let named: Vec<(&str, &str)> = streams
        .items
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    pick_by_name("delivery stream", stream, &named)
}

/// The id of the one row of `rows` (id, name) with the name `name`. No row,
/// or two rows, is an error that says what to send. Pure.
pub fn pick_by_name(noun: &str, name: &str, rows: &[(&str, &str)]) -> Result<String, CliError> {
    let found: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| *row == name)
        .map(|(id, _)| *id)
        .collect();
    match found.as_slice() {
        [id] => Ok((*id).to_string()),
        [] => Err(CliError::Failure(format!(
            "No {noun} has the name {name:?}. Send its name or its id."
        ))),
        ids => Err(CliError::Failure(format!(
            "More than one {noun} has the name {name:?}. Send the id: {}.",
            ids.join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_client::types_org::BoardDetail;

    #[test]
    fn a_uuid_is_an_id_and_each_other_reference_is_a_name() {
        assert!(is_uuid("6f1c2a3b-4d5e-6f70-8192-a3b4c5d6e7f8"));
        assert!(!is_uuid("platform"));
        assert!(!is_uuid("Todo"));
        assert!(!is_uuid("6f1c2a3b-4d5e-6f70-8192-a3b4c5d6e7fz"));
        assert!(!is_uuid("6f1c2a3b4d5e6f708192a3b4c5d6e7f8"));
    }

    fn detail() -> BoardDetail {
        let column = |id: &str, name: &str, position: i32| {
            serde_json::json!({
                "id": id, "board_id": "b-1", "name": name, "position": position,
                "created_at": "2026-10-18T00:00:00Z", "updated_at": "2026-10-18T00:00:00Z"
            })
        };
        serde_json::from_value(serde_json::json!({
            "id": "b-1", "name": "Platform", "slug": "platform-delivery",
            "board_level": "delivery", "team_id": null, "code_prefix": "PLAT",
            "created_at": "2026-10-18T00:00:00Z", "updated_at": "2026-10-18T00:00:00Z",
            "columns": [column("c-1", "Backlog", 0), column("c-2", "In Review", 1)],
            "transitions": []
        }))
        .expect("a board detail")
    }

    #[test]
    fn a_column_is_found_by_its_name_or_its_id() {
        let detail = detail();
        assert_eq!(find_column(&detail, "Backlog").expect("by name").id, "c-1");
        assert_eq!(
            find_column(&detail, "in review").expect("no case").id,
            "c-2"
        );
        assert_eq!(
            find_column(&detail, "c-2").expect("by id").name,
            "In Review"
        );
        let err = find_column(&detail, "Done").expect_err("unknown");
        let text = err.to_string();
        assert!(text.contains("\"Done\""), "{text}");
        assert!(text.contains("Backlog, In Review"), "{text}");
    }

    #[test]
    fn a_name_picks_one_row_or_says_what_to_send() {
        let rows = [
            ("id-1", "ci-deploy"),
            ("id-2", "nightly"),
            ("id-3", "nightly"),
        ];
        assert_eq!(
            pick_by_name("service account", "ci-deploy", &rows).expect("one row"),
            "id-1"
        );
        let none = pick_by_name("service account", "gone", &rows).expect_err("no row");
        assert!(none.to_string().contains("No service account"), "{none}");
        let two = pick_by_name("service account", "nightly", &rows).expect_err("two rows");
        assert!(two.to_string().contains("id-2, id-3"), "{two}");
    }
}
