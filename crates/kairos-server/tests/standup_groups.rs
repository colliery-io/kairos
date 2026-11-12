//! KAIROS-T-0764 (COLLIERY-I-0674, KAIROS-A-0084): the routes of the AdHoc
//! stand-up.
//!
//! - Each member creates a group, reads each group and edits each group
//!   (bob edits the group of alice). Only the creator deletes it: bob gets
//!   403 and the group stays; alice deletes it.
//! - The name is unique with no regard to case (409). A service account
//!   and a person who is not a member are refused as people (422).
//! - The stop of a person spans the boards of two teams: the claims of
//!   alice on web and api (the claim of her agent key counts), and her
//!   initiative. Not a task that no one claims: there is no Unclaimed stop.
//!   A person who is not in the group gives 404.
//! - An unknown field of a body gives 422, an unknown query parameter
//!   gives 400.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::KairosClient;
use kairos_client::types::{CreateInitiativeRequest, CreateTaskRequest};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_service_accounts::CreateApiKeyRequest;
use kairos_client::types_work::{CreateStandupGroupRequest, UpdateStandupGroupRequest, WorkItem};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

fn titles(items: &[WorkItem]) -> Vec<&str> {
    let mut out: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    out.sort();
    out
}

async fn task(client: &KairosClient, board: &str, title: &str) -> String {
    client
        .create_task(&CreateTaskRequest {
            board_id: Some(board.to_string()),
            column_id: None,
            title: title.into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("task")
        .short_code
}

async fn walk(client: &KairosClient, columns: &HashMap<String, String>, code: &str, path: &[&str]) {
    for name in path {
        client
            .transition_task(code, &columns[*name])
            .await
            .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
    }
}

async fn columns(client: &KairosClient, board: &str) -> HashMap<String, String> {
    client
        .get_board(board)
        .await
        .expect("board")
        .columns
        .into_iter()
        .map(|c| (c.name, c.id))
        .collect()
}

#[tokio::test]
async fn the_ad_hoc_stand_up_routes_against_live_stack() {
    let db = "kairos_standup_groups_t0764_test";
    let _scratch = recreate_scratch_db(db);
    let scratch_url = with_database(&common::admin_database_url(), db);
    let mut conn = PgConnection::establish(&scratch_url).expect("scratch database");
    run_public_migrations(&mut conn).expect("public migrations");
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
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    for client in [&svc, &alice, &bob] {
        let _ = client.whoami().await;
    }
    let mut ids = Vec::new();
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        let user_id: Uuid = users::table
            .filter(users::email.eq(email))
            .select(users::id)
            .first(&mut conn)
            .expect("user row");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("membership");
        ids.push(user_id);
    }
    let (alice_id, bob_id) = (ids[1], ids[2]);
    let status = |e: kairos_client::Error| e.status();

    // Two teams: alice is in web only, bob in api only.
    let mut boards = Vec::new();
    for (name, slug, prefix, member) in [
        ("Web", "web", "WEB", alice_id),
        ("Api", "api", "API", bob_id),
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
                user_id: member.to_string(),
            },
        )
        .await
        .expect("team member");
        boards.push(team.delivery_board_id.expect("delivery board"));
    }
    let (web_board, api_board) = (boards[0].clone(), boards[1].clone());
    for board in [&web_board, &api_board] {
        for user in [alice_id, bob_id] {
            let _ = svc
                .add_board_member(
                    board,
                    &AddBoardMemberRequest {
                        user_id: user.to_string(),
                        capabilities: vec!["manage_tasks".into(), "transition_items".into()],
                    },
                )
                .await;
        }
    }
    let web_columns = columns(&svc, &web_board).await;
    let api_columns = columns(&svc, &api_board).await;

    // --- create: alice makes a group of alice and bob (bob by email).
    let group = alice
        .create_standup_group(&CreateStandupGroupRequest {
            name: "  Release crew ".into(),
            people: vec![alice_id.to_string(), "bob@kairos.test".into()],
        })
        .await
        .expect("a group");
    assert_eq!(group.name, "Release crew", "the name is trimmed");
    assert_eq!(group.created_by.user_id, alice_id.to_string());
    assert!(group.can_delete);
    let mut people: Vec<String> = group.people.iter().map(|p| p.user_id.clone()).collect();
    people.sort();
    let mut expected = vec![alice_id.to_string(), bob_id.to_string()];
    expected.sort();
    assert_eq!(people, expected);

    // Refusals of a create.
    let taken = bob
        .create_standup_group(&CreateStandupGroupRequest {
            name: "RELEASE CREW".into(),
            people: vec![],
        })
        .await;
    assert_eq!(taken.map_err(status).err(), Some(Some(409)));
    for people in [vec!["svc".to_string()], vec![Uuid::new_v4().to_string()]] {
        let refused = bob
            .create_standup_group(&CreateStandupGroupRequest {
                name: "Other".into(),
                people: people.clone(),
            })
            .await;
        assert_eq!(refused.map_err(status).err(), Some(Some(422)), "{people:?}");
    }
    let blank = bob
        .create_standup_group(&CreateStandupGroupRequest {
            name: "   ".into(),
            people: vec![],
        })
        .await;
    assert_eq!(blank.map_err(status).err(), Some(Some(422)));

    // --- read: bob reads each group and the group of alice.
    let list = bob.standup_groups().await.expect("the list");
    assert_eq!(list.items.len(), 1);
    assert!(!list.items[0].can_delete, "bob did not create it");
    let read = bob.standup_group(&group.id).await.expect("the group");
    assert_eq!(read.name, "Release crew");

    // --- edit: bob renames the group of alice; then alice only.
    let edited = bob
        .update_standup_group(
            &group.id,
            &UpdateStandupGroupRequest {
                name: Some("Launch".into()),
                people: None,
            },
        )
        .await
        .expect("bob edits the group");
    assert_eq!(edited.name, "Launch");
    assert_eq!(edited.people.len(), 2);
    let none = bob
        .update_standup_group(&group.id, &UpdateStandupGroupRequest::default())
        .await;
    assert_eq!(none.map_err(status).err(), Some(Some(422)), "no change");

    // --- the stop of alice spans web and api.
    let agent_key = alice
        .create_agent_key(&CreateApiKeyRequest {
            name: "laptop".into(),
            expires_at: None,
            no_expiry: false,
            scope: None,
        })
        .await
        .expect("agent key")
        .key;
    let agent = server.client(&agent_key, "acme");
    let web_claim = task(&alice, &web_board, "web claim").await;
    walk(&alice, &web_columns, &web_claim, &["Todo", "Active"]).await;
    let api_claim = task(&alice, &api_board, "api claim").await;
    walk(&agent, &api_columns, &api_claim, &["Todo", "Active"]).await;
    let open = task(&bob, &web_board, "web todo, no claim").await;
    walk(&bob, &web_columns, &open, &["Todo"]).await;

    let initiatives = svc.get_board("initiatives").await.expect("initiatives");
    svc.add_board_member(
        &initiatives.board.id,
        &AddBoardMemberRequest {
            user_id: alice_id.to_string(),
            capabilities: vec!["configure_boards".into()],
        },
    )
    .await
    .expect("initiative board member");
    let initiative = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: "initiatives".into(),
            column_id: None,
            title: "api initiative".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
            parent: None,
        })
        .await
        .expect("initiative")
        .short_code;
    svc.create_relationship(&CreateRelationshipRequest {
        source_short_code: initiative.clone(),
        target_short_code: api_claim.clone(),
        relationship: "parent".into(),
    })
    .await
    .expect("parent edge");
    let response = http
        .post(format!(
            "{}/api/initiatives/{initiative}/owner/claim",
            server.base_url
        ))
        .bearer_auth(&alice_token)
        .header("x-tenant", "acme")
        .send()
        .await
        .expect("owner claim");
    assert!(response.status().is_success(), "owner claim");

    let work = bob
        .standup_group_person(&group.id, &alice_id.to_string())
        .await
        .expect("the stop of alice");
    assert_eq!(
        titles(&work.tasks),
        vec!["api claim", "web claim"],
        "the claims of alice on the boards of both teams; not the task that no one claims"
    );
    let agent_card = work
        .tasks
        .iter()
        .find(|t| t.title == "api claim")
        .expect("the agent claim");
    assert!(agent_card.claim.as_ref().is_some_and(|c| c.agent));
    assert_eq!(titles(&work.initiatives), vec!["api initiative"]);
    assert!(
        work.tasks
            .iter()
            .all(|t| !t.reasons.contains(&"unclaimed".to_string())),
        "no Unclaimed stop"
    );

    // A person who is not in the group, and an unknown group: 404.
    let alone = alice
        .update_standup_group(
            &group.id,
            &UpdateStandupGroupRequest {
                name: None,
                people: Some(vec!["alice@kairos.test".into()]),
            },
        )
        .await
        .expect("alice only");
    assert_eq!(alone.people.len(), 1);
    assert_eq!(
        bob.standup_group_person(&group.id, &bob_id.to_string())
            .await
            .map_err(status)
            .err(),
        Some(Some(404))
    );
    assert_eq!(
        bob.standup_group(&Uuid::new_v4().to_string())
            .await
            .map_err(status)
            .err(),
        Some(Some(404))
    );
    assert_eq!(
        bob.standup_group("nope").await.map_err(status).err(),
        Some(Some(404))
    );

    // --- unknown inputs.
    let response = http
        .post(format!("{}/api/standup-groups", server.base_url))
        .bearer_auth(&bob_token)
        .header("x-tenant", "acme")
        .json(&serde_json::json!({"name": "X", "people": [], "team": "web"}))
        .send()
        .await
        .expect("an unknown field");
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let response = http
        .get(format!("{}/api/standup-groups?team=web", server.base_url))
        .bearer_auth(&bob_token)
        .header("x-tenant", "acme")
        .send()
        .await
        .expect("an unknown parameter");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

    // --- delete: bob is refused, alice deletes.
    assert_eq!(
        bob.delete_standup_group(&group.id)
            .await
            .map_err(status)
            .err(),
        Some(Some(403))
    );
    assert!(
        bob.standup_group(&group.id).await.is_ok(),
        "the group stays"
    );
    let deleted = alice
        .delete_standup_group(&group.id)
        .await
        .expect("alice deletes the group");
    assert_eq!(deleted.name, "Launch");
    assert_eq!(
        bob.standup_group(&group.id).await.map_err(status).err(),
        Some(Some(404))
    );
    assert!(
        bob.standup_groups()
            .await
            .expect("the list")
            .items
            .is_empty()
    );
}
