//! A claim ends when its person leaves, and a stale claim is reported
//! (KAIROS-T-0531, COLLIERY-I-0640):
//!
//! 1. `DELETE /api/members/{user_id}` ends each claim of the person with
//!    the reason `left`. The task stays in Active, free. The members of
//!    the team of the board get `claim_dropped`; the admin does not.
//! 2. The SCIM deprovision (PATCH `active: false`) does the same.
//! 3. The periodic pass `stale-claims` runs as a job. It tells the holder
//!    of a claim with no write for 14 days, and the owner of the initiative
//!    of the task (`claim_stale`). The claim stays. A claim with a recent
//!    state change is not stale.
//! 4. The report comes once. A write after the report starts a new quiet
//!    period, and a new report comes after it. A fresh write stops the
//!    report. `0` days reports nothing.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_claim_leave_and_stale_t0531_test`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateInitiativeRequest, CreateTaskRequest, SetTaskStateRequest};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_notifications::UpdateNotificationSettingsRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{EntityKind, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::blocking::BlockingTenantPool;
use kairos_server::jobs::{JobSettings, Worker, kinds};
use kairos_server::middleware::auth::Authenticator;
use kairos_server::periodic;

const SCRATCH_DB: &str = "kairos_claim_leave_and_stale_t0531_test";
const PATCH_URN: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

#[derive(QueryableByName, Debug)]
struct Note {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Text)]
    text: String,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// The notifications after the first `seen`, as (person, kind, text),
/// sorted by person.
fn new_notes(conn: &mut PgConnection, seen: &mut usize) -> Vec<(Uuid, String, String)> {
    let rows: Vec<Note> = diesel::sql_query(
        "SELECT user_id, kind, text FROM org_acme.notifications ORDER BY created_at, id",
    )
    .load(conn)
    .expect("notifications");
    let mut new: Vec<(Uuid, String, String)> = rows
        .into_iter()
        .skip(*seen)
        .map(|n| (n.user_id, n.kind, n.text))
        .collect();
    *seen += new.len();
    new.sort();
    new
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
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
        })
        .await
        .expect("task")
        .short_code
}

/// The task goes Backlog -> Todo -> Active with `client`, who gets the
/// claim.
async fn to_active(client: &KairosClient, code: &str, columns: &HashMap<String, String>) {
    for name in ["Todo", "Active"] {
        client
            .transition_task(code, &columns[name])
            .await
            .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
    }
}

/// Run the pass `stale-claims` once as a periodic job, with `days`.
async fn run_pass_as_job(url: &str, days: u32) {
    let worker = Arc::new(Worker::new(
        BlockingTenantPool::new(url, 8),
        Uuid::new_v4(),
        JobSettings {
            heartbeat: Duration::from_millis(100),
            lease: Duration::from_millis(600),
            max_tries: 3,
            poll: Duration::from_millis(20),
            retry_after: Duration::ZERO,
            ..JobSettings::default()
        },
    ));
    worker
        .add_periodic(
            periodic::STALE_CLAIMS,
            periodic::STALE_CLAIMS_INTERVAL,
            periodic::stale_claims_pass(url.to_string(), days),
        )
        .await
        .expect("add")
        .expect("the first run");
    worker.drain().await;
}

/// Make the claim and the task of `code` `days` days old.
fn age(conn: &mut PgConnection, code: &str, days: i32) {
    for sql in [
        "UPDATE org_acme.task_claims SET claimed_at = now() - make_interval(days => $2) \
         WHERE task_id = (SELECT id FROM org_acme.tasks WHERE short_code = $1)",
        "UPDATE org_acme.tasks SET updated_at = now() - make_interval(days => $2) \
         WHERE short_code = $1",
    ] {
        diesel::sql_query(sql)
            .bind::<Text, _>(code)
            .bind::<diesel::sql_types::Integer, _>(days)
            .execute(conn)
            .expect("age");
    }
}

fn pass_now(conn: &mut PgConnection, days: u32) -> u64 {
    kairos_db::task_claims::stale_claims_pass(conn, days, chrono::Utc::now(), &mut || {})
        .expect("pass")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_claim_ends_when_its_person_leaves_and_a_stale_claim_is_reported() {
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
    let carol_token = user_token(&http, "carol").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");
    for client in [&admin, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
    ] {
        let user_id = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let svc_id = user(&mut conn, "svc@kairos.test");
    let alice_id = user(&mut conn, "alice@kairos.test");
    let bob_id = user(&mut conn, "bob@kairos.test");
    let carol_id = user(&mut conn, "carol@kairos.test");

    let web = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team web");
    for user_id in [alice_id, bob_id, carol_id] {
        admin
            .add_team_member(
                &web.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let board = web.delivery_board_id.clone().expect("web board");
    // KAIROS-T-0641: the tasks walk to Active under an initiative that
    // stays in Discovery.
    common::parent_gate_off(&admin, &board).await;
    let detail = admin.get_board(&board).await.expect("board");
    let columns: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let mut notes = 0usize;

    // --- 1. REST: an admin removes alice -----------------------------------
    let a1 = task(&alice, &board, "Alice works this").await;
    to_active(&alice, &a1, &columns).await;
    let before = alice.get_task(&a1).await.expect("a1");
    assert_eq!(
        before.claim.as_ref().map(|c| c.user_id.clone()),
        Some(alice_id.to_string())
    );
    let _ = new_notes(&mut conn, &mut notes);
    admin
        .remove_org_member(&alice_id.to_string())
        .await
        .expect("remove alice");
    let after = bob.get_task(&a1).await.expect("a1 after");
    assert!(after.claim.is_none(), "the claim ended: {after:?}");
    assert_eq!(
        after.column_id, columns["Active"],
        "the task stays in Active"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM org_acme.activity_log WHERE action = 'release' \
                 AND details = 'release:{alice_id} reason:left'"
            )
        ),
        1
    );
    let told = new_notes(&mut conn, &mut notes);
    let mut people: Vec<Uuid> = told.iter().map(|(u, _, _)| *u).collect();
    let mut want = vec![bob_id, carol_id];
    want.sort();
    people.sort();
    assert_eq!(people, want, "the team is told, not the admin: {told:?}");
    for (_, kind, text) in &told {
        assert_eq!(kind, "claim_dropped");
        assert!(
            text.contains(&a1) && text.contains("left the organization"),
            "{text}"
        );
        assert!(text.contains("free in Active"), "{text}");
    }
    // The task is free: bob takes it with a claim.
    bob.claim_task(&a1).await.expect("bob claims the free task");

    // --- 2. SCIM: bob is deprovisioned -------------------------------------
    let b1 = task(&bob, &board, "Bob works this").await;
    to_active(&bob, &b1, &columns).await;
    let response = http
        .post(format!("{}/api/scim-tokens", server.base_url))
        .bearer_auth(&svc_token)
        .header("x-tenant", "acme")
        .json(&json!({"name": "idp"}))
        .send()
        .await
        .expect("scim token");
    assert_eq!(response.status(), 201);
    let body: serde_json::Value = response.json().await.expect("token body");
    let scim_token = body["token"].as_str().expect("token").to_string();
    let _ = new_notes(&mut conn, &mut notes);
    let response = http
        .patch(format!("{}/scim/v2/Users/{bob_id}", server.base_url))
        .bearer_auth(&scim_token)
        .header("content-type", "application/scim+json")
        .body(
            json!({
                "schemas": [PATCH_URN],
                "Operations": [{"op": "replace", "value": {"active": false}}],
            })
            .to_string(),
        )
        .send()
        .await
        .expect("deprovision bob");
    assert!(response.status().is_success(), "{}", response.status());
    for code in [&a1, &b1] {
        let task = carol.get_task(code).await.expect("task");
        assert!(task.claim.is_none(), "bob's claim on {code} ended");
        assert_eq!(task.column_id, columns["Active"]);
    }
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM org_acme.activity_log WHERE action = 'release' \
                 AND details = 'release:{bob_id} reason:left'"
            )
        ),
        2
    );
    let told = new_notes(&mut conn, &mut notes);
    assert_eq!(told.len(), 2, "carol is told for each task: {told:?}");
    for (person, kind, _) in &told {
        assert_eq!((person, kind.as_str()), (&carol_id, "claim_dropped"));
    }

    // --- 3. The pass as a job: a stale claim is reported --------------------
    let initiative = admin
        .create_initiative(&CreateInitiativeRequest {
            board_id: "initiatives".into(),
            column_id: None,
            title: "Web work".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("initiative")
        .short_code;
    diesel::sql_query(
        "INSERT INTO org_acme.item_owners (item_id, user_id) \
         SELECT id, $1 FROM org_acme.initiatives WHERE short_code = $2",
    )
    .bind::<SqlUuid, _>(svc_id)
    .bind::<Text, _>(&initiative)
    .execute(&mut conn)
    .expect("svc owns the initiative");
    let c1 = task(&carol, &board, "Carol forgot this").await;
    let c2 = task(&carol, &board, "Carol works this").await;
    admin
        .create_relationship(&CreateRelationshipRequest {
            source_short_code: initiative.clone(),
            target_short_code: c1.clone(),
            relationship: "parent".into(),
        })
        .await
        .expect("parent");
    to_active(&carol, &c1, &columns).await;
    to_active(&carol, &c2, &columns).await;
    age(&mut conn, &c1, 20);
    carol
        .set_task_state(
            &c2,
            &SetTaskStateRequest {
                next_step: Some(Some("Finish the page".into())),
                ..Default::default()
            },
        )
        .await
        .expect("state");
    age(&mut conn, &c2, 20);
    // The settings show the two kinds, on.
    let settings = carol.notification_settings().await.expect("settings");
    assert!(settings.claim_dropped && settings.claim_stale);
    let _ = new_notes(&mut conn, &mut notes);

    run_pass_as_job(&scratch_url, 14).await;
    let told = new_notes(&mut conn, &mut notes);
    assert_eq!(told.len(), 2, "the holder and the owner: {told:?}");
    let holder = told.iter().find(|n| n.0 == carol_id).expect("carol told");
    assert_eq!(holder.1, "claim_stale");
    assert!(
        holder
            .2
            .starts_with(&format!("Your claim on {c1} has no write")),
        "{}",
        holder.2
    );
    let owner = told.iter().find(|n| n.0 == svc_id).expect("the owner told");
    assert_eq!(owner.1, "claim_stale");
    assert!(
        owner.2.contains(&c1) && owner.2.contains("initiative that you own"),
        "{}",
        owner.2
    );
    assert!(
        !told.iter().any(|n| n.2.contains(&c2)),
        "c2 has a new state"
    );
    let still = carol.get_task(&c1).await.expect("c1");
    assert_eq!(
        still.claim.map(|c| c.user_id),
        Some(carol_id.to_string()),
        "the claim stays"
    );
    let jobs: Vec<String> = kairos_db::jobs::list(&mut conn, kinds::PERIODIC)
        .expect("jobs")
        .into_iter()
        .filter(|j| j.dedupe_key.as_deref() == Some(periodic::key(periodic::STALE_CLAIMS).as_str()))
        .map(|j| j.state)
        .collect();
    assert_eq!(jobs, vec!["done", "queued"], "the run adds the next run");

    // --- 4. Once, a new period, a fresh write, 0 days -----------------------
    assert_eq!(pass_now(&mut conn, 14), 0, "the report comes once");
    diesel::sql_query(
        "UPDATE org_acme.task_claims SET stale_notified_at = now() - interval '30 days' \
         WHERE task_id = (SELECT id FROM org_acme.tasks WHERE short_code = $1)",
    )
    .bind::<Text, _>(&c1)
    .execute(&mut conn)
    .expect("an old report");
    assert_eq!(
        pass_now(&mut conn, 14),
        1,
        "a write after the report re-arms it"
    );
    let told = new_notes(&mut conn, &mut notes);
    assert_eq!(told.len(), 2, "{told:?}");
    carol
        .append_note(EntityKind::Task, &c1, "Still on it.")
        .await
        .expect("note");
    diesel::sql_query(
        "UPDATE org_acme.task_claims SET stale_notified_at = NULL \
         WHERE task_id = (SELECT id FROM org_acme.tasks WHERE short_code = $1)",
    )
    .bind::<Text, _>(&c1)
    .execute(&mut conn)
    .expect("no report");
    assert_eq!(pass_now(&mut conn, 14), 0, "a fresh note is a write");
    age(&mut conn, &c1, 20);
    assert_eq!(pass_now(&mut conn, 0), 0, "0 days turns the pass off");
    // A person who turned the kind off is not told.
    carol
        .update_notification_settings(&UpdateNotificationSettingsRequest {
            claim_stale: Some(false),
            ..Default::default()
        })
        .await
        .expect("turn claim_stale off");
    let _ = new_notes(&mut conn, &mut notes);
    assert_eq!(pass_now(&mut conn, 14), 1);
    let told = new_notes(&mut conn, &mut notes);
    assert_eq!(
        told.iter().map(|n| n.0).collect::<Vec<_>>(),
        vec![svc_id],
        "only the owner: {told:?}"
    );
}
