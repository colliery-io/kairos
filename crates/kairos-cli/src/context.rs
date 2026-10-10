//! Shared per-invocation context for the command tree (KAIROS-T-0037): the
//! `--url/--tenant/--json` flags every API-touching leaf command carries,
//! and the `KairosClient` builder over the T-0036 credential cache +
//! refreshing token provider.

use std::sync::Arc;

use serde::Serialize;

use crate::auth::{self, Source};
use crate::credentials;
use crate::error::CliError;
use crate::provider::CachedTokenProvider;
use kairos_client::KairosClient;

/// Flags shared by every command that talks to the API.
#[derive(clap::Args, Debug, Default)]
pub struct Common {
    /// Deployment base URL (defaults to KAIROS_URL, then the only cached deployment)
    #[arg(long)]
    pub url: Option<String>,
    /// Tenant slug override (defaults to the tenant cached at login)
    #[arg(long)]
    pub tenant: Option<String>,
    /// Print the raw JSON DTO instead of the human-readable rendering
    #[arg(long)]
    pub json: bool,
}

/// A `KairosClient` for the resolved deployment ([`auth::resolve`]): the
/// key in the environment, else (auto-refreshing) tokens from the
/// credential cache. Missing credentials are an auth error (exit 2) with
/// the `kairos login` instruction.
pub fn client(common: &Common) -> Result<KairosClient, CliError> {
    let path = credentials::credentials_path()?;
    let source = auth::resolve(common.url.as_deref(), &auth::Env::from_process(), || {
        credentials::load(&path)
    })?;
    let (deployment, store) = match source {
        Source::Key {
            deployment,
            var,
            key,
        } => {
            let mut client = auth::key_client(&deployment, var, key);
            if let Some(tenant) = common.tenant.clone() {
                client = client.with_tenant(tenant);
            }
            return Ok(client);
        }
        Source::Cache { deployment, store } => (deployment, store),
    };
    let entry = credentials::entry_for(&store, &deployment)?;

    let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
    let mut client = KairosClient::new(&deployment, provider);
    if let Some(tenant) = common.tenant.clone().or(entry.tenant) {
        client = client.with_tenant(tenant);
    }
    Ok(client)
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
