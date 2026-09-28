//! Integration test for task ↔ repository binding (KAIROS-T-0104, design
//! in KAIROS-I-0010 §D2, decision KAIROS-A-0019): create-time routing,
//! `PUT …/repository`, and the repository filter on board items and search
//! — through the typed `kairos_client` against the booted production
//! router.
//!
//! The rule is COLLIERY-A-0023, which amends A-0019 (COLLIERY-T-0217): a
//! board or a team chooses where a task goes, and the repository is an
//! optional link to ANY live repository. Until then this file pinned the
//! opposite: a repository alone routed the task to its owner's delivery
//! board, and a board, a team or a `PUT …/repository` that disagreed with
//! the owner was refused. Each of those cases is still here, with the new
//! outcome.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_repos_t0104_test`.
//!
//! Repositories are created through the db layer here: the repository
//! API is KAIROS-T-0106's, and this test is about what a task does with
//! one. Cast: `svc` (org admin) does everything — capability edges are
//! KAIROS-T-0105's concern.

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
use kairos_client::types_repositories::UpdateRepositoryRequest;
use kairos_client::types_search::{SearchFilter, SearchRequest};
use kairos_db::models::enums::Forge;
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, repositories, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_repos_t0104_test";

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

fn validation_message(err: &Error) -> String {
    match err {
        Error::Validation { message, .. } => message.clone(),
        other => panic!("expected a 422 validation error, got {other}"),
    }
}

fn new_repo(slug: &str, name: &str, team: Uuid, actor: Uuid) -> NewRepository {
    NewRepository {
        slug: slug.to_string(),
        forge: Forge::Github,
        repo_full_name: name.to_string(),
        repo_url: format!("https://github.com/{name}"),
        default_branch: "main".to_string(),
        team_id: team,
        description: String::new(),
        created_by: actor,
        updated_by: actor,
    }
}

#[tokio::test]
async fn task_repository_binding_against_live_stack() {
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
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting admin membership");

    // Two teams, each with its delivery board; one repo per team.
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
    let platform_board = platform
        .delivery_board_id
        .clone()
        .expect("teams are created with a delivery board");
    let web_board = web
        .delivery_board_id
        .clone()
        .expect("teams are created with a delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    let web_id: Uuid = web.id.parse().expect("uuid");

    // Pin the direct connection to the tenant for the db-layer setup.
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let payments = repositories::create(
        &mut conn,
        new_repo("payments-api", "acme/payments-api", platform_id, svc_id),
    )
    .expect("payments repo");
    let infra = repositories::create(
        &mut conn,
        new_repo("platform-infra", "acme/platform-infra", platform_id, svc_id),
    )
    .expect("infra repo");
    let portal = repositories::create(
        &mut conn,
        new_repo("portal-web", "acme/portal-web", web_id, svc_id),
    )
    .expect("portal repo");

    let base = CreateTaskRequest {
        board_id: None,
        column_id: None,
        title: "Routed".into(),
        content: String::new(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
    };

    // =======================================================================
    // A repository alone does not choose a board (COLLIERY-T-0217)
    // =======================================================================
    // Until COLLIERY-T-0217 this request landed on platform's delivery
    // board, because platform owns payments-api.
    let err = rejection(
        svc.create_task(&CreateTaskRequest {
            repository: Some("payments-api".into()),
            ..base.clone()
        })
        .await,
    );
    let message = validation_message(&err);
    assert!(
        message.contains("board") && message.contains("team"),
        "the refusal names both arguments that are missing: {message}"
    );
    assert!(
        message.contains("does not choose a board"),
        "and says why the repository was not enough: {message}"
    );
    // The pre-rename wire field (KAIROS-T-0115) is not a field of the route
    // any more (COLLIERY-T-0259). The refusal names it.
    let (status, body) = svc
        .raw_request(
            reqwest::Method::POST,
            "/api/tasks",
            Some(&serde_json::json!({ "title": "Aliased", "repository_id": "portal-web" })),
        )
        .await
        .expect("raw create");
    assert_eq!(status, 422, "repository_id alone: {body}");
    assert_eq!(body["error"]["details"]["field"], "repository_id", "{body}");
    // Nothing at all is the same refusal.
    let err = rejection(svc.create_task(&base).await);
    let message = validation_message(&err);
    assert!(
        message.contains("board") && message.contains("team"),
        "{message}"
    );

    // =======================================================================
    // A team and no board → the delivery board of that team
    // =======================================================================
    let by_team = svc
        .create_task(&CreateTaskRequest {
            team_id: Some(platform.id.clone()),
            ..base.clone()
        })
        .await
        .expect("a team names its delivery board");
    assert_eq!(by_team.board_id, platform_board);
    assert_eq!(by_team.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(by_team.repository_id, None);
    // The team chooses the board and the repository is only carried along:
    // web's board, platform's repository. Until COLLIERY-T-0217 this was
    // refused, because web is not the owner of payments-api.
    let team_and_repo = svc
        .create_task(&CreateTaskRequest {
            team_id: Some(web.id.clone()),
            repository: Some("payments-api".into()),
            ..base.clone()
        })
        .await
        .expect("a team, and a repository of another team");
    assert_eq!(team_and_repo.board_id, web_board);
    assert_eq!(team_and_repo.team_id.as_deref(), Some(web.id.as_str()));
    assert_eq!(
        team_and_repo.repository_id.as_deref(),
        Some(payments.id.to_string().as_str())
    );
    // A team that does not exist has no delivery board to name.
    let err = rejection(
        svc.create_task(&CreateTaskRequest {
            team_id: Some(Uuid::new_v4().to_string()),
            ..base.clone()
        })
        .await,
    );
    assert!(validation_message(&err).contains("delivery board"), "{err}");

    // =======================================================================
    // A board and a repository: the board decides, the repository is a link
    // =======================================================================
    let routed = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(platform_board.clone()),
            repository: Some("payments-api".into()),
            ..base.clone()
        })
        .await
        .expect("a board and a repository of the board's own team");
    assert_eq!(routed.board_id, platform_board);
    assert_eq!(routed.team_id.as_deref(), Some(platform.id.as_str()));
    assert_eq!(
        routed.repository_id.as_deref(),
        Some(payments.id.to_string().as_str())
    );
    let embedded = routed
        .repository
        .as_ref()
        .expect("the repository ref is embedded on create");
    assert_eq!(embedded.slug, "payments-api");
    assert_eq!(embedded.repo_full_name, "acme/payments-api");
    assert_eq!(embedded.team_id, platform.id);

    // By UUID works too.
    let by_uuid = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            repository: Some(portal.id.to_string()),
            ..base.clone()
        })
        .await
        .expect("repo by UUID");
    assert_eq!(by_uuid.board_id, web_board);
    // The pre-rename wire field (KAIROS-T-0115) is refused, and the refusal
    // gives the name that the route accepts (COLLIERY-T-0259).
    let (status, body) = svc
        .raw_request(
            reqwest::Method::POST,
            "/api/tasks",
            Some(&serde_json::json!({
                "title": "Aliased",
                "board_id": web_board,
                "repository_id": "portal-web",
            })),
        )
        .await
        .expect("raw create");
    assert_eq!(status, 422, "repository_id on create: {body}");
    assert_eq!(body["error"]["code"], "VALIDATION", "{body}");
    assert_eq!(body["error"]["details"]["field"], "repository_id", "{body}");
    assert!(
        body["error"]["details"]["allowed"]
            .as_array()
            .is_some_and(|allowed| allowed.iter().any(|name| name == "repository")),
        "the refusal gives `repository` as a field of the route: {body}"
    );
    assert_eq!(by_uuid.team_id.as_deref(), Some(web.id.as_str()));

    // The repository of a DIFFERENT team. Until COLLIERY-T-0217 this was a
    // 422 that named platform's board as the only one allowed. Now the task
    // is on the board that was named, with that board's team, and it links
    // to a repository that team does not own.
    let cross = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(web_board.clone()),
            repository: Some("payments-api".into()),
            ..base.clone()
        })
        .await
        .expect("web's board, platform's repository");
    assert_eq!(cross.board_id, web_board, "the task is on the named board");
    assert_eq!(
        cross.team_id.as_deref(),
        Some(web.id.as_str()),
        "with the team of that board, not the owner of the repository"
    );
    let embedded = cross.repository.as_ref().expect("the link");
    assert_eq!(embedded.slug, "payments-api");
    assert_eq!(
        embedded.team_id, platform.id,
        "the repository keeps its owner"
    );
    let agreeing = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(platform_board.clone()),
            repository: Some("payments-api".into()),
            ..base.clone()
        })
        .await
        .expect("repo + its own delivery board agree");
    assert_eq!(agreeing.board_id, platform_board);

    // =======================================================================
    // No repository: a board, behaviour unchanged
    // =======================================================================
    let plain = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(platform_board.clone()),
            ..base.clone()
        })
        .await
        .expect("repo-less create is unchanged");
    assert_eq!(plain.repository_id, None);
    assert_eq!(plain.repository, None);

    // Unknown repository → 422. The board is named, so the refusal is about
    // the repository and not about a missing board.
    let err = rejection(
        svc.create_task(&CreateTaskRequest {
            board_id: Some(platform_board.clone()),
            repository: Some("nope".into()),
            ..base.clone()
        })
        .await,
    );
    assert!(validation_message(&err).contains("nope"), "{err}");

    // =======================================================================
    // PUT /repository: set, change, another team's, unknown, clear
    // =======================================================================
    let bound = svc
        .set_task_repository(&plain.short_code, Some("payments-api"))
        .await
        .expect("binding a repo-less task on the owner's board");
    assert_eq!(
        bound.repository.as_ref().map(|r| r.slug.as_str()),
        Some("payments-api")
    );
    assert_eq!(
        bound.version, plain.version,
        "routing fields never bump the version"
    );
    let rehomed = svc
        .set_task_repository(&plain.short_code, Some(&infra.id.to_string()))
        .await
        .expect("re-homing to another repo of the same team");
    assert_eq!(
        rehomed.repository.as_ref().map(|r| r.slug.as_str()),
        Some("platform-infra")
    );
    // A repository of another team. Until COLLIERY-T-0217 this was refused:
    // the repository had to belong to the team of the task's board. It is a
    // link now, and setting it changes nothing else.
    let other_team = svc
        .set_task_repository(&plain.short_code, Some("portal-web"))
        .await
        .expect("a repository owned by another team");
    assert_eq!(
        other_team.repository.as_ref().map(|r| r.slug.as_str()),
        Some("portal-web")
    );
    assert_eq!(
        other_team.board_id, platform_board,
        "the board does not change"
    );
    assert_eq!(
        other_team.team_id.as_deref(),
        Some(platform.id.as_str()),
        "the team does not change"
    );
    assert_eq!(other_team.column_id, plain.column_id);
    // The one 422 left: a repository that does not exist.
    let err = rejection(
        svc.set_task_repository(&plain.short_code, Some("nope"))
            .await,
    );
    assert!(validation_message(&err).contains("nope"), "{err}");
    let cleared = svc
        .set_task_repository(&plain.short_code, None)
        .await
        .expect("clearing");
    assert_eq!(cleared.repository_id, None);
    assert_eq!(cleared.repository, None);
    assert_eq!(
        cleared.board_id, platform_board,
        "clearing leaves the board"
    );
    assert_eq!(
        cleared.team_id.as_deref(),
        Some(platform.id.as_str()),
        "clearing leaves the team"
    );
    let err = rejection(
        svc.set_task_repository("ACME-T-9999", Some("payments-api"))
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // The activity log records the binding changes without item_history.
    {
        use kairos_db::schema::activity_log;
        let plain_id: Uuid = plain.id.parse().expect("uuid");
        let rows: i64 = activity_log::table
            .filter(activity_log::entity_id.eq(plain_id))
            .filter(activity_log::action.eq("repository"))
            .count()
            .get_result(&mut conn)
            .expect("activity rows");
        assert_eq!(rows, 4, "set, re-home, another team's, clear");
    }

    // =======================================================================
    // Reads carry the embedded ref: get, list, board items, search
    // =======================================================================
    let fetched = svc.get_task(&routed.short_code).await.expect("get");
    assert_eq!(
        fetched.repository.as_ref().map(|r| r.slug.as_str()),
        Some("payments-api")
    );
    let listed = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("list");
    let listed_routed = listed
        .items
        .iter()
        .find(|t| t.short_code == routed.short_code)
        .expect("routed task in the list");
    assert_eq!(
        listed_routed.repository.as_ref().map(|r| r.slug.as_str()),
        Some("payments-api")
    );

    // Board items: every task carries its ref; the filter narrows tasks only.
    let items = svc.board_items(&platform_board).await.expect("board items");
    let platform_tasks: Vec<_> = items.columns.iter().flat_map(|c| c.tasks.iter()).collect();
    assert_eq!(
        platform_tasks.len(),
        4,
        "by_team, routed, agreeing, plain: {platform_tasks:?}"
    );
    assert!(
        platform_tasks
            .iter()
            .filter(|t| t.repository_id.is_some())
            .all(|t| t.repository.is_some()),
        "every bound task carries the embedded ref"
    );
    let filtered = svc
        .board_items_for_repository(&platform_board, "payments-api")
        .await
        .expect("filtered board items");
    let filtered_tasks: Vec<_> = filtered
        .columns
        .iter()
        .flat_map(|c| c.tasks.iter())
        .collect();
    assert_eq!(
        filtered_tasks.len(),
        2,
        "routed + agreeing: {filtered_tasks:?}"
    );
    let by_uuid_filter = svc
        .board_items_for_repository(&platform_board, &payments.id.to_string())
        .await
        .expect("filter by UUID");
    assert_eq!(
        by_uuid_filter
            .columns
            .iter()
            .map(|c| c.tasks.len())
            .sum::<usize>(),
        2
    );
    let err = rejection(
        svc.board_items_for_repository(&platform_board, "nope")
            .await,
    );
    assert!(
        matches!(err, Error::Validation { .. }),
        "unknown slug → 422: {err}"
    );

    // Search: `filter.repository` is a task-level filter taking a slug or a
    // UUID (KAIROS-T-0115), resolved in the handler like board items.
    let hits = svc
        .search(&SearchRequest {
            q: None,
            filter: Some(SearchFilter {
                repository: Some("payments-api".into()),
                ..Default::default()
            }),
            traverse: None,
            sort: None,
            limit: None,
            offset: None,
        })
        .await
        .expect("search by repository");
    // Four, on two boards: the repository filter follows the link, not the
    // board. `team_and_repo` and `cross` are on web's board.
    assert_eq!(hits.results.tasks.len(), 4, "{:?}", hits.results.tasks);
    assert!(
        hits.results
            .tasks
            .iter()
            .all(|t| t.repository.as_ref().map(|r| r.slug.as_str()) == Some("payments-api")),
        "search hits carry the embedded ref"
    );
    let by_uuid = svc
        .search(&SearchRequest {
            q: None,
            filter: Some(SearchFilter {
                repository: Some(payments.id.to_string()),
                ..Default::default()
            }),
            traverse: None,
            sort: None,
            limit: None,
            offset: None,
        })
        .await
        .expect("search by repository uuid");
    assert_eq!(by_uuid.results.tasks.len(), 4);
    // The old wire name is refused (COLLIERY-T-0259).
    let (status, body) = svc
        .raw_request(
            reqwest::Method::POST,
            "/api/search",
            Some(&serde_json::json!({ "filter": { "repository_id": "payments-api" } })),
        )
        .await
        .expect("raw search");
    assert_eq!(status, 400, "repository_id in the filter: {body}");
    assert_eq!(body["error"]["code"], "VALIDATION", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("repository_id")),
        "the refusal names the field: {body}"
    );
    let (status, _) = svc
        .raw_request(
            reqwest::Method::POST,
            "/api/search",
            Some(&serde_json::json!({ "filter": { "repository": "nope" } })),
        )
        .await
        .expect("raw search");
    assert_eq!(status, 422, "unknown repository slug is a validation error");

    // =======================================================================
    // A change of owner leaves the linked tasks where they are
    // =======================================================================
    // Ownership is a fact about the repository. Until COLLIERY-T-0217 it was
    // also the rule for where a bound task could be, so a change of owner
    // left every bound task in a place the rule no longer allowed.
    let rehomed_repo = svc
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                team: Some("web".into()),
                ..Default::default()
            },
        )
        .await
        .expect("web takes payments-api");
    assert_eq!(rehomed_repo.team.slug, "web");
    for (before, board, team) in [
        (&routed, &platform_board, &platform.id),
        (&agreeing, &platform_board, &platform.id),
        (&cross, &web_board, &web.id),
        (&team_and_repo, &web_board, &web.id),
    ] {
        let after = svc.get_task(&before.short_code).await.expect("get");
        assert_eq!(
            &after.board_id, board,
            "{} stays on its board",
            after.short_code
        );
        assert_eq!(after.column_id, before.column_id);
        assert_eq!(
            after.team_id.as_deref(),
            Some(team.as_str()),
            "{} keeps the team of its board",
            after.short_code
        );
        let link = after.repository.as_ref().expect("still linked");
        assert_eq!(link.slug, "payments-api");
        assert_eq!(link.team_id, web.id, "the link shows the new owner");
    }

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
