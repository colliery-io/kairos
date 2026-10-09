//! `kairos keys` — API keys. Without `--service-account`: your own agent keys
//! (KAIROS-T-0359, KAIROS-A-0024). An agent key acts as you, with your
//! capabilities. With `--service-account`: the keys of a service account
//! (KAIROS-A-0017 / KAIROS-T-0060, org-admin). The made key is shown ONCE;
//! store it immediately.

use kairos_client::types_service_accounts::{ApiKey, CreateApiKeyRequest};

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Your agent keys, or the API keys of a service account (org-admin).
#[derive(clap::Subcommand, Debug)]
pub enum KeysCommand {
    /// Make an agent key that acts as you, or a key for a service account (the
    /// raw key is shown ONCE)
    Create {
        /// Service account id (UUID). Omit it to make your own agent key.
        #[arg(long = "service-account")]
        service_account: Option<String>,
        /// Label for the key (e.g. "laptop-claude-code" or "gha-main")
        #[arg(long)]
        name: String,
        /// Optional RFC 3339 expiry (e.g. 2027-01-01T00:00:00Z)
        #[arg(long = "expires-at")]
        expires_at: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// List your agent keys, or the keys of a service account (prefixes only;
    /// never the secret)
    List {
        /// Service account id (UUID). Omit it to list your own agent keys.
        #[arg(long = "service-account")]
        service_account: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Revoke a key (requires --confirm)
    Revoke {
        /// Key id (UUID; see `kairos keys list`)
        key_id: String,
        /// The service account of the key. Omit it to revoke your own agent key.
        #[arg(long = "service-account")]
        service_account: Option<String>,
        /// Actually revoke it
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
}

fn key_table(keys: &[ApiKey]) -> Table {
    let mut table = Table::new(&["ID", "NAME", "PREFIX", "EXPIRES", "LAST_USED", "REVOKED"]);
    for k in keys {
        table.row(vec![
            k.id.clone(),
            k.name.clone(),
            k.prefix.clone(),
            k.expires_at.clone().unwrap_or_else(|| "-".to_string()),
            k.last_used_at.clone().unwrap_or_else(|| "-".to_string()),
            k.revoked_at.clone().unwrap_or_else(|| "-".to_string()),
        ]);
    }
    table
}

impl KeysCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Create {
                service_account,
                name,
                expires_at,
                common,
            } => {
                let client = client(&common)?;
                let request = CreateApiKeyRequest { name, expires_at };
                let created = match &service_account {
                    Some(sa) => client.create_api_key(sa, &request).await?,
                    None => client.create_agent_key(&request).await?,
                };
                if common.json {
                    return print_json(&created);
                }
                // The raw key is shown exactly once — make it unmissable.
                match &service_account {
                    Some(sa) => println!("Kairos made an API key for the service account {sa}."),
                    None => println!("Kairos made an agent key. The key acts as you."),
                }
                println!();
                println!("    {}", created.key);
                println!();
                println!("Keep the key in a safe place now. Kairos does NOT show it again.");
                if service_account.is_none() {
                    println!(
                        "Put the key in the settings of your agent. For Claude Code, set the \
                         environment variable KAIROS_MCP_KEY. Do not put the key in a repository."
                    );
                }
                Ok(())
            }
            Self::List {
                service_account,
                common,
            } => {
                let client = client(&common)?;
                let list = match &service_account {
                    Some(sa) => client.list_api_keys(sa).await?,
                    None => client.list_agent_keys().await?,
                };
                if common.json {
                    return print_json(&list);
                }
                if list.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", key_table(&list.items).render());
                }
                println!("total: {}", list.total);
                Ok(())
            }
            Self::Revoke {
                key_id,
                service_account,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("the API key {key_id}"))?;
                let client = client(&common)?;
                let deleted = match &service_account {
                    Some(sa) => client.revoke_api_key(sa, &key_id).await?,
                    None => client.revoke_agent_key(&key_id).await?,
                };
                if common.json {
                    return print_json(&deleted);
                }
                println!("Kairos revoked the key {}.", deleted.id);
                Ok(())
            }
        }
    }
}
