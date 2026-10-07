//! Integration test for the form of the fields of a repository
//! (COLLIERY-T-0267): `repo_full_name`, `repo_url` and `default_branch`.
//!
//! Until COLLIERY-T-0267 the create and the update of a repository examined
//! only the slug. `POST` and `PATCH /api/repositories` accepted an empty
//! value and a blank value for each of the three fields.
//!
//! The test shows four things:
//!
//! - the create and the update refuse a value that does not have the form
//!   of its field, with 422 `VALIDATION` and `details.field`;
//! - the create with no `default_branch` gives `main`;
//! - a repository from before the rule can be read, changed in a different
//!   field, and deleted;
//! - a PATCH that changes no value writes nothing.
//!
//! The rules are in `kairos_core::repositories`, and the unit tests of that
//! module have the full table of values. The MCP tools have the same cases
//! in `tests/mcp.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_repository_fields_t0267_test`.
//!
//! Cast: `svc` org admin; `bob` member of `platform`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Timestamptz, Uuid as SqlUuid};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::KairosClient;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_repository_fields_t0267_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

/// The number of live repositories of the tenant.
fn repository_count(conn: &mut PgConnection) -> i64 {
    sql_query("SELECT count(*) AS count FROM org_acme.repositories WHERE deleted_at IS NULL")
        .get_result::<CountRow>(conn)
        .expect("count of repositories")
        .count
}

/// The number of rows of the activity log about one repository.
fn activity_count(conn: &mut PgConnection, repository: Uuid) -> i64 {
    sql_query("SELECT count(*) AS count FROM org_acme.activity_log WHERE entity_id = $1")
        .bind::<SqlUuid, _>(repository)
        .get_result::<CountRow>(conn)
        .expect("count of activity rows")
        .count
}

/// What a write can change in the row of a repository.
#[derive(QueryableByName, Debug, Clone, PartialEq, Eq)]
struct StoredRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    repo_full_name: String,
    #[diesel(sql_type = Text)]
    repo_url: String,
    #[diesel(sql_type = Text)]
    default_branch: String,
    #[diesel(sql_type = Text)]
    description: String,
    #[diesel(sql_type = SqlUuid)]
    updated_by: Uuid,
    #[diesel(sql_type = Timestamptz)]
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn stored(conn: &mut PgConnection, slug: &str) -> StoredRow {
    sql_query(
        "SELECT id, repo_full_name, repo_url, default_branch, description, updated_by, \
                updated_at \
           FROM org_acme.repositories WHERE slug = $1 AND deleted_at IS NULL",
    )
    .bind::<Text, _>(slug)
    .get_result(conn)
    .unwrap_or_else(|e| panic!("the row of {slug}: {e}"))
}

/// The body of a correct create, with `changes` on top of it. A `null` in
/// `changes` removes the field.
fn create_body(name: &str, changes: Value) -> Value {
    let mut body = json!({
        "forge": "github",
        "repo_full_name": format!("acme/{name}"),
        "repo_url": format!("https://github.com/acme/{name}"),
        "team": "platform",
    });
    for (key, value) in changes.as_object().expect("an object") {
        body[key] = value.clone();
    }
    body
}

/// The refusal must be 422 `VALIDATION` about `field`, with a text.
fn assert_refusal(status: u16, body: &Value, field: &str, case: &str) -> String {
    assert_eq!(status, 422, "{case}: {body}");
    assert_eq!(body["error"]["code"], "VALIDATION", "{case}: {body}");
    assert_eq!(
        body["error"]["details"],
        json!({ "field": field }),
        "{case}: {body}"
    );
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("{case}: no message in {body}"))
        .to_string();
    assert!(
        message.starts_with(&format!("The {field}")),
        "{case}: {message}"
    );
    message
}

async fn post(client: &KairosClient, body: &Value) -> (u16, Value) {
    client
        .raw_request(Method::POST, "/api/repositories", Some(body))
        .await
        .expect("POST /api/repositories")
}

async fn patch(client: &KairosClient, slug: &str, body: &Value) -> (u16, Value) {
    client
        .raw_request(
            Method::PATCH,
            &format!("/api/repositories/{slug}"),
            Some(body),
        )
        .await
        .expect("PATCH /api/repositories/{slug}")
}

#[tokio::test]
async fn repository_fields_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let config = base_config(&scratch_url);
    let router = app::router(app::state_with(config, pool.clone(), auth.clone()));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");
    let bob = server.client(&bob_token, "acme");

    for client in [&svc, &bob] {
        let _ = client.whoami().await;
    }
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    for (user_id, role) in [(svc_id, OrgRole::Admin), (bob_id, OrgRole::Member)] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let platform = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            code_prefix: "PLATFORM".into(),
            team_type: None,
        })
        .await
        .expect("platform");
    svc.add_team_member(
        &platform.id,
        &AddTeamMemberRequest {
            user_id: bob_id.to_string(),
        },
    )
    .await
    .expect("bob → platform");

    // =======================================================================
    // Create: each field, with a value that does not have its form
    // =======================================================================
    // (field, value, a second change to the body)
    let refused: &[(&str, &str, Value)] = &[
        ("repo_full_name", "", json!({"slug": "empty-name"})),
        ("repo_full_name", "   ", json!({"slug": "blank-name"})),
        ("repo_full_name", " acme/spaces", json!({})),
        ("repo_full_name", "acme/spaces ", json!({})),
        ("repo_full_name", "acme/my repo", json!({})),
        ("repo_full_name", "acme/fidius.git", json!({})),
        ("repo_full_name", "fidius", json!({})),
        ("repo_full_name", "acme/portal/web", json!({})),
        ("repo_full_name", "/acme/fidius", json!({})),
        ("repo_full_name", "acme//fidius", json!({})),
        ("repo_full_name", "portal", json!({"forge": "gitlab"})),
        (
            "repo_full_name",
            "",
            json!({"forge": "other", "slug": "x1"}),
        ),
        ("repo_url", "", json!({})),
        ("repo_url", "   ", json!({})),
        ("repo_url", " https://github.com/acme/fidius", json!({})),
        ("repo_url", "https://github.com/acme/fidius ", json!({})),
        ("repo_url", "https://github.com/acme/my repo", json!({})),
        ("repo_url", "github.com/acme/fidius", json!({})),
        ("repo_url", "git@github.com:acme/fidius.git", json!({})),
        ("repo_url", "ftp://github.com/acme/fidius", json!({})),
        ("repo_url", "https://", json!({})),
        ("repo_url", "https:///acme/fidius", json!({})),
        (
            "repo_url",
            "https://alice:s3cret@github.com/acme/fidius",
            json!({}),
        ),
        (
            "repo_url",
            "https://alice@github.com/acme/fidius",
            json!({}),
        ),
        ("default_branch", "", json!({})),
        ("default_branch", "  ", json!({})),
        ("default_branch", " main", json!({})),
        ("default_branch", "main ", json!({})),
        ("default_branch", "my branch", json!({})),
        ("default_branch", "-main", json!({})),
        ("default_branch", "release..1", json!({})),
        ("default_branch", "main/", json!({})),
        ("default_branch", "main.lock", json!({})),
    ];
    for (field, value, more) in refused {
        let case = format!("create with {field} = {value:?}");
        let mut body = create_body("fidius", more.clone());
        body[*field] = json!(value);
        // A member of the owner team and an organization admin get the
        // same refusal.
        for client in [&bob, &svc] {
            let (status, response) = post(client, &body).await;
            let message = assert_refusal(status, &response, field, &case);
            // No refusal shows a password.
            assert!(!message.contains("s3cret"), "{case}: {message}");
        }
    }
    assert_eq!(
        repository_count(&mut conn),
        0,
        "a refused create writes nothing"
    );

    // The texts.
    let text_of = |response: &Value| response["error"]["message"].as_str().map(str::to_string);
    let (_, response) = post(&bob, &create_body("fidius", json!({"repo_full_name": ""}))).await;
    assert_eq!(
        text_of(&response).as_deref(),
        Some(
            "The repo_full_name is empty. For the forge github, the name has 2 parts, for \
             example acme/payments-api."
        )
    );
    let (_, response) = post(
        &bob,
        &create_body(
            "fidius",
            json!({"forge": "gitlab", "repo_full_name": "portal"}),
        ),
    )
    .await;
    assert_eq!(
        text_of(&response).as_deref(),
        Some(
            "The repo_full_name \"portal\" has 1 part. For the forge gitlab, the name has 2 \
             or more parts, for example acme/portal/web."
        )
    );
    let (_, response) = post(
        &bob,
        &create_body(
            "fidius",
            json!({"repo_url": "https://alice:s3cret@github.com/acme/fidius"}),
        ),
    )
    .await;
    assert_eq!(
        text_of(&response).as_deref(),
        Some(
            "The repo_url has a user name or a password in it. Each member of the \
             organization can read the URL. Remove the user name and the password."
        )
    );
    let (_, response) = post(&bob, &create_body("fidius", json!({"repo_url": ""}))).await;
    assert_eq!(
        text_of(&response).as_deref(),
        Some(
            "The repo_url is empty. Send a value, for example \
             https://github.com/acme/payments-api."
        )
    );
    let (_, response) = post(
        &bob,
        &create_body("fidius", json!({"default_branch": " main"})),
    )
    .await;
    assert_eq!(
        text_of(&response).as_deref(),
        Some(
            "The default_branch \" main\" has a space at its start or at its end. Remove the \
             spaces."
        )
    );

    // =======================================================================
    // Create: the values that the rule accepts
    // =======================================================================
    // No default_branch, and a null: the default branch is main.
    let (status, fidius) = post(&bob, &create_body("fidius", json!({}))).await;
    assert_eq!(status, 201, "{fidius}");
    assert_eq!(fidius["default_branch"], "main");
    let (status, null_branch) = post(
        &bob,
        &create_body("null-branch", json!({"default_branch": null})),
    )
    .await;
    assert_eq!(status, 201, "{null_branch}");
    assert_eq!(null_branch["default_branch"], "main");
    // GitLab with a subgroup, and the forge `other` with one part.
    let (status, portal) = post(
        &bob,
        &create_body(
            "portal",
            json!({
                "forge": "gitlab",
                "repo_full_name": "acme/portal/web",
                "repo_url": "https://gitlab.com/acme/portal/web",
                "default_branch": "release/2026.09",
            }),
        ),
    )
    .await;
    assert_eq!(status, 201, "{portal}");
    assert_eq!(portal["slug"], "acme-portal-web");
    assert_eq!(portal["default_branch"], "release/2026.09");
    let (status, site) = post(
        &svc,
        &create_body(
            "site",
            json!({
                "forge": "other",
                "repo_full_name": "site",
                "repo_url": "http://git.acme.example:8080/site",
            }),
        ),
    )
    .await;
    assert_eq!(status, 201, "{site}");
    assert_eq!(site["slug"], "site");
    assert_eq!(repository_count(&mut conn), 4);

    // =======================================================================
    // Update: a new value must have the form of its field
    // =======================================================================
    let before = stored(&mut conn, "acme-fidius");
    let activity_before = activity_count(&mut conn, before.id);
    for (field, value, _) in refused
        .iter()
        .filter(|(field, _, _)| *field != "repo_full_name")
    {
        let case = format!("update with {field} = {value:?}");
        for client in [&bob, &svc] {
            let (status, response) = patch(client, "acme-fidius", &json!({ *field: value })).await;
            assert_refusal(status, &response, field, &case);
        }
        // A correct field in the same body does not go to the row.
        let (status, response) = patch(
            &bob,
            "acme-fidius",
            &json!({ *field: value, "description": "Not written." }),
        )
        .await;
        assert_refusal(status, &response, field, &case);
    }
    assert_eq!(
        stored(&mut conn, "acme-fidius"),
        before,
        "a refused update writes nothing"
    );
    assert_eq!(activity_count(&mut conn, before.id), activity_before);
    // The full name is not a field of the update.
    let (status, response) =
        patch(&bob, "acme-fidius", &json!({"repo_full_name": "acme/new"})).await;
    assert_eq!(status, 422, "{response}");
    assert_eq!(
        response["error"]["details"]["allowed"],
        json!([
            "slug",
            "repo_url",
            "default_branch",
            "team",
            "description",
            "code_index_build",
            "code_index_summaries"
        ]),
        "{response}"
    );

    // A correct change.
    let (status, response) = patch(
        &bob,
        "acme-fidius",
        &json!({
            "repo_url": "https://github.com/acme/fidius-rs",
            "default_branch": "trunk",
        }),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let changed = stored(&mut conn, "acme-fidius");
    assert_eq!(changed.repo_url, "https://github.com/acme/fidius-rs");
    assert_eq!(changed.default_branch, "trunk");
    assert!(changed.updated_at > before.updated_at);
    assert_eq!(activity_count(&mut conn, before.id), activity_before + 1);

    // =======================================================================
    // A PATCH that changes no value writes nothing
    // =======================================================================
    // svc sends the values of the row. bob made the last write, and the row
    // keeps him.
    let before = stored(&mut conn, "acme-fidius");
    assert_eq!(before.updated_by, bob_id);
    let activity_before = activity_count(&mut conn, before.id);
    let same: &[Value] = &[
        json!({"description": ""}),
        json!({"default_branch": "trunk"}),
        json!({"repo_url": "https://github.com/acme/fidius-rs"}),
        json!({"slug": "acme-fidius"}),
        json!({"team": "platform"}),
        json!({"team": platform.id}),
        // Each field, as the GUI sends them.
        json!({
            "slug": "acme-fidius",
            "repo_url": "https://github.com/acme/fidius-rs",
            "default_branch": "trunk",
            "team": "platform",
            "description": "",
        }),
    ];
    for body in same {
        let (status, response) = patch(&svc, "acme-fidius", body).await;
        assert_eq!(status, 200, "{body}: {response}");
        // The response is the repository, as for each success.
        assert_eq!(response["slug"], "acme-fidius", "{body}");
        assert_eq!(response["id"], json!(before.id), "{body}");
        assert_eq!(response["default_branch"], "trunk", "{body}");
        assert_eq!(
            response["updated_at"],
            json!(before.updated_at.to_rfc3339()),
            "{body}"
        );
        assert_eq!(stored(&mut conn, "acme-fidius"), before, "{body}");
        assert_eq!(
            activity_count(&mut conn, before.id),
            activity_before,
            "{body}"
        );
    }
    // A body with no field. Before COLLIERY-T-0267 the response was 200,
    // and the server wrote `updated_at`, `updated_by` and an activity row.
    // It is a refusal now, as for a board, a team and a stream.
    let (status, response) = patch(&svc, "acme-fidius", &json!({})).await;
    assert_eq!(status, 422, "{response}");
    assert_eq!(response["error"]["code"], "VALIDATION", "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no field to change")),
        "{response}"
    );
    assert_eq!(stored(&mut conn, "acme-fidius"), before);
    assert_eq!(activity_count(&mut conn, before.id), activity_before);
    // One value is the same and one is different: the write has the
    // different one.
    let (status, response) = patch(
        &svc,
        "acme-fidius",
        &json!({"default_branch": "trunk", "description": "Run the tests first."}),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let after = stored(&mut conn, "acme-fidius");
    assert_eq!(after.description, "Run the tests first.");
    assert_eq!(after.updated_by, svc_id);
    assert!(after.updated_at > before.updated_at);
    assert_eq!(activity_count(&mut conn, before.id), activity_before + 1);

    // =======================================================================
    // A repository from before the rule
    // =======================================================================
    // The values of this row do not pass the rule: a full name with a space
    // and `.git`, an empty URL, a blank default branch.
    let old_id = Uuid::new_v4();
    sql_query(
        "INSERT INTO org_acme.repositories \
            (id, slug, forge, repo_full_name, repo_url, default_branch, team_id, \
             description, created_by, updated_by) \
         VALUES ($1, 'old-one', 'github', 'Old One.git', '', ' ', $2, '', $3, $3)",
    )
    .bind::<SqlUuid, _>(old_id)
    .bind::<SqlUuid, _>(platform.id.parse::<Uuid>().expect("team id"))
    .bind::<SqlUuid, _>(svc_id)
    .execute(&mut conn)
    .expect("the old row");

    // Read: the list and the detail.
    let (status, list) = bob
        .raw_request(Method::GET, "/api/repositories", None)
        .await
        .expect("list");
    assert_eq!(status, 200, "{list}");
    let listed = list
        .as_array()
        .expect("a list")
        .iter()
        .find(|r| r["slug"] == "old-one")
        .unwrap_or_else(|| panic!("old-one is not in {list}"));
    assert_eq!(listed["repo_url"], "");
    let (status, detail) = bob
        .raw_request(Method::GET, "/api/repositories/old-one", None)
        .await
        .expect("detail");
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["repo_full_name"], "Old One.git");
    assert_eq!(detail["repo_url"], "");
    assert_eq!(detail["default_branch"], " ");

    // Change a different field.
    let (status, response) = patch(
        &bob,
        "old-one",
        &json!({"description": "Ask the platform team."}),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["description"], "Ask the platform team.");
    assert_eq!(response["repo_url"], "");
    // The GUI sends each field, with the old values.
    let (status, response) = patch(
        &bob,
        "old-one",
        &json!({
            "slug": "old-one",
            "repo_url": "",
            "default_branch": " ",
            "team": "platform",
            "description": "Ask the platform team first.",
        }),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let row = stored(&mut conn, "old-one");
    assert_eq!(row.description, "Ask the platform team first.");
    assert_eq!(row.repo_url, "");
    assert_eq!(row.default_branch, " ");
    assert_eq!(row.repo_full_name, "Old One.git");
    // A new value must have the form, and the old value of the other field
    // does not stop the change.
    let (status, response) = patch(&bob, "old-one", &json!({"repo_url": " "})).await;
    assert_refusal(status, &response, "repo_url", "old-one to a blank URL");
    let (status, response) = patch(
        &bob,
        "old-one",
        &json!({"repo_url": "https://github.com/acme/old-one", "default_branch": " "}),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let row = stored(&mut conn, "old-one");
    assert_eq!(row.repo_url, "https://github.com/acme/old-one");
    assert_eq!(row.default_branch, " ");
    // Then the old URL does not come back.
    let (status, response) = patch(&bob, "old-one", &json!({"repo_url": ""})).await;
    assert_refusal(status, &response, "repo_url", "old-one to its old URL");

    // =======================================================================
    // COLLIERY-T-0265: the refusal of a slug names the field
    // =======================================================================
    // The GUI shows a refusal with `details.field` below that field.
    let count_before = repository_count(&mut conn);
    for slug in ["under_score", "Upper", "k", "-leading"] {
        let body = create_body("slug-case", json!({"slug": slug}));
        let (status, response) = post(&bob, &body).await;
        assert_eq!(status, 422, "create with the slug {slug:?}: {response}");
        assert_eq!(response["error"]["code"], "VALIDATION", "{response}");
        assert_eq!(
            response["error"]["details"],
            json!({"field": "slug"}),
            "create with the slug {slug:?}: {response}"
        );
        assert_eq!(
            response["error"]["message"],
            format!(
                "The repository slug {slug:?} is not correct. A repository slug must match \
                 ^[a-z0-9][a-z0-9-]{{1,62}}$."
            ),
        );
        let (status, response) = patch(&bob, "old-one", &json!({"slug": slug})).await;
        assert_eq!(status, 422, "update to the slug {slug:?}: {response}");
        assert_eq!(
            response["error"]["details"],
            json!({"field": "slug"}),
            "update to the slug {slug:?}: {response}"
        );
    }
    assert_eq!(repository_count(&mut conn), count_before);
    assert_eq!(stored(&mut conn, "old-one").repo_full_name, "Old One.git");

    // Delete (an organization admin).
    let (status, response) = svc
        .raw_request(Method::DELETE, "/api/repositories/old-one", None)
        .await
        .expect("delete");
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["deleted"], true);
    assert_eq!(repository_count(&mut conn), 4);

    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
