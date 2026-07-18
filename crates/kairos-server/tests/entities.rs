//! Integration test for the KAIROS-T-0018 entity endpoint families
//! (contracts per KAIROS-S-0005 / A-0004 / A-0006), through the typed
//! `kairos_client::KairosClient` against the booted production router on a
//! real port (KAIROS-T-0024: the client doubles as the test harness).
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer — tokens come from the password grant
//! with the seeded test users (see `tests/common/mod.rs`). For isolation
//! the test owns the uniquely named scratch database
//! (`kairos_entities_t0018_test`); the shared `kairos` database is never
//! touched (shared-services discipline).
//!
//! Cast (per KAIROS-A-0006):
//! - `svc`   — org ADMIN: implicit full access (creates the off-board ADR),
//! - `alice` — org member holding explicit capability grants per board,
//! - `bob`   — org member with NO grants (the 403 matrix), granted
//!   `manage_tasks` mid-test to prove 403 → 201 after grant.
//!
//! Capability grants and the parent edge are seeded through the kairos-db
//! services directly (`abac::grant_capability`, `graph::link_items`) — the
//! admin API for grants is KAIROS-T-0019.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::Value;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, Pagination, UpdateContentRequest,
};
use kairos_client::{EntityKind, Error, KairosClient};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, RelationshipType};
use kairos_db::schema::{
    board_columns, board_transitions, boards, organization_members, organizations, templates, users,
};
use kairos_db::{TenantPool, abac, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_entities_t0018_test";

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The tenant board of a level (provisioned defaults; delivery is created
/// by this test).
fn board_id(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

/// Column ids of a board in position order.
fn columns_of(conn: &mut PgConnection, board: Uuid) -> Vec<Uuid> {
    board_columns::table
        .filter(board_columns::board_id.eq(board))
        .order(board_columns::position.asc())
        .select(board_columns::id)
        .load(conn)
        .expect("board columns")
}

/// Transition edges of a board.
fn edges_of(conn: &mut PgConnection, board: Uuid) -> Vec<(Uuid, Uuid)> {
    board_transitions::table
        .filter(board_transitions::board_id.eq(board))
        .select((
            board_transitions::from_column_id,
            board_transitions::to_column_id,
        ))
        .load(conn)
        .expect("board transitions")
}

/// A column reachable from `from` per the board's transition graph.
fn valid_target(edges: &[(Uuid, Uuid)], from: Uuid) -> Uuid {
    edges
        .iter()
        .find(|(f, _)| *f == from)
        .map(|(_, t)| *t)
        .expect("default boards always allow a move out of the first column")
}

/// A column NOT reachable from `from` (there is always one on the default
/// boards: they are linear-ish, never complete graphs).
fn invalid_target(columns: &[Uuid], edges: &[(Uuid, Uuid)], from: Uuid) -> Uuid {
    columns
        .iter()
        .copied()
        .find(|c| !edges.iter().any(|(f, t)| *f == from && t == c))
        .expect("default boards are not complete graphs")
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// `GET /api/{family}/{code}` through the typed client, family-generic.
async fn get_any(client: &KairosClient, kind: EntityKind, code: &str) -> Result<(), Error> {
    match kind {
        EntityKind::Strategy => client.get_strategy(code).await.map(drop),
        EntityKind::Initiative => client.get_initiative(code).await.map(drop),
        EntityKind::Task => client.get_task(code).await.map(drop),
        EntityKind::Document => client.get_document(code).await.map(drop),
        EntityKind::Adr => client.get_adr(code).await.map(drop),
    }
}

/// `PATCH /api/{family}/{code}` through the typed client, family-generic.
async fn update_any(
    client: &KairosClient,
    kind: EntityKind,
    code: &str,
    request: &UpdateContentRequest,
) -> Result<(), Error> {
    match kind {
        EntityKind::Strategy => client.update_strategy(code, request).await.map(drop),
        EntityKind::Initiative => client.update_initiative(code, request).await.map(drop),
        EntityKind::Task => client.update_task(code, request).await.map(drop),
        EntityKind::Document => client.update_document(code, request).await.map(drop),
        EntityKind::Adr => client.update_adr(code, request).await.map(drop),
    }
}

/// `DELETE /api/{family}/{code}` through the typed client, family-generic.
async fn delete_any(client: &KairosClient, kind: EntityKind, code: &str) -> Result<(), Error> {
    match kind {
        EntityKind::Strategy => client.delete_strategy(code).await.map(drop),
        EntityKind::Initiative => client.delete_initiative(code).await.map(drop),
        EntityKind::Task => client.delete_task(code).await.map(drop),
        EntityKind::Document => client.delete_document(code).await.map(drop),
        EntityKind::Adr => client.delete_adr(code).await.map(drop),
    }
}

/// `POST /api/{family}/{code}/transition` through the typed client.
async fn transition_any(
    client: &KairosClient,
    kind: EntityKind,
    code: &str,
    to_column_id: &str,
) -> Result<(), Error> {
    match kind {
        EntityKind::Strategy => client
            .transition_strategy(code, to_column_id)
            .await
            .map(drop),
        EntityKind::Initiative => client
            .transition_initiative(code, to_column_id)
            .await
            .map(drop),
        EntityKind::Task => client.transition_task(code, to_column_id).await.map(drop),
        EntityKind::Adr => client.transition_adr(code, to_column_id).await.map(drop),
        EntityKind::Document => unreachable!("documents have no transition endpoint"),
    }
}

#[tokio::test]
async fn entity_endpoints_against_live_stack() {
    // --- scratch database + tenant ----------------------------------------
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

    // --- live server + typed clients (one per user, X-Tenant acme) ---------
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
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth,
    ));
    let server = spawn_server(router).await;
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // --- JIT-provision the three users, then grant membership --------------
    for client in [&alice, &bob, &svc] {
        let err = rejection(client.list_tasks(Pagination::default()).await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user_id, role) in [
        (alice_id, OrgRole::Member),
        (bob_id, OrgRole::Member),
        (svc_id, OrgRole::Admin),
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

    // --- tenant-schema seeding (boards, grants) -----------------------------
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning seeding connection to the tenant schema");
    // Delivery boards are per-team, created post-provisioning (A-0002).
    kairos_db::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("creating the delivery board");

    let strategy_board = board_id(&mut conn, BoardLevel::Strategy);
    let initiative_board = board_id(&mut conn, BoardLevel::Initiative);
    let delivery_board = board_id(&mut conn, BoardLevel::Delivery);
    let adr_board = board_id(&mut conn, BoardLevel::Adr);

    // Alice: explicit per-board grants (A-0006 whitelist); NOT an org admin.
    for (board, capability) in [
        (strategy_board, "manage_strategies"),
        (strategy_board, "transition_items"),
        (initiative_board, "manage_initiatives"),
        (initiative_board, "manage_documents"),
        (initiative_board, "transition_items"),
        (delivery_board, "manage_tasks"),
        (delivery_board, "transition_items"),
        (adr_board, "manage_adrs"),
        (adr_board, "transition_items"),
    ] {
        abac::grant_capability(&mut conn, board, alice_id, capability, svc_id)
            .expect("granting alice capability");
    }

    // =======================================================================
    // Strategies: full happy path + 409 + 422 invalid transition
    // =======================================================================
    let strategy = alice
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.to_string(),
            column_id: None,
            title: "Grow ARR".into(),
            content: "# Strategy\nDouble revenue.".into(),
            hypothesis: Some("Enterprise demand exists".into()),
        })
        .await
        .expect("creating strategy");
    let strategy_code = strategy.short_code.clone();
    assert!(strategy_code.contains("-S-"), "{strategy_code}");
    assert_eq!(strategy.version, 1);
    assert_eq!(
        strategy.hypothesis.as_deref(),
        Some("Enterprise demand exists")
    );
    let strategy_columns = columns_of(&mut conn, strategy_board);
    let strategy_edges = edges_of(&mut conn, strategy_board);
    assert_eq!(
        strategy.column_id,
        strategy_columns[0].to_string(),
        "defaults to the first column"
    );

    // Reads are open tenant-wide (A-0006): bob can GET.
    let body = bob
        .get_strategy(&strategy_code)
        .await
        .expect("bob reads the strategy");
    assert_eq!(body.title, "Grow ARR");

    let body = alice
        .list_strategies(Pagination {
            limit: Some(10),
            offset: Some(0),
        })
        .await
        .expect("listing strategies");
    assert_eq!(body.total, 1);
    assert_eq!(body.limit, 10);
    assert_eq!(body.offset, 0);
    assert_eq!(body.items.len(), 1);

    let body = alice
        .update_strategy(
            &strategy_code,
            &UpdateContentRequest {
                title: Some("Grow ARR 2x".into()),
                content: "v2".into(),
                version: 1,
            },
        )
        .await
        .expect("updating strategy");
    assert_eq!(body.version, 2);
    assert_eq!(body.title, "Grow ARR 2x");

    // Stale version → 409 CONFLICT carrying the CURRENT entity state.
    let err = rejection(
        alice
            .update_strategy(
                &strategy_code,
                &UpdateContentRequest {
                    title: None,
                    content: "stale write".into(),
                    version: 1,
                },
            )
            .await,
    );
    match &err {
        Error::Conflict { code, current, .. } => {
            assert_eq!(code, "CONFLICT");
            assert_eq!(current["version"], 2);
            assert_eq!(current["title"], "Grow ARR 2x");
            assert_eq!(current["content"], "v2");
        }
        other => panic!("expected Conflict, got {other}"),
    }

    // Invalid transition → 422 with the allowed targets.
    let bad_target = invalid_target(&strategy_columns, &strategy_edges, strategy_columns[0]);
    let good_target = valid_target(&strategy_edges, strategy_columns[0]);
    let err = rejection(
        alice
            .transition_strategy(&strategy_code, &bad_target.to_string())
            .await,
    );
    match &err {
        Error::InvalidTransition {
            allowed_targets, ..
        } => {
            let allowed = allowed_targets.as_array().expect("allowed_targets array");
            assert!(
                allowed.iter().any(|c| c["id"] == good_target.to_string()),
                "{allowed_targets}"
            );
        }
        other => panic!("expected InvalidTransition, got {other}"),
    }

    let body = alice
        .transition_strategy(&strategy_code, &good_target.to_string())
        .await
        .expect("valid transition");
    assert_eq!(body.column_id, good_target.to_string());

    let body = alice
        .delete_strategy(&strategy_code)
        .await
        .expect("deleting strategy");
    assert_eq!(body.short_code, strategy_code);
    assert_eq!(body.cascade_count, 0);

    // =======================================================================
    // Initiatives: happy path (kept alive as the document parent + cascade
    // root) + 409 + 422 invalid transition
    // =======================================================================
    let initiative = alice
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.to_string(),
            column_id: None,
            title: "Ship the API".into(),
            content: "M2 endpoints".into(),
            complexity: Some("m".into()),
            bucket_type: None,
        })
        .await
        .expect("creating initiative");
    let initiative_code = initiative.short_code.clone();
    let initiative_id = Uuid::parse_str(&initiative.id).expect("DTO id is a UUID string");
    assert_eq!(initiative.complexity.as_deref(), Some("m"));
    assert!(!initiative.is_bucket);

    // Bad enum value → 422 VALIDATION.
    let err = rejection(
        alice
            .create_initiative(&CreateInitiativeRequest {
                board_id: initiative_board.to_string(),
                column_id: None,
                title: "x".into(),
                content: String::new(),
                complexity: Some("huge".into()),
                bucket_type: None,
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    let body = alice
        .update_initiative(
            &initiative_code,
            &UpdateContentRequest {
                title: None,
                content: "M2 endpoints, refined".into(),
                version: 1,
            },
        )
        .await
        .expect("updating initiative");
    assert_eq!(body.version, 2);
    assert_eq!(body.title, "Ship the API", "omitted title is kept");

    let err = rejection(
        alice
            .update_initiative(
                &initiative_code,
                &UpdateContentRequest {
                    title: None,
                    content: "stale".into(),
                    version: 1,
                },
            )
            .await,
    );
    match &err {
        Error::Conflict { current, .. } => assert_eq!(current["version"], 2),
        other => panic!("expected Conflict, got {other}"),
    }

    let initiative_columns = columns_of(&mut conn, initiative_board);
    let initiative_edges = edges_of(&mut conn, initiative_board);
    let bad_target = invalid_target(
        &initiative_columns,
        &initiative_edges,
        initiative_columns[0],
    );
    let err = rejection(
        alice
            .transition_initiative(&initiative_code, &bad_target.to_string())
            .await,
    );
    assert!(matches!(err, Error::InvalidTransition { .. }), "{err}");

    let good_target = valid_target(&initiative_edges, initiative_columns[0]);
    let body = alice
        .transition_initiative(&initiative_code, &good_target.to_string())
        .await
        .expect("valid initiative transition");
    assert_eq!(body.column_id, good_target.to_string());

    // =======================================================================
    // Tasks: the full 403 → grant → 201 arc, 409, 422, 404, delete
    // =======================================================================
    // bob has NO grants: 403 naming the missing capability (A-0006).
    let create_task_request = CreateTaskRequest {
        board_id: delivery_board.to_string(),
        column_id: None,
        title: "Wire the endpoints".into(),
        content: "T-0018".into(),
        task_type: None,
        team_id: None,
    };
    let err = rejection(bob.create_task(&create_task_request).await);
    match &err {
        Error::Forbidden {
            code,
            capability,
            details,
            ..
        } => {
            assert_eq!(code, "FORBIDDEN");
            assert_eq!(capability.as_deref(), Some("manage_tasks"));
            assert_eq!(details["board_id"], delivery_board.to_string());
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // Grant via the abac service (the admin API is T-0019) → 201.
    abac::grant_capability(&mut conn, delivery_board, bob_id, "manage_tasks", svc_id)
        .expect("granting bob manage_tasks");
    let task_bob = bob
        .create_task(&create_task_request)
        .await
        .expect("bob creates a task after the grant");
    let task_bob_code = task_bob.short_code.clone();
    let task_bob_id = Uuid::parse_str(&task_bob.id).expect("DTO id is a UUID string");
    assert_eq!(task_bob.task_type, "task", "task_type defaults to task");

    let task_bug = alice
        .create_task(&CreateTaskRequest {
            board_id: delivery_board.to_string(),
            column_id: None,
            title: "Fix the flaky login".into(),
            content: "repro steps".into(),
            task_type: Some("bug".into()),
            team_id: None,
        })
        .await
        .expect("creating bug task");
    let task_bug_code = task_bug.short_code.clone();
    assert_eq!(task_bug.task_type, "bug");

    let body = alice
        .list_tasks(Pagination::default())
        .await
        .expect("listing tasks");
    assert_eq!(body.total, 2);
    assert_eq!(body.limit, 50, "default limit");

    // 409 with current state.
    let body = alice
        .update_task(
            &task_bug_code,
            &UpdateContentRequest {
                title: Some("Fix login".into()),
                content: "v2".into(),
                version: 1,
            },
        )
        .await
        .expect("updating task");
    assert_eq!(body.version, 2);
    let err = rejection(
        alice
            .update_task(
                &task_bug_code,
                &UpdateContentRequest {
                    title: None,
                    content: "stale".into(),
                    version: 1,
                },
            )
            .await,
    );
    match &err {
        Error::Conflict { code, current, .. } => {
            assert_eq!(code, "CONFLICT");
            assert_eq!(current["version"], 2);
            assert_eq!(current["title"], "Fix login");
            assert_eq!(
                current["short_code"], task_bug_code,
                "details.current is the FULL entity DTO"
            );
        }
        other => panic!("expected Conflict, got {other}"),
    }

    // 422 invalid transition with allowed targets; then a valid move.
    let delivery_columns = columns_of(&mut conn, delivery_board);
    let delivery_edges = edges_of(&mut conn, delivery_board);
    let bad_target = invalid_target(&delivery_columns, &delivery_edges, delivery_columns[0]);
    let err = rejection(
        alice
            .transition_task(&task_bug_code, &bad_target.to_string())
            .await,
    );
    match &err {
        Error::InvalidTransition {
            allowed_targets, ..
        } => assert!(allowed_targets.is_array(), "{allowed_targets}"),
        other => panic!("expected InvalidTransition, got {other}"),
    }

    let good_target = valid_target(&delivery_edges, delivery_columns[0]);
    let body = alice
        .transition_task(&task_bug_code, &good_target.to_string())
        .await
        .expect("valid task transition");
    assert_eq!(body.column_id, good_target.to_string());

    // bob holds manage_tasks but NOT transition_items → 403.
    let err = rejection(
        bob.transition_task(&task_bob_code, &good_target.to_string())
            .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("transition_items"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // Create against an unknown board / malformed UUID → 422 VALIDATION.
    // Uuid::nil() is a valid UUID but no such board: alice holds no grant
    // on it either — the ABAC check runs first and correctly 403s.
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                board_id: Uuid::nil().to_string(),
                column_id: None,
                title: "x".into(),
                content: String::new(),
                task_type: None,
                team_id: None,
            })
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let err = rejection(
        alice
            .create_task(&CreateTaskRequest {
                board_id: "not-a-uuid".into(),
                column_id: None,
                title: "x".into(),
                content: String::new(),
                task_type: None,
                team_id: None,
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // Direct delete of a leaf task: cascade 0.
    let body = alice
        .delete_task(&task_bug_code)
        .await
        .expect("deleting bug task");
    assert_eq!(body.cascade_count, 0);
    let err = rejection(alice.get_task(&task_bug_code).await);
    assert!(
        matches!(err, Error::NotFound { .. }),
        "soft-deleted rows 404: {err}"
    );

    // =======================================================================
    // ADRs: board + off-board, ITEM_NOT_ON_BOARD, transitions
    // =======================================================================
    let adr = alice
        .create_adr(&CreateAdrRequest {
            board_id: Some(adr_board.to_string()),
            column_id: None,
            title: "Use Postgres".into(),
            content: "## Context".into(),
            decision_maker: Some("dylan".into()),
            decision_date: Some("2026-07-10".into()),
        })
        .await
        .expect("creating adr");
    let adr_code = adr.short_code.clone();
    assert!(adr_code.contains("-A-"), "{adr_code}");
    assert_eq!(adr.decision_date.as_deref(), Some("2026-07-10"));

    // Malformed decision_date → 422 VALIDATION.
    let err = rejection(
        alice
            .create_adr(&CreateAdrRequest {
                board_id: Some(adr_board.to_string()),
                column_id: None,
                title: "x".into(),
                content: String::new(),
                decision_maker: None,
                decision_date: Some("July 10".into()),
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    let body = alice
        .update_adr(
            &adr_code,
            &UpdateContentRequest {
                title: None,
                content: "## Context\n## Decision".into(),
                version: 1,
            },
        )
        .await
        .expect("updating adr");
    assert_eq!(body.version, 2);
    let err = rejection(
        alice
            .update_adr(
                &adr_code,
                &UpdateContentRequest {
                    title: None,
                    content: "stale".into(),
                    version: 1,
                },
            )
            .await,
    );
    match &err {
        Error::Conflict { current, .. } => assert_eq!(current["version"], 2),
        other => panic!("expected Conflict, got {other}"),
    }

    let adr_columns = columns_of(&mut conn, adr_board);
    let adr_edges = edges_of(&mut conn, adr_board);
    let bad_target = invalid_target(&adr_columns, &adr_edges, adr_columns[0]);
    let err = rejection(
        alice
            .transition_adr(&adr_code, &bad_target.to_string())
            .await,
    );
    assert!(matches!(err, Error::InvalidTransition { .. }), "{err}");
    let good_target = valid_target(&adr_edges, adr_columns[0]);
    let body = alice
        .transition_adr(&adr_code, &good_target.to_string())
        .await
        .expect("valid adr transition");
    assert_eq!(body.column_id, Some(good_target.to_string()));

    // Off-board ADR: org-admin-only (A-0006 fallback). alice (member) 403s;
    // svc (admin) creates it.
    let off_board_request = CreateAdrRequest {
        board_id: None,
        column_id: None,
        title: "Off-board decision".into(),
        content: "no board".into(),
        decision_maker: None,
        decision_date: None,
    };
    let err = rejection(alice.create_adr(&off_board_request).await);
    match &err {
        Error::Forbidden {
            capability,
            details,
            ..
        } => {
            assert_eq!(capability.as_deref(), Some("manage_adrs"));
            assert_eq!(details["board_id"], Value::Null);
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    let off_board_adr = svc
        .create_adr(&off_board_request)
        .await
        .expect("svc creates the off-board adr");
    assert_eq!(off_board_adr.board_id, None);
    let off_board_code = off_board_adr.short_code.clone();

    // Transitioning an off-board ADR → 422 ITEM_NOT_ON_BOARD (T-0010).
    let err = rejection(
        svc.transition_adr(&off_board_code, &good_target.to_string())
            .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "ITEM_NOT_ON_BOARD");
        }
        other => panic!("expected 422 ITEM_NOT_ON_BOARD, got {other}"),
    }

    svc.delete_adr(&off_board_code)
        .await
        .expect("deleting off-board adr");

    // =======================================================================
    // Documents: required parent contract, template stamping, inherited
    // authorization, 409, delete
    // =======================================================================
    // Missing parent → 422 VALIDATION (the T-0018 contract).
    let err = rejection(
        alice
            .create_document(&CreateDocumentRequest {
                title: "Orphan".into(),
                content: None,
                template_id: None,
                parent_short_code: None,
            })
            .await,
    );
    match &err {
        Error::Validation {
            status: 422,
            message,
            ..
        } => assert!(message.contains("parent_short_code"), "{message}"),
        other => panic!("expected 422 Validation, got {other}"),
    }

    // Unknown parent → 422; non-workflow parent (an ADR) → 422.
    let err = rejection(
        alice
            .create_document(&CreateDocumentRequest {
                title: "x".into(),
                content: None,
                template_id: None,
                parent_short_code: Some("ACME-I-9999".into()),
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );
    let err = rejection(
        alice
            .create_document(&CreateDocumentRequest {
                title: "x".into(),
                content: None,
                template_id: None,
                parent_short_code: Some(adr_code.clone()),
            })
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // bob holds nothing on the initiative board → 403 with the inherited
    // capability named.
    let err = rejection(
        bob.create_document(&CreateDocumentRequest {
            title: "Spec".into(),
            content: None,
            template_id: None,
            parent_short_code: Some(initiative_code.clone()),
        })
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_documents"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // alice creates the document; the supports edge authorizes against the
    // initiative's board.
    let document = alice
        .create_document(&CreateDocumentRequest {
            title: "API Spec".into(),
            content: Some("endpoint table".into()),
            template_id: None,
            parent_short_code: Some(initiative_code.clone()),
        })
        .await
        .expect("creating document");
    let document_code = document.short_code.clone();
    assert!(document_code.contains("-D-"), "{document_code}");

    // Template stamping (KAIROS-A-0003): provisioning copied the system
    // templates into the tenant.
    let (template_id, template_content): (Uuid, String) = templates::table
        .filter(templates::slug.eq("prd"))
        .select((templates::id, templates::content))
        .first(&mut conn)
        .expect("tenant prd template");
    let stamped = alice
        .create_document(&CreateDocumentRequest {
            title: "PRD for the API".into(),
            content: None,
            template_id: Some(template_id.to_string()),
            parent_short_code: Some(initiative_code.clone()),
        })
        .await
        .expect("creating document from template");
    assert_eq!(stamped.content, template_content);
    assert_eq!(stamped.template_id, Some(template_id.to_string()));

    let body = bob
        .list_documents(Pagination::default())
        .await
        .expect("listing documents");
    assert_eq!(body.total, 2);

    // PATCH via inherited board capability; then the 409.
    let body = alice
        .update_document(
            &document_code,
            &UpdateContentRequest {
                title: None,
                content: "endpoint table v2".into(),
                version: 1,
            },
        )
        .await
        .expect("updating document");
    assert_eq!(body.version, 2);
    let err = rejection(
        alice
            .update_document(
                &document_code,
                &UpdateContentRequest {
                    title: None,
                    content: "stale".into(),
                    version: 1,
                },
            )
            .await,
    );
    match &err {
        Error::Conflict { current, .. } => assert_eq!(current["version"], 2),
        other => panic!("expected Conflict, got {other}"),
    }

    // bob cannot delete it either (same inherited gate).
    let err = rejection(bob.delete_document(&document_code).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    let body = alice
        .delete_document(&document_code)
        .await
        .expect("deleting document");
    assert_eq!(body.cascade_count, 0);

    // =======================================================================
    // Soft-delete cascade: initiative → child task (A-0001 parent edge)
    // =======================================================================
    graph::link_items(
        &mut conn,
        initiative_id,
        task_bob_id,
        RelationshipType::Parent,
        svc_id,
    )
    .expect("linking initiative -> task");
    let body = alice
        .delete_initiative(&initiative_code)
        .await
        .expect("deleting initiative");
    assert_eq!(body.short_code, initiative_code);
    assert_eq!(body.cascade_count, 1);
    assert_eq!(body.cascaded_short_codes[0], task_bob_code);
    let err = rejection(alice.get_task(&task_bob_code).await);
    assert!(
        matches!(err, Error::NotFound { .. }),
        "cascaded child 404s: {err}"
    );

    // =======================================================================
    // 404 matrix: unknown short codes across every family and verb
    // =======================================================================
    let all_kinds = [
        EntityKind::Strategy,
        EntityKind::Initiative,
        EntityKind::Task,
        EntityKind::Document,
        EntityKind::Adr,
    ];
    for kind in all_kinds {
        let err = rejection(get_any(&alice, kind, "ACME-X-9999").await);
        assert!(matches!(err, Error::NotFound { .. }), "GET {kind}: {err}");
        assert_eq!(err.code(), Some("NOT_FOUND"), "GET {kind}: {err}");

        let err = rejection(
            update_any(
                &alice,
                kind,
                "ACME-X-9999",
                &UpdateContentRequest {
                    title: None,
                    content: "x".into(),
                    version: 1,
                },
            )
            .await,
        );
        assert!(matches!(err, Error::NotFound { .. }), "PATCH {kind}: {err}");

        let err = rejection(delete_any(&alice, kind, "ACME-X-9999").await);
        assert!(
            matches!(err, Error::NotFound { .. }),
            "DELETE {kind}: {err}"
        );
    }
    for kind in [
        EntityKind::Strategy,
        EntityKind::Initiative,
        EntityKind::Task,
        EntityKind::Adr,
    ] {
        let err =
            rejection(transition_any(&alice, kind, "ACME-X-9999", &Uuid::nil().to_string()).await);
        assert!(
            matches!(err, Error::NotFound { .. }),
            "transition {kind}: {err}"
        );
    }

    // =======================================================================
    // 403 matrix: bob (no grants beyond manage_tasks) across the other
    // families' creates — each names its capability
    // =======================================================================
    let err = rejection(
        bob.create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.to_string(),
            column_id: None,
            title: "x".into(),
            content: String::new(),
            hypothesis: None,
        })
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_strategies"), "{err}");
        }
        other => panic!("expected Forbidden, got {other}"),
    }
    let err = rejection(
        bob.create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.to_string(),
            column_id: None,
            title: "x".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_initiatives"), "{err}");
        }
        other => panic!("expected Forbidden, got {other}"),
    }
    let err = rejection(
        bob.create_adr(&CreateAdrRequest {
            board_id: Some(adr_board.to_string()),
            column_id: None,
            title: "x".into(),
            content: String::new(),
            decision_maker: None,
            decision_date: None,
        })
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_adrs"), "{err}");
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // --- teardown -----------------------------------------------------------
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
