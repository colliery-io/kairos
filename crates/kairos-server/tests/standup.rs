//! KAIROS-T-0722 (COLLIERY-I-0663): the stand-up routes.
//!
//! - `GET /api/teams/{id}/standup` gives the team and its people (the db
//!   test `team_work` shows that a service account is not in it). A person
//!   who is not a member and claims a task of the team has a turn, with
//!   `member: false`.
//! - `GET /api/teams/{id}/standup/people/{user_id}` gives the work of a
//!   person in the team: bob reads the work of alice. A claim on the board
//!   of a different team is left out; the claim of the agent key of alice
//!   counts for alice.
//! - `GET /api/teams/{id}/standup/unclaimed` gives the Todo and Blocked
//!   tasks of the team that no one claims.
//! - An unknown team and an unknown person give 404. An unknown query
//!   parameter gives 400.
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
use kairos_client::types_work::WorkItem;
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
async fn the_stand_up_routes_give_the_round_the_lanes_and_the_unclaimed_stop() {
    let db = "kairos_standup_t0722_test";
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

    // Two teams, web and api; alice and bob are in both.
    let mut boards = Vec::new();
    for (name, slug, prefix) in [("Web", "web", "WEB"), ("Api", "api", "API")] {
        let team = svc
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
        for user_id in [alice_id, bob_id] {
            svc.add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
        }
        boards.push(team.delivery_board_id.expect("delivery board"));
    }
    let (web_board, api_board) = (boards[0].clone(), boards[1].clone());
    let web_columns = columns(&svc, &web_board).await;
    let api_columns = columns(&svc, &api_board).await;
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

    // alice claims a web task and an api task; her agent claims a web task.
    let web_claim = task(&alice, &web_board, "web claim").await;
    walk(&alice, &web_columns, &web_claim, &["Todo", "Active"]).await;
    let api_claim = task(&alice, &api_board, "api claim").await;
    walk(&alice, &api_columns, &api_claim, &["Todo", "Active"]).await;
    let agent_claim = task(&alice, &web_board, "agent claim").await;
    walk(&agent, &web_columns, &agent_claim, &["Todo", "Active"]).await;

    // alice owns an initiative with a web task under it.
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
            title: "web initiative".into(),
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
        target_short_code: web_claim.clone(),
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

    // Unclaimed: a Todo web task and a Backlog web task.
    let todo = task(&bob, &web_board, "web todo").await;
    walk(&bob, &web_columns, &todo, &["Todo"]).await;
    let _backlog = task(&bob, &web_board, "web backlog").await;

    // --- the round: by slug, alice and bob.
    let standup = bob.standup("web").await.expect("the stand-up");
    assert_eq!(standup.team.slug, "web");
    assert!(standup.people.iter().all(|p| p.member));
    let mut people: Vec<String> = standup.people.iter().map(|p| p.user_id.clone()).collect();
    people.sort();
    let mut expected = vec![alice_id.to_string(), bob_id.to_string()];
    expected.sort();
    assert_eq!(people, expected, "the people of web: alice and bob");

    // --- the work of alice in web, as bob reads it.
    let work = bob
        .standup_person("web", &alice_id.to_string())
        .await
        .expect("the work of alice");
    assert_eq!(titles(&work.tasks), vec!["agent claim", "web claim"]);
    let agent_card = work
        .tasks
        .iter()
        .find(|t| t.title == "agent claim")
        .expect("the agent claim");
    let claim = agent_card.claim.as_ref().expect("a claim");
    assert!(claim.agent, "the claim of the agent key of alice");
    assert_eq!(titles(&work.initiatives), vec!["web initiative"]);
    assert!(work.tasks.iter().all(|t| t.reasons == vec!["claimed"]));

    // The same person in api: the api claim only. The team by id.
    let api_team = svc.standup("api").await.expect("api").team.id;
    let api_work = bob
        .standup_person(&api_team, &alice_id.to_string())
        .await
        .expect("the work of alice in api");
    assert_eq!(titles(&api_work.tasks), vec!["api claim"]);
    assert!(api_work.initiatives.is_empty());

    // --- the unclaimed stop of web.
    let unclaimed = alice.standup_unclaimed("web").await.expect("unclaimed");
    assert_eq!(titles(&unclaimed.tasks), vec!["web todo"]);
    assert_eq!(unclaimed.tasks[0].state, "todo");
    assert_eq!(unclaimed.tasks[0].reasons, vec!["unclaimed"]);

    let status = |e: kairos_client::Error| e.status();
    // --- svc is not a member of web, and claims a web task: svc gets a
    // turn, marked (Dylan, 2026-10-29).
    let svc_claim = task(&svc, &web_board, "svc claim").await;
    walk(&svc, &web_columns, &svc_claim, &["Todo", "Active"]).await;
    let standup = bob.standup("web").await.expect("the stand-up");
    let outsider = standup
        .people
        .iter()
        .find(|p| p.user_id == ids[0].to_string())
        .expect("svc has a turn");
    assert!(!outsider.member);
    assert_eq!(standup.people.len(), 3);
    let svc_work = bob
        .standup_person("web", &ids[0].to_string())
        .await
        .expect("the work of svc");
    assert_eq!(titles(&svc_work.tasks), vec!["svc claim"]);
    // A padded id is refused, as each unknown input.
    let padded = format!(" {}", ids[0]);
    assert_eq!(
        bob.standup_person("web", &padded)
            .await
            .map_err(status)
            .err(),
        Some(Some(404))
    );

    // --- refusals.
    assert_eq!(
        bob.standup("nope").await.map_err(status).err(),
        Some(Some(404))
    );
    assert_eq!(
        bob.standup_unclaimed("nope").await.map_err(status).err(),
        Some(Some(404))
    );
    for person in [Uuid::new_v4().to_string(), "alice".to_string()] {
        let refused = bob.standup_person("web", &person).await;
        let err = refused.expect_err("an unknown person");
        assert_eq!(err.status(), Some(404), "{person}: {err}");
    }
    let response = http
        .get(format!(
            "{}/api/teams/web/standup?shuffle=1",
            server.base_url
        ))
        .bearer_auth(&bob_token)
        .header("x-tenant", "acme")
        .send()
        .await
        .expect("an unknown parameter");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}
