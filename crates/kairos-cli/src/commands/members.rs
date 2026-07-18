//! `kairos members` — organization membership (S-0005 `/api/members`,
//! org-admin gated; the last-admin guard surfaces as 422 `LAST_ADMIN`).

use kairos_client::types_org::{AddOrgMemberRequest, OrgMember, UpdateOrgMemberRequest};

use crate::commands::entities::{ListArgs, require_confirm};
use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// Organization membership (org-admin operations).
#[derive(clap::Subcommand, Debug)]
pub enum MembersCommand {
    /// List organization members (paginated)
    List(ListArgs),
    /// Add a member by email (the person must have logged in once so
    /// their account exists)
    Add {
        /// The user's email
        #[arg(long)]
        email: String,
        /// admin|member (defaults to member)
        #[arg(long)]
        role: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Change a member's role (demoting the last admin is rejected with
    /// LAST_ADMIN)
    SetRole {
        /// User id (UUID; see `kairos members list`)
        user_id: String,
        /// admin|member
        #[arg(long)]
        role: String,
        #[command(flatten)]
        common: Common,
    },
    /// Remove a member (requires --confirm; removing the last admin is
    /// rejected with LAST_ADMIN)
    Remove {
        /// User id (UUID)
        user_id: String,
        /// Actually remove the membership
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        common: Common,
    },
}

fn member_table(members: &[OrgMember]) -> Table {
    let mut table = Table::new(&["USER_ID", "EMAIL", "NAME", "ROLE", "JOINED"]);
    for member in members {
        table.row(vec![
            member.user_id.clone(),
            member.email.clone(),
            member.display_name.clone(),
            member.role.clone(),
            member.joined_at.clone(),
        ]);
    }
    table
}

impl MembersCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List(args) => {
                let client = client(&args.common)?;
                let envelope = client.list_org_members(args.page()).await?;
                if args.common.json {
                    return print_json(&envelope);
                }
                if envelope.items.is_empty() {
                    println!("(none)");
                } else {
                    print!("{}", member_table(&envelope.items).render());
                }
                println!(
                    "total: {} (limit {}, offset {})",
                    envelope.total, envelope.limit, envelope.offset
                );
                Ok(())
            }
            Self::Add {
                email,
                role,
                common,
            } => {
                let client = client(&common)?;
                let member = client
                    .add_org_member(&AddOrgMemberRequest { email, role })
                    .await?;
                if common.json {
                    return print_json(&member);
                }
                println!(
                    "Added {} ({}) as {} (user id {})",
                    member.display_name, member.email, member.role, member.user_id
                );
                Ok(())
            }
            Self::SetRole {
                user_id,
                role,
                common,
            } => {
                let client = client(&common)?;
                let member = client
                    .update_org_member(&user_id, &UpdateOrgMemberRequest { role })
                    .await?;
                if common.json {
                    return print_json(&member);
                }
                println!(
                    "{} ({}) is now {}",
                    member.display_name, member.email, member.role
                );
                Ok(())
            }
            Self::Remove {
                user_id,
                confirm,
                common,
            } => {
                require_confirm(confirm, &format!("membership of user {user_id}"))?;
                let client = client(&common)?;
                let response = client.remove_org_member(&user_id).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("Removed user {} from the organization", response.user_id);
                Ok(())
            }
        }
    }
}
