//! Integration test for `/api/repositories` (KAIROS-T-0106, design in
//! KAIROS-I-0010 §D4, decision KAIROS-A-0019): the directory, self-serve
//! registration by the owning team, the admin-only delete with the
//! in-use refusal, and the detail's in-flight links.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_repositories_t0106_test`.
//!
//! Cast: `svc` org admin; `bob` member of `platform`; `alice` member of
//! `web` (so: not platform — cannot register platform's repos).

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
use kairos_client::types_forge::CreateForgeConnectionRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::{CreateRepositoryRequest, UpdateRepositoryRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::forge::auth::github_signature;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_repositories_t0106_test";

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

fn request(slug: Option<&str>, name: &str, team: &str) -> CreateRepositoryRequest {
    CreateRepositoryRequest {
        slug: slug.map(str::to_string),
        forge: "github".into(),
        repo_full_name: name.into(),
        repo_url: format!("https://github.com/{name}"),
        default_branch: None,
        team: team.into(),
        description: Some("Run `cargo test` before every PR.".into()),
    }
}

#[tokio::test]
async fn repository_api_against_live_stack() {
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
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");

    for client in [&svc, &alice, &bob] {
        let _ = client.whoami().await;
    }
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    for (user_id, role) in [
        (svc_id, OrgRole::Admin),
        (alice_id, OrgRole::Member),
        (bob_id, OrgRole::Member),
    ] {
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
            team_type: None,
        })
        .await
        .expect("platform");
    let web = svc
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            team_type: None,
        })
        .await
        .expect("web");
    svc.add_team_member(
        &platform.id,
        &AddTeamMemberRequest {
            user_id: bob_id.to_string(),
        },
    )
    .await
    .expect("bob → platform");
    svc.add_team_member(
        &web.id,
        &AddTeamMemberRequest {
            user_id: alice_id.to_string(),
        },
    )
    .await
    .expect("alice → web");

    // =======================================================================
    // Self-serve registration: the owning team's member, by team slug
    // =======================================================================
    let payments = bob
        .create_repository(&request(
            Some("payments-api"),
            "acme/payments-api",
            "platform",
        ))
        .await
        .expect("a platform member registers platform's repo");
    assert_eq!(payments.slug, "payments-api");
    assert_eq!(payments.team.slug, "platform");
    assert_eq!(
        payments.delivery_board_id.as_deref(),
        platform.delivery_board_id.as_deref()
    );
    assert_eq!(payments.default_branch, "main");
    assert_eq!(payments.open_tasks, 0);
    assert!(!payments.has_webhook);
    // Not a member of platform → 403 (alice is on web).
    let err = rejection(
        alice
            .create_repository(&request(None, "acme/platform-infra", "platform"))
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    // …but alice can register web's, and an admin can register anything.
    let portal = alice
        .create_repository(&request(Some("portal-web"), "acme/portal-web", &web.id))
        .await
        .expect("a web member registers web's repo (by team UUID)");
    assert_eq!(portal.team.id, web.id);
    let infra = svc
        .create_repository(&request(None, "acme/platform-infra", "platform"))
        .await
        .expect("admin registers for platform");
    assert_eq!(infra.slug, "acme-platform-infra", "derived slug");

    // Validation and conflicts.
    let err = rejection(
        svc.create_repository(&request(Some("Bad Slug"), "acme/x", "platform"))
            .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "slug vocabulary: {err}"
    );
    let err = rejection(
        svc.create_repository(&request(Some("payments-api"), "acme/other", "platform"))
            .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "slug taken: {err}");
    let err = rejection(
        svc.create_repository(&request(Some("dup"), "acme/payments-api", "platform"))
            .await,
    );
    assert!(
        matches!(err, Error::Conflict { .. }),
        "forge name taken: {err}"
    );
    let err = rejection(
        svc.create_repository(&request(None, "acme/y", "nope"))
            .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "unknown team: {err}"
    );
    let err = rejection(
        svc.create_repository(&CreateRepositoryRequest {
            forge: "bitbucket".into(),
            ..request(None, "acme/z", "platform")
        })
        .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "unknown forge: {err}"
    );

    // =======================================================================
    // The directory (open tenant-wide) and the detail
    // =======================================================================
    let all = alice.list_repositories(None).await.expect("directory");
    assert_eq!(
        all.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>(),
        ["acme-platform-infra", "payments-api", "portal-web"],
        "by slug"
    );
    let platform_only = alice
        .list_repositories(Some("platform"))
        .await
        .expect("one team's");
    assert_eq!(platform_only.len(), 2);
    let err = rejection(alice.list_repositories(Some("nope")).await);
    assert!(matches!(err, Error::Validation { .. }), "{err}");

    // Counts: a task bound to the repo shows up as open work.
    let task = bob
        .create_task(&CreateTaskRequest {
            board_id: None,
            column_id: None,
            title: "Bound".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository_id: Some("payments-api".into()),
        })
        .await
        .expect("task against the repo");
    let detail = alice.get_repository("payments-api").await.expect("detail");
    assert_eq!(detail.repository.open_tasks, 1);
    assert_eq!(detail.connection_id, None);
    assert!(detail.in_flight.is_empty());
    assert_eq!(
        detail.repository.description,
        "Run `cargo test` before every PR."
    );
    let by_uuid = alice.get_repository(&payments.id).await.expect("by UUID");
    assert_eq!(by_uuid.repository.slug, "payments-api");
    let err = rejection(alice.get_repository("nope").await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Edit: same gate, evaluated on the current owner; re-home
    // =======================================================================
    let err = rejection(
        alice
            .update_repository(
                "payments-api",
                &UpdateRepositoryRequest {
                    description: Some("hijack".into()),
                    ..Default::default()
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let edited = bob
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                default_branch: Some("trunk".into()),
                description: Some("Trunk-based.".into()),
                ..Default::default()
            },
        )
        .await
        .expect("the owner edits");
    assert_eq!(edited.default_branch, "trunk");
    // KAIROS-T-0112: re-homing needs the NEW owner's consent — bob (platform
    // only) cannot push platform's repo onto web; an admin can.
    let err = rejection(
        bob.update_repository(
            "acme-platform-infra",
            &UpdateRepositoryRequest {
                team: Some("web".into()),
                ..Default::default()
            },
        )
        .await,
    );
    assert!(
        matches!(err, Error::Forbidden { .. }),
        "re-home needs manage on both teams: {err}"
    );
    // A task bound to it before the re-home is left STALE (still on
    // platform's board) and the detail says so; binding attributes the team.
    let pre = bob
        .create_task(&CreateTaskRequest {
            board_id: None,
            column_id: None,
            title: "Bound before the re-home".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository_id: Some("acme-platform-infra".into()),
        })
        .await
        .expect("task on infra");
    assert_eq!(pre.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(
        alice
            .get_repository("acme-platform-infra")
            .await
            .expect("detail")
            .stale_tasks,
        0
    );
    let rehomed = svc
        .update_repository(
            "acme-platform-infra",
            &UpdateRepositoryRequest {
                team: Some("web".into()),
                slug: Some("infra".into()),
                ..Default::default()
            },
        )
        .await
        .expect("admin re-homes and renames");
    assert_eq!(rehomed.slug, "infra");
    assert_eq!(rehomed.team.slug, "web");
    let detail = alice.get_repository("infra").await.expect("detail");
    assert_eq!(detail.stale_tasks, 1, "the pre-bound task is now stale");
    // Re-binding that task to the same repo re-checks the rule against ITS
    // board (platform) and refuses: the repo now belongs to web.
    let err = rejection(
        bob.set_task_repository(&pre.short_code, Some("infra"))
            .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    // Unbinding it clears the staleness.
    bob.set_task_repository(&pre.short_code, None)
        .await
        .expect("unbind the stale task");
    assert_eq!(
        alice
            .get_repository("infra")
            .await
            .expect("detail")
            .stale_tasks,
        0
    );
    // A team that still owns repositories cannot be deleted (409, naming them).
    let err = rejection(svc.delete_team(&web.id).await);
    match &err {
        Error::Conflict { message, .. } => assert!(message.contains("infra"), "{message}"),
        other => panic!("expected Conflict, got {other}"),
    }
    // Now alice (web) may edit it and bob (platform) may not.
    alice
        .update_repository(
            "infra",
            &UpdateRepositoryRequest {
                description: Some("ours now".into()),
                ..Default::default()
            },
        )
        .await
        .expect("new owner edits");
    let err = rejection(
        bob.update_repository(
            "infra",
            &UpdateRepositoryRequest {
                description: Some("mine again".into()),
                ..Default::default()
            },
        )
        .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // =======================================================================
    // Webhook connection hangs off the repo; in_flight shows its links
    // =======================================================================
    let connected = svc
        .create_forge_connection(&CreateForgeConnectionRequest {
            repository: "payments-api".into(),
        })
        .await
        .expect("connect");
    let detail = alice.get_repository("payments-api").await.expect("detail");
    assert_eq!(
        detail.connection_id.as_deref(),
        Some(connected.connection.id.as_str())
    );
    assert!(detail.repository.has_webhook);
    // Deliver a PR opened against the bound task.
    let body = serde_json::json!({
        "action": "opened",
        "repository": { "full_name": "acme/payments-api" },
        "pull_request": {
            "number": 7,
            "title": format!("{}: bound work", task.short_code),
            "html_url": "https://github.com/acme/payments-api/pull/7",
            "state": "open",
            "draft": false,
            "merged": false,
            "user": { "login": "bob" },
            "head": { "ref": "feature/bound" },
            "updated_at": "2026-09-22T10:00:00Z",
        },
    })
    .to_string();
    let response = http
        .post(format!(
            "{}{}",
            server.base_url,
            connected
                .webhook_url
                .trim_start_matches("https://kairos.test")
        ))
        .header("x-github-event", "pull_request")
        .header(
            "x-hub-signature-256",
            github_signature(&connected.webhook_secret, body.as_bytes()),
        )
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .expect("webhook delivery");
    assert_eq!(response.status(), 200);
    let detail = alice.get_repository("payments-api").await.expect("detail");
    assert_eq!(detail.in_flight.len(), 1, "{:?}", detail.in_flight);
    assert_eq!(detail.in_flight[0].item_short_code, task.short_code);
    assert_eq!(detail.in_flight[0].external_id, "7");
    // …and it matches the team rollup for this single-repo team.
    let rollup = svc
        .team_links(&platform.id, None)
        .await
        .expect("team rollup");
    assert_eq!(rollup.len(), 1);
    assert_eq!(rollup[0].external_id, detail.in_flight[0].external_id);

    // =======================================================================
    // Delete: admin only; refused while referenced
    // =======================================================================
    let err = rejection(bob.delete_repository("payments-api").await);
    assert!(
        matches!(err, Error::Forbidden { .. }),
        "owner is not enough: {err}"
    );
    let err = rejection(svc.delete_repository("payments-api").await);
    assert!(matches!(err, Error::Conflict { .. }), "referenced: {err}");
    svc.delete_forge_connection(&connected.connection.id)
        .await
        .expect("disconnect");
    let err = rejection(svc.delete_repository("payments-api").await);
    assert!(matches!(err, Error::Conflict { .. }), "still a task: {err}");
    bob.set_task_repository(&task.short_code, None)
        .await
        .expect("unbind the task");
    svc.delete_repository("payments-api")
        .await
        .expect("nothing references it any more");
    let err = rejection(alice.get_repository("payments-api").await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    // The slug and forge name are free again.
    svc.create_repository(&request(
        Some("payments-api"),
        "acme/payments-api",
        "platform",
    ))
    .await
    .expect("re-registering after delete");

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
