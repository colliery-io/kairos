//! COLLIERY-T-0262 — an entry of `GET /api/activity` about an item has the
//! short code and the title of the item, against the booted production
//! router.
//!
//! Before this ticket the entry had only the id of the item. The page of
//! the activity read the first 200 items of each type to make the links,
//! so an entry for an item after the first 200 had no link.
//!
//! - an entry for an item that is not in the first 200 of its type has
//!   `entity_short_code` and `entity_title`,
//! - an entry for an archived item has them too, and `entity_archived_at`,
//! - an entry that is not about an item (a team) has null in the three
//!   fields, and so has an entry for an item that Kairos does not have.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

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

/// The number of tasks of the test: more than one page of a list route
/// (200 items at most).
const TASKS: usize = 205;

#[tokio::test]
async fn an_entry_has_the_short_code_of_its_item_against_live_stack() {
    let mut stack = Stack::boot("kairos_activity_links_t0262_test").await;

    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let team_id = team["id"].as_str().expect("team id").to_string();
    let mut tasks = Vec::with_capacity(TASKS);
    for number in 0..TASKS {
        let task = stack
            .ok(
                Method::POST,
                "/api/tasks",
                Some(json!({"board_id": "payments-delivery", "title": format!("Task {number}")})),
            )
            .await;
        tasks.push(task);
    }

    // --- an item that is not in the first 200 of its type ---------------------
    let first_page = stack.ok(Method::GET, "/api/tasks?limit=200", None).await;
    assert_eq!(first_page["total"], TASKS, "{first_page}");
    let listed: Vec<&str> = first_page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|task| task["id"].as_str().expect("id"))
        .collect();
    assert_eq!(listed.len(), 200, "the list route gives 200 items at most");
    let later: Vec<&Value> = tasks
        .iter()
        .filter(|task| !listed.contains(&task["id"].as_str().expect("id")))
        .collect();
    assert_eq!(later.len(), TASKS - 200);
    for task in &later {
        let id = task["id"].as_str().expect("id");
        let feed = stack
            .ok(Method::GET, &format!("/api/activity?entity_id={id}"), None)
            .await;
        assert_eq!(feed["total"], 1, "{feed}");
        let entry = &feed["items"][0];
        assert_eq!(entry["action"], "create", "{entry}");
        assert_eq!(entry["entity_type"], "task", "{entry}");
        // THE DEFECT: before COLLIERY-T-0262 the entry had no short code,
        // and the page found no link for this item.
        assert_eq!(entry["entity_short_code"], task["short_code"], "{entry}");
        assert_eq!(entry["entity_title"], task["title"], "{entry}");
        assert_eq!(entry["entity_archived_at"], Value::Null, "{entry}");
    }

    // --- each entry about an item, on a page of the feed ----------------------
    let feed = stack.ok(Method::GET, "/api/activity?limit=200", None).await;
    let entries = feed["items"].as_array().expect("items");
    assert_eq!(entries.len(), 200, "{feed}");
    for entry in entries {
        if entry["entity_type"] == "task" {
            let task = tasks
                .iter()
                .find(|task| task["id"] == entry["entity_id"])
                .unwrap_or_else(|| panic!("the task of the entry: {entry}"));
            assert_eq!(entry["entity_short_code"], task["short_code"], "{entry}");
        }
    }

    // --- an archived item -----------------------------------------------------
    let archived = later[0];
    let code = archived["short_code"].as_str().expect("short code");
    let id = archived["id"].as_str().expect("id");
    stack
        .ok(Method::DELETE, &format!("/api/tasks/{code}"), None)
        .await;
    let feed = stack
        .ok(
            Method::GET,
            &format!("/api/activity?entity_id={id}&action=delete"),
            None,
        )
        .await;
    assert_eq!(feed["total"], 1, "{feed}");
    let entry = &feed["items"][0];
    assert_eq!(entry["entity_short_code"], code, "{entry}");
    assert_eq!(entry["entity_title"], archived["title"], "{entry}");
    assert!(entry["entity_archived_at"].is_string(), "{entry}");
    // A person can open the archived item.
    let item = stack
        .ok(Method::GET, &format!("/api/tasks/{code}"), None)
        .await;
    assert!(item["archived_at"].is_string(), "{item}");

    // --- an entry that is not about an item -----------------------------------
    let feed = stack
        .ok(
            Method::GET,
            &format!("/api/activity?entity_id={team_id}"),
            None,
        )
        .await;
    assert_eq!(feed["total"], 1, "{feed}");
    let entry = &feed["items"][0];
    assert_eq!(entry["entity_type"], "team", "{entry}");
    assert_eq!(entry["entity_short_code"], Value::Null, "{entry}");
    assert_eq!(entry["entity_title"], Value::Null, "{entry}");
    assert_eq!(entry["entity_archived_at"], Value::Null, "{entry}");

    // --- an item that Kairos does not have -------------------------------------
    let gone = Uuid::new_v4();
    diesel::sql_query(format!(
        "INSERT INTO activity_log (actor_id, action, entity_id, entity_type, details) \
         SELECT actor_id, 'create', '{gone}', 'task', 'task:gone' FROM activity_log LIMIT 1"
    ))
    .execute(&mut stack.conn)
    .expect("an entry for an item that Kairos does not have");
    let feed = stack
        .ok(
            Method::GET,
            &format!("/api/activity?entity_id={gone}"),
            None,
        )
        .await;
    assert_eq!(feed["total"], 1, "{feed}");
    let entry = &feed["items"][0];
    assert_eq!(entry["entity_type"], "task", "{entry}");
    assert_eq!(entry["entity_short_code"], Value::Null, "{entry}");
    assert_eq!(entry["entity_title"], Value::Null, "{entry}");

    assert_eq!(count(&mut stack.conn, "tasks"), TASKS as i64);
    stack.shutdown();
}
