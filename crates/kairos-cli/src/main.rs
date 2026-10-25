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
//! - `kairos login --url <deployment> --email <email>` — a local account
//!   (COLLIERY-T-0213): the password comes from the terminal or from
//!   standard input, `POST /api/login` gives a session bearer, and the
//!   bearer goes into the same cache.
//! - `kairos whoami [--json]` — `GET /api/whoami` through `KairosClient`
//!   with a refreshing [`provider::CachedTokenProvider`].
//! - `kairos logout [--url]` — clears the cached entry, and ends a local
//!   session on the server.
//!
//! The command tree (KAIROS-T-0037), nouns mirroring the S-0005 API:
//! `orgs`, `boards` (list/show grouped by column), `strategies`,
//! `initiatives`, `tasks`, `documents`, `adrs` (each
//! list/get/create/edit/transition/delete; deletes require `--confirm`),
//! `search` (filters + traverse via flags, or `--query-json`), `teams`
//! (+members), `streams` (+teams), `members`, and `admin tenants`. Human
//! tables by default, `--json` everywhere.
//!
//! `kairos index` (COLLIERY-T-1852) needs no deployment: it builds the code
//! index of a checkout and serves its code tools to an agent over stdio
//! ([`index`]).
//!
//! Exit codes (KAIROS-A-0015, KAIROS-T-0588): 0 success · 1 API/validation
//! error, a transport failure · 2 auth error · 3 `self-update --check` with a
//! newer server · 64 usage error. With `--json`, an error is the S-0005
//! envelope on stderr.

mod auth;
#[cfg(test)]
mod cli_doc_tests;
mod commands;
mod context;
mod credentials;
mod error;
mod index;
mod listing;
mod oidc;
mod password;
mod project_config;
mod provider;
mod self_update;
mod table;

use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use commands::admin::AdminCommand;
use commands::boards::BoardsCommand;
use commands::entities::{
    AdrsCommand, DocumentsCommand, InitiativesCommand, StrategiesCommand, TasksCommand,
};
use commands::grants::GrantsCommand;
use commands::keys::KeysCommand;
use commands::members::MembersCommand;
use commands::orgs::OrgsCommand;
use commands::pages::PagesCommand;
use commands::repos::ReposCommand;
use commands::search::SearchArgs;
use commands::service_accounts::ServiceAccountsCommand;
use commands::streams::StreamsCommand;
use commands::teams::TeamsCommand;
use credentials::{CredentialStore, DeploymentCredentials, unix_now};
use error::{CliError, EXIT_USAGE};
use index::IndexArgs;
use kairos_client::types_auth::LoginRequest;
use kairos_client::types_org::WhoamiResponse;
use kairos_client::{Error as ApiError, KairosClient};

#[derive(Parser)]
#[command(
    name = "kairos",
    version,
    about = "The Kairos command-line interface",
    long_about = "The Kairos command-line interface (KAIROS-A-0015).\nThe exit codes are: 0 for \
         success, 1 for an error of the API, of the input or of the network, 2 for an error of \
         the login, 3 for `self-update --check` when the server is newer, 64 for an argument \
         that the CLI does not know. With --json, an error is a JSON envelope on stderr.\n\n\
         The credentials: `kairos login`, or a key in the environment. With KAIROS_KEY (else \
         KAIROS_MCP_KEY) set to an agent key or a service-account key, each command sends that \
         key and does not use the login. KAIROS_URL (or --url) names the deployment."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in to a Kairos deployment: through its OIDC issuer, or with a local account
    #[command(long_about = "Log in to a Kairos deployment.\n\n\
        Without --email, the login goes through the OIDC issuer of the deployment \
        (OAuth Device Authorization Grant).\n\
        With --email, the login uses a local account. The CLI asks for the password \
        on the terminal and does not show it.\n\n\
        The password is never an argument. For a script, send it on standard input:\n  \
        printf '%s' \"$PASSWORD\" | kairos login --url <URL> --email <EMAIL>\n\n\
        With no login, the CLI can use an agent key: set KAIROS_URL and KAIROS_KEY (or \
        KAIROS_MCP_KEY). Do this when the issuer gives the CLI no device grant, for example \
        Google.")]
    Login {
        /// Deployment base URL, e.g. https://kairos.example.com (defaults to KAIROS_URL)
        #[arg(long)]
        url: Option<String>,
        /// Email of a local account. The CLI asks for the password, or reads it from standard input
        // COLLIERY-T-0213. There is no `--password`, and there must not be
        // one: an argument is visible in the process list and stays in the
        // shell history. The conflicts are explicit because each of the
        // three options means "the OAuth login", and a command that names
        // both logins has no one meaning to fall back to.
        #[arg(long, conflicts_with_all = ["issuer", "client_id", "bearer"])]
        email: Option<String>,
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
    /// Forget the cached credentials for a deployment, and end a local session
    Logout {
        /// Deployment base URL (defaults to KAIROS_URL, then the only cached deployment)
        #[arg(long)]
        url: Option<String>,
    },
    /// Show the authenticated user, organization, role, and teams
    Whoami {
        /// Deployment base URL (defaults to KAIROS_URL, then the only cached deployment)
        #[arg(long)]
        url: Option<String>,
        /// Tenant slug override (defaults to the tenant cached at login)
        #[arg(long)]
        tenant: Option<String>,
        /// Print the raw /api/whoami JSON
        #[arg(long)]
        json: bool,
    },
    /// Your work: the initiatives you own, and the tasks you or your agent
    /// keys claim, the tasks under your initiatives, and the tasks these
    /// block (Todo, Active, Blocked)
    Me(Box<commands::me::MeArgs>),
    /// Write the live state of a task: branch, commit, pushed, PR, next
    /// step (the write is a patch: an option you do not give is kept)
    State(Box<commands::state::StateArgs>),
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
    /// The activity log of the organization, newest first: filter by item,
    /// person, action, time or team
    Activity(Box<commands::activity::ActivityArgs>),
    /// Your notifications: list them, mark them read
    #[command(subcommand)]
    Notifications(commands::notifications::NotificationsCommand),
    /// Teams and their membership
    #[command(subcommand)]
    Teams(TeamsCommand),
    /// Repositories: the codebases that tasks link to (COLLIERY-A-0023)
    #[command(subcommand)]
    Repos(ReposCommand),
    /// The published site of a repository: publish a built site, print its
    /// base path, unpublish it, turn it on or off
    #[command(subcommand)]
    Pages(PagesCommand),
    /// Delivery streams and their teams
    #[command(subcommand)]
    Streams(StreamsCommand),
    /// Organization membership (org-admin operations)
    #[command(subcommand)]
    Members(MembersCommand),
    /// Service accounts: machine principals authenticated by API keys
    #[command(subcommand)]
    ServiceAccounts(ServiceAccountsCommand),
    /// Your agent keys, or the API keys of a service account
    #[command(subcommand)]
    Keys(KeysCommand),
    /// Capability grants: manage_structure (on the organization or on one
    /// board), and the board capabilities
    #[command(subcommand)]
    Grants(GrantsCommand),
    /// Deployment administration (tenant provisioning)
    #[command(subcommand)]
    Admin(AdminCommand),
    /// The metadata definitions of the organization (writes: organization admins)
    #[command(subcommand)]
    Metadata(commands::meta::MetadataCommand),
    /// The templates of the organization (writes: organization admins)
    #[command(subcommand)]
    Templates(commands::meta::TemplatesCommand),
    /// Make a relationship: parent, blocks, informs, supports, supersedes,
    /// amends, impacts (a repository) or external (a URI)
    Link(commands::links::LinkArgs),
    /// Remove a relationship that `kairos link` makes
    Unlink(commands::links::UnlinkArgs),
    /// The code index of a checkout, and its code tools for an agent (COLLIERY-T-1852)
    Index(IndexArgs),
    /// The config of the person (config.toml): the deployment and the boards of each
    /// project, by its origin remote
    #[command(subcommand)]
    Config(commands::config::ConfigCommand),
    /// The MCP server of Kairos over stdio, for the Claude Code plugin: it forwards to /mcp of
    /// the deployment of the project
    McpProxy(commands::mcp_proxy::McpProxyArgs),
    /// Install the version of the server (or --latest, or --version) in place of this binary
    SelfUpdate(self_update::SelfUpdateArgs),
}

#[tokio::main]
async fn main() -> ExitCode {
    // KAIROS-T-0588: a usage error exits 64, not clap's 2, which is the
    // auth code. `--help` and `--version` are not errors.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => ExitCode::from(EXIT_USAGE),
            };
        }
    };
    let json = json_requested(std::env::args().skip(1));
    match run(cli.command).await {
        Ok(code) => code,
        Err(err) => {
            if json {
                eprintln!("{}", err.to_json());
            } else {
                eprintln!("error: {err}");
            }
            ExitCode::from(err.exit_code())
        }
    }
}

/// Whether the command has `--json` (KAIROS-T-0588): then an error is the
/// JSON envelope on stderr. The flag belongs to each command, not to the
/// root, so the parsed tree has no one place for it. The arguments do: a
/// `--json` that is not the flag does not parse (clap takes no value that
/// starts with `--`), so after `try_parse` the only `--json` before `--` is
/// the flag.
fn json_requested(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--json")
}

/// Whether the daily version check runs after the command (KAIROS-T-0432,
/// KAIROS-T-0595). Not for the commands that set up the CLI. Not for
/// `index mcp`: it is a long stdio session with a check of its own at the
/// start. The other `index` commands make the check.
fn makes_daily_check(command: &Command) -> bool {
    match command {
        Command::Login { .. }
        | Command::Logout { .. }
        | Command::SelfUpdate(_)
        | Command::Config(_)
        | Command::McpProxy(_) => false,
        Command::Index(args) => !args.serves_mcp(),
        _ => true,
    }
}

async fn run(command: Command) -> Result<ExitCode, CliError> {
    // KAIROS-T-0432: one line a day when the server is newer, after a
    // command that succeeded. The check asks the deployment of the command
    // (KAIROS-T-0595).
    let check = makes_daily_check(&command);
    // `self-update --check` has an exit code of its own (KAIROS-T-0588).
    if let Command::SelfUpdate(args) = command {
        return args.run().await;
    }
    let result = dispatch(command).await;
    if check && result.is_ok() {
        self_update::daily_check().await;
    }
    result.map(|()| ExitCode::SUCCESS)
}

async fn dispatch(command: Command) -> Result<(), CliError> {
    match command {
        Command::Login {
            url,
            email: Some(email),
            tenant,
            ..
        } => login_local(&login_url(url, &auth::Env::from_process())?, &email, tenant).await,
        Command::Login {
            url,
            email: None,
            issuer,
            tenant,
            client_id,
            bearer,
        } => {
            let url = login_url(url, &auth::Env::from_process())?;
            login(&url, issuer.as_deref(), tenant, client_id, bearer).await
        }
        Command::Logout { url } => logout(url.or(auth::Env::from_process().url).as_deref()).await,
        Command::Whoami { url, tenant, json } => whoami(url.as_deref(), tenant, json).await,
        Command::Me(args) => (*args).run().await,
        Command::State(args) => (*args).run().await,
        Command::Orgs(command) => command.run().await,
        Command::Boards(command) => command.run().await,
        Command::Strategies(command) => command.run().await,
        Command::Initiatives(command) => command.run().await,
        Command::Tasks(command) => command.run().await,
        Command::Documents(command) => command.run().await,
        Command::Adrs(command) => command.run().await,
        Command::Search(args) => (*args).run().await,
        Command::Activity(args) => (*args).run().await,
        Command::Notifications(command) => command.run().await,
        Command::Teams(command) => command.run().await,
        Command::Grants(command) => command.run().await,
        Command::Repos(command) => command.run().await,
        Command::Pages(command) => command.run().await,
        Command::Streams(command) => command.run().await,
        Command::Members(command) => command.run().await,
        Command::ServiceAccounts(command) => command.run().await,
        Command::Keys(command) => command.run().await,
        Command::Admin(command) => command.run().await,
        Command::Index(command) => command.run().await,
        Command::Link(args) => args.run().await,
        Command::Unlink(args) => args.run().await,
        Command::Metadata(command) => command.run().await,
        Command::Templates(command) => command.run().await,
        Command::Config(command) => command.run().await,
        Command::McpProxy(args) => args.run().await,
        Command::SelfUpdate(args) => args.run().await.map(|_| ()),
    }
}

/// The deployment of `kairos login`: `--url`, else `KAIROS_URL`
/// (KAIROS-T-0594).
fn login_url(url: Option<String>, env: &auth::Env) -> Result<String, CliError> {
    url.or_else(|| env.url.clone()).ok_or_else(|| {
        CliError::Failure(format!(
            "The CLI does not know the deployment. Use --url or set {}.",
            auth::URL_VAR
        ))
    })
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
        return Err(CliError::Failure(
            "The value of --url is empty. Give the URL of the deployment.".to_string(),
        ));
    }
    let http = reqwest::Client::new();

    let issuer = match issuer_override {
        Some(issuer) => {
            let issuer = issuer.trim_end_matches('/').to_string();
            println!("The CLI uses the issuer of --issuer: {issuer}");
            issuer
        }
        None => {
            let issuer = oidc::discover_issuer(&http, &deployment).await?;
            println!("The OIDC issuer of the deployment is: {issuer}");
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
                "(As an alternative, open {} and enter code: {})",
                grant.verification_uri, grant.user_code
            );
        }
        None => {
            println!("To sign in, open: {}", grant.verification_uri);
            println!("and enter code: {}", grant.user_code);
        }
    }
    println!();
    println!(
        "The CLI waits for the approval. It asks the issuer each {} seconds. The code expires \
         in {} seconds.",
        grant.interval.unwrap_or(5).max(1),
        grant.expires_in
    );

    let token = oidc::poll_device_grant(&http, &endpoints, &client_id, &grant).await?;
    if token.refresh_token.is_none() {
        eprintln!(
            "warning: The issuer gave no refresh token. You must log in again when the access \
             token expires."
        );
    }

    // Select the bearer this deployment expects (T-0054). `id_token` requires
    // the issuer to have returned one (the `openid` scope is always requested).
    let bearer = token
        .bearer_for(api_bearer)
        .ok_or_else(|| {
            CliError::Auth(
                "The command has --bearer id_token, and the issuer gave no id_token.\n\
                 Make sure that the OAuth client asks for the scope `openid`, or use `--bearer \
                 access_token`."
                    .to_string(),
            )
        })?
        .to_string();

    let path = credentials::credentials_path()?;
    // KAIROS-T-0594: no refresh of another process writes between the read
    // and the write.
    let _lock = credentials::lock(&path)?;
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
            kind: credentials::CredentialKind::Oauth,
            email: None,
        },
    );
    credentials::save(&path, &store)?;

    println!();
    println!("You are logged in to {deployment}.");
    println!("The credentials are in {} (mode 0600).", path.display());
    Ok(())
}

/// The store to merge a fresh login into: a corrupted cache is NOT fatal
/// here — login is the documented fix, so start over (with a note).
fn load_store_for_login(path: &std::path::Path) -> CredentialStore {
    match credentials::load(path) {
        Ok(store) => store,
        Err(_) => {
            eprintln!(
                "warning: The CLI could not read the credential cache at {}. The CLI made a \
                 new cache.",
                path.display()
            );
            CredentialStore::default()
        }
    }
}

/// `kairos login --email` — log in with a local account (COLLIERY-T-0213,
/// KAIROS-I-0018).
///
/// A deployment with local accounts and no issuer has no device endpoint, so
/// the OAuth login has nothing to talk to. Before this command, a person
/// with a local account could not use the CLI at all.
///
/// The order of the steps is for the person at the terminal:
///
/// 1. `GET /api/config` first. If the deployment says that local accounts
///    are off, the CLI stops before it asks for a password that it cannot
///    use. A failure of `/api/config` itself does not stop the login: on a
///    deployment with both logins the endpoint answers 502 when the issuer
///    is down, and that is the moment a local admin account is for.
/// 2. The password, from the terminal or from standard input.
/// 3. `POST /api/login`. The request, and the password in it, are dropped
///    when the call returns.
/// 4. The session bearer goes into the credential cache, in the entry of
///    this deployment, with the expiry that the server gave.
async fn login_local(url: &str, email: &str, tenant: Option<String>) -> Result<(), CliError> {
    let deployment = credentials::normalize_url(url);
    if deployment.is_empty() {
        return Err(CliError::Failure(
            "The value of --url is empty. Give the URL of the deployment.".to_string(),
        ));
    }
    let email = email.trim();
    if email.is_empty() {
        return Err(CliError::Failure(
            "The value of --email is empty. Give the email of the account.".to_string(),
        ));
    }

    let mut client = KairosClient::anonymous(&deployment);
    if let Some(tenant) = &tenant {
        client = client.with_tenant(tenant);
    }

    match client.deployment_config().await {
        Ok(config) if !config.local_auth => return Err(local_accounts_off(&deployment)),
        Ok(_) => {}
        // Nothing answered. The login cannot succeed, so do not ask for a
        // password.
        Err(err @ ApiError::Transport(_)) => return Err(err.into()),
        // See step 1 above: `POST /api/login` decides.
        Err(_) => {}
    }

    let response = {
        let request = LoginRequest {
            email: email.to_string(),
            password: password::read(email)?,
        };
        client.login(&request).await
        // `request` goes out of scope here, and the password with it.
    }
    .map_err(|err| login_failure(err, &deployment))?;

    let expires_at = chrono::DateTime::parse_from_rfc3339(&response.expires_at)
        .ok()
        .and_then(|at| u64::try_from(at.timestamp()).ok())
        .ok_or_else(|| {
            CliError::Failure(
                "The deployment gave a session with an expiry that the CLI cannot read.\n\
                 Make sure that the CLI and the deployment are the same version."
                    .to_string(),
            )
        })?;

    let path = credentials::credentials_path()?;
    let _lock = credentials::lock(&path)?;
    let mut store = load_store_for_login(&path);
    store.deployments.insert(
        deployment.clone(),
        DeploymentCredentials::local_session(
            &response.token,
            expires_at,
            response.user.email.clone(),
            tenant,
        ),
    );
    credentials::save(&path, &store)?;

    println!(
        "You are logged in to {deployment} as {}.",
        response.user.email
    );
    println!("The session expires at {}.", response.expires_at);
    println!("The credentials are in {} (mode 0600).", path.display());
    Ok(())
}

/// `--email` on a deployment that has local accounts off (COLLIERY-T-0213).
fn local_accounts_off(deployment: &str) -> CliError {
    CliError::Failure(format!(
        "Local accounts are off on this deployment.\n\
         Run `kairos login --url {deployment}` to log in through the issuer."
    ))
}

/// What the CLI says when `POST /api/login` fails (COLLIERY-T-0213).
///
/// The 401 has ONE message, written here and not taken from the server. The
/// server gives the same 401 for a wrong password and for an account that
/// does not exist, so that a caller cannot find out which addresses are
/// accounts. The CLI must not undo that. A fixed text cannot differ between
/// the two cases, and it names no address.
fn login_failure(err: ApiError, deployment: &str) -> CliError {
    match err {
        ApiError::Unauthorized { .. } => CliError::Auth(
            "The deployment did not accept the email and the password.\n\
             Make sure that they are correct. Then run the command again."
                .to_string(),
        ),
        // The throttle of KAIROS-T-0202: 429 `TOO_MANY_REQUESTS`, with the
        // wait in `details.retry_after_secs`.
        ApiError::Other {
            status: 429,
            details,
            ..
        } => CliError::Failure(match details["retry_after_secs"].as_u64() {
            Some(seconds) => format!(
                "The number of incorrect logins is too large.\n\
                 Wait {seconds} seconds. Then try again."
            ),
            None => {
                "The number of incorrect logins is too large.\nWait one minute. Then try again."
                    .to_string()
            }
        }),
        // The route is mounted only with local accounts on. This is the
        // answer when `/api/config` could not say so first.
        ApiError::NotFound { .. } | ApiError::UnexpectedResponse { status: 404, .. } => {
            local_accounts_off(deployment)
        }
        other => other.into(),
    }
}

/// `kairos logout` — drop the deployment's entry from the cache, and end a
/// local session on the server (COLLIERY-T-0213).
///
/// An OAuth token cannot be ended from here, so for an OAuth entry the
/// command only forgets it, as before. A session bearer can: without the
/// call to `POST /api/logout`, a bearer that was copied from the cache stays
/// a working credential for the rest of its fortnight.
///
/// The entry is removed BEFORE the call and whatever the call does. A
/// person who logs out wants the credential off this machine. A server that
/// does not answer is not a reason to keep it.
async fn logout(url: Option<&str>) -> Result<(), CliError> {
    let path = credentials::credentials_path()?;
    let Some((deployment, entry)) = forget(&path, url)? else {
        println!("The cache has no credentials. The CLI changed nothing.");
        return Ok(());
    };

    if entry.is_local_session() {
        let mut client = KairosClient::with_static_token(&deployment, entry.access_token);
        if let Some(tenant) = entry.tenant {
            client = client.with_tenant(tenant);
        }
        if let Err(err) = client.logout().await {
            let reason = match err {
                ApiError::Transport(_) => "the deployment did not answer".to_string(),
                other => match other.status() {
                    Some(status) => format!("the deployment answered {status}"),
                    None => "the answer of the deployment was not readable".to_string(),
                },
            };
            return Err(CliError::Failure(format!(
                "The deployment did not end the session: {reason}.\n\
                 The CLI removed the local entry from the cache. The session stays in effect \
                 until it expires.\n\
                 To end it now, ask an organization admin to end your sessions."
            )));
        }
        println!("You are logged out of {deployment}.");
        println!("The server ended the session.");
        return Ok(());
    }

    println!("You are logged out of {deployment}.");
    Ok(())
}

/// Remove the entry of `url` (else of the only deployment) from the cache
/// at `path`, under the lock of the cache (KAIROS-T-0594). `None` when the
/// cache is empty.
fn forget(
    path: &std::path::Path,
    url: Option<&str>,
) -> Result<Option<(String, DeploymentCredentials)>, CliError> {
    let _lock = credentials::lock(path)?;
    let mut store = credentials::load(path)?;
    if store.deployments.is_empty() {
        return Ok(None);
    }
    let deployment = credentials::resolve_deployment(&store, url)?;
    let Some(entry) = store.deployments.remove(&deployment) else {
        return Err(CliError::Failure(format!(
            "The cache has no credentials for {deployment}. The cache has: {}.",
            store
                .deployments
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    credentials::save(path, &store)?;
    Ok(Some((deployment, entry)))
}

/// `kairos whoami` — the identity probe via `kairos-client`.
async fn whoami(
    url: Option<&str>,
    tenant_override: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let (client, credential) = context::client_with_credential(&context::Common {
        url: url.map(str::to_string),
        tenant: tenant_override,
        json,
    })?;

    let identity = client.whoami().await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&identity).map_err(|err| CliError::Failure(format!(
                "The CLI cannot write the JSON: {err}."
            )))?
        );
    } else {
        print_identity(&identity);
        // KAIROS-T-0594: the source of the credential. `--json` prints the
        // response of the server only.
        println!("{credential}");
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
    // KAIROS-T-0337: the grants of manage_structure.
    if !identity.structure.is_empty() {
        let scopes: Vec<String> = identity
            .structure
            .iter()
            .map(|grant| match &grant.board_slug {
                Some(board) => format!("board {board}"),
                None => "organization".to_string(),
            })
            .collect();
        println!("manage_structure: {}", scopes.join(", "));
    }
    // KAIROS-T-0536: the scope of the agent key in use.
    if let Some(key) = &identity.agent_key {
        match &key.scope {
            Some(scope) => println!("key scope: {}", scope.describe()),
            None => println!("key scope: none (the key can do each thing that you can do)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(env!("CARGO_PKG_NAME"), "kairos-cli");
    }

    /// KAIROS-T-0588: an argument the CLI does not know is a usage error
    /// (64); `--help` and `--version` are not errors.
    #[test]
    fn usage_errors_are_not_help() {
        let kind = |args: &[&str]| match Cli::try_parse_from(args) {
            Ok(_) => panic!("{args:?} must not parse"),
            Err(err) => err.kind(),
        };
        assert_eq!(
            kind(&["kairos", "index", "update", "--no-such-flag"]),
            ErrorKind::UnknownArgument
        );
        assert_eq!(EXIT_USAGE, 64);
        assert_eq!(kind(&["kairos", "--help"]), ErrorKind::DisplayHelp);
        assert_eq!(kind(&["kairos", "--version"]), ErrorKind::DisplayVersion);
        assert_eq!(
            kind(&["kairos"]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    /// KAIROS-T-0595: the daily check runs for `index` too, but not for
    /// its MCP server and not for the commands that set up the CLI.
    #[test]
    fn the_daily_check_runs_for_index_but_not_for_its_mcp_server() {
        let check = |argv: &[&str]| {
            let cli = Cli::try_parse_from(argv).expect("parses");
            makes_daily_check(&cli.command)
        };
        assert!(check(&["kairos", "index", "status"]));
        assert!(check(&["kairos", "index", "update"]));
        assert!(check(&["kairos", "index", "--full"]));
        assert!(check(&["kairos", "tasks", "get", "X-T-1"]));
        assert!(!check(&["kairos", "index", "mcp"]));
        assert!(!check(&["kairos", "self-update", "--check"]));
        assert!(!check(&["kairos", "logout"]));
        assert!(!check(&["kairos", "login", "--url", "https://k.test"]));
    }

    /// KAIROS-T-0595: `self-update` takes `--json` and `--allow-downgrade`;
    /// `--check` excludes `--allow-downgrade`.
    #[test]
    fn self_update_takes_json_and_allow_downgrade() {
        let cli = Cli::try_parse_from(["kairos", "self-update", "--allow-downgrade", "--json"])
            .expect("parses");
        match cli.command {
            Command::SelfUpdate(args) => assert!(args.allow_downgrade && args.json),
            _ => panic!("self-update"),
        }
        assert!(Cli::try_parse_from(["kairos", "self-update", "--check", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from(["kairos", "self-update", "--check", "--allow-downgrade"]).is_err()
        );
    }

    /// KAIROS-T-0588: `--json` anywhere before `--` asks for the JSON
    /// envelope; after `--` it is a value.
    #[test]
    fn json_is_found_in_the_arguments() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(json_requested(args(&["tasks", "get", "X-T-1", "--json"])));
        assert!(json_requested(args(&[
            "whoami",
            "--json",
            "--url",
            "https://x"
        ])));
        assert!(!json_requested(args(&["tasks", "get", "X-T-1"])));
        assert!(!json_requested(args(&[
            "tasks", "note", "X-T-1", "--", "--json"
        ])));
    }

    /// KAIROS-T-0337: `grants add` grants manage_structure by default, on
    /// the organization; `--board` narrows it to one board. `boards create`
    /// takes the level, the prefix and an optional team.
    #[test]
    fn grants_and_boards_create_parse() {
        let cli = Cli::try_parse_from(["kairos", "grants", "add", "--user", "admin-agent"])
            .expect("grants add parses");
        match cli.command {
            Command::Grants(GrantsCommand::Add {
                user,
                capability,
                board,
                ..
            }) => {
                assert_eq!(user, "admin-agent");
                assert_eq!(capability, "manage_structure");
                assert_eq!(board, None);
            }
            _ => panic!("expected grants add"),
        }
        let cli = Cli::try_parse_from([
            "kairos",
            "grants",
            "add",
            "--user",
            "admin-agent",
            "--board",
            "ops-delivery",
        ])
        .expect("grants add --board parses");
        assert!(matches!(
            cli.command,
            Command::Grants(GrantsCommand::Add { board: Some(_), .. })
        ));
        assert!(
            Cli::try_parse_from(["kairos", "grants", "remove", "g1"]).is_ok(),
            "remove parses; --confirm is checked at run time"
        );
        let cli = Cli::try_parse_from([
            "kairos",
            "boards",
            "create",
            "--name",
            "Ops ADRs",
            "--slug",
            "ops-adrs",
            "--level",
            "adr",
            "--team",
            "ops",
            "--code-prefix",
            "OPS",
        ])
        .expect("boards create parses");
        assert!(matches!(cli.command, Command::Boards(_)));
        assert!(
            Cli::try_parse_from(["kairos", "boards", "create", "--name", "x"]).is_err(),
            "slug, level and prefix are required"
        );
    }

    /// KAIROS-T-0646: `repos local` takes one folder or none. A second
    /// folder or an unknown flag is refused, and the error names it.
    #[test]
    fn repos_local_takes_one_folder_or_none() {
        for argv in [
            &["kairos", "repos", "local"][..],
            &["kairos", "repos", "local", "boards/acme", "--json"][..],
        ] {
            let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            assert!(matches!(cli.command, Command::Repos(_)));
        }
        let err = Cli::try_parse_from(["kairos", "repos", "local", "a", "b"])
            .err()
            .expect("a second folder is refused");
        assert!(err.to_string().contains("'b'"), "{err}");
        let err = Cli::try_parse_from(["kairos", "repos", "local", "--fetch"])
            .err()
            .expect("an unknown flag is refused");
        assert!(err.to_string().contains("--fetch"), "{err}");
    }

    /// KAIROS-T-0386: `repos write-token` parses its subcommands, and takes
    /// no token as an argument.
    #[test]
    fn repos_write_token_parses() {
        for verb in ["set", "remove", "status"] {
            let cli = Cli::try_parse_from(["kairos", "repos", "write-token", verb, "site"])
                .unwrap_or_else(|e| panic!("{verb}: {e}"));
            assert!(matches!(cli.command, Command::Repos(_)));
        }
        assert!(
            Cli::try_parse_from([
                "kairos",
                "repos",
                "write-token",
                "set",
                "site",
                "--token",
                "x"
            ])
            .is_err(),
            "the token is never an argument"
        );
    }

    /// KAIROS-T-0385: `boards dispatch` parses its subcommands.
    #[test]
    fn boards_dispatch_parses() {
        for args in [
            &["kairos", "boards", "dispatch", "list", "web"][..],
            &[
                "kairos",
                "boards",
                "dispatch",
                "add-target",
                "web",
                "--name",
                "Runner",
                "--hook-url",
                "https://hooks.example.com/k",
            ][..],
            &[
                "kairos",
                "boards",
                "dispatch",
                "add-rule",
                "web",
                "--column",
                "Todo",
                "--target",
                "t1",
                "--task-type",
                "bug",
                "--metadata",
                "priority=high",
            ][..],
            &["kairos", "boards", "dispatch", "remove-rule", "web", "r1"][..],
            &["kairos", "boards", "dispatch", "remove-target", "web", "t1"][..],
            &["kairos", "boards", "dispatch", "enable-target", "web", "t1"][..],
            &["kairos", "boards", "dispatch", "redeliver", "web", "t1"][..],
            &[
                "kairos",
                "boards",
                "dispatch",
                "redeliver",
                "web",
                "t1",
                "--since",
                "2026-10-17T09:00:00Z",
            ][..],
            &["kairos", "boards", "dispatch", "test-target", "web", "t1"][..],
            &["kairos", "boards", "dispatch", "log", "web", "--limit", "5"][..],
        ] {
            let cli = Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert!(matches!(cli.command, Command::Boards(_)));
        }
        assert!(
            Cli::try_parse_from(["kairos", "boards", "dispatch", "add-rule", "web"]).is_err(),
            "--column and --target are required"
        );
    }

    /// KAIROS-T-0318: `repos update --code-index-build` takes on and off
    /// only, and the error names the refused value.
    /// KAIROS-T-0408: `kairos pages` and its subcommands parse; `--repo` is
    /// required.
    #[test]
    fn pages_commands_parse() {
        for args in [
            vec!["kairos", "pages", "publish", "dist", "--repo", "payments"],
            vec!["kairos", "pages", "base-path", "--repo", "payments"],
            vec![
                "kairos",
                "pages",
                "unpublish",
                "--repo",
                "payments",
                "--confirm",
            ],
            vec!["kairos", "pages", "on", "--repo", "payments"],
            vec!["kairos", "pages", "off", "--repo", "payments", "--json"],
        ] {
            let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert!(matches!(cli.command, Command::Pages(_)), "{args:?}");
        }
        for args in [
            vec!["kairos", "pages", "publish", "dist"],
            vec!["kairos", "pages", "base-path"],
            vec!["kairos", "pages", "publish", "--repo", "payments"],
        ] {
            assert!(
                Cli::try_parse_from(&args).is_err(),
                "{args:?} must be refused"
            );
        }
    }

    #[test]
    fn repos_update_takes_on_or_off_for_the_code_index_builder() {
        for value in ["on", "off"] {
            let cli = Cli::try_parse_from([
                "kairos",
                "repos",
                "update",
                "site",
                "--code-index-build",
                value,
            ])
            .expect("on and off parse");
            assert!(matches!(cli.command, Command::Repos(_)));
        }
        let err = Cli::try_parse_from([
            "kairos",
            "repos",
            "update",
            "site",
            "--code-index-build",
            "maybe",
        ])
        .err()
        .expect("maybe is refused");
        let text = err.to_string();
        assert!(text.contains("maybe"), "{text}");
        assert!(text.contains("--code-index-build"), "{text}");
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
                email,
                issuer,
                tenant,
                client_id,
                bearer,
            } => {
                assert_eq!(email, None);
                assert_eq!(url.as_deref(), Some("http://localhost:8080"));
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

        // KAIROS-T-0594: `--url` is optional; KAIROS_URL stands in for it.
        let cli = Cli::try_parse_from(["kairos", "login"]).expect("login parses");
        assert!(matches!(cli.command, Command::Login { url: None, .. }));
    }

    /// KAIROS-T-0594: `login` reads KAIROS_URL when `--url` is absent, and
    /// `--url` wins. With neither, the error names both.
    #[test]
    fn login_reads_kairos_url() {
        let env = auth::Env::from_lookup(|name| {
            (name == auth::URL_VAR).then(|| "https://env.example".to_string())
        });
        assert_eq!(login_url(None, &env).unwrap(), "https://env.example");
        assert_eq!(
            login_url(Some("https://flag.example".into()), &env).unwrap(),
            "https://flag.example"
        );
        let err = login_url(None, &auth::Env::default()).unwrap_err();
        assert_eq!(err.exit_code(), error::EXIT_FAILURE);
        let text = err.to_string();
        assert!(
            text.contains("--url") && text.contains("KAIROS_URL"),
            "{text}"
        );
    }

    /// KAIROS-T-0594: `logout` removes the entry of the URL that it gets
    /// (the caller gives `--url`, else KAIROS_URL), and keeps the others.
    #[test]
    fn logout_forgets_the_entry_of_the_url() {
        let dir = tempfile::tempdir().expect("a folder");
        let path = dir.path().join("credentials.json");
        let mut store = CredentialStore::default();
        for url in ["https://a.example", "https://b.example"] {
            store.deployments.insert(
                url.into(),
                DeploymentCredentials::local_session(
                    &kairos_client::types_auth::Secret::new("kairos_ss_x"),
                    u64::MAX,
                    "ada@example.test".into(),
                    None,
                ),
            );
        }
        credentials::save(&path, &store).expect("save");
        // Two deployments and no URL: the CLI does not guess.
        assert!(forget(&path, None).is_err());
        let (gone, _) = forget(&path, Some("https://b.example/"))
            .expect("forget")
            .expect("an entry");
        assert_eq!(gone, "https://b.example");
        let left = credentials::load(&path).expect("load");
        assert_eq!(
            left.deployments.keys().collect::<Vec<_>>(),
            vec!["https://a.example"]
        );
        // The lock is free after the call.
        assert!(credentials::try_lock_soon(&path).expect("try").is_some());
    }

    /// COLLIERY-T-0213: `--email` selects the password login, and it does
    /// not go together with an option of the OAuth login.
    #[test]
    fn login_email_parses_and_conflicts_with_the_oauth_options() {
        let cli = Cli::try_parse_from([
            "kairos",
            "login",
            "--url",
            "http://localhost:8080",
            "--email",
            "ada@example.test",
            "--tenant",
            "acme",
        ])
        .unwrap_or_else(|err| panic!("login --email parses: {err}"));
        assert!(matches!(cli.command, Command::Login { .. }));

        for (flag, value) in [
            ("--issuer", "https://idp.example.test"),
            ("--client-id", "kairos-cli"),
            ("--bearer", "id_token"),
        ] {
            let err = Cli::try_parse_from([
                "kairos",
                "login",
                "--url",
                "http://localhost:8080",
                "--email",
                "ada@example.test",
                flag,
                value,
            ])
            .err()
            .expect("--email with an OAuth option is a usage error");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{flag}: {err}"
            );
        }
    }

    /// COLLIERY-T-0213: the password is never an argument. An argument is
    /// visible in the process list and stays in the shell history. The test
    /// reads the clap definition, so a flag that a later change adds under
    /// this name, or as an alias, fails here.
    #[test]
    fn login_has_no_password_flag() {
        use clap::CommandFactory;
        let command = Cli::command();
        let login = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "login")
            .expect("the login command");
        assert!(
            login
                .get_arguments()
                .any(|arg| arg.get_long() == Some("email")),
            "login must offer --email"
        );
        for arg in login.get_arguments() {
            let mut names: Vec<String> = vec![arg.get_id().to_string()];
            names.extend(arg.get_long().map(str::to_string));
            names.extend(
                arg.get_all_aliases()
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::to_string),
            );
            for name in names {
                assert!(
                    !name.to_lowercase().contains("password"),
                    "login must not take the password as an argument, found {name:?}"
                );
            }
        }

        let err = Cli::try_parse_from([
            "kairos",
            "login",
            "--url",
            "http://localhost:8080",
            "--email",
            "ada@example.test",
            "--password",
            "x",
        ])
        .err()
        .expect("--password is not an option");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument, "{err}");
    }

    /// KAIROS-T-0589: one flag name per concept, and each old name still
    /// parses as a hidden alias. `show` is the verb, `get` its alias.
    #[test]
    fn the_new_flag_names_parse_and_the_old_names_stay_as_aliases() {
        use clap::CommandFactory;
        let pairs: [(&[&str], &[&str]); 9] = [
            (
                &[
                    "kairos", "tasks", "create", "--board", "b", "--title", "t", "--kind", "bug",
                ],
                &[
                    "kairos", "tasks", "create", "--board", "b", "--title", "t", "--type", "bug",
                ],
            ),
            (
                &[
                    "kairos",
                    "teams",
                    "create",
                    "--name",
                    "n",
                    "--slug",
                    "s",
                    "--code-prefix",
                    "S",
                    "--kind",
                    "platform",
                ],
                &[
                    "kairos",
                    "teams",
                    "create",
                    "--name",
                    "n",
                    "--slug",
                    "s",
                    "--code-prefix",
                    "S",
                    "--type",
                    "platform",
                ],
            ),
            (
                &["kairos", "teams", "update", "s", "--kind", "platform"],
                &["kairos", "teams", "update", "s", "--type", "platform"],
            ),
            (
                &["kairos", "search", "--kind", "bug"],
                &["kairos", "search", "--task-type", "bug"],
            ),
            (
                &[
                    "kairos", "boards", "dispatch", "add-rule", "web", "--target", "t", "--kind",
                    "bug", "--repo", "r",
                ],
                &[
                    "kairos",
                    "boards",
                    "dispatch",
                    "add-rule",
                    "web",
                    "--target",
                    "t",
                    "--task-type",
                    "bug",
                    "--repository",
                    "r",
                ],
            ),
            (
                &["kairos", "boards", "flow", "web", "--until", "2026-10-18"],
                &["kairos", "boards", "flow", "web", "--to", "2026-10-18"],
            ),
            (
                &["kairos", "index", "update", "--repo", "kairos"],
                &["kairos", "index", "update", "--repository", "kairos"],
            ),
            (
                &["kairos", "tasks", "show", "ACME-T-0001"],
                &["kairos", "tasks", "get", "ACME-T-0001"],
            ),
            (
                &["kairos", "streams", "show", "payments"],
                &["kairos", "streams", "get", "payments"],
            ),
        ];
        for (new, old) in pairs {
            Cli::try_parse_from(new).unwrap_or_else(|e| panic!("{new:?}: {e}"));
            Cli::try_parse_from(old).unwrap_or_else(|e| panic!("{old:?}: {e}"));
        }
        for args in [
            &["kairos", "teams", "show", "platform"][..],
            &["kairos", "teams", "get", "platform"][..],
            &["kairos", "repos", "show", "kairos"][..],
            &["kairos", "repos", "get", "kairos"][..],
            &[
                "kairos",
                "tasks",
                "note",
                "ACME-T-0001",
                "--text-file",
                "note.md",
            ][..],
            &[
                "kairos",
                "tasks",
                "create",
                "--board",
                "b",
                "--title",
                "t",
                "--content-file",
                "-",
            ][..],
            &[
                "kairos",
                "documents",
                "create",
                "--board",
                "b",
                "--title",
                "t",
                "--template",
                "rfc",
                "--content-file",
                "body.md",
            ][..],
        ] {
            Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        }
        // A note needs --text or --text-file, not both.
        for args in [
            &["kairos", "tasks", "note", "ACME-T-0001"][..],
            &[
                "kairos",
                "tasks",
                "note",
                "ACME-T-0001",
                "--text",
                "x",
                "--text-file",
                "f",
            ][..],
            &[
                "kairos",
                "tasks",
                "create",
                "--board",
                "b",
                "--title",
                "t",
                "--content",
                "x",
                "--content-file",
                "f",
            ][..],
        ] {
            assert!(
                Cli::try_parse_from(args).is_err(),
                "{args:?} must be refused"
            );
        }
        // The old names are hidden: the help shows the new name only.
        let mut command = Cli::command();
        let tasks = command.find_subcommand_mut("tasks").expect("tasks");
        let help = tasks.render_long_help().to_string();
        assert!(help.contains("show"), "{help}");
        assert!(!help.contains("  get "), "{help}");
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
                assert_eq!(args.board.as_deref(), Some("b-1"));
                assert_eq!(args.title, "Fix login");
                assert_eq!(args.task_type.as_deref(), Some("bug"));
                assert_eq!(args.content.as_deref(), Some("body"));
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

        // KAIROS-T-0362: the verb is `archive`, and `delete` stays as an
        // alias of it.
        for verb in ["archive", "delete"] {
            let cli = Cli::try_parse_from(["kairos", "adrs", verb, "ACME-A-0001", "--confirm"])
                .expect("adrs archive parses");
            match cli.command {
                Command::Adrs(AdrsCommand::Archive(args)) => assert!(args.confirm),
                _ => panic!("expected adrs archive"),
            }
        }

        // KAIROS-T-0362: a task can be cancelled with a reason, and purged.
        let cli = Cli::try_parse_from([
            "kairos",
            "tasks",
            "cancel",
            "ACME-T-0001",
            "--reason",
            "Not needed",
        ])
        .expect("tasks cancel parses");
        match cli.command {
            Command::Tasks(TasksCommand::Cancel(args)) => assert_eq!(args.reason, "Not needed"),
            _ => panic!("expected tasks cancel"),
        }
        assert!(
            Cli::try_parse_from(["kairos", "tasks", "cancel", "ACME-T-0001"]).is_err(),
            "a cancel needs --reason"
        );
        // KAIROS-T-0506: the live state of a task.
        let cli = Cli::try_parse_from([
            "kairos",
            "state",
            "ACME-T-0001",
            "--branch",
            "acme-t-0001",
            "--commit",
            "abc1234",
            "--pushed",
            "--pr",
            "https://example.com/pr/1",
            "--next",
            "Wait for CI",
        ])
        .expect("state parses");
        match cli.command {
            Command::State(args) => {
                assert_eq!(args.branch.as_deref(), Some("acme-t-0001"));
                assert_eq!(args.commit.as_deref(), Some("abc1234"));
                assert!(args.pushed);
                assert_eq!(args.pr_url.as_deref(), Some("https://example.com/pr/1"));
                assert_eq!(args.next_step.as_deref(), Some("Wait for CI"));
            }
            _ => panic!("expected state"),
        }
        let cli = Cli::try_parse_from(["kairos", "state", "ACME-T-0001", "--next", "Rebase"])
            .expect("state with one field parses");
        match cli.command {
            Command::State(args) => assert!(!args.pushed && args.branch.is_none()),
            _ => panic!("expected state"),
        }
        let cli = Cli::try_parse_from(["kairos", "tasks", "purge", "ACME-T-0001", "--confirm"])
            .expect("tasks purge parses");
        match cli.command {
            Command::Tasks(TasksCommand::Purge(args)) => assert!(args.confirm),
            _ => panic!("expected tasks purge"),
        }
        assert!(
            Cli::try_parse_from(["kairos", "adrs", "purge", "ACME-A-0001", "--confirm"]).is_err(),
            "only a task has purge"
        );
        let cli = Cli::try_parse_from([
            "kairos",
            "boards",
            "archive-completed",
            "acme-delivery",
            "--confirm",
        ])
        .expect("boards archive-completed parses");
        match cli.command {
            Command::Boards(BoardsCommand::ArchiveCompleted { board, confirm, .. }) => {
                assert_eq!(board, "acme-delivery");
                assert!(confirm);
            }
            _ => panic!("expected boards archive-completed"),
        }

        // Documents have no transition endpoint in S-0005 — no verb either.
        assert!(
            Cli::try_parse_from(["kairos", "documents", "transition", "ACME-D-0001"]).is_err(),
            "documents must not offer transition"
        );

        // KAIROS-I-0012: tasks (and only tasks) move between delivery boards.
        let cli = Cli::try_parse_from([
            "kairos",
            "tasks",
            "move",
            "ACME-T-0001",
            "--to-board",
            "web-delivery",
        ])
        .expect("tasks move parses");
        match cli.command {
            Command::Tasks(TasksCommand::Move(args)) => {
                assert_eq!(args.short_code, "ACME-T-0001");
                assert_eq!(args.to_board, "web-delivery");
            }
            _ => panic!("expected tasks move"),
        }
        assert!(
            Cli::try_parse_from(["kairos", "tasks", "move", "ACME-T-0001"]).is_err(),
            "--to-board is required"
        );
        assert!(
            Cli::try_parse_from([
                "kairos",
                "initiatives",
                "move",
                "ACME-I-0001",
                "--to-board",
                "web-delivery"
            ])
            .is_err(),
            "only tasks live on per-team boards"
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
                assert_eq!(service_account.as_deref(), Some("sa-1"));
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

        // The agent keys of a person (KAIROS-T-0359): no --service-account.
        let cli = Cli::try_parse_from(["kairos", "keys", "create", "--name", "laptop"])
            .expect("keys create without a service account parses");
        match cli.command {
            Command::Keys(KeysCommand::Create {
                service_account,
                name,
                expires_at,
                ..
            }) => {
                assert_eq!(service_account, None);
                assert_eq!(name, "laptop");
                assert_eq!(expires_at, None);
            }
            _ => panic!("expected keys create"),
        }
        assert!(Cli::try_parse_from(["kairos", "keys", "list", "--json"]).is_ok());
        let cli = Cli::try_parse_from(["kairos", "keys", "revoke", "k-1", "--confirm"])
            .expect("keys revoke without a service account parses");
        match cli.command {
            Command::Keys(KeysCommand::Revoke {
                key_id,
                service_account,
                confirm,
                ..
            }) => {
                assert_eq!(key_id, "k-1");
                assert_eq!(service_account, None);
                assert!(confirm);
            }
            _ => panic!("expected keys revoke"),
        }
        assert!(Cli::try_parse_from(["kairos", "keys", "create"]).is_err());

        // KAIROS-T-0536: the scope of an agent key.
        let cli = Cli::try_parse_from([
            "kairos",
            "keys",
            "create",
            "--name",
            "ci",
            "--scope",
            "manage_tasks,transition_items",
            "--scope-board",
            "web",
            "--scope-board",
            "ops",
        ])
        .expect("keys create with a scope parses");
        match cli.command {
            Command::Keys(KeysCommand::Create {
                scope,
                read_only,
                scope_board,
                ..
            }) => {
                let request =
                    commands::keys::scope_request(scope, read_only, scope_board).expect("a scope");
                assert_eq!(
                    request.capabilities,
                    Some(vec![
                        "manage_tasks".to_string(),
                        "transition_items".to_string()
                    ])
                );
                assert_eq!(
                    request.boards,
                    Some(vec!["web".to_string(), "ops".to_string()])
                );
            }
            _ => panic!("expected keys create"),
        }
        let cli = Cli::try_parse_from(["kairos", "keys", "create", "--name", "r", "--read-only"])
            .expect("--read-only parses");
        match cli.command {
            Command::Keys(KeysCommand::Create {
                scope,
                read_only,
                scope_board,
                ..
            }) => {
                let request =
                    commands::keys::scope_request(scope, read_only, scope_board).expect("a scope");
                assert_eq!(request.capabilities, Some(Vec::new()));
                assert_eq!(request.boards, None);
            }
            _ => panic!("expected keys create"),
        }
        assert_eq!(commands::keys::scope_request(None, false, Vec::new()), None);
        for args in [
            &[
                "kairos",
                "keys",
                "create",
                "--name",
                "x",
                "--scope",
                "purge",
                "--read-only",
            ][..],
            &[
                "kairos",
                "keys",
                "create",
                "--name",
                "x",
                "--service-account",
                "sa",
                "--scope",
                "purge",
            ][..],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }

    /// KAIROS-T-0388: `admin agent-keys list` takes a user filter and a
    /// page; `revoke` takes the key id and --confirm.
    #[test]
    fn admin_agent_keys_parse() {
        use commands::admin::AgentKeysCommand;
        let cli = Cli::try_parse_from([
            "kairos",
            "admin",
            "agent-keys",
            "list",
            "--user",
            "robin@example.com",
            "--limit",
            "10",
        ])
        .expect("admin agent-keys list parses");
        match cli.command {
            Command::Admin(AdminCommand::AgentKeys(AgentKeysCommand::List { user, list })) => {
                assert_eq!(user.as_deref(), Some("robin@example.com"));
                assert_eq!(list.limit, Some(10));
            }
            _ => panic!("expected admin agent-keys list"),
        }
        let cli = Cli::try_parse_from([
            "kairos",
            "admin",
            "agent-keys",
            "revoke",
            "k-1",
            "--confirm",
        ])
        .expect("admin agent-keys revoke parses");
        match cli.command {
            Command::Admin(AdminCommand::AgentKeys(AgentKeysCommand::Revoke {
                key_id,
                confirm,
                ..
            })) => {
                assert_eq!(key_id, "k-1");
                assert!(confirm);
            }
            _ => panic!("expected admin agent-keys revoke"),
        }
        assert!(Cli::try_parse_from(["kairos", "admin", "agent-keys", "revoke"]).is_err());
    }

    /// KAIROS-T-0590: each verb that deletes a stored object refuses to run
    /// without --confirm. The refusal comes before the client, so the test
    /// needs no deployment: exit 1 (CLI_ERROR), and the text names the flag.
    #[tokio::test]
    async fn destructive_verbs_refuse_without_confirm() {
        let verbs: &[&[&str]] = &[
            &["boards", "columns", "remove", "web", "Todo"],
            &["boards", "transitions", "remove", "web", "Todo", "Active"],
            &["boards", "dispatch", "remove-target", "web", "t1"],
            &["boards", "dispatch", "remove-rule", "web", "r1"],
            &["metadata", "delete", "priority"],
            &["templates", "delete", "bug-report"],
            &["repos", "credential", "remove", "payments"],
            &["repos", "write-token", "remove", "payments"],
            &["pages", "unpublish", "--repo", "payments"],
        ];
        for verb in verbs {
            let args: Vec<&str> = std::iter::once("kairos")
                .chain(verb.iter().copied())
                .collect();
            let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let err = dispatch(cli.command)
                .await
                .expect_err(&format!("{args:?} must refuse without --confirm"));
            assert_eq!(err.exit_code(), crate::error::EXIT_FAILURE, "{args:?}");
            assert_eq!(err.code(), crate::error::CODE_CLI_ERROR, "{args:?}");
            assert!(err.to_string().contains("--confirm"), "{args:?}: {err}");
            let confirmed: Vec<&str> = args.iter().copied().chain(["--confirm"]).collect();
            Cli::try_parse_from(&confirmed).unwrap_or_else(|e| panic!("{confirmed:?}: {e}"));
        }
    }
}
