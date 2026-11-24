//! KAIROS-T-0411 (KAIROS-A-0025 decisions 2 to 7) — the owner of a
//! strategy or an initiative, in the database layer:
//!
//! - the migration adds the empty table; a second run changes nothing;
//! - a new item has no owner;
//! - a claim of an item with no owner; a second claim is refused with the
//!   name of the owner; a service account cannot claim;
//! - the owner stays in each column of the board, Completed too;
//! - a hand-off, the refusal of a hand-off to the owner, and a set on an
//!   item with no owner; a clear and its refusal;
//! - the owner who leaves: each item of the person has no owner;
//! - each change has its `activity_log` row, and the owner gives no
//!   capability.
//!
//! Who may do which change is the rule of the server
//! (`kairos-server/tests/item_owners.rs`).
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its
//! own scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::item_owners::{self, OwnerError};
use kairos_db::items::{CreateInitiative, CreateStrategy};
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::{NewServiceAccountUser, NewUser, User};
use kairos_db::{boards, items, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
static SCRATCH_DB: scratch::ScratchName =
    scratch::ScratchName::new("kairos_db_item_owners_t0411_test");

/// The migration under test, verbatim.
const UP: &str = include_str!("../migrations/tenant/2026-10-13-000100_item_owners/up.sql");
const DOWN: &str = include_str!("../migrations/tenant/2026-10-13-000100_item_owners/down.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

fn insert_user(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: external_id.to_string(),
            user_name: external_id.to_string(),
            email: email.to_string(),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn insert_service_account(conn: &mut PgConnection) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{}", Uuid::new_v4()),
            "ci@acme.test",
            "CI",
        ))
        .returning(users::id)
        .get_result(conn)
        .expect("inserting service account")
}

fn board_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards;
    boards::table
        .filter(boards::slug.eq(slug))
        .select(boards::id)
        .first(conn)
        .expect("board")
}

fn column(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name}: {e}"))
}

/// The `details` of the activity rows of the item with `action`, oldest
/// first, with the entity type.
fn activity(conn: &mut PgConnection, item: Uuid, action: ActivityAction) -> Vec<(String, String)> {
    use kairos_db::schema::activity_log;
    activity_log::table
        .filter(activity_log::entity_id.eq(item))
        .filter(activity_log::action.eq(action))
        .order(activity_log::occurred_at.asc())
        .select((activity_log::entity_type, activity_log::details))
        .load::<(Option<String>, String)>(conn)
        .expect("activity")
        .into_iter()
        .map(|(kind, details)| (kind.unwrap_or_default(), details))
        .collect()
}

fn owner(conn: &mut PgConnection, item: Uuid) -> Option<Uuid> {
    item_owners::owner_of(conn, item)
        .expect("owner")
        .map(|o| o.user_id)
}

#[test]
fn a_strategy_or_an_initiative_has_one_owner_by_claim() {
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
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let scratch_url = with_database(&admin_url, &SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // --- the migration: re-runnable, and down then up gives the table ------
    conn.batch_execute(UP).expect("a second up changes nothing");
    conn.batch_execute(DOWN).expect("down");
    conn.batch_execute(UP).expect("up");

    let robin = insert_user(&mut conn, "dex|robin", "robin@acme.test", "Robin");
    let sam = insert_user(&mut conn, "dex|sam", "sam@acme.test", "Sam");
    let ci = insert_service_account(&mut conn);
    // Robin and Sam are members of the organization: a notification goes
    // to a member only.
    {
        use kairos_db::models::{NewOrganizationMember, OrgRole};
        use kairos_db::schema::{organization_members, organizations};
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme");
        for user_id in [robin, sam] {
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id,
                    role: OrgRole::Member,
                })
                .execute(&mut conn)
                .expect("membership");
        }
    }

    let initiatives = board_by_slug(&mut conn, "initiatives");
    let strategy_board = board_by_slug(&mut conn, "strategy");
    let initiative = items::create_initiative(
        &mut conn,
        CreateInitiative {
            board_id: initiatives,
            column_id: None,
            title: "Shared work",
            content: "",
            complexity: None,
            bucket_type: None,
        },
        robin,
    )
    .expect("creating initiative")
    .id;
    let strategy = items::create_strategy(
        &mut conn,
        CreateStrategy {
            board_id: strategy_board,
            column_id: None,
            title: "Direction",
            content: "",
            hypothesis: None,
        },
        robin,
    )
    .expect("creating strategy")
    .id;

    // --- a new item has no owner (decision 6) -------------------------------
    assert_eq!(owner(&mut conn, initiative), None);
    assert_eq!(owner(&mut conn, strategy), None);

    // --- a claim (decision 3) -----------------------------------------------
    let claimed = item_owners::claim(&mut conn, initiative, robin).expect("claim");
    assert_eq!(claimed.user_id, robin);
    match item_owners::claim(&mut conn, initiative, sam) {
        Err(OwnerError::HasOwner { name, .. }) => assert_eq!(name, "Robin"),
        other => panic!("a second claim is refused: {other:?}"),
    }
    assert_eq!(
        owner(&mut conn, initiative),
        Some(robin),
        "one owner at most"
    );
    match item_owners::claim(&mut conn, strategy, ci) {
        Err(OwnerError::NotAPerson(_)) => {}
        other => panic!("a service account cannot claim: {other:?}"),
    }
    assert_eq!(owner(&mut conn, strategy), None);
    match item_owners::claim(&mut conn, Uuid::new_v4(), robin) {
        Err(OwnerError::ItemNotFound(_)) => {}
        other => panic!("only a strategy or an initiative: {other:?}"),
    }

    // --- the owner stays in each column, Completed too (decision 5) ---------
    for name in [
        "Design",
        "Decompose",
        "Ready",
        "Active",
        "Monitoring",
        "Completed",
    ] {
        let to = column(&mut conn, initiatives, name);
        boards::transition_initiative(&mut conn, initiative, to, sam)
            .unwrap_or_else(|e| panic!("transition to {name}: {e}"));
        assert_eq!(owner(&mut conn, initiative), Some(robin), "in {name}");
    }

    // --- a hand-off (decision 4) --------------------------------------------
    let handed = item_owners::hand_off(&mut conn, initiative, sam, robin).expect("hand-off");
    assert_eq!(handed.user_id, sam);
    match item_owners::hand_off(&mut conn, initiative, sam, robin) {
        Err(OwnerError::AlreadyOwner { name, .. }) => assert_eq!(name, "Sam"),
        other => panic!("a hand-off to the owner is refused: {other:?}"),
    }
    match item_owners::hand_off(&mut conn, initiative, ci, robin) {
        Err(OwnerError::NotAPerson(_)) => {}
        other => panic!("a service account cannot be the owner: {other:?}"),
    }
    // An admin sets the owner of an item with no owner.
    item_owners::hand_off(&mut conn, strategy, robin, sam).expect("set");
    assert_eq!(owner(&mut conn, strategy), Some(robin));

    // --- a clear -------------------------------------------------------------
    let ended = item_owners::clear(&mut conn, initiative, robin).expect("clear");
    assert_eq!(ended.user_id, sam);
    assert_eq!(owner(&mut conn, initiative), None);
    match item_owners::clear(&mut conn, initiative, robin) {
        Err(OwnerError::NoOwner { .. }) => {}
        other => panic!("a clear of no owner is refused: {other:?}"),
    }

    // --- the owner leaves (decision 7) --------------------------------------
    item_owners::claim(&mut conn, initiative, robin).expect("claim again");
    let dropped = item_owners::drop_owner_of_user(&mut conn, robin, sam).expect("leave");
    assert_eq!(dropped.len(), 2, "the two items of Robin: {dropped:?}");
    assert_eq!(owner(&mut conn, initiative), None);
    assert_eq!(owner(&mut conn, strategy), None);
    assert!(
        item_owners::drop_owner_of_user(&mut conn, robin, sam)
            .expect("again")
            .is_empty(),
        "a second run changes nothing"
    );

    // --- the history -----------------------------------------------------------
    let initiative_kind = "initiative".to_string();
    assert_eq!(
        activity(&mut conn, initiative, ActivityAction::Claim),
        vec![
            (initiative_kind.clone(), format!("claim:{robin}")),
            (initiative_kind.clone(), format!("claim:{robin}")),
        ]
    );
    assert_eq!(
        activity(&mut conn, initiative, ActivityAction::HandOff),
        vec![(initiative_kind.clone(), format!("hand_off:{robin}->{sam}"))]
    );
    assert_eq!(
        activity(&mut conn, initiative, ActivityAction::Release),
        vec![
            (
                initiative_kind.clone(),
                format!("release:{sam} reason:clear")
            ),
            (initiative_kind, format!("release:{robin} reason:left")),
        ]
    );
    assert_eq!(
        activity(&mut conn, strategy, ActivityAction::HandOff),
        vec![("strategy".to_string(), format!("hand_off:none->{robin}"))]
    );
    // The new owner is told.
    let told: i64 = {
        use kairos_db::schema::notifications;
        notifications::table
            .filter(notifications::user_id.eq(sam))
            .filter(notifications::item_id.eq(initiative))
            .count()
            .get_result(&mut conn)
            .expect("notifications")
    };
    assert_eq!(told, 1, "the hand-off to Sam is a notification");

    // --- the owner gives no capability (decision 2) -------------------------
    item_owners::claim(&mut conn, initiative, sam).expect("claim");
    assert!(
        !kairos_db::abac::check_capability(&mut conn, initiatives, sam, "manage_initiatives")
            .expect("check"),
        "the owner with no grant has no right"
    );
    assert!(!kairos_db::abac::is_board_member(&mut conn, initiatives, sam).expect("member"));

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}
