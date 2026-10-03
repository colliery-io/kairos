//! The read token of a repository (COLLIERY-T-3105):
//! `/api/repositories/{slug}/credential`, the status on each read surface,
//! the encryption in the database, the access check, and the refusals.
//!
//! The fetch of the builder with the token is in `tests/code_index.rs`
//! (`a_private_repository_is_fetched_with_the_stored_read_token`).
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_repository_credentials_t3105_test`.
//!
//! Cast: `svc` org admin; `bob` member of `platform`, the owner team of the
//! repositories; `alice` member of `web` (no right on them).

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types_auth::Secret;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::CreateRepositoryRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;

const SCRATCH_DB: &str = "kairos_repository_credentials_t3105_test";
const TOKEN: &str = "github_pat_t3105_API_SECRET_42";
/// The token of `missing-repo`. Its URL has it, so that the error of git
/// names it.
const OTHER_TOKEN: &str = "github_pat_t3105_OTHER_SECRET_77";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// A git repository with one commit on `main`, at `dir`.
fn repository_with_a_commit(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@kairos.test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@kairos.test")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "private\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
}

#[derive(QueryableByName)]
struct Text {
    #[diesel(sql_type = diesel::sql_types::Text)]
    text: String,
}

#[derive(QueryableByName)]
struct Bytes {
    #[diesel(sql_type = diesel::sql_types::Bytea)]
    bytes: Vec<u8>,
}

/// Each row of the credentials of the tenant, as JSON text.
fn credential_rows(conn: &mut PgConnection) -> Vec<String> {
    diesel::sql_query(
        "SELECT row_to_json(rc)::text AS text FROM org_acme.repository_credentials rc",
    )
    .load::<Text>(conn)
    .unwrap()
    .into_iter()
    .map(|r| r.text)
    .collect()
}

/// The activity rows of the read tokens: `actor details`.
fn credential_activity(conn: &mut PgConnection) -> Vec<String> {
    diesel::sql_query(
        "SELECT actor_id::text || ' ' || details AS text FROM org_acme.activity_log \
         WHERE details LIKE 'repository_credential%' ORDER BY occurred_at",
    )
    .load::<Text>(conn)
    .unwrap()
    .into_iter()
    .map(|r| r.text)
    .collect()
}

fn assert_no_token(what: &str, text: &str) {
    assert!(!text.contains(TOKEN), "{what} has the token: {text}");
}

/// A session of the MCP endpoint, for the tool `get_repository`.
struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    session_id: String,
}

impl Mcp {
    async fn open(base_url: &str, token: &str) -> Self {
        let http = reqwest::Client::new();
        let url = format!("{base_url}/mcp");
        let response = http
            .post(&url)
            .bearer_auth(token)
            .header("X-Tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "colliery-t3105-test", "version": "0.0.0"},
                },
            }))
            .send()
            .await
            .expect("initialize");
        assert_eq!(response.status(), 200);
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("session id")
            .to_string();
        let mcp = Self {
            http,
            url,
            token: token.to_string(),
            session_id,
        };
        let response = mcp
            .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert_eq!(response.status(), 202);
        mcp
    }

    async fn post(&self, body: Value) -> reqwest::Response {
        self.http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("X-Tenant", "acme")
            .header("mcp-session-id", &self.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&body)
            .send()
            .await
            .expect("mcp request")
    }

    /// The text of a tool call.
    async fn call(&self, tool: &str, arguments: Value) -> String {
        let body = self
            .post(json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await
            .text()
            .await
            .unwrap();
        let message: Value = serde_json::from_str::<Value>(&body)
            .ok()
            .filter(|v| v.get("jsonrpc").is_some())
            .or_else(|| {
                body.lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .find(|v| v.get("jsonrpc").is_some())
            })
            .unwrap_or_else(|| panic!("no JSON-RPC message: {body:?}"));
        message["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text: {message}"))
            .to_string()
    }
}

#[tokio::test]
async fn the_read_token_of_a_repository() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // A private git repository behind basic auth, which takes TOKEN.
    let work = tempfile::tempdir().unwrap();
    repository_with_a_commit(&work.path().join("private"));
    let git_server =
        common::git_http::serve(work.path().to_path_buf(), "x-access-token", TOKEN).await;

    // 3 servers on one database: with a key, with no key, with a
    // different key.
    let key = SecretsKey::from_bytes([7u8; 32]);
    let other_key = SecretsKey::from_bytes([8u8; 32]);
    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let server_with = |secrets_key: Option<SecretsKey>| {
        let mut config = base_config(&scratch_url);
        config.secrets_key = secrets_key;
        app::router(app::state_with(config, pool.clone(), auth.clone()))
    };
    let keyed = spawn_server(server_with(Some(key.clone()))).await;
    let unkeyed = spawn_server(server_with(None)).await;
    let rekeyed = spawn_server(server_with(Some(other_key))).await;
    let svc = keyed.client(&svc_token, "acme");
    let alice = keyed.client(&alice_token, "acme");
    let bob = keyed.client(&bob_token, "acme");
    for client in [&svc, &alice, &bob] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        let user_id = user_id(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (name, slug, prefix, member) in [
        ("Platform", "platform", "PLATFORM", "bob@kairos.test"),
        ("Web", "web", "WEB", "alice@kairos.test"),
    ] {
        let team = svc
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
        svc.add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: user_id(&mut conn, member).to_string(),
            },
        )
        .await
        .expect("team member");
    }
    let repository = |slug: &str, url: String| CreateRepositoryRequest {
        slug: Some(slug.into()),
        forge: "github".into(),
        repo_full_name: format!("acme/{slug}"),
        repo_url: url,
        default_branch: None,
        team: "platform".into(),
        description: None,
    };
    bob.create_repository(&repository(
        "private-repo",
        format!("{}/private/.git", git_server.base_url),
    ))
    .await
    .expect("private-repo");
    // A repository whose URL has its token in it: the error of git names
    // the URL, so the test can see that the token is removed from it.
    bob.create_repository(&repository(
        "missing-repo",
        format!("{}/missing-{OTHER_TOKEN}.git", git_server.base_url),
    ))
    .await
    .expect("missing-repo");
    let path = "/api/repositories/private-repo/credential";

    // =======================================================================
    // AC3: with no KAIROS_SECRETS_KEY, a write is refused, and the refusal
    // names the setting.
    // =======================================================================
    let (status, body) = unkeyed
        .client(&bob_token, "acme")
        .raw_request(Method::PUT, path, Some(&json!({ "token": TOKEN })))
        .await
        .unwrap();
    println!("AC3 PUT with no key: {status} {body}");
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["code"], "SECRETS_NOT_CONFIGURED");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("KAIROS_SECRETS_KEY"),
        "{body}"
    );
    assert_no_token("the refusal", &body.to_string());
    assert!(credential_rows(&mut conn).is_empty());

    // =======================================================================
    // AC5: an unknown field of the body is refused and named.
    // =======================================================================
    let (status, body) = bob
        .raw_request(
            Method::PUT,
            path,
            Some(&json!({ "token": TOKEN, "username": "x-access-token" })),
        )
        .await
        .unwrap();
    println!("AC5 PUT with an unknown field: {status} {body}");
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["details"]["field"], "username", "{body}");
    assert_no_token("the refusal", &body.to_string());
    // A token that is not one word is refused, and named.
    let (status, body) = bob
        .raw_request(Method::PUT, path, Some(&json!({ "token": "two words" })))
        .await
        .unwrap();
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["details"]["field"], "token", "{body}");

    // =======================================================================
    // AC4: a member of a different team, with no admin role, cannot set,
    // remove or check the token. The status is open to read.
    // =======================================================================
    for (method, uri, body) in [
        (
            Method::PUT,
            path.to_string(),
            Some(json!({ "token": TOKEN })),
        ),
        (Method::DELETE, path.to_string(), None),
        (Method::POST, format!("{path}/check"), None),
    ] {
        let (status, response) = alice
            .raw_request(method.clone(), &uri, body.as_ref())
            .await
            .unwrap();
        println!("AC4 alice {method} {uri}: {status}");
        assert_eq!(status, 403, "{method} {uri}: {response}");
    }
    assert!(credential_rows(&mut conn).is_empty());
    let (status, body) = alice.raw_request(Method::GET, path, None).await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(body["set"], false);

    // =======================================================================
    // AC1: the owner team sets the token. The row has ciphertext only.
    // =======================================================================
    let set = bob
        .set_repository_credential("private-repo", &Secret::new(TOKEN))
        .await
        .expect("bob sets the token");
    assert!(set.set);
    assert_eq!(set.set_by.as_deref(), Some(bob_id.to_string().as_str()));
    let rows = credential_rows(&mut conn);
    println!("AC1 the row: {rows:?}");
    assert_eq!(rows.len(), 1);
    assert_no_token("the row", &rows[0]);
    let hex: String = TOKEN.bytes().map(|b| format!("{b:02x}")).collect();
    assert!(!rows[0].contains(&hex), "the row has the token in hex");
    let ciphertext =
        diesel::sql_query("SELECT ciphertext AS bytes FROM org_acme.repository_credentials")
            .get_result::<Bytes>(&mut conn)
            .unwrap()
            .bytes;
    assert!(
        !ciphertext
            .windows(TOKEN.len())
            .any(|w| w == TOKEN.as_bytes()),
        "the ciphertext has the token bytes"
    );
    assert_eq!(ciphertext.len(), TOKEN.len() + 16, "ciphertext and tag");

    // =======================================================================
    // AC2: each read shows set, set_by and set_at, and never the token.
    // =======================================================================
    let (_, credential) = alice.raw_request(Method::GET, path, None).await.unwrap();
    println!("AC2 GET credential: {credential}");
    assert_eq!(credential["set"], true);
    assert_eq!(credential["set_by"], bob_id.to_string());
    assert!(credential["set_at"].is_string());
    assert_no_token("GET credential", &credential.to_string());
    let (_, detail) = alice
        .raw_request(Method::GET, "/api/repositories/private-repo", None)
        .await
        .unwrap();
    assert_eq!(detail["credential"]["set"], true, "{detail}");
    assert_eq!(detail["credential"]["set_by"], bob_id.to_string());
    assert_no_token("GET repository", &detail.to_string());
    let (_, list) = alice
        .raw_request(Method::GET, "/api/repositories", None)
        .await
        .unwrap();
    assert_no_token("GET repositories", &list.to_string());
    let listed = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == "private-repo")
        .unwrap();
    assert_eq!(listed["credential"]["set"], true);
    let mcp = Mcp::open(&keyed.base_url, &alice_token).await;
    let text = mcp
        .call("get_repository", json!({"repository": "private-repo"}))
        .await;
    println!("AC2 MCP get_repository: {text}");
    assert!(text.contains("- read token: set by "), "{text}");
    assert!(text.contains("; not checked"), "{text}");
    assert_no_token("get_repository", &text);
    let text = mcp
        .call("get_repository", json!({"repository": "missing-repo"}))
        .await;
    assert!(text.contains("- read token: not set"), "{text}");

    // A replacement is a second activity row; the check result is cleared.
    bob.set_repository_credential("private-repo", &Secret::new(TOKEN))
        .await
        .expect("bob replaces the token");

    // =======================================================================
    // The access check: git ls-remote with the token.
    // =======================================================================
    let checked = bob
        .check_repository_credential("private-repo")
        .await
        .expect("the check runs");
    println!("check: {checked:?}");
    assert_eq!(checked.last_check_ok, Some(true), "{checked:?}");
    assert!(checked.last_checked_at.is_some());
    let seen = git_server.seen();
    assert!(
        seen.iter()
            .any(|s| s.user.as_deref() == Some("x-access-token")),
        "{seen:?}"
    );
    assert!(seen.iter().all(|s| !s.uri.contains(TOKEN)), "{seen:?}");

    // =======================================================================
    // AC7: when git fails, the stored error has no token.
    // =======================================================================
    svc.set_repository_credential("missing-repo", &Secret::new(OTHER_TOKEN))
        .await
        .expect("svc sets a token");
    let failed = svc
        .check_repository_credential("missing-repo")
        .await
        .expect("the check runs");
    println!("AC7 a failed check: {failed:?}");
    assert_eq!(failed.last_check_ok, Some(false));
    let error = failed.last_check_error.clone().unwrap_or_default();
    assert!(error.contains("git ls-remote failed"), "{error}");
    assert!(error.contains("missing-[token].git"), "{error}");
    assert!(
        !error.contains(OTHER_TOKEN),
        "the stored error has the token: {error}"
    );
    for row in credential_rows(&mut conn) {
        assert_no_token("a row after a failed check", &row);
        assert!(!row.contains(OTHER_TOKEN), "{row}");
    }

    // =======================================================================
    // AC8: a ciphertext moved to a different repository does not decrypt.
    // A changed key gives the "set it again" error.
    // =======================================================================
    diesel::sql_query(
        "UPDATE org_acme.repository_credentials m SET ciphertext = p.ciphertext, \
         nonce = p.nonce, key_id = p.key_id \
         FROM org_acme.repository_credentials p, org_acme.repositories pr, \
              org_acme.repositories mr \
         WHERE pr.slug = 'private-repo' AND p.repository_id = pr.id \
           AND mr.slug = 'missing-repo' AND m.repository_id = mr.id",
    )
    .execute(&mut conn)
    .unwrap();
    let (status, body) = svc
        .raw_request(
            Method::POST,
            "/api/repositories/missing-repo/credential/check",
            None,
        )
        .await
        .unwrap();
    println!("AC8 a moved ciphertext: {status} {body}");
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "CREDENTIAL_UNREADABLE");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not decrypt"),
        "{body}"
    );
    let (status, body) = rekeyed
        .client(&bob_token, "acme")
        .raw_request(Method::POST, &format!("{path}/check"), None)
        .await
        .unwrap();
    println!("AC8 a changed key: {status} {body}");
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("different KAIROS_SECRETS_KEY. Set the token again."),
        "{body}"
    );
    let (status, body) = unkeyed
        .client(&bob_token, "acme")
        .raw_request(Method::POST, &format!("{path}/check"), None)
        .await
        .unwrap();
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no KAIROS_SECRETS_KEY"),
        "{body}"
    );

    // =======================================================================
    // AC9: remove deletes the row. A second remove, and a check, get 404.
    // =======================================================================
    let removed = bob
        .remove_repository_credential("private-repo")
        .await
        .expect("bob removes the token");
    assert!(!removed.set);
    let rows = credential_rows(&mut conn);
    assert_eq!(rows.len(), 1, "only the row of missing-repo: {rows:?}");
    for (method, uri) in [
        (Method::DELETE, path.to_string()),
        (Method::POST, format!("{path}/check")),
    ] {
        let (status, body) = bob.raw_request(method, &uri, None).await.unwrap();
        assert_eq!(status, 404, "{uri}: {body}");
    }

    // The activity rows: the actor, the slug, never the token.
    let activity = credential_activity(&mut conn);
    println!("activity: {activity:#?}");
    assert_eq!(
        activity,
        vec![
            format!("{bob_id} repository_credential_set:private-repo"),
            format!("{bob_id} repository_credential_replaced:private-repo"),
            format!("{svc_id} repository_credential_set:missing-repo"),
            format!("{bob_id} repository_credential_removed:private-repo"),
        ]
    );

    // A removed repository keeps no token.
    svc.delete_repository("missing-repo")
        .await
        .expect("svc removes the repository");
    assert!(credential_rows(&mut conn).is_empty());
}
