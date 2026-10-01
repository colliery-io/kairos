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
//! Exit codes (KAIROS-A-0015): 0 success · 1 API/validation error ·
//! 2 auth error.

mod commands;
mod context;
mod credentials;
mod error;
mod index;
mod oidc;
mod password;
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
use commands::repos::ReposCommand;
use commands::search::SearchArgs;
use commands::service_accounts::ServiceAccountsCommand;
use commands::streams::StreamsCommand;
use commands::teams::TeamsCommand;
use credentials::{CredentialStore, DeploymentCredentials, unix_now};
use error::CliError;
use index::IndexCommand;
use kairos_client::types_auth::LoginRequest;
use kairos_client::types_org::WhoamiResponse;
use kairos_client::{Error as ApiError, KairosClient};
use provider::CachedTokenProvider;

#[derive(Parser)]
#[command(
    name = "kairos",
    version,
    about = "The Kairos command-line interface",
    long_about = "The Kairos command-line interface (KAIROS-A-0015).\nThe exit codes are: 0 for \
         success, 1 for an error of the API or of the input, 2 for an error of the login."
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
        printf '%s' \"$PASSWORD\" | kairos login --url <URL> --email <EMAIL>")]
    Login {
        /// Deployment base URL (e.g. https://kairos.example.com)
        #[arg(long)]
        url: String,
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
    /// Repositories: the codebases that tasks link to (COLLIERY-A-0023)
    #[command(subcommand)]
    Repos(ReposCommand),
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
    /// The code index of a checkout, and its code tools for an agent (COLLIERY-T-1852)
    #[command(subcommand)]
    Index(IndexCommand),
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
            email: Some(email),
            tenant,
            ..
        } => login_local(&url, &email, tenant).await,
        Command::Login {
            url,
            email: None,
            issuer,
            tenant,
            client_id,
            bearer,
        } => login(&url, issuer.as_deref(), tenant, client_id, bearer).await,
        Command::Logout { url } => logout(url.as_deref()).await,
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
        Command::Repos(command) => command.run().await,
        Command::Streams(command) => command.run().await,
        Command::Members(command) => command.run().await,
        Command::ServiceAccounts(command) => command.run().await,
        Command::Keys(command) => command.run().await,
        Command::Admin(command) => command.run().await,
        Command::Index(command) => command.run().await,
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
    let mut store = credentials::load(&path)?;
    if store.deployments.is_empty() {
        println!("The cache has no credentials. The CLI changed nothing.");
        return Ok(());
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
    credentials::save(&path, &store)?;

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
            serde_json::to_string_pretty(&identity).map_err(|err| CliError::Failure(format!(
                "The CLI cannot write the JSON: {err}."
            )))?
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
                email,
                issuer,
                tenant,
                client_id,
                bearer,
            } => {
                assert_eq!(email, None);
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
