//! `kairos keys` — API keys for a service account (KAIROS-A-0017 /
//! KAIROS-T-0060). The minted key is shown ONCE; store it immediately.

use kairos_client::types_service_accounts::{ApiKey, CreateApiKeyRequest};

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// API keys for a service account (org-admin operations).
#[derive(clap::Subcommand, Debug)]
pub enum KeysCommand {
    /// Mint an API key for a service account (the raw key is shown ONCE)
    Create {
        /// Service account id (UUID)
        #[arg(long = "service-account")]
        service_account: String,
        /// Operator label for the key (e.g. "gha-main")
        #[arg(long)]
        name: String,
        /// Optional RFC 3339 expiry (e.g. 2027-01-01T00:00:00Z)
        #[arg(long = "expires-at")]
        expires_at: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// List a service account's keys (prefixes only; never the secret)
    List {
        /// Service account id (UUID)
        #[arg(long = "service-account")]
        service_account: String,
        #[command(flatten)]
        common: Common,
    },
    /// Revoke a key (requires --confirm)
    Revoke {
        /// Key id (UUID; see `kairos keys list`)
        key_id: String,
        /// The service account the key belongs to
        #[arg(long = "service-account")]
        service_account: String,
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
                let created = client
                    .create_api_key(&service_account, &CreateApiKeyRequest { name, expires_at })
                    .await?;
                if common.json {
                    return print_json(&created);
                }
                // The raw key is shown exactly once — make it unmissable.
                println!("API key minted for service account {service_account}.");
                println!();
                println!("    {}", created.key);
                println!();
                println!("Store it now — it will NOT be shown again.");
                Ok(())
            }
            Self::List {
                service_account,
                common,
            } => {
                let client = client(&common)?;
                let list = client.list_api_keys(&service_account).await?;
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
                require_confirm(confirm, &format!("API key {key_id}"))?;
                let client = client(&common)?;
                let deleted = client.revoke_api_key(&service_account, &key_id).await?;
                if common.json {
                    return print_json(&deleted);
                }
                println!("Revoked key {}.", deleted.id);
                Ok(())
            }
        }
    }
}
