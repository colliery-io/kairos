//! `kairos service-accounts` — machine principals authenticated by API keys
//! (KAIROS-A-0017 / KAIROS-T-0060). Org-admin operations; thin veneers over
//! `kairos-client`.

use kairos_client::types_service_accounts::{CreateServiceAccountRequest, ServiceAccount};

use crate::commands::entities::require_confirm;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Service accounts (org-admin operations).
#[derive(clap::Subcommand, Debug)]
pub enum ServiceAccountsCommand {
    /// Create a service account (a machine principal; grant it board
    /// capabilities with `kairos boards ...`, then mint a key with
    /// `kairos keys create`)
    Create {
        /// Operator label (e.g. "ci-deploy")
        #[arg(long)]
        name: String,
        #[command(flatten)]
        common: Common,
    },
    /// List the organization's service accounts
    List {
        #[command(flatten)]
        common: Common,
    },
    /// Delete a service account and all its keys (requires --confirm)
    Delete {
        /// Service account id (UUID; see `kairos service-accounts list`)
        id: String,
        /// Actually delete it
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
}

fn account_table(accounts: &[ServiceAccount]) -> Table {
    let mut table = Table::new(&["ID", "NAME", "CREATED"]);
    for a in accounts {
        table.row(vec![a.id.clone(), a.name.clone(), a.created_at.clone()]);
    }
    table
}

impl ServiceAccountsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Create { name, common } => {
                let client = client(&common)?;
                let sa = client
                    .create_service_account(&CreateServiceAccountRequest { name })
                    .await?;
                if common.json {
                    return print_json(&sa);
                }
                println!("Created service account {} ({}).", sa.name, sa.id);
                println!(
                    "Mint a key with: kairos keys create --service-account {}",
                    sa.id
                );
                Ok(())
            }
            Self::List { common } => {
                let client = client(&common)?;
                let list = client.list_service_accounts().await?;
                if common.json {
                    return print_json(&list);
                }
                if list.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", account_table(&list.items).render());
                }
                println!("total: {}", list.total);
                Ok(())
            }
            Self::Delete {
                id,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("service account {id} and all its keys"))?;
                let client = client(&common)?;
                let deleted = client.delete_service_account(&id).await?;
                if common.json {
                    return print_json(&deleted);
                }
                println!("Deleted service account {}.", deleted.id);
                Ok(())
            }
        }
    }
}
