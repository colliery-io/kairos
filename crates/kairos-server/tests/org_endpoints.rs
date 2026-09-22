//! Integration test for the KAIROS-T-0019 organizational + admin endpoint
//! families (S-0005 boards/teams/delivery-streams/board-members, the
//! org-membership scope addition, and /api/admin/tenants), through the
//! typed `kairos_client::KairosClient` against the booted production
//! router on a real port (KAIROS-T-0024).
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. Owns the uniquely named scratch
//! database `kairos_org_t0019_test` (shared-services discipline).
//!
//! Unlike `entities.rs`, this test provisions its tenant THROUGH THE API:
//! the deployment-admin tenant lifecycle (`KAIROS_DEPLOYMENT_ADMINS`) is
//! part of what is under test, including "the caller is the new tenant's
//! first org admin and can immediately act".
//!
//! Cast:
//! - `svc`   — deployment admin (server B) AND first org admin of `acme`,
//! - `alice` — org member, later team member,
//! - `bob`   — org member used for the capability grant/revoke lifecycle.

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
use kairos_client::types::{CreateStrategyRequest, CreateTaskRequest, Pagination};
use kairos_client::types_org::{
    AddBoardMemberRequest, AddOrgMemberRequest, AddStreamTeamRequest, AddTeamMemberRequest,
    BoardColumn, CreateBoardRequest, CreateColumnRequest, CreateStreamRequest, CreateTeamRequest,
    CreateTenantRequest, CreateTransitionRequest, ReplaceCapabilitiesRequest, UpdateColumnRequest,
    UpdateOrgMemberRequest,
};
use kairos_db::schema::users;
use kairos_db::{TenantPool, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_org_t0019_test";

/// `(public.users.id, external_id)` by email.
fn user_row(conn: &mut PgConnection, email: &str) -> (Uuid, String) {
    users::table
        .filter(users::email.eq(email))
        .select((users::id, users::external_id))
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// The column with this name from a board-detail column list.
fn column_by_name<'a>(columns: &'a [BoardColumn], name: &str) -> &'a BoardColumn {
    columns
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no column {name} in {columns:?}"))
}

#[tokio::test]
async fn org_and_admin_endpoints_against_live_stack() {
    // --- scratch database (public migrations only — NO tenant yet) ---------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");

    // --- live tokens + two server variants ---------------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let svc_token = user_token(&http, "svc").await;

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    // Server A: KAIROS_DEPLOYMENT_ADMINS unset → admin routes always 403.
    let router_a = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let server_a = spawn_server(router_a).await;

    // JIT-provision the three users through the auth middleware (the admin
    // routes authenticate first, then 403 on the empty admin list; no
    // tenant context on the admin family).
    for token in [&alice_token, &bob_token, &svc_token] {
        let err = rejection(
            server_a
                .client_untenanted(token)
                .list_tenants(Pagination::default())
                .await,
        );
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("FORBIDDEN"), "{err}");
    }
    let (alice_id, _alice_sub) = user_row(&mut conn, "alice@kairos.test");
    let (bob_id, _bob_sub) = user_row(&mut conn, "bob@kairos.test");
    let (svc_id, svc_sub) = user_row(&mut conn, "svc@kairos.test");

    // Server B: svc's OIDC sub is a deployment admin.
    let mut config_b = base_config(&scratch_url);
    config_b.deployment_admins = vec![svc_sub.clone()];
    let router = app::router(app::state_with(config_b, pool.clone(), auth.clone()));
    let server = spawn_server(router).await;

    // Tenant-scoped clients (X-Tenant acme) + cross-tenant admin clients.
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let svc = server.client(&svc_token, "acme");
    let alice_admin = server.client_untenanted(&alice_token);
    let bob_admin = server.client_untenanted(&bob_token);
    let svc_admin = server.client_untenanted(&svc_token);

    // =======================================================================
    // Tenant admin lifecycle (deployment-admin gate + initial org admin)
    // =======================================================================
    // Non-deployment-admin caller → 403 even on server B.
    let acme_request = CreateTenantRequest {
        slug: "acme".into(),
        name: "Acme Inc".into(),
        initial_admin_external_id: None,
    };
    let err = rejection(alice_admin.create_tenant(&acme_request).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // Deployment admin creates the tenant; the caller becomes its first
    // org admin (scope addition, day-zero review 2026-07-10).
    let created = svc_admin
        .create_tenant(&acme_request)
        .await
        .expect("provisioning acme through the API");
    assert_eq!(created.slug, "acme");
    assert_eq!(created.schema, "org_acme");
    assert_eq!(created.boards_created.len(), 3);
    assert_eq!(created.initial_admin.external_id, svc_sub);
    assert_eq!(created.initial_admin.role, "admin");

    // Duplicate slug → 409; invalid slug → 422; unknown initial admin → 422.
    let err = rejection(
        svc_admin
            .create_tenant(&CreateTenantRequest {
                slug: "acme".into(),
                name: "Again".into(),
                initial_admin_external_id: None,
            })
            .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    let err = rejection(
        svc_admin
            .create_tenant(&CreateTenantRequest {
                slug: "Not A Slug".into(),
                name: "Bad".into(),
                initial_admin_external_id: None,
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );
    let err = rejection(
        svc_admin
            .create_tenant(&CreateTenantRequest {
                slug: "ghost".into(),
                name: "Ghost".into(),
                initial_admin_external_id: Some("never-logged-in".into()),
            })
            .await,
    );
    assert_eq!(err.status(), Some(422), "{err}");
    assert!(
        err.message()
            .expect("enveloped message")
            .contains("authenticate once first"),
        "{err}"
    );

    // List: deployment admin sees acme; non-admin is 403.
    let body = svc_admin
        .list_tenants(Pagination::default())
        .await
        .expect("listing tenants");
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].slug, "acme");
    assert!(body.items[0].schema_exists);
    let err = rejection(bob_admin.list_tenants(Pagination::default()).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // The creator can IMMEDIATELY act in the new tenant as org admin.
    let body = svc.whoami().await.expect("svc whoami");
    assert_eq!(body.organization.role, "admin");

    // =======================================================================
    // Organization membership family (/api/members, scope addition)
    // =======================================================================
    // Not yet members → tenant middleware rejects alice/bob.
    for client in [&alice, &bob] {
        let err = rejection(client.list_tasks(Pagination::default()).await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }

    // Unknown email → 404 with the "log in once first" contract.
    let err = rejection(
        svc.add_org_member(&AddOrgMemberRequest {
            email: "dave@kairos.test".into(),
            role: None,
        })
        .await,
    );
    match &err {
        Error::NotFound { message, .. } => {
            assert!(message.contains("log in once"), "{message}");
        }
        other => panic!("expected NotFound, got {other}"),
    }

    // Add alice and bob by email.
    let body = svc
        .add_org_member(&AddOrgMemberRequest {
            email: "alice@kairos.test".into(),
            role: None,
        })
        .await
        .expect("adding alice");
    assert_eq!(body.role, "member");
    assert_eq!(body.user_id, alice_id.to_string());
    svc.add_org_member(&AddOrgMemberRequest {
        email: "bob@kairos.test".into(),
        role: Some("member".into()),
    })
    .await
    .expect("adding bob");

    // Duplicate → 409; non-admin caller → 403 naming the gate.
    let err = rejection(
        svc.add_org_member(&AddOrgMemberRequest {
            email: "alice@kairos.test".into(),
            role: None,
        })
        .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    let err = rejection(
        alice
            .add_org_member(&AddOrgMemberRequest {
                email: "bob@kairos.test".into(),
                role: None,
            })
            .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_org_members"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // Reads open tenant-wide: bob lists members.
    let body = bob
        .list_org_members(Pagination::default())
        .await
        .expect("bob lists members");
    assert_eq!(body.total, 3);

    // LAST_ADMIN guard on PATCH and DELETE (svc is the only admin).
    let svc_user_id = svc_id.to_string();
    let err = rejection(
        svc.update_org_member(
            &svc_user_id,
            &UpdateOrgMemberRequest {
                role: "member".into(),
            },
        )
        .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "LAST_ADMIN");
        }
        other => panic!("expected 422 LAST_ADMIN, got {other}"),
    }
    let err = rejection(svc.remove_org_member(&svc_user_id).await);
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "LAST_ADMIN");
        }
        other => panic!("expected 422 LAST_ADMIN, got {other}"),
    }

    // Promote alice → two admins → demote alice again (allowed).
    let body = svc
        .update_org_member(
            &alice_id.to_string(),
            &UpdateOrgMemberRequest {
                role: "admin".into(),
            },
        )
        .await
        .expect("promoting alice");
    assert_eq!(body.role, "admin");
    let body = svc
        .update_org_member(
            &alice_id.to_string(),
            &UpdateOrgMemberRequest {
                role: "member".into(),
            },
        )
        .await
        .expect("demoting alice");
    assert_eq!(body.role, "member");

    // Remove bob → membership gone; re-add for the capability lifecycle.
    svc.remove_org_member(&bob_id.to_string())
        .await
        .expect("removing bob");
    let err = rejection(bob.list_tasks(Pagination::default()).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    svc.add_org_member(&AddOrgMemberRequest {
        email: "bob@kairos.test".into(),
        role: None,
    })
    .await
    .expect("re-adding bob");

    // =======================================================================
    // Boards: config lifecycle with T-0010 rule violations
    // =======================================================================
    let body = alice
        .list_boards(Pagination::default())
        .await
        .expect("listing boards");
    assert_eq!(body.total, 3, "{body:?}");
    let strategy_board = body
        .items
        .iter()
        .find(|b| b.board_level == "strategy")
        .map(|b| b.id.clone())
        .expect("strategy board");

    let detail = bob
        .get_board(&strategy_board)
        .await
        .expect("strategy board detail");
    assert_eq!(detail.columns.len(), 5);
    assert_eq!(detail.transitions.len(), 4);
    let draft_col = column_by_name(&detail.columns, "Draft").id.clone();

    // Config writes are gated by configure_boards (403 names it).
    let err = rejection(
        alice
            .add_column(
                &strategy_board,
                &CreateColumnRequest {
                    name: "Spike".into(),
                    position: 5,
                },
            )
            .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("configure_boards"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // Org admin adds a column; duplicates are typed 422s.
    let spike = svc
        .add_column(
            &strategy_board,
            &CreateColumnRequest {
                name: "Spike".into(),
                position: 5,
            },
        )
        .await
        .expect("adding Spike column");
    let spike_col = spike.id.clone();
    let err = rejection(
        svc.add_column(
            &strategy_board,
            &CreateColumnRequest {
                name: "Spike".into(),
                position: 6,
            },
        )
        .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "DUPLICATE_COLUMN_NAME");
        }
        other => panic!("expected 422 DUPLICATE_COLUMN_NAME, got {other}"),
    }
    let err = rejection(
        svc.add_column(
            &strategy_board,
            &CreateColumnRequest {
                name: "Другое".into(),
                position: 5,
            },
        )
        .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "DUPLICATE_COLUMN_POSITION");
        }
        other => panic!("expected 422 DUPLICATE_COLUMN_POSITION, got {other}"),
    }

    // Transitions: add Draft -> Spike; duplicate is a typed 422.
    let edge_request = CreateTransitionRequest {
        from_column_id: draft_col.clone(),
        to_column_id: spike_col.clone(),
    };
    let edge = svc
        .add_transition(&strategy_board, &edge_request)
        .await
        .expect("adding Draft -> Spike");
    let edge_id = edge.id.clone();
    let err = rejection(svc.add_transition(&strategy_board, &edge_request).await);
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "DUPLICATE_TRANSITION");
        }
        other => panic!("expected 422 DUPLICATE_TRANSITION, got {other}"),
    }

    // Put an item in Draft, then try to remove Draft → COLUMN_NOT_EMPTY.
    let strategy = svc
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.clone(),
            column_id: None,
            title: "Own the market".into(),
            content: "# Strategy".into(),
            hypothesis: None,
        })
        .await
        .expect("creating strategy in Draft");
    let strategy_code = strategy.short_code.clone();
    let err = rejection(svc.remove_column(&strategy_board, &draft_col).await);
    match &err {
        Error::Other {
            status,
            code,
            details,
            ..
        } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "COLUMN_NOT_EMPTY");
            assert_eq!(details["item_count"], 1);
        }
        other => panic!("expected 422 COLUMN_NOT_EMPTY, got {other}"),
    }

    // PATCH column: rename + move to the front.
    let body = svc
        .update_column(
            &strategy_board,
            &spike_col,
            &UpdateColumnRequest {
                name: Some("Spike 2".into()),
                position: Some(0),
                is_done: None,
            },
        )
        .await
        .expect("renaming and moving Spike");
    assert_eq!(body.name, "Spike 2");
    assert_eq!(body.position, 0);

    // Remove the transition by id; unknown id is 404; then remove the
    // (empty) column.
    svc.remove_transition(&strategy_board, &edge_id)
        .await
        .expect("removing the transition");
    let err = rejection(svc.remove_transition(&strategy_board, &edge_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    svc.remove_column(&strategy_board, &spike_col)
        .await
        .expect("removing the empty column");

    // Board CRUD: create from defaults, delete only when empty.
    let sandbox = svc
        .create_board(&CreateBoardRequest {
            name: "Sandbox".into(),
            slug: "sandbox".into(),
            board_level: "strategy".into(),
            team_id: None,
        })
        .await
        .expect("creating sandbox board");
    assert_eq!(sandbox.columns.len(), 5);
    let sandbox_id = sandbox.board.id.clone();
    let err = rejection(svc.delete_board(&strategy_board).await);
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "BOARD_NOT_EMPTY");
        }
        other => panic!("expected 422 BOARD_NOT_EMPTY, got {other}"),
    }
    svc.delete_board(&sandbox_id)
        .await
        .expect("deleting sandbox");
    let err = rejection(svc.get_board(&sandbox_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Teams: creation creates the delivery board (T-0010 deferred decision)
    // =======================================================================
    let err = rejection(
        alice
            .create_team(&CreateTeamRequest {
                name: "Platform".into(),
                slug: "platform".into(),
                team_type: None,
            })
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    let team = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: Some("platform".into()),
        })
        .await
        .expect("creating platform team");
    let team_id = team.id.clone();
    let delivery_board = team
        .delivery_board_id
        .clone()
        .expect("team creation creates the delivery board");

    // The delivery board exists, belongs to the team, and carries the
    // seeded delivery defaults (5 columns, 7 transitions).
    let detail = bob
        .get_board(&delivery_board)
        .await
        .expect("delivery board detail");
    assert_eq!(detail.board.board_level, "delivery");
    assert_eq!(detail.board.team_id.as_deref(), Some(team_id.as_str()));
    let column_names: Vec<&str> = detail.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        column_names,
        ["Backlog", "Todo", "Blocked", "Active", "Completed"]
    );
    assert_eq!(detail.transitions.len(), 7);

    // Duplicate team slug → 409.
    let err = rejection(
        svc.create_team(&CreateTeamRequest {
            name: "Platform 2".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "{err}");

    // Team membership lifecycle.
    let body = svc
        .add_team_member(
            &team_id,
            &AddTeamMemberRequest {
                user_id: alice_id.to_string(),
            },
        )
        .await
        .expect("adding alice to the team");
    assert_eq!(body.email, "alice@kairos.test");
    let err = rejection(
        svc.add_team_member(
            &team_id,
            &AddTeamMemberRequest {
                user_id: alice_id.to_string(),
            },
        )
        .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    let err = rejection(
        svc.add_team_member(
            &team_id,
            &AddTeamMemberRequest {
                user_id: Uuid::new_v4().to_string(),
            },
        )
        .await,
    );
    assert_eq!(err.status(), Some(422), "{err}");
    let body = bob
        .list_team_members(&team_id)
        .await
        .expect("listing team members");
    assert_eq!(body.len(), 1);
    svc.remove_team_member(&team_id, &alice_id.to_string())
        .await
        .expect("removing alice from the team");
    let err = rejection(
        svc.remove_team_member(&team_id, &alice_id.to_string())
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Delivery streams + stream/team membership
    // =======================================================================
    let err = rejection(
        bob.create_stream(&CreateStreamRequest {
            name: "Payments".into(),
            slug: "payments".into(),
            description: None,
        })
        .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let stream = svc
        .create_stream(&CreateStreamRequest {
            name: "Payments".into(),
            slug: "payments".into(),
            description: Some("Money movement".into()),
        })
        .await
        .expect("creating payments stream");
    let stream_id = stream.id.clone();

    svc.add_stream_team(
        &stream_id,
        &AddStreamTeamRequest {
            team_id: team_id.clone(),
        },
    )
    .await
    .expect("adding the team to the stream");
    let err = rejection(
        svc.add_stream_team(
            &stream_id,
            &AddStreamTeamRequest {
                team_id: team_id.clone(),
            },
        )
        .await,
    );
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    let stream_teams = alice
        .list_stream_teams(&stream_id)
        .await
        .expect("listing stream teams");
    assert_eq!(stream_teams.len(), 1);
    assert_eq!(stream_teams[0].slug, "platform");
    svc.remove_stream_team(&stream_id, &team_id)
        .await
        .expect("removing the team from the stream");
    let err = rejection(svc.remove_stream_team(&stream_id, &team_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Board authorization: grant via API flips a 403 into a 201
    // =======================================================================
    // bob (member, no grants) cannot create strategies.
    let bobs_plan = CreateStrategyRequest {
        board_id: strategy_board.clone(),
        column_id: None,
        title: "Bob's plan".into(),
        content: String::new(),
        hypothesis: None,
    };
    let err = rejection(bob.create_strategy(&bobs_plan).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // alice lacks manage_members; capability vocabulary is validated.
    let err = rejection(
        alice
            .add_board_member(
                &strategy_board,
                &AddBoardMemberRequest {
                    user_id: bob_id.to_string(),
                    capabilities: vec!["manage_strategies".into()],
                },
            )
            .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_members"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }
    let err = rejection(
        svc.add_board_member(
            &strategy_board,
            &AddBoardMemberRequest {
                user_id: bob_id.to_string(),
                capabilities: vec!["hack_the_planet".into()],
            },
        )
        .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // Grant via API → previously-403 write succeeds.
    let member = svc
        .add_board_member(
            &strategy_board,
            &AddBoardMemberRequest {
                user_id: bob_id.to_string(),
                capabilities: vec!["manage_strategies".into(), "transition_items".into()],
            },
        )
        .await
        .expect("granting bob capabilities");
    assert_eq!(
        member.capabilities,
        ["manage_strategies", "transition_items"]
    );
    bob.create_strategy(&bobs_plan)
        .await
        .expect("bob creates a strategy after the grant");

    // The members view shows the grants.
    let members = alice
        .list_board_members(&strategy_board)
        .await
        .expect("listing board members");
    assert_eq!(members.len(), 1, "{members:?}");
    assert_eq!(members[0].email, "bob@kairos.test");

    // PATCH replaces the whole set: transition_items is revoked.
    let body = svc
        .replace_capabilities(
            &strategy_board,
            &bob_id.to_string(),
            &ReplaceCapabilitiesRequest {
                capabilities: vec!["manage_strategies".into()],
            },
        )
        .await
        .expect("replacing bob's capability set");
    assert_eq!(body.capabilities, ["manage_strategies"]);
    let err = rejection(bob.transition_strategy(&strategy_code, &draft_col).await);
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("transition_items"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // DELETE revokes everything → the write 403s again; PATCHing a
    // non-member is 404.
    let body = svc
        .remove_board_member(&strategy_board, &bob_id.to_string())
        .await
        .expect("revoking bob's membership");
    assert_eq!(body.revoked_capabilities, ["manage_strategies"]);
    let err = rejection(
        bob.create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.clone(),
            column_id: None,
            title: "Denied again".into(),
            content: String::new(),
            hypothesis: None,
        })
        .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let err = rejection(
        svc.replace_capabilities(
            &strategy_board,
            &bob_id.to_string(),
            &ReplaceCapabilitiesRequest {
                capabilities: vec!["manage_strategies".into()],
            },
        )
        .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // whoami exposes board-scoped capability grants (KAIROS-T-0052)
    // =======================================================================
    // Plain members (no board grants) report an empty capability set; the org
    // admin reports role=admin with no explicit grants (its access is the
    // A-0006 bypass, not a `board_member_capabilities` row).
    let me = bob.whoami().await.expect("bob whoami");
    assert_eq!(me.organization.role, "member");
    assert!(me.capabilities.is_empty(), "bob has no grants yet: {me:?}");
    let me = alice.whoami().await.expect("alice whoami");
    assert!(me.capabilities.is_empty(), "alice has no grants: {me:?}");
    let me = svc.whoami().await.expect("svc whoami");
    assert_eq!(me.organization.role, "admin");
    assert!(
        me.capabilities.is_empty(),
        "org admin holds no explicit grants (bypass): {me:?}"
    );

    // Grant bob a board-config capability on the delivery board → whoami now
    // surfaces it, grouped by board with the grant list. This is exactly the
    // input the GUI's per-board admin gating consumes (T-0052): a capability-
    // granted non-admin reaches that board's config; a plain member does not.
    let delivery_slug = svc
        .get_board(&delivery_board)
        .await
        .expect("delivery board detail")
        .board
        .slug;
    svc.add_board_member(
        &delivery_board,
        &AddBoardMemberRequest {
            user_id: bob_id.to_string(),
            capabilities: vec!["configure_boards".into(), "manage_members".into()],
        },
    )
    .await
    .expect("granting bob board-config capabilities");

    let me = bob.whoami().await.expect("bob whoami after grant");
    assert_eq!(me.organization.role, "member");
    assert_eq!(me.capabilities.len(), 1, "one board grouped: {me:?}");
    let board_caps = &me.capabilities[0];
    assert_eq!(board_caps.board_id, delivery_board);
    assert_eq!(board_caps.board_slug, delivery_slug);
    assert_eq!(
        board_caps.grants,
        ["configure_boards", "manage_members"],
        "grants present and sorted: {me:?}"
    );

    // The whoami-driven gate (mirrors the GUI's `gating::can_access` and the
    // A-0006 server authority): a board-config grant admits bob to the
    // per-board admin surface; alice (still no grants) stays gated out.
    let holds_board_config = |caps: &[kairos_client::types_org::WhoamiBoardCapabilities]| {
        caps.iter().any(|b| {
            b.grants
                .iter()
                .any(|g| g == "configure_boards" || g == "manage_members")
        })
    };
    assert!(holds_board_config(&me.capabilities), "bob is admitted");
    let alice_me = alice
        .whoami()
        .await
        .expect("alice whoami after bob's grant");
    assert!(
        !holds_board_config(&alice_me.capabilities),
        "alice remains gated out (grant did not leak): {alice_me:?}"
    );

    // Restore the pre-section state (no grants) for the assertions below.
    svc.remove_board_member(&delivery_board, &bob_id.to_string())
        .await
        .expect("revoking bob's delivery-board grants");

    // =======================================================================
    // Board items view: grouped by column, all four entity families
    // =======================================================================
    // Put a task on the team's delivery board for the cross-family check.
    let task = svc
        .create_task(&CreateTaskRequest {
            board_id: Some(delivery_board.clone()),
            repository_id: None,
            column_id: None,
            title: "Wire the API".into(),
            content: String::new(),
            task_type: None,
            work_class: None,
            team_id: None,
        })
        .await
        .expect("creating task on the delivery board");

    let items = bob
        .board_items(&strategy_board)
        .await
        .expect("strategy board items");
    assert_eq!(items.board.id, strategy_board);
    assert_eq!(items.columns.len(), 5, "{items:?}");
    let draft_group = items
        .columns
        .iter()
        .find(|g| g.column.name == "Draft")
        .expect("Draft group");
    // Both strategies (svc's and bob's) sit in Draft; nothing else does.
    assert_eq!(draft_group.strategies.len(), 2, "{items:?}");
    assert_eq!(draft_group.tasks.len(), 0);
    assert_eq!(draft_group.initiatives.len(), 0);
    assert_eq!(draft_group.adrs.len(), 0);
    for group in items.columns.iter().filter(|g| g.column.name != "Draft") {
        assert_eq!(group.strategies.len(), 0);
    }

    let items = bob
        .board_items(&delivery_board)
        .await
        .expect("delivery board items");
    let backlog_group = items
        .columns
        .iter()
        .find(|g| g.column.name == "Backlog")
        .expect("Backlog group");
    assert_eq!(backlog_group.tasks.len(), 1);
    assert_eq!(
        backlog_group.tasks[0].short_code, task.short_code,
        "{items:?}"
    );

    // =======================================================================
    // Tenant teardown lifecycle: confirm semantics per T-0008
    // =======================================================================
    svc_admin
        .create_tenant(&CreateTenantRequest {
            slug: "beta".into(),
            name: "Beta".into(),
            initial_admin_external_id: None,
        })
        .await
        .expect("provisioning beta");
    let err = rejection(svc_admin.delete_tenant("beta", None).await);
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "CONFIRMATION_REQUIRED");
        }
        other => panic!("expected 422 CONFIRMATION_REQUIRED, got {other}"),
    }
    let err = rejection(bob_admin.delete_tenant("beta", Some(true)).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let body = svc_admin
        .delete_tenant("beta", Some(true))
        .await
        .expect("dropping beta");
    assert!(body.dropped);
    let err = rejection(svc_admin.delete_tenant("beta", Some(true)).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let body = svc_admin
        .list_tenants(Pagination::default())
        .await
        .expect("listing tenants after teardown");
    assert_eq!(body.total, 1, "{body:?}");

    // --- teardown -----------------------------------------------------------
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
