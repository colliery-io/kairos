//! Integration test for `POST /api/search` (KAIROS-T-0021; contract per
//! KAIROS-A-0007 / S-0005), through the typed `kairos_client::KairosClient`
//! against the booted production router on a real port (KAIROS-T-0024).
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer — tokens come from the password grant
//! with the seeded test users (see `tests/common/mod.rs`). For isolation
//! the test owns the uniquely named scratch database
//! (`kairos_search_endpoint_t0021_test`); the shared `kairos` database is
//! never touched (shared-services discipline).
//!
//! Covered:
//! - every composition example from S-0005's Unified Search section,
//!   verbatim, over HTTP against seeded data (the KAIROS-T-0014 fixture
//!   shape: one strategy -> two initiatives -> four tasks, a document, an
//!   ADR, metadata, blocks edges, one soft-deleted task)
//! - results grouped by type with empty groups OMITTED from the JSON
//!   (asserted at the wire level via `raw_request`, since the typed DTO
//!   deliberately defaults omitted groups); each entity fully typed (the
//!   complete T-0018 DTO)
//! - pagination envelope correctness: total counted before the page is
//!   cut, applied limit/offset echoed, defaults (25/0) applied
//! - 400 `VALIDATION` with field-level detail for: no capability present,
//!   missing traverse depth, over-cap depth, bad entity_type, malformed
//!   dates (+ unknown fields and a blank q)
//! - 401 unauthenticated (raw HTTP: the typed client always injects a
//!   token), 403 non-member (search is a read: tenant-open, no capability
//!   needed beyond membership)
//! - unknown traverse root -> 404
//! - KAIROS-A-0020 / KAIROS-T-0157: archived work is hidden by default and
//!   findable when `filter.include_deleted` asks for it — including beside
//!   a text `q`, on its own as a whole request, and as a traverse root —
//!   and every archived hit comes back marked with `archived_at`

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use reqwest::Method;
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, spawn_server,
    user_token, with_database,
};
use kairos_client::types_search::{
    SearchFilter, SearchRequest, SearchResponse, SearchSort, SearchTraverse, SearchTraverseFrom,
};
use kairos_client::{Error, KairosClient};
use kairos_db::items::{
    self, CreateAdr, CreateDocument, CreateInitiative, CreateStrategy, CreateTask,
};
use kairos_db::models::{
    BoardLevel, FieldType, MetadataDefinition, NewItemMetadata, NewMetadataDefinition,
    NewOrganizationMember, NewUser, OrgRole, RelationshipType, TaskType, User,
};
use kairos_db::schema::{
    boards, item_metadata, metadata_definitions, organization_members, organizations, users,
};
use kairos_db::{TenantPool, create_board, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_search_endpoint_t0021_test";

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// A traverse clause from a short code (the S-0005 examples' shape).
fn traverse_from(
    short_code: &str,
    relationships: &[&str],
    direction: &str,
    depth: Option<u32>,
) -> SearchTraverse {
    SearchTraverse {
        from: SearchTraverseFrom {
            short_code: Some(short_code.to_string()),
            id: None,
        },
        relationships: relationships.iter().map(|r| r.to_string()).collect(),
        direction: direction.to_string(),
        depth,
    }
}

/// Assert the request fails as 400 `VALIDATION` naming `field` in
/// `details.field`.
async fn assert_validation_400(client: &KairosClient, request: SearchRequest, field: &str) {
    let err = rejection(client.search(&request).await);
    match &err {
        Error::Validation {
            status: 400,
            field: got,
            ..
        } => assert_eq!(got.as_deref(), Some(field), "{request:?} -> {err}"),
        other => panic!("expected 400 Validation for {request:?}, got {other}"),
    }
}

/// The names of the NON-EMPTY groups of a typed search response, sorted.
/// (Omission of empty groups from the wire JSON is asserted separately via
/// `raw_request` — the typed DTO defaults omitted groups to empty.)
fn present_groups(body: &SearchResponse) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if !body.results.strategies.is_empty() {
        keys.push("strategies");
    }
    if !body.results.initiatives.is_empty() {
        keys.push("initiatives");
    }
    if !body.results.tasks.is_empty() {
        keys.push("tasks");
    }
    if !body.results.documents.is_empty() {
        keys.push("documents");
    }
    if !body.results.adrs.is_empty() {
        keys.push("adrs");
    }
    keys.sort_unstable();
    keys
}

/// The `short_code` values of one typed result group, sorted.
fn sorted_codes(codes: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut codes: Vec<String> = codes.into_iter().collect();
    codes.sort_unstable();
    codes
}

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The board with this slug in the current tenant schema.
fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    boards::table
        .filter(boards::slug.eq(slug))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

/// The tenant's metadata definition with this slug, creating it if the
/// provisioning defaults did not already seed one (T-0014's approach).
fn metadata_definition(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid {
    if let Some(id) = metadata_definitions::table
        .filter(metadata_definitions::slug.eq(slug))
        .select(metadata_definitions::id)
        .first::<Uuid>(conn)
        .optional()
        .expect("looking up metadata definition")
    {
        return id;
    }
    diesel::insert_into(metadata_definitions::table)
        .values(NewMetadataDefinition {
            name: name.into(),
            slug: slug.into(),
            field_type: FieldType::String,
            is_system_default: false,
        })
        .returning(MetadataDefinition::as_returning())
        .get_result(conn)
        .expect("inserting metadata definition")
        .id
}

fn set_metadata(conn: &mut PgConnection, item_id: Uuid, definition_id: Uuid, value: &str) {
    diesel::insert_into(item_metadata::table)
        .values(NewItemMetadata {
            item_id,
            metadata_definition_id: definition_id,
            value: value.into(),
        })
        .execute(conn)
        .expect("inserting item metadata");
}

#[tokio::test]
async fn search_endpoint_against_live_stack() {
    // --- scratch database + tenant ------------------------------------------
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

    // --- live server + typed clients ------------------------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let server = spawn_server(router).await;
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");

    // --- auth matrix: 401 unauthenticated, 403 non-member ---------------------
    // Unauthenticated is a protocol-level probe (the typed client always
    // injects a bearer token): raw HTTP with no Authorization header.
    let response = http
        .post(format!("{}/api/search", server.base_url))
        .header("x-tenant", "acme")
        .json(&json!({"q": "anything"}))
        .send()
        .await
        .expect("raw unauthenticated request");
    assert_eq!(response.status().as_u16(), 401);
    let body: serde_json::Value = response.json().await.expect("S-0005 envelope");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    // First authenticated requests JIT-provision the users; neither is an
    // org member yet, so both get the membership 403.
    for client in [&alice, &bob] {
        let err = rejection(
            client
                .search(&SearchRequest {
                    q: Some("anything".into()),
                    ..SearchRequest::default()
                })
                .await,
        );
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }
    // Alice becomes a plain member (search needs NO capability grants:
    // reads are tenant-open per A-0006). Bob stays a non-member.
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice_id,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("granting alice membership");

    // --- tenant-schema seeding (the T-0014 fixture shape) ---------------------
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning seeding connection to the tenant schema");
    // Item writes need a tenant-visible creator; seed rows are attributed
    // to a directly-inserted user (T-0014's approach — JIT users live in
    // public.users either way).
    let seeder: Uuid = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "dex|seeder".into(),
            user_name: "dex|seeder".into(),
            email: "seeder@acme.test".into(),
            display_name: "Seeder".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("inserting seed user")
        .id;

    let strategy_board = board_id_by_slug(&mut conn, "strategy");
    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let adr_board = board_id_by_slug(&mut conn, "adrs");
    let delivery_board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(seeder),
    )
    .expect("creating delivery board")
    .id;

    let s1 = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: "Authentication overhaul",
            content: "Rework authentication and identity across the platform",
            hypothesis: None,
        },
        seeder,
    )
    .expect("creating strategy");

    let make_initiative = |conn: &mut PgConnection, title: &str, content: &str| {
        items::create_initiative(
            conn,
            CreateInitiative {
                board_id: initiative_board,
                column_id: None,
                title,
                content,
                complexity: None,
                bucket_type: None,
            },
            seeder,
        )
        .expect("creating initiative")
    };
    let i1 = make_initiative(&mut conn, "Auth service", "OAuth flows and token handling");
    let i2 = make_initiative(
        &mut conn,
        "Session management",
        "Cookies and server-side session storage",
    );

    let make_task = |conn: &mut PgConnection, title: &str, content: &str, task_type| {
        items::create_task(
            conn,
            CreateTask {
                board_id: delivery_board,
                column_id: None,
                title,
                content,
                task_type,
                work_class: kairos_db::models::enums::WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            seeder,
        )
        .expect("creating task")
    };
    let t1 = make_task(
        &mut conn,
        "Implement login endpoint",
        "Wire up the auth login endpoint issuing authentication tokens",
        TaskType::Bug,
    );
    let t2 = make_task(
        &mut conn,
        "Fix logout redirect",
        "Logout leaves a dangling redirect loop",
        TaskType::Bug,
    );
    let t3 = make_task(
        &mut conn,
        "Refactor session store",
        "Move session persistence behind one interface",
        TaskType::TechDebt,
    );
    let t4 = make_task(
        &mut conn,
        "Write onboarding notes",
        "Collect the delivery onboarding notes",
        TaskType::Task,
    );

    let d1 = items::create_document(
        &mut conn,
        CreateDocument {
            title: "Search design notes",
            content: Some("Hydration pipeline and traversal sketches"),
            template_id: None,
        },
        seeder,
    )
    .expect("creating document");
    let a1 = items::create_adr(
        &mut conn,
        CreateAdr {
            board_id: Some(adr_board),
            column_id: None,
            title: "Unified search endpoint",
            content: "One endpoint, composable query capabilities",
            decision_maker: None,
            decision_date: None,
        },
        seeder,
    )
    .expect("creating adr");

    let link = |conn: &mut PgConnection, source: Uuid, target: Uuid, rel| {
        graph::link_items(conn, source, target, rel, seeder)
            .unwrap_or_else(|e| panic!("linking {source} -{rel:?}-> {target}: {e}"));
    };
    link(&mut conn, s1.id, i1.id, RelationshipType::Parent);
    link(&mut conn, s1.id, i2.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t1.id, RelationshipType::Parent);
    link(&mut conn, i1.id, t2.id, RelationshipType::Parent);
    link(&mut conn, i2.id, t3.id, RelationshipType::Parent);
    link(&mut conn, i2.id, t4.id, RelationshipType::Parent);
    link(&mut conn, i1.id, d1.id, RelationshipType::Supports);
    link(&mut conn, s1.id, a1.id, RelationshipType::Supports);
    link(&mut conn, t3.id, i1.id, RelationshipType::Blocks);
    link(&mut conn, t2.id, t1.id, RelationshipType::Blocks);

    let priority = metadata_definition(&mut conn, "Priority", "priority");
    let component = metadata_definition(&mut conn, "Component", "component");
    set_metadata(&mut conn, t1.id, priority, "critical");
    set_metadata(&mut conn, t1.id, component, "auth-api");
    set_metadata(&mut conn, t2.id, priority, "low");
    set_metadata(&mut conn, t2.id, component, "web-ui");

    // Soft-delete t4 AFTER linking so its parent edge stays in the graph.
    items::soft_delete_item(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        t4.id,
        seeder,
    )
    .expect("soft-deleting t4");

    // Bob is authenticated but still not a member: 403 even after data
    // exists (the non-member matrix entry).
    let err = rejection(
        bob.search(&SearchRequest {
            q: Some("authentication".into()),
            ..SearchRequest::default()
        })
        .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");

    // ==========================================================================
    // S-0005 composition example 1: full-text search
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            q: Some("authentication".into()),
            ..SearchRequest::default()
        })
        .await
        .expect("full-text search");
    assert_eq!(present_groups(&body), ["strategies", "tasks"], "{body:?}");
    assert_eq!(
        body.results.strategies[0].short_code,
        s1.short_code.as_str()
    );
    assert_eq!(body.results.tasks[0].short_code, t1.short_code.as_str());
    assert_eq!(body.total, 2);
    assert_eq!(body.limit, 25, "default limit applied and echoed");
    assert_eq!(body.offset, 0, "default offset applied and echoed");

    // Wire shape: empty groups (initiatives/documents/adrs) are OMITTED
    // from the JSON, not sent as empty arrays (S-0005; raw probe because
    // the typed DTO cannot see the difference).
    let (status, raw) = alice
        .raw_request(
            Method::POST,
            "/api/search",
            Some(&json!({"q": "authentication"})),
        )
        .await
        .expect("raw wire-shape probe");
    assert_eq!(status, 200, "{raw}");
    let mut keys: Vec<&str> = raw["results"]
        .as_object()
        .expect("results object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["strategies", "tasks"],
        "empty groups are omitted from the wire JSON: {raw}"
    );

    // Entities come back fully typed — the complete T-0018 task DTO.
    let task = &body.results.tasks[0];
    assert_eq!(task.id, t1.id.to_string());
    assert_eq!(task.short_code, t1.short_code);
    assert_eq!(task.title, "Implement login endpoint");
    assert_eq!(task.task_type, "bug");
    assert_eq!(task.board_id, delivery_board.to_string());
    assert_eq!(task.column_id, t1.column_id.to_string());
    assert_eq!(task.team_id, None);
    assert_eq!(task.version, 1);
    assert_eq!(task.created_by, seeder.to_string());
    assert!(!task.created_at.is_empty() && !task.updated_at.is_empty());
    let strategy = &body.results.strategies[0];
    assert_eq!(strategy.short_code, s1.short_code);
    assert_eq!(
        strategy.content,
        "Rework authentication and identity across the platform"
    );

    // ==========================================================================
    // S-0005 composition example 2: structured filter — all critical bugs
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                task_type: Some(vec!["bug".into()]),
                work_class: None,
                metadata: Some(BTreeMap::from([(
                    "priority".to_string(),
                    "critical".to_string(),
                )])),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        })
        .await
        .expect("structured filter");
    assert_eq!(present_groups(&body), ["tasks"]);
    assert_eq!(body.results.tasks[0].short_code, t1.short_code.as_str());
    assert_eq!(body.total, 1);

    // ==========================================================================
    // S-0005 composition example 3: graph — all tasks under strategy s1
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            traverse: Some(traverse_from(
                &s1.short_code,
                &["parent"],
                "outbound",
                Some(5),
            )),
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        })
        .await
        .expect("graph traversal");
    assert_eq!(present_groups(&body), ["tasks"]);
    let expected = sorted_codes([
        t1.short_code.clone(),
        t2.short_code.clone(),
        t3.short_code.clone(),
    ]);
    assert_eq!(
        sorted_codes(body.results.tasks.iter().map(|t| t.short_code.clone())),
        expected,
        "live task descendants only; soft-deleted t4 excluded"
    );
    assert_eq!(body.total, 3);

    // ==========================================================================
    // S-0005 composition example 4: combined q + traverse + filter —
    // search "auth" in tasks descended from s1
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            q: Some("auth".into()),
            traverse: Some(traverse_from(
                &s1.short_code,
                &["parent"],
                "outbound",
                Some(5),
            )),
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        })
        .await
        .expect("combined composition");
    assert_eq!(present_groups(&body), ["tasks"]);
    assert_eq!(body.results.tasks[0].short_code, t1.short_code.as_str());
    assert_eq!(body.total, 1);

    // ==========================================================================
    // S-0005 composition example 5: everything blocking initiative i1
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            traverse: Some(traverse_from(
                &i1.short_code,
                &["blocks"],
                "inbound",
                Some(1),
            )),
            ..SearchRequest::default()
        })
        .await
        .expect("inbound blocks traversal");
    assert_eq!(present_groups(&body), ["tasks"]);
    assert_eq!(body.results.tasks[0].short_code, t3.short_code.as_str());
    assert_eq!(body.total, 1);

    // ==========================================================================
    // Pagination envelope: total pre-pagination, applied limit/offset echoed
    // ==========================================================================
    let body = alice
        .search(&SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                board_id: Some(delivery_board.to_string()),
                ..SearchFilter::default()
            }),
            sort: Some(SearchSort {
                field: "title".into(),
                order: "asc".into(),
            }),
            limit: Some(2),
            offset: Some(1),
            ..SearchRequest::default()
        })
        .await
        .expect("paginated search");
    assert_eq!(body.total, 3, "total is counted before the page is cut");
    assert_eq!(body.limit, 2);
    assert_eq!(body.offset, 1);
    let titles: Vec<&str> = body
        .results
        .tasks
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Implement login endpoint", "Refactor session store"],
        "title asc, offset 1: 'Fix logout redirect' is skipped"
    );

    // An offset past the result set: empty results object, envelope intact.
    // (Raw probe: "ALL groups omitted" is a wire-shape property.)
    let (status, raw) = alice
        .raw_request(
            Method::POST,
            "/api/search",
            Some(&json!({"q": "authentication", "limit": 10, "offset": 5})),
        )
        .await
        .expect("raw past-offset probe");
    assert_eq!(status, 200, "{raw}");
    assert_eq!(raw["total"], 2);
    assert_eq!(raw["limit"], 10);
    assert_eq!(raw["offset"], 5);
    assert_eq!(
        raw["results"].as_object().expect("results object").len(),
        0,
        "all groups omitted: {raw}"
    );

    // ==========================================================================
    // 400 VALIDATION matrix (field-level detail per S-0005 envelope)
    // ==========================================================================
    // No capability present: q/filter/traverse all absent.
    let err = rejection(alice.search(&SearchRequest::default()).await);
    match &err {
        Error::Validation {
            status: 400,
            details,
            ..
        } => assert_eq!(details["fields"], json!(["q", "filter", "traverse"])),
        other => panic!("expected 400 Validation, got {other}"),
    }
    // A filter that says NOTHING is still no capability — `{}` on the wire.
    let (status, raw) = alice
        .raw_request(Method::POST, "/api/search", Some(&json!({"filter": {}})))
        .await
        .expect("raw empty-filter probe");
    assert_eq!(status, 400, "{raw}");
    assert_eq!(error_code(&raw), "VALIDATION");

    // Missing traverse depth (required per A-0007).
    assert_validation_400(
        &alice,
        SearchRequest {
            traverse: Some(traverse_from(&s1.short_code, &["parent"], "outbound", None)),
            ..SearchRequest::default()
        },
        "traverse.depth",
    )
    .await;

    // Depth over the server cap.
    let err = rejection(
        alice
            .search(&SearchRequest {
                traverse: Some(traverse_from(
                    &s1.short_code,
                    &["parent"],
                    "outbound",
                    Some(11),
                )),
                ..SearchRequest::default()
            })
            .await,
    );
    match &err {
        Error::Validation {
            status: 400,
            field,
            details,
            ..
        } => {
            assert_eq!(field.as_deref(), Some("traverse.depth"));
            assert_eq!(details["depth"], 11);
            assert_eq!(details["cap"], 10);
        }
        other => panic!("expected 400 Validation, got {other}"),
    }

    // Bad entity_type.
    assert_validation_400(
        &alice,
        SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec!["epic".into()]),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        },
        "filter.entity_type",
    )
    .await;

    // Malformed dates.
    assert_validation_400(
        &alice,
        SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                created_after: Some("March 1st 2026".into()),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        },
        "filter.created_after",
    )
    .await;
    assert_validation_400(
        &alice,
        SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                created_before: Some("2026-13-40".into()),
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        },
        "filter.created_before",
    )
    .await;

    // A blank q and a limit over the cap round out the validation surface.
    assert_validation_400(
        &alice,
        SearchRequest {
            q: Some("   ".into()),
            ..SearchRequest::default()
        },
        "q",
    )
    .await;
    assert_validation_400(
        &alice,
        SearchRequest {
            q: Some("auth".into()),
            limit: Some(101),
            ..SearchRequest::default()
        },
        "limit",
    )
    .await;

    // Unknown fields are typos, not silently ignored (mirrors the core
    // request model): 400 VALIDATION. Not expressible in the typed request
    // (deny_unknown_fields) — raw probe.
    let (status, raw) = alice
        .raw_request(Method::POST, "/api/search", Some(&json!({"query": "auth"})))
        .await
        .expect("raw unknown-field probe");
    assert_eq!(status, 400, "{raw}");
    assert_eq!(error_code(&raw), "VALIDATION");

    // ==========================================================================
    // Unknown traverse root -> 404
    // ==========================================================================
    let err = rejection(
        alice
            .search(&SearchRequest {
                traverse: Some(traverse_from(
                    "ACME-S-9999",
                    &["parent"],
                    "outbound",
                    Some(1),
                )),
                ..SearchRequest::default()
            })
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    assert_eq!(err.code(), Some("NOT_FOUND"), "{err}");

    // ==========================================================================
    // KAIROS-A-0020 / KAIROS-T-0157: archived work is searchable WHEN ASKED
    // ==========================================================================
    // t4 ("Write onboarding notes") is the soft-deleted fixture row, and
    // "onboarding" appears in nothing else.
    let archived = |extra: SearchFilter| SearchRequest {
        q: Some("onboarding".into()),
        filter: Some(extra),
        ..SearchRequest::default()
    };
    // Default: hidden. This is rule 3, and nothing below may weaken it.
    let body = alice
        .search(&archived(SearchFilter::default()))
        .await
        .expect("text search, default liveness");
    assert_eq!(
        body.total, 0,
        "a text search hides archived work by default"
    );

    // Asked for: found. Until T-0157 this returned nothing — the full-text
    // candidate set was built live-only, so the archived id was intersected
    // away before `include_deleted` was ever consulted. The flag was not
    // rejected next to `q`, it was ignored, which is the worse failure: the
    // caller was told "no matches" instead of "not supported".
    let body = alice
        .search(&archived(SearchFilter {
            include_deleted: true,
            ..SearchFilter::default()
        }))
        .await
        .expect("text search including archived");
    assert_eq!(present_groups(&body), ["tasks"]);
    assert_eq!(body.results.tasks[0].short_code, t4.short_code.as_str());
    assert!(
        body.results.tasks[0].archived_at.is_some(),
        "an archived hit is served MARKED — an auditor must never mistake \
         retired work for live work"
    );

    // `include_deleted` on its own is a whole question, not an empty one.
    let body = alice
        .search(&SearchRequest {
            filter: Some(SearchFilter {
                include_deleted: true,
                ..SearchFilter::default()
            }),
            ..SearchRequest::default()
        })
        .await
        .expect("include_deleted alone is a complete request");
    assert!(
        body.results
            .tasks
            .iter()
            .any(|t| t.short_code == t4.short_code.as_str()),
        "\"show me everything, archived included\" reaches the archived row"
    );

    // Traverse FROM archived work: 404 by default, its subgraph when asked.
    // t4 has no descendants, so i2 — which does — is archived for this
    // probe and put back immediately afterwards. The stamp is written
    // directly rather than through `soft_delete_item`, which cascades:
    // archiving the descendants too would prove nothing about whether the
    // ROOT lookup honours the flag.
    let archive_i2 = |conn: &mut PgConnection, at: &str| {
        sql_query(format!(
            "UPDATE initiatives SET deleted_at = {at} WHERE id = $1"
        ))
        .bind::<diesel::sql_types::Uuid, _>(i2.id)
        .execute(conn)
        .expect("stamping i2's deleted_at");
    };
    archive_i2(&mut conn, "now()");
    let from_i2 = |filter: Option<SearchFilter>| SearchRequest {
        traverse: Some(traverse_from(
            &i2.short_code,
            &["parent"],
            "outbound",
            Some(1),
        )),
        filter,
        ..SearchRequest::default()
    };
    let err = rejection(alice.search(&from_i2(None)).await);
    assert_eq!(
        err.code(),
        Some("NOT_FOUND"),
        "an archived root is still invisible by default: {err}"
    );
    let body = alice
        .search(&from_i2(Some(SearchFilter {
            include_deleted: true,
            ..SearchFilter::default()
        })))
        .await
        .expect("traverse from an archived root");
    assert!(
        body.results
            .tasks
            .iter()
            .any(|t| t.short_code == t3.short_code.as_str()),
        "\"what hung off this once?\" is the audit question, and it now has \
         an answer instead of a 404"
    );
    archive_i2(&mut conn, "NULL");

    // --- teardown --------------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
