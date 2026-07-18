//! Integration test for the ABAC capability service (KAIROS-T-0011,
//! contract per KAIROS-A-0006, layering per KAIROS-A-0009).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration`); the database is never mocked
//! (KAIROS-A-0012). For isolation the test drops and recreates a dedicated
//! scratch database (`kairos_abac_test`) on the same server.
//!
//! Covered here, on a freshly provisioned tenant:
//! - `check_capability` truth table against the real SQL: exact match,
//!   `manage_*`, `*`, non-matches, whitelist default-deny, and stored
//!   capabilities containing literal `%`/`_` NOT acting as SQL wildcards
//! - grant/revoke flows: `activity_log` rows
//!   (`capability_grant`/`capability_revoke`), the composite-PK UNIQUE
//!   constraint surfacing as typed `AlreadyGranted`, revoke of a missing
//!   grant as typed `GrantNotFound`, and revoke actually removing access
//! - org-admin bypass via `authorize` (admin passes with zero grants;
//!   plain member denied)
//! - document authorization resolving through the `supports` edge to the
//!   parent initiative's board; board items resolving to their own board;
//!   off-board ADRs, orphan documents, and unknown ids resolving to None

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::abac as rules;
use kairos_db::abac::{self, AbacError};
use kairos_db::models::{
    ActivityAction, NewAdr, NewDocument, NewInitiative, NewItemRelationship, NewOrganizationMember,
    NewStrategy, NewUser, OrgRole, RelationshipType, User,
};
use kairos_db::{provision_tenant, run_public_migrations, schema};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:5432/kairos";

const SCRATCH_DB: &str = "kairos_abac_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

fn insert_user(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid {
    diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: external_id.into(),
            email: email.into(),
            display_name: name.into(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

/// The board with this slug in the current tenant schema.
fn board_id_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    schema::boards::table
        .filter(schema::boards::slug.eq(slug))
        .select(schema::boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("board {slug:?} not found: {e}"))
}

/// The first column of a board (position order) — items need a placement.
fn first_column(conn: &mut PgConnection, board: Uuid) -> Uuid {
    schema::board_columns::table
        .filter(schema::board_columns::board_id.eq(board))
        .order(schema::board_columns::position.asc())
        .select(schema::board_columns::id)
        .first(conn)
        .expect("board has columns")
}

/// All `activity_log.details` values for (action, entity_id), in order.
fn activity_details(conn: &mut PgConnection, action: ActivityAction, entity: Uuid) -> Vec<String> {
    schema::activity_log::table
        .filter(schema::activity_log::action.eq(action.as_str()))
        .filter(schema::activity_log::entity_id.eq(entity))
        .order(schema::activity_log::occurred_at.asc())
        .select(schema::activity_log::details)
        .load(conn)
        .expect("loading activity_log")
}

/// Assert the SQL check agrees with the pure core matcher for one stored
/// grant — the two layers implement ONE contract (KAIROS-A-0006).
fn assert_check(
    conn: &mut PgConnection,
    board: Uuid,
    user: Uuid,
    granted: &str,
    required: &str,
    expected: bool,
) {
    let sql = abac::check_capability(conn, board, user, required).expect("check_capability");
    assert_eq!(
        sql, expected,
        "SQL check: grant {granted:?}, required {required:?}"
    );
    let pure = rules::is_authorized(&[granted.to_string()], required);
    assert_eq!(
        pure, expected,
        "core matcher disagrees with SQL for grant {granted:?}, required {required:?}"
    );
}

#[test]
fn abac_capability_lifecycle() {
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

    // Fresh tenant, then work inside its schema (same mechanism as the
    // pool: unqualified tenant tables resolve via search_path).
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // ---- seed users + org membership ---------------------------------------
    let org_id: Uuid = schema::organizations::table
        .filter(schema::organizations::slug.eq("acme"))
        .select(schema::organizations::id)
        .first(&mut conn)
        .expect("provisioned org row");

    let org_admin = insert_user(&mut conn, "dex|admin", "admin@acme.test", "Org Admin");
    let manager = insert_user(&mut conn, "dex|manager", "manager@acme.test", "Manager");
    let helper = insert_user(&mut conn, "dex|helper", "helper@acme.test", "Helper");
    let superuser = insert_user(&mut conn, "dex|super", "super@acme.test", "Board Admin");
    let hostile = insert_user(&mut conn, "dex|hostile", "hostile@acme.test", "Hostile");

    diesel::insert_into(schema::organization_members::table)
        .values(&[
            NewOrganizationMember {
                organization_id: org_id,
                user_id: org_admin,
                role: OrgRole::Admin,
            },
            NewOrganizationMember {
                organization_id: org_id,
                user_id: manager,
                role: OrgRole::Member,
            },
        ])
        .execute(&mut conn)
        .expect("inserting org members");

    let initiative_board = board_id_by_slug(&mut conn, "initiatives");
    let strategy_board = board_id_by_slug(&mut conn, "strategy");

    // ---- whitelist default: no grants, no access ----------------------------
    assert!(
        !abac::check_capability(&mut conn, initiative_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "whitelist: a user with no grants has no write access"
    );

    // ---- grant flow: activity row + composite-PK UNIQUE ----------------------
    abac::grant_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect("granting manage_tasks");
    assert_eq!(
        activity_details(&mut conn, ActivityAction::CapabilityGrant, initiative_board),
        [format!("capability:manage_tasks user:{manager}")],
        "grant writes a capability_grant activity row"
    );

    // Duplicate grant: composite PK honored, typed error, no extra audit row.
    let err = abac::grant_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect_err("duplicate grant must fail");
    assert!(
        matches!(
            &err,
            AbacError::AlreadyGranted { board_id, user_id, capability }
                if *board_id == initiative_board && *user_id == manager && capability == "manage_tasks"
        ),
        "expected AlreadyGranted, got {err:?}"
    );
    assert_eq!(
        activity_details(&mut conn, ActivityAction::CapabilityGrant, initiative_board).len(),
        1,
        "the rejected duplicate grant wrote no activity row"
    );

    // Empty capability is rejected before touching the database.
    let err = abac::grant_capability(&mut conn, initiative_board, manager, "", org_admin)
        .expect_err("empty capability must fail");
    assert!(
        matches!(err, AbacError::EmptyCapability),
        "expected EmptyCapability, got {err:?}"
    );

    // ---- check_capability truth table against the real SQL -------------------
    // Exact grant: matches itself, nothing else, and is board-scoped.
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::MANAGE_TASKS,
        true,
    );
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::MANAGE_DOCUMENTS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        rules::TRANSITION_ITEMS,
        false,
    );
    // '_' in the STORED grant is escaped in the SQL: it does not act as the
    // LIKE single-char wildcard ('managextasks' must NOT match).
    assert_check(
        &mut conn,
        initiative_board,
        manager,
        "manage_tasks",
        "managextasks",
        false,
    );
    assert!(
        !abac::check_capability(&mut conn, strategy_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "grants are board-scoped: no access on another board"
    );

    // manage_* glob: all manage capabilities, not workflow/config ones.
    abac::grant_capability(
        &mut conn,
        initiative_board,
        helper,
        rules::GLOB_MANAGE,
        org_admin,
    )
    .expect("granting manage_*");
    for required in [
        rules::MANAGE_STRATEGIES,
        rules::MANAGE_INITIATIVES,
        rules::MANAGE_TASKS,
        rules::MANAGE_DOCUMENTS,
        rules::MANAGE_ADRS,
        rules::MANAGE_MEMBERS,
    ] {
        assert_check(
            &mut conn,
            initiative_board,
            helper,
            "manage_*",
            required,
            true,
        );
    }
    assert_check(
        &mut conn,
        initiative_board,
        helper,
        "manage_*",
        rules::TRANSITION_ITEMS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        helper,
        "manage_*",
        rules::CONFIGURE_BOARDS,
        false,
    );

    // '*' grant: everything on the board.
    abac::grant_capability(
        &mut conn,
        initiative_board,
        superuser,
        rules::GLOB_ALL,
        org_admin,
    )
    .expect("granting *");
    for required in rules::CAPABILITIES {
        assert_check(&mut conn, initiative_board, superuser, "*", required, true);
    }

    // A STORED capability containing a literal '%' must NOT wildcard in SQL.
    abac::grant_capability(&mut conn, initiative_board, hostile, "manage%", org_admin)
        .expect("granting hostile literal-% capability");
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        rules::MANAGE_TASKS,
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        "manageX",
        false,
    );
    assert_check(
        &mut conn,
        initiative_board,
        hostile,
        "manage%",
        "manage%",
        true,
    );

    // ---- org-admin bypass -----------------------------------------------------
    assert!(
        abac::is_org_admin(&mut conn, "acme", org_admin).expect("is_org_admin"),
        "role='admin' member is an org admin"
    );
    assert!(
        !abac::is_org_admin(&mut conn, "acme", manager).expect("is_org_admin"),
        "role='member' is not an org admin"
    );
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            strategy_board,
            org_admin,
            rules::CONFIGURE_BOARDS
        )
        .expect("authorize"),
        "org admin bypasses the whitelist with zero grants"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            strategy_board,
            manager,
            rules::CONFIGURE_BOARDS
        )
        .expect("authorize"),
        "plain member without grants is denied on this board"
    );
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            initiative_board,
            manager,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "authorize falls through to the capability check for non-admins"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "nonexistent",
            initiative_board,
            org_admin,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "an unknown org slug grants nothing"
    );

    // ---- document inherits the parent's board via the supports edge -----------
    let discovery = first_column(&mut conn, initiative_board);
    let initiative_id: Uuid = diesel::insert_into(schema::initiatives::table)
        .values(NewInitiative {
            short_code: "I-0001".into(),
            title: "ABAC".into(),
            content: "".into(),
            board_id: initiative_board,
            column_id: discovery,
            complexity: None,
            is_bucket: false,
            bucket_type: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::initiatives::id)
        .get_result(&mut conn)
        .expect("inserting initiative");

    let document_id: Uuid = diesel::insert_into(schema::documents::table)
        .values(NewDocument {
            short_code: "D-0001".into(),
            title: "PRD".into(),
            content: "".into(),
            template_id: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::documents::id)
        .get_result(&mut conn)
        .expect("inserting document");

    // Before the edge exists the document has no board context.
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, document_id).expect("resolve"),
        None,
        "a document with no supports edge resolves to no board"
    );

    // S-0004 edge semantics: target supports source (document supports
    // initiative) — source = initiative (parent), target = document.
    diesel::insert_into(schema::item_relationships::table)
        .values(NewItemRelationship {
            source_id: initiative_id,
            target_id: document_id,
            relationship: RelationshipType::Supports,
        })
        .execute(&mut conn)
        .expect("inserting supports edge");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, document_id).expect("resolve"),
        Some(initiative_board),
        "document authorization resolves through supports to the parent initiative's board"
    );
    // End-to-end per A-0006: editing the document requires manage_documents
    // on the PARENT's board — helper's manage_* covers it, manager's
    // manage_tasks does not.
    let doc_board = abac::resolve_authorization_board(&mut conn, document_id)
        .expect("resolve")
        .expect("document has a board");
    assert!(
        abac::authorize(
            &mut conn,
            "acme",
            doc_board,
            helper,
            rules::MANAGE_DOCUMENTS
        )
        .expect("authorize"),
        "helper's manage_* on the parent board authorizes editing the document"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            doc_board,
            manager,
            rules::MANAGE_DOCUMENTS
        )
        .expect("authorize"),
        "manager's manage_tasks does not authorize editing the document"
    );

    // Board items resolve to their own board.
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, initiative_id).expect("resolve"),
        Some(initiative_board)
    );
    let strategy_col = first_column(&mut conn, strategy_board);
    let strategy_id: Uuid = diesel::insert_into(schema::strategies::table)
        .values(NewStrategy {
            short_code: "S-0001".into(),
            title: "Strategy".into(),
            content: "".into(),
            board_id: strategy_board,
            column_id: strategy_col,
            hypothesis: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::strategies::id)
        .get_result(&mut conn)
        .expect("inserting strategy");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, strategy_id).expect("resolve"),
        Some(strategy_board)
    );

    // An ADR not placed on a board has no board context; unknown ids too.
    let off_board_adr: Uuid = diesel::insert_into(schema::adrs::table)
        .values(NewAdr {
            short_code: "A-0001".into(),
            title: "Off board".into(),
            content: "".into(),
            board_id: None,
            column_id: None,
            decision_maker: None,
            decision_date: None,
            created_by: org_admin,
            updated_by: org_admin,
        })
        .returning(schema::adrs::id)
        .get_result(&mut conn)
        .expect("inserting off-board adr");
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, off_board_adr).expect("resolve"),
        None
    );
    assert_eq!(
        abac::resolve_authorization_board(&mut conn, Uuid::new_v4()).expect("resolve"),
        None
    );

    // ---- revoke flow: removes access, writes activity, typed on missing -------
    abac::revoke_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect("revoking manage_tasks");
    assert!(
        !abac::check_capability(&mut conn, initiative_board, manager, rules::MANAGE_TASKS)
            .expect("check"),
        "revoke removes access"
    );
    assert!(
        !abac::authorize(
            &mut conn,
            "acme",
            initiative_board,
            manager,
            rules::MANAGE_TASKS
        )
        .expect("authorize"),
        "authorize denies after revoke"
    );
    assert_eq!(
        activity_details(
            &mut conn,
            ActivityAction::CapabilityRevoke,
            initiative_board
        ),
        [format!("capability:manage_tasks user:{manager}")],
        "revoke writes a capability_revoke activity row"
    );

    let err = abac::revoke_capability(
        &mut conn,
        initiative_board,
        manager,
        rules::MANAGE_TASKS,
        org_admin,
    )
    .expect_err("revoking a missing grant must fail");
    assert!(
        matches!(
            &err,
            AbacError::GrantNotFound { board_id, user_id, capability }
                if *board_id == initiative_board && *user_id == manager && capability == "manage_tasks"
        ),
        "expected GrantNotFound, got {err:?}"
    );
    assert_eq!(
        activity_details(
            &mut conn,
            ActivityAction::CapabilityRevoke,
            initiative_board
        )
        .len(),
        1,
        "the rejected revoke wrote no activity row"
    );

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
