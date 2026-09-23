//! Integration test for the KAIROS-I-0012 team lifecycle: moving a task to
//! another delivery board (`POST /api/tasks/{code}/move`) and deleting a
//! team once its board holds no live cards.
//!
//! The two halves are one story: a team disbands, its live work moves to
//! another team (or is deleted), and only then does the team go. Before
//! I-0012 a soft-deleted card blocked the delete forever — that rule is
//! what this test pins in its new form.
//!
//! Cast: `svc` is the org admin; `alice` holds `manage_tasks` on platform
//! only (the two-sided rule's negative case) and then on both.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_move_i0012_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{AddBoardMemberRequest, CreateTeamRequest};
use kairos_db::models::enums::Forge;
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, repositories, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_move_i0012_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// A typed 422 (`SAME_BOARD`, `NOT_DELIVERY_BOARD`, …): the client keeps
/// codes other than `VALIDATION`/`INVALID_TRANSITION` in `Error::Other`.
fn unprocessable(err: &Error, expected_code: &str) -> String {
    match err {
        Error::Other {
            status,
            code,
            message,
            ..
        } if *status == 422 && code == expected_code => message.clone(),
        Error::Validation { message, .. } if expected_code == "VALIDATION" => message.clone(),
        other => panic!("expected a 422 {expected_code}, got {other}"),
    }
}

#[tokio::test]
async fn task_move_and_team_deletion_against_live_stack() {
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
    let alice_token = user_token(&http, "alice").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");

    let _ = svc.whoami().await;
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting svc admin membership");
    let _ = alice.whoami().await;
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice_id,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("granting alice membership");

    // Two teams with their delivery boards; a third that will be disbanded.
    let platform = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await
        .expect("platform team");
    let web = svc
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            team_type: None,
        })
        .await
        .expect("web team");
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let web_board = web.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");

    // ========================================================================
    // Two-sided ABAC: manage_tasks on BOTH boards (or org admin)
    // ========================================================================
    let task = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(platform_board.clone()),
            column_id: None,
            title: "Bulk invoice export".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .expect("creating the task");

    // No grants at all → refused on the SOURCE board.
    let err = rejection(alice.move_task(&task.short_code, &web_board).await);
    assert!(
        matches!(err, Error::Forbidden { .. }),
        "a plain member may not move work: {err}"
    );

    // Grant alice manage_tasks on platform only → still refused, now on the
    // TARGET: moving work onto another team's board is their call too.
    alice_grant(&svc, &platform_board, &alice_id.to_string()).await;
    let err = rejection(alice.move_task(&task.short_code, &web_board).await);
    assert!(
        matches!(err, Error::Forbidden { .. }),
        "one-sided power is not enough: {err}"
    );

    // Grant the target as well → allowed.
    alice_grant(&svc, &web_board, &alice_id.to_string()).await;
    let moved = alice
        .move_task(&task.short_code, &web_board)
        .await
        .expect("both sides granted");
    assert_eq!(moved.board_id, web_board);
    assert_eq!(moved.team_id.as_deref(), Some(web.id.as_str()));
    let web_detail = svc.get_board(&web_board).await.expect("web board");
    let entry = web_detail.columns.first().expect("columns").id.clone();
    assert_eq!(moved.column_id, entry, "lands in the entry column");

    // The board is addressable by slug too, and an org admin needs no grants.
    let back = svc
        .move_task(&task.short_code, "platform-delivery")
        .await
        .expect("admin moves it back by slug");
    assert_eq!(back.board_id, platform_board);

    // ========================================================================
    // Refusals
    // ========================================================================
    let err = rejection(svc.move_task(&task.short_code, &platform_board).await);
    assert!(
        unprocessable(&err, "SAME_BOARD").contains("already on board"),
        "{err}"
    );
    let err = rejection(svc.move_task(&task.short_code, "initiatives").await);
    assert!(
        unprocessable(&err, "NOT_DELIVERY_BOARD").contains("not a delivery board"),
        "{err}"
    );
    let err = rejection(svc.move_task(&task.short_code, "no-such-board").await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // The KAIROS-T-0104 rule: a bound task lives on its repository's owner's
    // board, and the refusal says where it may go instead.
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    repositories::create(
        &mut conn,
        NewRepository {
            slug: "payments-api".into(),
            forge: Forge::Github,
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: "main".into(),
            team_id: platform_id,
            description: String::new(),
            created_by: svc_id,
            updated_by: svc_id,
        },
    )
    .expect("registering payments-api");
    svc.set_task_repository(&task.short_code, Some("payments-api"))
        .await
        .expect("binding the repository");
    let err = rejection(svc.move_task(&task.short_code, &web_board).await);
    let message = unprocessable(&err, "REPOSITORY_OWNER_MISMATCH");
    assert!(message.contains("payments-api"), "{message}");
    assert!(
        message.contains(&platform_board),
        "the refusal names the board it may go to: {message}"
    );
    svc.set_task_repository(&task.short_code, None)
        .await
        .expect("unbinding");

    // The repository guard (KAIROS-T-0112) fires BEFORE the board guard, so
    // a disbanding team re-homes or retires its repositories first.
    let err = rejection(svc.delete_team(&platform.id).await);
    assert!(
        matches!(err, Error::Conflict { .. }),
        "owned repositories block the team first: {err}"
    );
    svc.delete_repository("payments-api")
        .await
        .expect("retiring the repository");

    // ========================================================================
    // Team deletion: refused while a LIVE card is on the board, naming it
    // ========================================================================
    let err = rejection(svc.delete_team(&platform.id).await);
    let message = unprocessable(&err, "BOARD_NOT_EMPTY");
    assert!(
        message.contains(&task.short_code),
        "names the card: {message}"
    );
    assert!(
        message.contains("live card"),
        "says what is in the way: {message}"
    );

    // Moving the card away clears the board …
    svc.move_task(&task.short_code, &web_board)
        .await
        .expect("moving the last card away");
    svc.delete_team(&platform.id)
        .await
        .expect("an empty board lets the team go");

    // … and so does deleting it (the "archived" half of the rule): a
    // soft-deleted card no longer blocks the team, which before I-0012 made
    // every team that had ever held work permanent.
    let leftover = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            column_id: None,
            title: "Retired work".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .expect("creating a task on web");
    let err = rejection(svc.delete_team(&web.id).await);
    assert!(
        unprocessable(&err, "BOARD_NOT_EMPTY").contains(&leftover.short_code),
        "{err}"
    );
    svc.delete_task(&leftover.short_code)
        .await
        .expect("deleting it");
    svc.delete_task(&task.short_code)
        .await
        .expect("deleting the moved one too");
    svc.delete_team(&web.id)
        .await
        .expect("deleted cards do not block the team");

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}

/// Grant alice `manage_tasks` on one board (org admin does the granting).
async fn alice_grant(svc: &kairos_client::KairosClient, board_id: &str, alice_id: &str) {
    svc.add_board_member(
        board_id,
        &AddBoardMemberRequest {
            user_id: alice_id.to_string(),
            capabilities: vec!["manage_tasks".into()],
        },
    )
    .await
    .expect("granting manage_tasks");
}
