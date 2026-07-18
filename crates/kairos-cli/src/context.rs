//! Shared per-invocation context for the command tree (KAIROS-T-0037): the
//! `--url/--tenant/--json` flags every API-touching leaf command carries,
//! and the `KairosClient` builder over the T-0036 credential cache +
//! refreshing token provider.

use std::sync::Arc;

use serde::Serialize;

use crate::credentials;
use crate::error::CliError;
use crate::provider::CachedTokenProvider;
use kairos_client::KairosClient;

/// Flags shared by every command that talks to the API.
#[derive(clap::Args, Debug, Default)]
pub struct Common {
    /// Deployment base URL (defaults to the only cached deployment)
    #[arg(long)]
    pub url: Option<String>,
    /// Tenant slug override (defaults to the tenant cached at login)
    #[arg(long)]
    pub tenant: Option<String>,
    /// Print the raw JSON DTO instead of the human-readable rendering
    #[arg(long)]
    pub json: bool,
}

/// A `KairosClient` for the resolved deployment, drawing (auto-refreshing)
/// tokens from the credential cache. Missing credentials are an auth error
/// (exit 2) with the `kairos login` instruction.
pub fn client(common: &Common) -> Result<KairosClient, CliError> {
    let path = credentials::credentials_path()?;
    let store = credentials::load(&path)?;
    let deployment = credentials::resolve_deployment(&store, common.url.as_deref())?;
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
            .map_err(|err| CliError::Failure(format!("cannot render JSON: {err}")))?
    );
    Ok(())
}
