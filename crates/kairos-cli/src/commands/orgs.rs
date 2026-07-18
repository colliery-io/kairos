//! `kairos orgs` — organization info for the authenticated user
//! (KAIROS-A-0015 lists `orgs` as a noun; S-0005 has no dedicated
//! `/api/orgs` endpoint, so this renders the `organization` object the
//! `/api/whoami` identity probe resolves — recorded as a client gap in
//! KAIROS-T-0037).

use crate::context::{Common, client, print_json};
use crate::error::CliError;

/// Organization info for your credentials.
#[derive(clap::Subcommand, Debug)]
pub enum OrgsCommand {
    /// Show the organization your credentials resolve to (id, slug, your
    /// role)
    Show {
        #[command(flatten)]
        common: Common,
    },
}

impl OrgsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::Show { common } => {
                let client = client(&common)?;
                let identity = client.whoami().await?;
                if common.json {
                    return print_json(&identity.organization);
                }
                println!("org:   {}", identity.organization.slug);
                println!("id:    {}", identity.organization.id);
                println!("role:  {}", identity.organization.role);
                Ok(())
            }
        }
    }
}
