//! The pages of four lists (KAIROS-T-0584):
//!
//! 1. `GET /api/me/notifications?read_before=`: the next page of the read
//!    notifications. It has no unread one. The pages together have each
//!    read notification once, also when two share the time of the read.
//! 2. `GET /api/boards/{id}/webhook-deliveries?before=`: the next page of
//!    the delivery log.
//! 3. `GET /api/repositories/{slug}/code-indexes/builds?offset=`: the
//!    pages of the runs, with an exact `total`.
//! 4. `GET /api/{family}/{code}/history?limit=&offset=`: the pages of the
//!    versions of an item.
//!
//! Each list: a page past the first, and the last page.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_list_pages_t0584_server_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Uuid as SqlUuid};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateTaskRequest, UpdateContentRequest};
use kairos_client::types_code_index::CodeIndexBuildListQuery;
use kairos_client::types_dispatch::WebhookDeliveriesQuery;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::CreateRepositoryRequest;
use kairos_client::{EntityKind, Error};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_list_pages_t0584_server_test";

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

/// Run `sql` in the schema of the tenant, with `n` and `id` bound as `$1`
/// and `$2`.
fn exec(conn: &mut PgConnection, sql: &str, n: i32, id: Uuid) {
    diesel::sql_query(sql)
        .bind::<Integer, _>(n)
        .bind::<SqlUuid, _>(id)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

#[tokio::test]
async fn each_list_has_pages_to_its_end() {
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
    for client in [&admin, &alice] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
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
    let alice_id = user(&mut conn, "alice@kairos.test");
    let web = admin
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
            &web.id,
            &AddTeamMemberRequest {
                user_id: alice_id.to_string(),
            },
        )
        .await
        .expect("alice in web");
    let board = web.delivery_board_id.clone().expect("board");
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // --- 1. The read notifications ------------------------------------------
    // 125 read ones; each pair shares the time of its read. 2 unread ones.
    for n in 0..125 {
        exec(
            &mut conn,
            "INSERT INTO notifications (user_id, kind, text, created_at, read_at) \
             VALUES ($2, 'mention', 'Read ' || $1, now() - interval '3 hours', \
                     now() - make_interval(secs => ($1 / 2) * 10))",
            n,
            alice_id,
        );
    }
    for n in 0..2 {
        exec(
            &mut conn,
            "INSERT INTO notifications (user_id, kind, text, created_at) \
             VALUES ($2, 'mention', 'Unread ' || $1, now() - make_interval(secs => $1))",
            n,
            alice_id,
        );
    }
    let first = alice.notifications().await.expect("first page");
    assert_eq!(first.unread, 2);
    let reads: Vec<String> = first
        .items
        .iter()
        .filter(|n| n.read_at.is_some())
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(reads.len(), 20, "the 20 newest read ones");
    let second = alice
        .notifications_read_before(reads.last().expect("a read one"))
        .await
        .expect("a page past the first");
    assert_eq!(second.items.len(), 100, "a full page");
    assert!(
        second.items.iter().all(|n| n.read_at.is_some()),
        "no unread one"
    );
    assert_eq!(second.unread, 2);
    let last = alice
        .notifications_read_before(&second.items[99].id)
        .await
        .expect("the last page");
    assert_eq!(last.items.len(), 5, "the last page has the rest");
    let end = alice
        .notifications_read_before(&last.items[4].id)
        .await
        .expect("past the end");
    assert!(end.items.is_empty(), "nothing past the end");
    let mut all: Vec<String> = reads.clone();
    all.extend(second.items.iter().map(|n| n.id.clone()));
    all.extend(last.items.iter().map(|n| n.id.clone()));
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 125, "each read one once, ties too");
    // An unread cursor, both cursors, and an unknown id are refused.
    let unread_id = first
        .items
        .iter()
        .find(|n| n.read_at.is_none())
        .expect("an unread one")
        .id
        .clone();
    let err = alice
        .notifications_read_before(&unread_id)
        .await
        .expect_err("an unread cursor");
    assert!(
        matches!(&err, Error::Validation { message, .. } if message.contains("before")),
        "{err}"
    );
    let err = alice
        .notifications_read_before(&format!("{}&before={unread_id}", reads[0]))
        .await
        .expect_err("both cursors");
    assert!(
        matches!(&err, Error::Validation { message, .. } if message.contains("not both")),
        "{err}"
    );
    let unknown = Uuid::new_v4().to_string();
    let err = alice
        .notifications_read_before(&unknown)
        .await
        .expect_err("an unknown id");
    assert!(
        matches!(&err, Error::NotFound { message, .. } if message.contains(&unknown)),
        "{err}"
    );

    // --- 2. The delivery log -------------------------------------------------
    let board_id = Uuid::parse_str(&board).expect("board id");
    let target = Uuid::new_v4();
    diesel::sql_query(
        "INSERT INTO webhook_targets (id, board_id, name, url, secret_ciphertext, \
             secret_nonce, secret_key_id, created_by) \
         VALUES ($1, $2, 'runner', 'https://hooks.example.com/run', '\\x00', '\\x00', 'k1', $3)",
    )
    .bind::<SqlUuid, _>(target)
    .bind::<SqlUuid, _>(board_id)
    .bind::<SqlUuid, _>(alice_id)
    .execute(&mut conn)
    .expect("a target");
    // 7 deliveries; each pair shares its time.
    for n in 0..7 {
        exec(
            &mut conn,
            "INSERT INTO webhook_deliveries (target_id, event, body, status, created_at) \
             VALUES ($2, 'ping', '{}', 'delivered', now() - make_interval(secs => ($1 / 2) * 10))",
            n,
            target,
        );
    }
    let page = |before: Option<String>| WebhookDeliveriesQuery {
        limit: Some(3),
        before,
        ..Default::default()
    };
    let everything = admin
        .list_webhook_deliveries(&board, &WebhookDeliveriesQuery::default())
        .await
        .expect("the full log");
    assert_eq!(everything.len(), 7);
    let one = admin
        .list_webhook_deliveries(&board, &page(None))
        .await
        .expect("page 1");
    let two = admin
        .list_webhook_deliveries(&board, &page(Some(one[2].id.clone())))
        .await
        .expect("a page past the first");
    let three = admin
        .list_webhook_deliveries(&board, &page(Some(two[2].id.clone())))
        .await
        .expect("the last page");
    assert_eq!((one.len(), two.len(), three.len()), (3, 3, 1));
    let paged: Vec<&str> = one
        .iter()
        .chain(&two)
        .chain(&three)
        .map(|d| d.id.as_str())
        .collect();
    let full: Vec<&str> = everything.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(paged, full, "the pages agree with the full log");
    let past = admin
        .list_webhook_deliveries(&board, &page(Some(three[0].id.clone())))
        .await
        .expect("past the end");
    assert!(past.is_empty());
    let unknown = Uuid::new_v4().to_string();
    let err = admin
        .list_webhook_deliveries(&board, &page(Some(unknown.clone())))
        .await
        .expect_err("an unknown cursor");
    assert!(
        matches!(&err, Error::NotFound { message, .. } if message.contains(&unknown)),
        "{err}"
    );
    let err = admin
        .list_webhook_deliveries(&board, &page(Some("x".into())))
        .await
        .expect_err("a cursor that is not a UUID");
    assert!(matches!(&err, Error::Validation { .. }), "{err}");

    // --- 3. The runs of the code index builder ------------------------------
    let repo = admin
        .create_repository(&CreateRepositoryRequest {
            slug: Some("site".into()),
            forge: "github".into(),
            repo_full_name: "acme/site".into(),
            repo_url: "https://github.com/acme/site".into(),
            default_branch: None,
            team: "web".into(),
            description: None,
        })
        .await
        .expect("repository");
    let repo_id = Uuid::parse_str(&repo.id).expect("repository id");
    diesel::sql_query("DELETE FROM code_index_builds WHERE repository_id = $1")
        .bind::<SqlUuid, _>(repo_id)
        .execute(&mut conn)
        .expect("no run yet");
    // 12 runs; each pair shares its start.
    for n in 0..12 {
        exec(
            &mut conn,
            "INSERT INTO code_index_builds (id, repository_id, trigger, outcome, started_at, \
                 finished_at) \
             VALUES (gen_random_uuid(), $2, 'push', 'ok', \
                     now() - make_interval(secs => ($1 / 2) * 60), now())",
            n,
            repo_id,
        );
    }
    let runs = |offset: Option<i64>| CodeIndexBuildListQuery {
        limit: Some(5),
        offset,
    };
    let full = admin
        .list_code_index_builds("site", &CodeIndexBuildListQuery::default())
        .await
        .expect("each run");
    assert_eq!((full.items.len(), full.total, full.offset), (12, 12, 0));
    let p1 = admin
        .list_code_index_builds("site", &runs(None))
        .await
        .expect("page 1");
    let p2 = admin
        .list_code_index_builds("site", &runs(Some(5)))
        .await
        .expect("a page past the first");
    let p3 = admin
        .list_code_index_builds("site", &runs(Some(10)))
        .await
        .expect("the last page");
    assert_eq!((p1.items.len(), p2.items.len(), p3.items.len()), (5, 5, 2));
    assert_eq!((p2.offset, p2.limit, p2.total), (5, 5, 12));
    let paged: Vec<&str> = [&p1, &p2, &p3]
        .iter()
        .flat_map(|p| p.items.iter().map(|r| r.id.as_str()))
        .collect();
    let all: Vec<&str> = full.items.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(paged, all, "the pages agree with the full list");
    let past = admin
        .list_code_index_builds("site", &runs(Some(12)))
        .await
        .expect("past the end");
    assert!(past.items.is_empty());
    assert_eq!(past.total, 12);

    // --- 4. The history of an item ------------------------------------------
    let task = admin
        .create_task(&CreateTaskRequest {
            board_id: Some(board.clone()),
            column_id: None,
            title: "Pages".into(),
            content: "Version 1.".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("task");
    let mut version = task.version;
    for n in 2..=7 {
        let updated = admin
            .update_task(
                &task.short_code,
                &UpdateContentRequest {
                    title: None,
                    content: format!("Version {n}."),
                    version,
                },
            )
            .await
            .expect("update");
        version = updated.version;
    }
    let versions = |page: &kairos_client::types::ListEnvelope<
        kairos_client::types_meta::HistoryVersion,
    >| page.items.iter().map(|v| v.version).collect::<Vec<_>>();
    let past_first = admin
        .history(EntityKind::Task, &task.short_code, Some(3), Some(3))
        .await
        .expect("a page past the first");
    assert_eq!(versions(&past_first), vec![4, 3, 2]);
    assert_eq!((past_first.total, past_first.offset), (7, 3));
    let last = admin
        .history(EntityKind::Task, &task.short_code, Some(3), Some(6))
        .await
        .expect("the last page");
    assert_eq!(versions(&last), vec![1]);
}
