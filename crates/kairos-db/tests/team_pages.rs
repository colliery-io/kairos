//! Integration test for the team-page scaffold (KAIROS-T-0082, design in
//! KAIROS-I-0007): shape, charter protection, history baselines,
//! idempotency, and sibling-slug uniqueness (including NULL-parent
//! roots).
//!
//! Runs against the real compose Postgres (`angreal services up`); owns
//! the scratch database `kairos_team_pages_test`.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::models::enums::{TeamPageKind, TeamType};
use kairos_db::models::team_pages::{NewTeamPage, TeamPage};
use kairos_db::models::teams::{NewTeam, Team};
use kairos_db::team_pages::seed_team_scaffold;
use kairos_db::{provision_tenant, run_public_migrations, schema};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:5432/kairos";
const SCRATCH_DB: &str = "kairos_team_pages_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// All live scaffold nodes of a team as (parent slug or "", slug, kind,
/// protected), sorted.
fn scaffold_shape(conn: &mut PgConnection, team: Uuid) -> Vec<(String, String, String, bool)> {
    let pages: Vec<TeamPage> = schema::team_pages::table
        .filter(schema::team_pages::team_id.eq(team))
        .filter(schema::team_pages::deleted_at.is_null())
        .select(TeamPage::as_select())
        .load(conn)
        .expect("loading team pages");
    let name_of = |id: Uuid| {
        pages
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.slug.clone())
            .unwrap_or_default()
    };
    let mut shape: Vec<(String, String, String, bool)> = pages
        .iter()
        .map(|p| {
            (
                p.parent_id.map(name_of).unwrap_or_default(),
                p.slug.clone(),
                p.kind.to_string(),
                p.is_protected,
            )
        })
        .collect();
    shape.sort();
    shape
}

#[test]
fn team_page_scaffold() {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let scratch_url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let actor = Uuid::new_v4();
    let team: Team = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Platform".to_string(),
            slug: "platform".to_string(),
            team_type: TeamType::Platform,
        })
        .returning(Team::as_returning())
        .get_result(&mut conn)
        .expect("creating team");

    // ---- scaffold shape -------------------------------------------------------
    seed_team_scaffold(&mut conn, team.id, actor).expect("seeding scaffold");
    let shape = scaffold_shape(&mut conn, team.id);
    assert_eq!(
        shape,
        [
            ("".to_string(), "charter".to_string(), "page".to_string(), true),
            ("".to_string(), "documentation".to_string(), "folder".to_string(), false),
            ("".to_string(), "support-processes".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "design-docs".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "explanation".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "how-to-guides".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "planning".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "reference".to_string(), "folder".to_string(), false),
            ("documentation".to_string(), "tutorials".to_string(), "folder".to_string(), false),
            ("support-processes".to_string(), "overview".to_string(), "page".to_string(), false),
        ],
        "the KAIROS-I-0007 scaffold: protected charter, support \
         processes, documentation with the diataxis + planning/design buckets"
    );

    // Only the charter is protected; only pages get history baselines.
    let history_count: i64 = schema::team_page_history::table
        .count()
        .get_result(&mut conn)
        .expect("counting history");
    assert_eq!(
        history_count, 2,
        "v1 baselines for the two scaffold PAGES (charter, overview), none for folders"
    );
    let versions: Vec<i32> = schema::team_page_history::table
        .select(schema::team_page_history::version)
        .load(&mut conn)
        .expect("loading versions");
    assert!(versions.iter().all(|v| *v == 1));

    // ---- idempotency ----------------------------------------------------------
    seed_team_scaffold(&mut conn, team.id, actor).expect("re-seeding scaffold");
    assert_eq!(
        scaffold_shape(&mut conn, team.id).len(),
        10,
        "re-running the scaffold adds nothing"
    );

    // ---- sibling-slug uniqueness, including NULL-parent roots -----------------
    let duplicate_root = diesel::insert_into(schema::team_pages::table)
        .values(NewTeamPage {
            team_id: team.id,
            parent_id: None,
            kind: TeamPageKind::Page,
            slug: "charter".to_string(),
            title: "Impostor charter".to_string(),
            content: String::new(),
            position: 9,
            is_protected: false,
            created_by: actor,
            updated_by: actor,
        })
        .execute(&mut conn);
    match duplicate_root {
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {}
        other => panic!("duplicate root slug must violate uniqueness, got {other:?}"),
    }

    // A second team scaffolds independently (same slugs, different team).
    let other: Team = diesel::insert_into(schema::teams::table)
        .values(NewTeam {
            name: "Web".to_string(),
            slug: "web".to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(Team::as_returning())
        .get_result(&mut conn)
        .expect("creating second team");
    seed_team_scaffold(&mut conn, other.id, actor).expect("seeding second scaffold");
    assert_eq!(scaffold_shape(&mut conn, other.id).len(), 10);

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
