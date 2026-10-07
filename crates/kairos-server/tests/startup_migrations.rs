//! KAIROS-T-0330 (the 2026-10-07 amendment of KAIROS-A-0009): the server
//! runs the tenant migrations when it starts, and a tenant that cannot
//! migrate answers with an error while the other tenants serve.
//!
//! Runs the real `kairos-server serve` binary against a scratch database
//! with two tenants:
//!
//! - `acme` has a pending tenant migration (the newest one is undone);
//! - `broken` cannot migrate (its `item_impacts` table is gone).
//!
//! Two servers start at the same time: exactly one applies the migration of
//! `acme` (the advisory lock), and both serve. `acme` answers; `broken`
//! answers 503 `TENANT_NOT_READY`. After the schema of `broken` is repaired
//! and `kairos-server migrate-tenants` runs, `broken` answers within the
//! re-check interval, with no restart.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`).

mod common;

use std::io::Read as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{drop_scratch_db, recreate_scratch_db, with_database};
use kairos_db::local_auth as db_local_auth;
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{provision_tenant, run_public_migrations};
use kairos_server::local_auth::hash_password;

const SCRATCH_DB: &str = "kairos_startup_migrations_t0330_test";
const NEWEST: &str = "20261006000000";
const PASSWORD: &str = "correct-horse-battery-staple";
const DOWN_SQL: &str =
    include_str!("../../kairos-db/migrations/tenant/2026-10-06-000000_item_teams/down.sql");

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn has_version(conn: &mut PgConnection, schema: &str) -> bool {
    sql_query(format!(
        "SELECT count(*) AS count FROM \"{schema}\".__diesel_schema_migrations \
          WHERE version = '{NEWEST}'"
    ))
    .get_result::<Count>(conn)
    .expect("count")
    .count
        == 1
}

fn org_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    organizations::table
        .filter(organizations::slug.eq(slug))
        .select(organizations::id)
        .first(conn)
        .expect("org id")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("addr")
        .port()
}

/// `kairos-server serve` on `port`, with local accounts and no IdP.
fn spawn_server(url: &str, port: u16) -> Child {
    Command::new(env!("CARGO_BIN_EXE_kairos-server"))
        .arg("serve")
        .env("DATABASE_URL", url)
        .env("KAIROS_BIND_ADDR", format!("127.0.0.1:{port}"))
        .env("KAIROS_LOCAL_AUTH", "true")
        .env("KAIROS_BASE_DOMAIN", "kairos.test")
        .env("KAIROS_EMBED_PROVIDER", "none")
        .env("KAIROS_EMBED_REFRESH_SECS", "0")
        .env("KAIROS_AUTH_MAX_FAILURES", "0")
        .env_remove("OIDC_ISSUER_URL")
        .env_remove("OIDC_AUDIENCE")
        .env_remove("KAIROS_SINGLE_TENANT")
        .env_remove("KAIROS_CODE_INDEX_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairos-server serve")
}

/// Stop a server and give back what it printed.
fn stop(mut child: Child) -> String {
    let _ = child.kill();
    let _ = child.wait();
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut out);
    }
    out
}

async fn wait_ready(http: &reqwest::Client, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(r) = http
            .get(format!("http://127.0.0.1:{port}/readyz"))
            .send()
            .await
            && r.status().is_success()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the server on {port} did not get ready"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn whoami(http: &reqwest::Client, port: u16, token: &str, tenant: &str) -> (u16, Value) {
    let response = http
        .get(format!("http://127.0.0.1:{port}/api/whoami"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-tenant", tenant)
        .send()
        .await
        .expect("whoami");
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_server_migrates_the_tenants_and_refuses_one_that_cannot_against_live_stack() {
    drop(recreate_scratch_db(SCRATCH_DB));
    let url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("acme");
    provision_tenant(&mut conn, "broken", "Broken Inc").expect("broken");

    // acme: the newest tenant migration is pending again.
    conn.batch_execute(&format!(
        "SET search_path TO org_acme; {DOWN_SQL}; \
         DELETE FROM __diesel_schema_migrations WHERE version = '{NEWEST}'; \
         SET search_path TO DEFAULT;"
    ))
    .expect("undo the newest migration of acme");
    // broken: the newest migration cannot run (its table is gone).
    conn.batch_execute(&format!(
        "DROP TABLE org_broken.item_impacts; \
         DELETE FROM org_broken.__diesel_schema_migrations WHERE version = '{NEWEST}';"
    ))
    .expect("break the schema of broken");
    assert!(!has_version(&mut conn, "org_acme"));

    // One person, a member of the two organizations.
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "local:ada".into(),
            user_name: "ada@example.test".into(),
            email: "ada@example.test".into(),
            display_name: "Ada".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("user");
    for slug in ["acme", "broken"] {
        let org = org_id(&mut conn, slug);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org,
                user_id: user.id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("membership");
    }
    db_local_auth::set_password(&mut conn, user.id, &hash_password(PASSWORD).expect("hash"))
        .expect("password");

    // --- two servers start at the same time --------------------------------
    let (port_a, port_b) = (free_port(), free_port());
    let a = spawn_server(&url, port_a);
    let b = spawn_server(&url, port_b);
    let http = reqwest::Client::new();
    wait_ready(&http, port_a).await;
    wait_ready(&http, port_b).await;

    // The migration of acme ran before either server served.
    assert!(
        has_version(&mut conn, "org_acme"),
        "acme is migrated at startup"
    );

    let response = http
        .post(format!("http://127.0.0.1:{port_a}/api/login"))
        .json(&json!({ "email": "ada@example.test", "password": PASSWORD }))
        .send()
        .await
        .expect("login");
    assert!(response.status().is_success(), "{}", response.status());
    let token = response.json::<Value>().await.expect("json")["token"]
        .as_str()
        .expect("token")
        .to_string();

    // --- acme serves; broken answers 503 TENANT_NOT_READY ------------------
    for port in [port_a, port_b] {
        let (status, body) = whoami(&http, port, &token, "acme").await;
        assert_eq!(status, 200, "acme on {port}: {body}");
        let (status, body) = whoami(&http, port, &token, "broken").await;
        assert_eq!(status, 503, "broken on {port}: {body}");
        assert_eq!(body["error"]["code"], "TENANT_NOT_READY", "{body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("kairos-server migrate-tenants"),
            "{message}"
        );
    }

    // --- repair broken, migrate it; it serves with no restart ---------------
    conn.batch_execute(
        "CREATE TABLE org_broken.item_impacts ( \
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(), \
            item_id UUID NOT NULL, \
            item_type TEXT NOT NULL CHECK (item_type IN ('document', 'adr')), \
            target_kind TEXT NOT NULL DEFAULT 'repository' \
                CHECK (target_kind IN ('repository')), \
            target_id UUID NOT NULL, \
            created_by UUID NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            UNIQUE (item_id, target_kind, target_id));",
    )
    .expect("repair broken");
    let output = Command::new(env!("CARGO_BIN_EXE_kairos-server"))
        .arg("migrate-tenants")
        .env("DATABASE_URL", &url)
        .output()
        .expect("migrate-tenants");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{printed}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        printed.contains(&format!("org_broken: applied tenant migration {NEWEST}")),
        "{printed}"
    );

    let deadline = Instant::now()
        + kairos_server::tenant_readiness::RECHECK_INTERVAL
        + Duration::from_secs(10);
    loop {
        let (status, body) = whoami(&http, port_a, &token, "broken").await;
        if status == 200 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "broken still refused: {status} {body}"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // --- the lock: one server applied the migration of acme ----------------
    let out_a = stop(a);
    let out_b = stop(b);
    let applied = format!("org_acme: applied tenant migration {NEWEST}");
    let appliers = [&out_a, &out_b]
        .iter()
        .filter(|out| out.contains(&applied))
        .count();
    assert_eq!(appliers, 1, "A:\n{out_a}\nB:\n{out_b}");
    for out in [&out_a, &out_b] {
        assert!(out.contains("org_broken: tenant migration FAILED"), "{out}");
    }

    drop(conn);
    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop_scratch_db(&mut admin, SCRATCH_DB);
}
