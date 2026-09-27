//! COLLIERY-T-0216 (COLLIERY-A-0023 rule 2) — the board of a task decides
//! its team, through the typed `kairos_client` against the booted production
//! router: create, the refusal of a team the board does not belong to, move,
//! and binding a repository.
//!
//! The db-layer half (the backfill migration, a teamless board, a repository
//! owned by another team) is `kairos-db/tests/task_team_backfill.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_team_t0216_server_test`.

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
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::enums::Forge;
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, repositories, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_team_t0216_server_test";

#[tokio::test]
async fn the_board_decides_the_team_against_live_stack() {
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

    // JIT-provision svc, then make it org admin.
    let _ = svc.whoami().await;
    let svc_id: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting admin membership");

    let team = |name: &str, slug: &str| CreateTeamRequest {
        name: name.into(),
        slug: slug.into(),
        team_type: None,
    };
    let platform = svc
        .create_team(&team("Platform", "platform"))
        .await
        .expect("platform");
    let web = svc.create_team(&team("Web", "web")).await.expect("web");
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let web_board = web.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");

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
    .expect("platform's repository");

    let base = CreateTaskRequest {
        board_id: Some(platform_board.clone()),
        column_id: None,
        title: "On platform's board".into(),
        content: String::new(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
    };

    // --- create: a board and no team -> the board's team ---------------------
    // Before COLLIERY-T-0216 this stored no team at all: the request named
    // none, and nothing looked at the board.
    let created = svc.create_task(&base).await.expect("create on a board");
    assert_eq!(created.board_id, platform_board);
    assert_eq!(created.team_id.as_deref(), Some(platform.id.as_str()));

    // Naming the board's own team is harmless.
    let named = svc
        .create_task(&CreateTaskRequest {
            team_id: Some(platform.id.clone()),
            ..base.clone()
        })
        .await
        .expect("the board's own team");
    assert_eq!(named.team_id.as_deref(), Some(platform.id.as_str()));

    // Naming any other is refused, and the refusal says who is right.
    let err = svc
        .create_task(&CreateTaskRequest {
            team_id: Some(web.id.clone()),
            ..base.clone()
        })
        .await
        .expect_err("web is not the team of platform's board");
    let message = match &err {
        Error::Validation { message, .. } => message.clone(),
        other => panic!("expected a 422 validation error, got {other}"),
    };
    assert!(
        message.contains(&web.id) && message.contains(&platform_board),
        "names the team that was sent and the board: {message}"
    );
    assert!(
        message.contains("the board decides"),
        "says which of the two wins: {message}"
    );

    // --- bind: a repository leaves the team alone ----------------------------
    let bound = svc
        .set_task_repository(&created.short_code, Some("payments-api"))
        .await
        .expect("binding");
    assert_eq!(
        bound.repository.as_ref().map(|r| r.slug.as_str()),
        Some("payments-api")
    );
    assert_eq!(bound.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(bound.board_id, platform_board);
    let cleared = svc
        .set_task_repository(&created.short_code, None)
        .await
        .expect("clearing");
    assert_eq!(cleared.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(cleared.board_id, platform_board);

    // --- move: the team follows the board ------------------------------------
    let moved = svc
        .move_task(&created.short_code, &web_board)
        .await
        .expect("moving to web's board");
    assert_eq!(moved.board_id, web_board);
    assert_eq!(moved.team_id.as_deref(), Some(web.id.as_str()));

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
