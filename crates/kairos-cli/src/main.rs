//! `kairos-cli` — the `kairos` command-line interface (clap over
//! `kairos-client`), per KAIROS-A-0015.
//!
//! The auth + configuration layer (KAIROS-T-0036):
//!
//! - `kairos login --url <deployment>` — Device Authorization Grant
//!   (KAIROS-A-0010) against the deployment's OIDC issuer, discovered from
//!   its RFC 9728 protected-resource metadata (or `--issuer`); tokens
//!   cached in `~/.config/kairos/credentials.json` (0600), keyed by
//!   deployment URL.
//! - `kairos whoami [--json]` — `GET /api/whoami` through `KairosClient`
//!   with a refreshing [`provider::CachedTokenProvider`].
//! - `kairos logout [--url]` — clears the cached entry.
//!
//! The command tree (KAIROS-T-0037), nouns mirroring the S-0005 API:
//! `orgs`, `boards` (list/show grouped by column), `strategies`,
//! `initiatives`, `tasks`, `documents`, `adrs` (each
//! list/get/create/edit/transition/delete; deletes require `--confirm`),
//! `search` (filters + traverse via flags, or `--query-json`), `teams`
//! (+members), `streams` (+teams), `members`, and `admin tenants`. Human
//! tables by default, `--json` everywhere.
//!
//! Exit codes (KAIROS-A-0015): 0 success · 1 API/validation error ·
//! 2 auth error.

mod commands;
mod context;
mod credentials;
mod error;
mod oidc;
mod provider;
mod table;

use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use commands::admin::AdminCommand;
use commands::boards::BoardsCommand;
use commands::entities::{
    AdrsCommand, DocumentsCommand, InitiativesCommand, StrategiesCommand, TasksCommand,
};
use commands::keys::KeysCommand;
use commands::members::MembersCommand;
use commands::orgs::OrgsCommand;
use commands::search::SearchArgs;
use commands::service_accounts::ServiceAccountsCommand;
use commands::streams::StreamsCommand;
use commands::teams::TeamsCommand;
use credentials::{CredentialStore, DeploymentCredentials, unix_now};
use error::CliError;
use kairos_client::KairosClient;
use kairos_client::types_org::WhoamiResponse;
use provider::CachedTokenProvider;

#[derive(Parser)]
#[command(
    name = "kairos",
    version,
    about = "The Kairos command-line interface",
    long_about = "The Kairos command-line interface (KAIROS-A-0015).\n\
                  Exit codes: 0 success, 1 API/validation error, 2 auth error."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in to a Kairos deployment via the OAuth Device Authorization Grant
    Login {
        /// Deployment base URL (e.g. https://kairos.example.com)
        #[arg(long)]
        url: String,
        /// OIDC issuer override (skips RFC 9728 discovery against the deployment)
        #[arg(long)]
        issuer: Option<String>,
        /// Tenant slug, sent as X-Tenant on API calls (dev/test tenant resolution)
        #[arg(long)]
        tenant: Option<String>,
        /// OAuth client id registered for the CLI at the issuer
        #[arg(long, default_value = "kairos-cli")]
        client_id: String,
        /// Which token to cache and send as the API bearer. Use `id_token`
        /// for issuers with opaque access tokens, e.g. Google Workspace
        /// (KAIROS-T-0054); `access_token` (default) for Dex/Keycloak.
        #[arg(long, value_enum, default_value = "access_token")]
        bearer: oidc::ApiBearer,
    },
    /// Forget the cached credentials for a deployment
    Logout {
        /// Deployment base URL (defaults to the only cached deployment)
        #[arg(long)]
        url: Option<String>,
    },
    /// Show the authenticated user, organization, role, and teams
    Whoami {
        /// Deployment base URL (defaults to the only cached deployment)
        #[arg(long)]
        url: Option<String>,
        /// Tenant slug override (defaults to the tenant cached at login)
        #[arg(long)]
        tenant: Option<String>,
        /// Print the raw /api/whoami JSON
        #[arg(long)]
        json: bool,
    },
    /// Organization info for your credentials
    #[command(subcommand)]
    Orgs(OrgsCommand),
    /// Boards: list them, or show one's items grouped by column
    #[command(subcommand)]
    Boards(BoardsCommand),
    /// Strategies (Flight Level 3)
    #[command(subcommand)]
    Strategies(StrategiesCommand),
    /// Initiatives (Flight Level 2)
    #[command(subcommand)]
    Initiatives(InitiativesCommand),
    /// Tasks, bugs, and tech debt (Flight Level 1)
    #[command(subcommand)]
    Tasks(TasksCommand),
    /// Supporting documents (attached to a workflow item)
    #[command(subcommand)]
    Documents(DocumentsCommand),
    /// Architecture Decision Records
    #[command(subcommand)]
    Adrs(AdrsCommand),
    /// Search and traverse all entity types (POST /api/search)
    Search(Box<SearchArgs>),
    /// Teams and their membership
    #[command(subcommand)]
    Teams(TeamsCommand),
    /// Delivery streams and their teams
    #[command(subcommand)]
    Streams(StreamsCommand),
    /// Organization membership (org-admin operations)
    #[command(subcommand)]
    Members(MembersCommand),
    /// Service accounts: machine principals authenticated by API keys
    #[command(subcommand)]
    ServiceAccounts(ServiceAccountsCommand),
    /// API keys for a service account
    #[command(subcommand)]
    Keys(KeysCommand),
    /// Deployment administration (tenant provisioning)
    #[command(subcommand)]
    Admin(AdminCommand),
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

async fn run(command: Command) -> Result<(), CliError> {
    match command {
        Command::Login {
            url,
            issuer,
            tenant,
            client_id,
            bearer,
        } => login(&url, issuer.as_deref(), tenant, client_id, bearer).await,
        Command::Logout { url } => logout(url.as_deref()),
        Command::Whoami { url, tenant, json } => whoami(url.as_deref(), tenant, json).await,
        Command::Orgs(command) => command.run().await,
        Command::Boards(command) => command.run().await,
        Command::Strategies(command) => command.run().await,
        Command::Initiatives(command) => command.run().await,
        Command::Tasks(command) => command.run().await,
        Command::Documents(command) => command.run().await,
        Command::Adrs(command) => command.run().await,
        Command::Search(args) => (*args).run().await,
        Command::Teams(command) => command.run().await,
        Command::Streams(command) => command.run().await,
        Command::Members(command) => command.run().await,
        Command::ServiceAccounts(command) => command.run().await,
        Command::Keys(command) => command.run().await,
        Command::Admin(command) => command.run().await,
    }
}

/// `kairos login` — discover the issuer, run the device grant, cache the
/// tokens (KAIROS-A-0010, KAIROS-A-0015).
async fn login(
    url: &str,
    issuer_override: Option<&str>,
    tenant: Option<String>,
    client_id: String,
    api_bearer: oidc::ApiBearer,
) -> Result<(), CliError> {
    let deployment = credentials::normalize_url(url);
    if deployment.is_empty() {
        return Err(CliError::Failure("--url must not be empty".to_string()));
    }
    let http = reqwest::Client::new();

    let issuer = match issuer_override {
        Some(issuer) => {
            let issuer = issuer.trim_end_matches('/').to_string();
            println!("Using issuer override: {issuer}");
            issuer
        }
        None => {
            let issuer = oidc::discover_issuer(&http, &deployment).await?;
            println!("Discovered OIDC issuer: {issuer}");
            issuer
        }
    };

    let endpoints = oidc::discover_endpoints(&http, &issuer).await?;
    let grant = oidc::start_device_grant(&http, &endpoints, &client_id).await?;

    println!();
    match &grant.verification_uri_complete {
        Some(complete) => {
            println!("To sign in, open: {complete}");
            println!(
                "(or visit {} and enter code: {})",
                grant.verification_uri, grant.user_code
            );
        }
        None => {
            println!("To sign in, visit: {}", grant.verification_uri);
            println!("and enter code: {}", grant.user_code);
        }
    }
    println!();
    println!(
        "Waiting for approval (polling every {}s; the code expires in {}s)...",
        grant.interval.unwrap_or(5).max(1),
        grant.expires_in
    );

    let token = oidc::poll_device_grant(&http, &endpoints, &client_id, &grant).await?;
    if token.refresh_token.is_none() {
        eprintln!(
            "warning: the issuer did not return a refresh token; \
             you will need to log in again when the access token expires"
        );
    }

    // Select the bearer this deployment expects (T-0054). `id_token` requires
    // the issuer to have returned one (the `openid` scope is always requested).
    let bearer = token
        .bearer_for(api_bearer)
        .ok_or_else(|| {
            CliError::Auth(
                "--bearer id_token was requested but the issuer returned no id_token.\n\
                 Ensure the OAuth client requests the `openid` scope, or use \
                 `--bearer access_token`."
                    .to_string(),
            )
        })?
        .to_string();

    let path = credentials::credentials_path()?;
    let mut store = load_store_for_login(&path);
    store.deployments.insert(
        deployment.clone(),
        DeploymentCredentials {
            access_token: bearer,
            refresh_token: token.refresh_token,
            expires_at: unix_now() + token.expires_in,
            issuer,
            client_id,
            tenant,
            api_bearer,
        },
    );
    credentials::save(&path, &store)?;

    println!();
    println!("Logged in to {deployment}.");
    println!("Credentials cached in {} (mode 0600).", path.display());
    Ok(())
}

/// The store to merge a fresh login into: a corrupted cache is NOT fatal
/// here — login is the documented fix, so start over (with a note).
fn load_store_for_login(path: &std::path::Path) -> CredentialStore {
    match credentials::load(path) {
        Ok(store) => store,
        Err(_) => {
            eprintln!(
                "warning: the credential cache at {} was unreadable and has been reset",
                path.display()
            );
            CredentialStore::default()
        }
    }
}

/// `kairos logout` — drop the deployment's entry from the cache.
fn logout(url: Option<&str>) -> Result<(), CliError> {
    let path = credentials::credentials_path()?;
    let mut store = credentials::load(&path)?;
    if store.deployments.is_empty() {
        println!("No cached credentials; nothing to do.");
        return Ok(());
    }
    let deployment = credentials::resolve_deployment(&store, url)?;
    match store.deployments.remove(&deployment) {
        Some(_) => {
            credentials::save(&path, &store)?;
            println!("Logged out of {deployment}.");
            Ok(())
        }
        None => Err(CliError::Failure(format!(
            "no cached credentials for {deployment} (cached: {})",
            store
                .deployments
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// `kairos whoami` — the identity probe via `kairos-client`.
async fn whoami(
    url: Option<&str>,
    tenant_override: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let path = credentials::credentials_path()?;
    let store = credentials::load(&path)?;
    let deployment = credentials::resolve_deployment(&store, url)?;
    let entry = credentials::entry_for(&store, &deployment)?;

    let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
    let mut client = KairosClient::new(&deployment, provider);
    if let Some(tenant) = tenant_override.or(entry.tenant) {
        client = client.with_tenant(tenant);
    }

    let identity = client.whoami().await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&identity)
                .map_err(|err| CliError::Failure(format!("cannot render JSON: {err}")))?
        );
    } else {
        print_identity(&identity);
    }
    Ok(())
}

/// The human-readable `whoami` rendering: user, org, role, teams
/// (KAIROS-A-0015).
fn print_identity(identity: &WhoamiResponse) {
    println!(
        "user:   {} <{}>",
        identity.user.display_name, identity.user.email
    );
    println!(
        "org:    {} (role: {})",
        identity.organization.slug, identity.organization.role
    );
    if identity.teams.is_empty() {
        println!("teams:  (none)");
    } else {
        let teams: Vec<&str> = identity.teams.iter().map(|t| t.name.as_str()).collect();
        println!("teams:  {}", teams.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(env!("CARGO_PKG_NAME"), "kairos-cli");
    }

    /// The clap surface parses per KAIROS-A-0015: login/logout/whoami with
    /// their documented flags.
    #[test]
    fn cli_parses() {
        let cli = Cli::try_parse_from([
            "kairos",
            "login",
            "--url",
            "http://localhost:8080",
            "--tenant",
            "acme",
        ])
        .expect("login parses");
        match cli.command {
            Command::Login {
                url,
                issuer,
                tenant,
                client_id,
                bearer,
            } => {
                assert_eq!(url, "http://localhost:8080");
                assert_eq!(issuer, None);
                assert_eq!(tenant.as_deref(), Some("acme"));
                assert_eq!(client_id, "kairos-cli");
                // Default bearer is the access token (T-0054).
                assert_eq!(bearer, oidc::ApiBearer::AccessToken);
            }
            _ => panic!("expected login"),
        }

        // `--bearer id_token` parses for Google-Workspace-style deployments.
        let cli = Cli::try_parse_from([
            "kairos",
            "login",
            "--url",
            "http://localhost:8080",
            "--bearer",
            "id_token",
        ])
        .expect("login --bearer id_token parses");
        match cli.command {
            Command::Login { bearer, .. } => {
                assert_eq!(bearer, oidc::ApiBearer::IdToken);
            }
            _ => panic!("expected login"),
        }

        let cli = Cli::try_parse_from(["kairos", "whoami", "--json"]).expect("whoami parses");
        match cli.command {
            Command::Whoami { url, json, .. } => {
                assert_eq!(url, None);
                assert!(json);
            }
            _ => panic!("expected whoami"),
        }

        let cli = Cli::try_parse_from(["kairos", "logout"]).expect("logout parses");
        assert!(matches!(cli.command, Command::Logout { url: None }));

        assert!(
            Cli::try_parse_from(["kairos", "login"]).is_err(),
            "--url is required"
        );
    }

    /// The KAIROS-T-0037 command tree parses: every A-0015 noun with its
    /// verbs and documented flags.
    #[test]
    fn command_tree_parses() {
        // Entity families: list/get/create/edit/transition/delete.
        let cli = Cli::try_parse_from([
            "kairos",
            "tasks",
            "create",
            "--board",
            "b-1",
            "--title",
            "Fix login",
            "--type",
            "bug",
            "--content",
            "body",
        ])
        .expect("tasks create parses");
        match cli.command {
            Command::Tasks(TasksCommand::Create(args)) => {
                assert_eq!(args.board, "b-1");
                assert_eq!(args.title, "Fix login");
                assert_eq!(args.task_type.as_deref(), Some("bug"));
                assert_eq!(args.content, "body");
            }
            _ => panic!("expected tasks create"),
        }

        let cli = Cli::try_parse_from([
            "kairos",
            "initiatives",
            "edit",
            "ACME-I-0001",
            "--title",
            "New",
            "--version",
            "3",
            "--json",
        ])
        .expect("initiatives edit parses");
        match cli.command {
            Command::Initiatives(InitiativesCommand::Edit(args)) => {
                assert_eq!(args.short_code, "ACME-I-0001");
                assert_eq!(args.version, Some(3));
                assert!(args.common.json);
            }
            _ => panic!("expected initiatives edit"),
        }

        let cli = Cli::try_parse_from([
            "kairos",
            "strategies",
            "transition",
            "ACME-S-0001",
            "--to",
            "col-2",
        ])
        .expect("strategies transition parses");
        match cli.command {
            Command::Strategies(StrategiesCommand::Transition(args)) => {
                assert_eq!(args.to_column, "col-2");
            }
            _ => panic!("expected strategies transition"),
        }

        let cli = Cli::try_parse_from(["kairos", "adrs", "delete", "ACME-A-0001", "--confirm"])
            .expect("adrs delete parses");
        match cli.command {
            Command::Adrs(AdrsCommand::Delete(args)) => assert!(args.confirm),
            _ => panic!("expected adrs delete"),
        }

        // Documents have no transition endpoint in S-0005 — no verb either.
        assert!(
            Cli::try_parse_from(["kairos", "documents", "transition", "ACME-D-0001"]).is_err(),
            "documents must not offer transition"
        );

        // Boards, search (filters + traverse + escape hatch), teams/streams
        // nesting, members, admin tenants.
        assert!(Cli::try_parse_from(["kairos", "boards", "list", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["kairos", "boards", "show", "b-1"]).is_ok());
        assert!(Cli::try_parse_from(["kairos", "orgs", "show"]).is_ok());
        let cli = Cli::try_parse_from([
            "kairos",
            "search",
            "--query",
            "auth",
            "--type",
            "task",
            "--type",
            "initiative",
            "--metadata",
            "priority=critical",
            "--from",
            "ACME-S-0001",
            "--relationships",
            "parent",
            "--depth",
            "3",
        ])
        .expect("search parses");
        match cli.command {
            Command::Search(args) => {
                assert_eq!(args.entity_type.len(), 2);
                assert_eq!(args.depth, Some(3));
            }
            _ => panic!("expected search"),
        }
        assert!(
            Cli::try_parse_from(["kairos", "search", "--from", "x", "--from-id", "y"]).is_err(),
            "--from and --from-id conflict"
        );
        assert!(
            Cli::try_parse_from(["kairos", "search", "--query-json", "{}"]).is_ok(),
            "--query-json parses"
        );
        assert!(
            Cli::try_parse_from(["kairos", "teams", "members", "add", "t-1", "--user", "u-1"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kairos", "streams", "teams", "remove", "s-1", "--team", "t-1"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kairos", "members", "set-role", "u-1", "--role", "admin"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kairos", "admin", "tenants", "create", "--slug", "acme", "--name", "Acme"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kairos", "admin", "tenants", "delete", "acme", "--confirm"])
                .is_ok()
        );

        // Service accounts + API keys (KAIROS-T-0060).
        let cli = Cli::try_parse_from(["kairos", "service-accounts", "create", "--name", "ci"])
            .expect("service-accounts create parses");
        match cli.command {
            Command::ServiceAccounts(ServiceAccountsCommand::Create { name, .. }) => {
                assert_eq!(name, "ci");
            }
            _ => panic!("expected service-accounts create"),
        }
        assert!(Cli::try_parse_from(["kairos", "service-accounts", "list"]).is_ok());
        assert!(
            Cli::try_parse_from(["kairos", "service-accounts", "delete", "sa-1", "--confirm"])
                .is_ok()
        );

        let cli = Cli::try_parse_from([
            "kairos",
            "keys",
            "create",
            "--service-account",
            "sa-1",
            "--name",
            "gha",
            "--expires-at",
            "2027-01-01T00:00:00Z",
        ])
        .expect("keys create parses");
        match cli.command {
            Command::Keys(KeysCommand::Create {
                service_account,
                name,
                expires_at,
                ..
            }) => {
                assert_eq!(service_account, "sa-1");
                assert_eq!(name, "gha");
                assert_eq!(expires_at.as_deref(), Some("2027-01-01T00:00:00Z"));
            }
            _ => panic!("expected keys create"),
        }
        assert!(
            Cli::try_parse_from(["kairos", "keys", "list", "--service-account", "sa-1"]).is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kairos",
                "keys",
                "revoke",
                "k-1",
                "--service-account",
                "sa-1",
                "--confirm",
            ])
            .is_ok()
        );
    }
}
