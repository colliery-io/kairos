//! `kairos admin tenants` — deployment-admin tenant provisioning (S-0005
//! `/api/admin/tenants`; the caller's OIDC `sub` must be listed in the
//! server's `KAIROS_DEPLOYMENT_ADMINS`, otherwise 403 with that guidance).

use kairos_client::types_org::CreateTenantRequest;

use crate::commands::entities::{ListArgs, require_confirm};
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Deployment-administration operations.
#[derive(clap::Subcommand, Debug)]
pub enum AdminCommand {
    /// Tenant provisioning (deployment-admin only, cross-tenant)
    #[command(subcommand)]
    Tenants(TenantsCommand),
}

/// Provision, list, and drop tenants.
#[derive(clap::Subcommand, Debug)]
pub enum TenantsCommand {
    /// List provisioned tenants (paginated)
    List(ListArgs),
    /// Provision a tenant: organization + schema + default boards
    Create {
        /// Organization slug (^[a-z][a-z0-9_-]{1,62}$)
        #[arg(long)]
        slug: String,
        /// Organization display name
        #[arg(long)]
        name: String,
        /// OIDC sub of the initial org admin (defaults to you; the user
        /// must have logged in once)
        #[arg(long, value_name = "OIDC_SUB")]
        initial_admin: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Drop a tenant — destructive and unrecoverable (requires --confirm)
    Delete {
        /// The tenant's slug
        slug: String,
        /// Actually drop the tenant's organization and schema
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
}

impl AdminCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Tenants(command) => command.run().await,
        }
    }
}

impl TenantsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List(args) => {
                let client = client(&args.common)?;
                let envelope = client.list_tenants(args.page()).await?;
                if args.common.json {
                    return print_json(&envelope);
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    let mut table = Table::new(&["SLUG", "NAME", "SCHEMA"]);
                    for tenant in &envelope.items {
                        table.row(vec![
                            tenant.slug.clone(),
                            tenant.name.clone(),
                            if tenant.schema_exists {
                                "ok".to_string()
                            } else {
                                "MISSING".to_string()
                            },
                        ]);
                    }
                    print!("{}", table.render());
                }
                println!(
                    "total: {} (limit {}, offset {})",
                    envelope.total, envelope.limit, envelope.offset
                );
                Ok(())
            }
            Self::Create {
                slug,
                name,
                initial_admin,
                common,
            } => {
                let client = client(&common)?;
                let report = client
                    .create_tenant(&CreateTenantRequest {
                        slug,
                        name,
                        initial_admin_external_id: initial_admin,
                    })
                    .await?;
                if common.json {
                    return print_json(&report);
                }
                println!(
                    "Provisioned tenant {} (schema {})",
                    report.slug, report.schema
                );
                println!("  boards created: {}", report.boards_created.join(", "));
                println!(
                    "  templates copied: {}, metadata definitions copied: {}",
                    report.templates_copied, report.metadata_definitions_copied
                );
                println!(
                    "  initial admin: {} ({})",
                    report.initial_admin.email, report.initial_admin.external_id
                );
                Ok(())
            }
            Self::Delete {
                slug,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("tenant {slug}"))?;
                let client = client(&common)?;
                let response = client.delete_tenant(&slug, Some(true)).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Dropped tenant {}", response.slug);
                Ok(())
            }
        }
    }
}
