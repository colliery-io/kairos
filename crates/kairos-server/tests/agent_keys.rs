//! The agent keys of a person (KAIROS-T-0359, KAIROS-A-0024).
//!
//! 1. alice makes an agent key and lists it; the list has no secret.
//! 2. A request with the key is alice: whoami shows alice, she makes and
//!    edits a task on the delivery board of her team, and the task and its
//!    history name alice.
//! 3. A request with the key cannot make another agent key (403).
//! 4. bob does not see the key in his list, and cannot revoke it (404).
//! 5. A service account cannot use the routes (403).
//! 6. alice revokes the key; the key then gets 401; a second revoke is 409.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_agent_keys_t0359_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateTaskRequest, UpdateContentRequest};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_service_accounts::{CreateApiKeyRequest, CreateServiceAccountRequest};
use kairos_client::{EntityKind, Error};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_agent_keys_t0359_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn key_request(name: &str) -> CreateApiKeyRequest {
    CreateApiKeyRequest {
        name: name.into(),
        expires_at: None,
    }
}

#[tokio::test]
async fn a_person_gives_an_agent_their_own_key() {
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
    let server = spawn_server(app::router(app::state_with(
        base_config(&scratch_url),
        pool,
        auth,
    )))
    .await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    for client in [&admin, &alice, &bob] {
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
    let alice_id = user_id(&mut conn, "alice@kairos.test").to_string();
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    admin
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: alice_id.clone(),
            },
        )
        .await
        .expect("team member");
    let board = team.delivery_board_id.clone().expect("delivery board");

    // 1. alice makes a key and lists it.
    let created = alice
        .create_agent_key(&key_request("laptop"))
        .await
        .expect("alice makes an agent key");
    assert!(
        created.key.starts_with("kairos_sk_acme_"),
        "{}",
        created.key
    );
    assert_eq!(created.name, "laptop");
    let listed = alice.list_agent_keys().await.expect("alice lists her keys");
    assert_eq!(listed.total, 1);
    assert_eq!(listed.items[0].id, created.id);
    assert_eq!(listed.items[0].prefix, created.prefix);
    assert!(listed.items[0].revoked_at.is_none());
    let raw = serde_json::to_string(&listed).expect("serialize");
    assert!(!raw.contains(&created.key), "the list shows no secret");

    // 2. The key acts as alice.
    let agent = server.client(&created.key, "acme");
    let me = agent.whoami().await.expect("whoami with the agent key");
    assert_eq!(me.user.id, alice_id);
    assert_eq!(me.user.email, "alice@kairos.test");
    let task = agent
        .create_task(&CreateTaskRequest {
            board_id: Some(board.clone()),
            column_id: None,
            title: "Made by an agent".into(),
            content: "first".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .expect("the agent makes a task on the board of alice");
    assert_eq!(task.created_by, alice_id);
    let edited = agent
        .update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "second".into(),
                version: task.version,
            },
        )
        .await
        .expect("the agent edits the task");
    assert_eq!(edited.updated_by, alice_id);
    let history = alice
        .history(EntityKind::Task, &task.short_code, None, None)
        .await
        .expect("history");
    assert!(!history.items.is_empty());
    for version in &history.items {
        assert_eq!(version.edited_by, alice_id, "the history names alice");
    }

    // 3. An agent key cannot make an agent key.
    match agent.create_agent_key(&key_request("more")).await {
        Err(Error::Forbidden { message, .. }) => {
            assert!(
                message.contains("An agent key cannot make agent keys"),
                "{message}"
            );
        }
        other => panic!("expected 403, got {other:?}"),
    }

    // 4. bob does not see the key and cannot revoke it.
    let bobs = bob.list_agent_keys().await.expect("bob lists his keys");
    assert_eq!(bobs.total, 0);
    match bob.revoke_agent_key(&created.id).await {
        Err(Error::NotFound { .. }) => {}
        other => panic!("expected 404, got {other:?}"),
    }
    assert!(
        alice.list_agent_keys().await.expect("list").items[0]
            .revoked_at
            .is_none(),
        "bob's attempt changed nothing"
    );

    // 5. A service account cannot use the routes.
    let sa = admin
        .create_service_account(&CreateServiceAccountRequest { name: "ci".into() })
        .await
        .expect("service account");
    let sa_key = admin
        .create_api_key(&sa.id, &key_request("ci-main"))
        .await
        .expect("service-account key");
    let machine = server.client(&sa_key.key, "acme");
    machine
        .whoami()
        .await
        .expect("the service account is a member");
    for result in [
        machine
            .create_agent_key(&key_request("nope"))
            .await
            .map(|_| ()),
        machine.list_agent_keys().await.map(|_| ()),
        machine.revoke_agent_key(&created.id).await.map(|_| ()),
    ] {
        match result {
            Err(Error::Forbidden { message, .. }) => {
                assert!(message.contains("A service account cannot have agent keys"));
            }
            other => panic!("expected 403, got {other:?}"),
        }
    }

    // 6. alice revokes the key; it then gets 401.
    let revoked = alice
        .revoke_agent_key(&created.id)
        .await
        .expect("alice revokes her key");
    assert!(revoked.deleted);
    match agent.whoami().await {
        Err(Error::Unauthorized { message, .. }) => {
            assert!(message.contains("POST /api/me/agent-keys"), "{message}");
        }
        other => panic!("expected 401, got {other:?}"),
    }
    match alice.revoke_agent_key(&created.id).await {
        Err(Error::Conflict { .. }) => {}
        other => panic!("expected 409, got {other:?}"),
    }
    let listed = alice.list_agent_keys().await.expect("list");
    assert!(listed.items[0].revoked_at.is_some());
}
