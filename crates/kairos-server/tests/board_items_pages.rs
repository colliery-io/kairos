//! COLLIERY-T-0261 — `GET /api/boards/{id}/items` has pages, against the
//! booted production router.
//!
//! Before this ticket the route had no `limit`: one response had each item
//! of the board, for a board of each size.
//!
//! - `limit` and `offset` give a page, and `total` is the number of items
//!   on all pages,
//! - the pages of a board have no item in common, and together they have
//!   each item, in the order of the board,
//! - the order is stable: the position of the column, the type, the short
//!   code,
//! - a filter (`repository`, `include_deleted`) applies before the page,
//! - `children_progress` and `blocks_summary` have the items of the page,
//! - the server changes a `limit` that is out of the limits, as the other
//!   list routes do.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

use std::collections::BTreeSet;

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The booted router on a scratch database, with `svc` as the org admin.
struct Stack {
    scratch_db: &'static str,
    admin_conn: PgConnection,
    /// Pinned to the tenant schema.
    conn: PgConnection,
    pool: TenantPool,
    router: Router,
    token: String,
}

impl Stack {
    async fn boot(scratch_db: &'static str) -> Self {
        let admin_conn = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme org row");

        let http = reqwest::Client::new();
        let token = user_token(&http, "svc").await;
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        // The routes of the local accounts are there only with local auth.
        let mut config = base_config(&scratch_url);
        config.local_auth = true;
        let router = app::router(app::state_with(config, pool.clone(), auth));

        // JIT-provision svc, then make it the org admin.
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&token),
            &TENANT,
            None,
        )
        .await;
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
            .expect("granting membership");
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");

        Stack {
            scratch_db,
            admin_conn,
            conn,
            pool,
            router,
            token,
        }
    }

    /// One request as the org admin.
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    /// One request that must pass. Returns the body.
    async fn ok(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let (status, answer) = self.send(method.clone(), uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {answer}");
        answer
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// The number of rows that a query counts, in the tenant.
fn count(conn: &mut PgConnection, from_where: &str) -> i64 {
    diesel::sql_query(format!("SELECT count(*) AS count FROM {from_where}"))
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("counting {from_where}: {e}"))
        .count
}

/// The short codes of a response, in the order of the response: column
/// after column, and in a column the strategies, the initiatives, the
/// tasks and the ADRs.
fn codes(response: &Value) -> Vec<String> {
    let mut codes = Vec::new();
    for column in response["columns"].as_array().expect("columns") {
        for family in ["strategies", "initiatives", "tasks", "adrs"] {
            for item in column[family].as_array().expect("the items of a type") {
                codes.push(item["short_code"].as_str().expect("short code").to_string());
            }
        }
    }
    codes
}

/// The keys of one of the two maps of a response.
fn keys(response: &Value, map: &str) -> BTreeSet<String> {
    response[map]
        .as_object()
        .unwrap_or_else(|| panic!("{map} is a map: {response}"))
        .keys()
        .cloned()
        .collect()
}

/// Each page of a list, with `limit` items on a page. Examines each page:
/// `total`, `limit`, `offset`, the columns, and the two maps.
async fn pages(stack: &Stack, uri: &str, limit: usize, total: usize) -> Vec<Vec<String>> {
    let separator = if uri.contains('?') { '&' } else { '?' };
    let mut pages = Vec::new();
    let mut offset = 0;
    loop {
        let page = stack
            .ok(
                Method::GET,
                &format!("{uri}{separator}limit={limit}&offset={offset}"),
                None,
            )
            .await;
        assert_eq!(page["total"], total, "{page}");
        assert_eq!(page["limit"], limit, "{page}");
        assert_eq!(page["offset"], offset, "{page}");
        let on_page = codes(&page);
        assert!(on_page.len() <= limit, "{page}");
        // The two maps have the items of the page only.
        let on_page_set: BTreeSet<String> = on_page.iter().cloned().collect();
        for map in ["children_progress", "blocks_summary"] {
            let foreign: Vec<String> = keys(&page, map).difference(&on_page_set).cloned().collect();
            assert!(foreign.is_empty(), "{map} has {foreign:?}: {page}");
        }
        offset += on_page.len();
        let last = on_page.is_empty() || offset >= total;
        pages.push(on_page);
        if last {
            return pages;
        }
    }
}

#[tokio::test]
async fn the_items_of_a_board_come_in_pages_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_items_pages_t0261_test").await;

    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("board id")
        .to_string();
    let board = stack
        .ok(Method::GET, &format!("/api/boards/{board_id}"), None)
        .await;
    let columns: Vec<String> = board["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .filter(|column| column["is_done"] == false)
        .map(|column| column["id"].as_str().expect("column id").to_string())
        .collect();
    assert!(columns.len() >= 3, "3 columns that are not done: {board}");
    stack
        .ok(
            Method::POST,
            "/api/repositories",
            Some(json!({
                "slug": "payments-api", "forge": "github",
                "repo_full_name": "acme/payments-api",
                "repo_url": "https://github.com/acme/payments-api",
                "team": "payments",
            })),
        )
        .await;

    // 12 tasks in 3 columns. The order of the create is not the order of
    // the board: task 0 goes to column 2, task 1 to column 1, task 2 to
    // column 0, and so on. Each fourth task has the repository.
    let mut tasks = Vec::new();
    for number in 0..12 {
        let mut body = json!({
            "board_id": board_id,
            "column_id": columns[2 - number % 3],
            "title": format!("Task {number}"),
        });
        if number % 4 == 0 {
            body["repository"] = json!("payments-api");
        }
        tasks.push(stack.ok(Method::POST, "/api/tasks", Some(body)).await);
    }
    let code = |number: usize| {
        tasks[number]["short_code"]
            .as_str()
            .expect("code")
            .to_string()
    };
    // Task 2 (column 0) blocks task 9 (column 2): the two ends are on
    // different pages when a page has 5 items.
    stack
        .ok(
            Method::POST,
            "/api/relationships",
            Some(json!({
                "source_short_code": code(2),
                "target_short_code": code(9),
                "relationship": "blocks",
            })),
        )
        .await;

    // --- the full board, with no parameter -----------------------------------
    let items_uri = format!("/api/boards/{board_id}/items");
    let full = stack.ok(Method::GET, &items_uri, None).await;
    assert_eq!(full["total"], 12, "{full}");
    assert_eq!(full["limit"], 200, "the default of limit: {full}");
    assert_eq!(full["offset"], 0, "{full}");
    let order = codes(&full);
    assert_eq!(order.len(), 12, "{full}");
    // The order: the position of the column, then the short code.
    let mut expected: Vec<(usize, String)> = (0..12).map(|n| (n % 3, code(n))).collect();
    expected.sort_by_key(|(column, code)| (2 - column, code.clone()));
    let expected: Vec<String> = expected.into_iter().map(|(_, code)| code).collect();
    assert_eq!(order, expected, "{full}");
    assert_eq!(
        keys(&full, "blocks_summary"),
        BTreeSet::from([code(2), code(9)]),
        "{full}"
    );

    // --- pages of 5 items -------------------------------------------------------
    let read = pages(&stack, &items_uri, 5, 12).await;
    // THE DEFECT: before COLLIERY-T-0261 the route refused `limit`, and the
    // response had each item.
    assert_eq!(
        read.iter().map(Vec::len).collect::<Vec<_>>(),
        [5, 5, 2],
        "{read:?}"
    );
    let together: Vec<String> = read.iter().flatten().cloned().collect();
    assert_eq!(together, order, "the pages give the board, in its order");
    let distinct: BTreeSet<&String> = together.iter().collect();
    assert_eq!(distinct.len(), 12, "no item is on 2 pages");
    // The order is stable: the same pages, a second time.
    assert_eq!(pages(&stack, &items_uri, 5, 12).await, read);
    // Each column is in each page, and the maps go with the items.
    let first = stack
        .ok(Method::GET, &format!("{items_uri}?limit=5"), None)
        .await;
    assert_eq!(
        first["columns"].as_array().expect("columns").len(),
        full["columns"].as_array().expect("columns").len(),
        "{first}"
    );
    assert_eq!(
        keys(&first, "blocks_summary"),
        BTreeSet::from([code(2)]),
        "task 2 is on the first page, and task 9 is not: {first}"
    );
    assert_eq!(first["blocks_summary"][code(2)]["blocks"], 1, "{first}");
    let last = stack
        .ok(Method::GET, &format!("{items_uri}?limit=5&offset=10"), None)
        .await;
    assert_eq!(
        keys(&last, "blocks_summary"),
        BTreeSet::from([code(9)]),
        "{last}"
    );
    assert_eq!(last["blocks_summary"][code(9)]["blocked_by"], 1, "{last}");

    // --- a filter applies before the page ---------------------------------------
    let with_repository = format!("{items_uri}?repository=payments-api");
    let read = pages(&stack, &with_repository, 2, 3).await;
    let together: BTreeSet<String> = read.iter().flatten().cloned().collect();
    assert_eq!(together, BTreeSet::from([code(0), code(4), code(8)]));
    assert_eq!(read.iter().map(Vec::len).collect::<Vec<_>>(), [2, 1]);

    for number in [1, 5] {
        stack
            .ok(
                Method::DELETE,
                &format!("/api/tasks/{}", code(number)),
                None,
            )
            .await;
    }
    let live = pages(&stack, &items_uri, 4, 10).await;
    let live: Vec<String> = live.into_iter().flatten().collect();
    let expected_live: Vec<String> = order
        .iter()
        .filter(|item| **item != code(1) && **item != code(5))
        .cloned()
        .collect();
    assert_eq!(live, expected_live);
    let archived = pages(&stack, &format!("{items_uri}?include_deleted=true"), 4, 12).await;
    let archived: Vec<String> = archived.into_iter().flatten().collect();
    assert_eq!(archived, order, "the archived cards are in their places");

    // --- a value that is out of the limits ---------------------------------------
    // The rule of the other list routes: the server changes the value.
    for (query, limit, offset, shown) in [
        ("limit=5000", 1000, 0, 10),
        ("limit=1000", 1000, 0, 10),
        ("limit=0", 1, 0, 1),
        ("limit=-3", 1, 0, 1),
        ("offset=-1", 200, 0, 10),
        ("offset=100", 200, 100, 0),
    ] {
        let page = stack
            .ok(Method::GET, &format!("{items_uri}?{query}"), None)
            .await;
        assert_eq!(page["limit"], limit, "{query}: {page}");
        assert_eq!(page["offset"], offset, "{query}: {page}");
        assert_eq!(page["total"], 10, "{query}: {page}");
        assert_eq!(codes(&page).len(), shown, "{query}: {page}");
    }
    let (status, answer) = stack
        .send(Method::GET, &format!("{items_uri}?limit=many"), None)
        .await;
    assert!(status.is_client_error(), "{status} {answer}");

    // --- the items of 2 types, and `children_progress` ----------------------------
    let plans = stack
        .ok(
            Method::POST,
            "/api/boards",
            Some(json!({"name": "Plans", "slug": "plans", "board_level": "initiative", "code_prefix": "PLANS"})),
        )
        .await;
    let plans_id = plans["id"].as_str().expect("board id").to_string();
    let mut initiatives = Vec::new();
    for number in 0..3 {
        let initiative = stack
            .ok(
                Method::POST,
                "/api/initiatives",
                Some(json!({"board_id": "plans", "title": format!("Initiative {number}")})),
            )
            .await;
        initiatives.push(
            initiative["short_code"]
                .as_str()
                .expect("short code")
                .to_string(),
        );
    }
    // The last initiative of the board is the parent of 2 tasks.
    for number in [2, 3] {
        stack
            .ok(
                Method::POST,
                "/api/relationships",
                Some(json!({
                    "source_short_code": initiatives[2],
                    "target_short_code": code(number),
                    "relationship": "parent",
                })),
            )
            .await;
    }
    let plans_uri = format!("/api/boards/{plans_id}/items");
    let full = stack.ok(Method::GET, &plans_uri, None).await;
    assert_eq!(codes(&full), initiatives, "{full}");
    assert_eq!(
        keys(&full, "children_progress"),
        BTreeSet::from([initiatives[2].clone()]),
        "{full}"
    );
    let read = pages(&stack, &plans_uri, 2, 3).await;
    assert_eq!(read, [initiatives[..2].to_vec(), initiatives[2..].to_vec()]);
    let first = stack
        .ok(Method::GET, &format!("{plans_uri}?limit=2"), None)
        .await;
    assert!(keys(&first, "children_progress").is_empty(), "{first}");
    let second = stack
        .ok(Method::GET, &format!("{plans_uri}?limit=2&offset=2"), None)
        .await;
    assert_eq!(
        second["children_progress"][&initiatives[2]]["total"], 2,
        "{second}"
    );

    assert_eq!(count(&mut stack.conn, "tasks"), 12);
    stack.shutdown();
}
