//! Integration tests for the diesel models, enum mappings, and the
//! search_path-pinned connection pool (KAIROS-T-0009).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration` — the database is never mocked, per
//! KAIROS-A-0012). Each test drops and recreates its own scratch database
//! on the same server so parallel test binaries never collide.
//!
//! Coverage:
//! - `models_round_trip`: insert+select round-trips through the typed
//!   models for public tables (organizations, users, organization_members)
//!   and tenant tables (teams, boards/columns/transitions, strategies,
//!   initiatives, tasks, documents, adrs, item_relationships,
//!   item_metadata, board_member_capabilities, item_history,
//!   activity_log), exercising every TEXT-CHECK enum, an `AsChangeset`
//!   update, and DB-level rejection of an unknown enum value on read.
//! - `pool_isolation_interleaved`: two tenants served interleaved from ONE
//!   pool; rows only ever come from the pinned tenant; reset-on-return is
//!   observed directly on the underlying connection. Pre-cursor to the
//!   KAIROS-T-0016 adversarial suite.

use chrono::NaiveDate;
// NOTE: deliberately NOT `use diesel::prelude::*` — the sync `RunQueryDsl`
// would make every `.load`/`.execute`/`.get_result` call ambiguous with
// diesel-async's. Sync usage is confined to the `setup` module below.
use diesel::prelude::{ExpressionMethods, QueryDsl, QueryableByName, SelectableHelper};
use diesel::sql_query;
use diesel::sql_types::Text;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use kairos_db::models::*;
use kairos_db::pool::{PoolError, TenantPool};
use kairos_db::schema as s;

use setup::{drop_scratch_database, scratch_database};

/// Synchronous scratch-database plumbing (own module so the sync
/// `RunQueryDsl` from `diesel::prelude` never enters the async test scope).
mod setup {
    use diesel::prelude::*;
    use diesel::sql_query;
    use kairos_db::{provision_tenant, run_public_migrations};

    /// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
    const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

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

    /// Drop + recreate `db_name`, run public migrations, provision
    /// `tenants`. Returns the scratch database URL.
    pub fn scratch_database(db_name: &str, tenants: &[&str]) -> String {
        let admin_url = admin_database_url();
        let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
            panic!(
                "cannot connect to compose postgres at {admin_url}: {e} \
                 (is the stack up? `angreal services up`)"
            )
        });
        sql_query(format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
            .execute(&mut admin_conn)
            .expect("dropping scratch database");
        sql_query(format!("CREATE DATABASE {db_name}"))
            .execute(&mut admin_conn)
            .expect("creating scratch database");

        let scratch_url = with_database(&admin_url, db_name);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        for slug in tenants {
            provision_tenant(&mut conn, slug, slug).expect("provisioning tenant");
        }
        scratch_url
    }

    pub fn drop_scratch_database(db_name: &str) {
        let mut admin_conn = PgConnection::establish(&admin_database_url())
            .expect("connecting for scratch teardown");
        sql_query(format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
            .execute(&mut admin_conn)
            .expect("dropping scratch database after test");
    }
}

#[derive(QueryableByName)]
struct SearchPathRow {
    #[diesel(sql_type = Text)]
    search_path: String,
}

async fn show_search_path(conn: &mut diesel_async::AsyncPgConnection) -> String {
    let row: SearchPathRow = sql_query("SHOW search_path")
        .get_result(conn)
        .await
        .expect("SHOW search_path");
    row.search_path
}

#[derive(QueryableByName)]
struct PidRow {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    pid: i32,
}

async fn backend_pid(conn: &mut diesel_async::AsyncPgConnection) -> i32 {
    let row: PidRow = sql_query("SELECT pg_backend_pid() AS pid")
        .get_result(conn)
        .await
        .expect("SELECT pg_backend_pid()");
    row.pid
}

/// The board (by level) and its columns, in position order.
async fn board_with_columns(
    conn: &mut diesel_async::AsyncPgConnection,
    level: BoardLevel,
) -> (Board, Vec<BoardColumn>) {
    let board: Board = s::boards::table
        .filter(s::boards::board_level.eq(level))
        .select(Board::as_select())
        .first(conn)
        .await
        .expect("loading board");
    let columns: Vec<BoardColumn> = s::board_columns::table
        .filter(s::board_columns::board_id.eq(board.id))
        .order(s::board_columns::position.asc())
        .select(BoardColumn::as_select())
        .load(conn)
        .await
        .expect("loading board columns");
    (board, columns)
}

#[tokio::test]
async fn models_round_trip() {
    const DB: &str = "kairos_models_roundtrip_test";
    let url = scratch_database(DB, &["alpha"]);
    let pool = TenantPool::new(&url, 4).await.expect("building pool");

    // ---- public tables (schema-qualified in schema.rs) --------------------
    let mut pconn = pool.public_conn().await.expect("public checkout");

    let org: Organization = s::organizations::table
        .filter(s::organizations::slug.eq("alpha"))
        .select(Organization::as_select())
        .first(&mut pconn)
        .await
        .expect("selecting provisioned organization");
    assert_eq!(org.slug, "alpha");

    let user: User = diesel::insert_into(s::users::table)
        .values(NewUser {
            external_id: "oidc|roundtrip".into(),
            user_name: "oidc|roundtrip".into(),
            email: "roundtrip@example.com".into(),
            display_name: "Round Trip".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut pconn)
        .await
        .expect("inserting user");
    assert_eq!(user.email, "roundtrip@example.com");

    // OrgRole round-trip.
    let member: OrganizationMember = diesel::insert_into(s::organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org.id,
            user_id: user.id,
            role: OrgRole::Admin,
        })
        .returning(OrganizationMember::as_returning())
        .get_result(&mut pconn)
        .await
        .expect("inserting organization member");
    assert_eq!(member.role, OrgRole::Admin);

    // System defaults seeded at provision time: FieldType + BoardLevel (and
    // the `columns` -> `columns_` rename on system_board_defaults).
    let sys_defs: Vec<SystemMetadataDefinition> = s::system_metadata_definitions::table
        .select(SystemMetadataDefinition::as_select())
        .load(&mut pconn)
        .await
        .expect("selecting system metadata definitions");
    // 3 since KAIROS-T-0078 retired 'Document status' (lifecycle is a
    // documents column now).
    assert_eq!(sys_defs.len(), 3);
    assert!(sys_defs.iter().all(|d| d.field_type == FieldType::Enum));

    let delivery_default: SystemBoardDefault = s::system_board_defaults::table
        .filter(s::system_board_defaults::board_level.eq(BoardLevel::Delivery))
        .select(SystemBoardDefault::as_select())
        .first(&mut pconn)
        .await
        .expect("selecting delivery board default");
    assert_eq!(delivery_default.board_level, BoardLevel::Delivery);
    assert!(delivery_default.columns.contains("Backlog"));

    // ---- tenant tables (unqualified; resolved via pinned search_path) -----
    let mut conn = pool.tenant("alpha").await.expect("tenant checkout");
    assert_eq!(conn.schema(), "org_alpha");

    // Boards provisioned by KAIROS-T-0008 defaults: BoardLevel round-trips.
    let mut levels: Vec<BoardLevel> = s::boards::table
        .select(s::boards::board_level)
        .load(&mut *conn)
        .await
        .expect("selecting board levels");
    levels.sort_by_key(|l| l.as_str());
    assert_eq!(
        levels,
        [
            BoardLevel::Adr,
            BoardLevel::Initiative,
            BoardLevel::Strategy
        ]
    );

    let (strategy_board, strategy_columns) =
        board_with_columns(&mut conn, BoardLevel::Strategy).await;
    assert_eq!(strategy_columns[0].name, "Draft");
    let draft = &strategy_columns[0];
    let completed = strategy_columns
        .iter()
        .find(|c| c.name == "Completed")
        .expect("Completed column");

    // board_transitions: insert a new edge and read it back.
    let transition: BoardTransition = diesel::insert_into(s::board_transitions::table)
        .values(NewBoardTransition {
            board_id: strategy_board.id,
            from_column_id: draft.id,
            to_column_id: completed.id,
        })
        .returning(BoardTransition::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting board transition");
    assert_eq!(transition.from_column_id, draft.id);

    // teams: TeamType round-trip.
    let team: Team = diesel::insert_into(s::teams::table)
        .values(NewTeam {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: TeamType::ComplicatedSubsystem,
        })
        .returning(Team::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting team");
    assert_eq!(team.team_type, TeamType::ComplicatedSubsystem);
    assert!(team.deleted_at.is_none());

    // metadata_definitions: provisioned copy (Enum) + a Date insert.
    let priority_def: MetadataDefinition = s::metadata_definitions::table
        .filter(s::metadata_definitions::slug.eq("priority"))
        .select(MetadataDefinition::as_select())
        .first(&mut *conn)
        .await
        .expect("selecting provisioned priority definition");
    assert_eq!(priority_def.field_type, FieldType::Enum);
    assert!(priority_def.is_system_default);

    let due_def: MetadataDefinition = diesel::insert_into(s::metadata_definitions::table)
        .values(NewMetadataDefinition {
            name: "Due Date".into(),
            slug: "due_date".into(),
            field_type: FieldType::Date,
            is_system_default: false,
        })
        .returning(MetadataDefinition::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting metadata definition");
    assert_eq!(due_def.field_type, FieldType::Date);

    // templates: provisioned system copy backs documents.template_id.
    let prd: Template = s::templates::table
        .filter(s::templates::slug.eq("prd"))
        .select(Template::as_select())
        .first(&mut *conn)
        .await
        .expect("selecting provisioned prd template");
    assert!(prd.is_system_default);

    // strategies: insert + AsChangeset update.
    let strategy: Strategy = diesel::insert_into(s::strategies::table)
        .values(NewStrategy {
            short_code: "S-0001".into(),
            title: "Own the roundtrip".into(),
            content: "strategy content".into(),
            board_id: strategy_board.id,
            column_id: draft.id,
            hypothesis: Some("models work".into()),
            created_by: user.id,
            updated_by: user.id,
        })
        .returning(Strategy::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting strategy");
    assert_eq!(strategy.version, 1);
    assert_eq!(strategy.hypothesis.as_deref(), Some("models work"));

    let updated: Strategy = diesel::update(s::strategies::table.find(strategy.id))
        .set(StrategyChangeset {
            title: Some("Own the roundtrip, v2".into()),
            version: Some(2),
            hypothesis: Some(None), // SET NULL through the double-Option
            ..Default::default()
        })
        .returning(Strategy::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("updating strategy via changeset");
    assert_eq!(updated.title, "Own the roundtrip, v2");
    assert_eq!(updated.version, 2);
    assert_eq!(updated.hypothesis, None);
    assert_eq!(updated.content, "strategy content", "content untouched");

    // initiatives: Complexity + BucketType round-trips (bucket CHECK).
    let (initiative_board, initiative_columns) =
        board_with_columns(&mut conn, BoardLevel::Initiative).await;
    let initiative: Initiative = diesel::insert_into(s::initiatives::table)
        .values(NewInitiative {
            short_code: "I-0001".into(),
            title: "Ad-hoc bucket".into(),
            content: String::new(),
            board_id: initiative_board.id,
            column_id: initiative_columns[0].id,
            complexity: Some(Complexity::Xl),
            is_bucket: true,
            bucket_type: Some(BucketType::AdHoc),
            created_by: user.id,
            updated_by: user.id,
        })
        .returning(Initiative::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting initiative");
    assert_eq!(initiative.complexity, Some(Complexity::Xl));
    assert_eq!(initiative.bucket_type, Some(BucketType::AdHoc));
    assert!(initiative.is_bucket);

    // tasks: TaskType round-trip; delivery items may reference a team.
    let task: Task = diesel::insert_into(s::tasks::table)
        .values(NewTask {
            short_code: "T-0001".into(),
            title: "Pay down debt".into(),
            content: String::new(),
            board_id: initiative_board.id,
            column_id: initiative_columns[0].id,
            task_type: TaskType::TechDebt,
            work_class: kairos_db::models::enums::WorkClass::Planned,
            team_id: Some(team.id),
            repository_id: None,
            created_by: user.id,
            updated_by: user.id,
        })
        .returning(Task::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting task");
    assert_eq!(task.task_type, TaskType::TechDebt);
    assert_eq!(task.team_id, Some(team.id));

    // documents: template-backed.
    let document: Document = diesel::insert_into(s::documents::table)
        .values(NewDocument {
            short_code: "D-0001".into(),
            title: "Roundtrip PRD".into(),
            content: "prd content".into(),
            template_id: Some(prd.id),
            created_by: user.id,
            updated_by: user.id,
        })
        .returning(Document::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting document");
    assert_eq!(document.template_id, Some(prd.id));

    // adrs: on the ADR board, with a DATE column.
    let (adr_board, adr_columns) = board_with_columns(&mut conn, BoardLevel::Adr).await;
    let decision_date = NaiveDate::from_ymd_opt(2026, 7, 9).unwrap();
    let adr: Adr = diesel::insert_into(s::adrs::table)
        .values(NewAdr {
            short_code: "A-0001".into(),
            title: "Use diesel models".into(),
            content: "adr content".into(),
            board_id: Some(adr_board.id),
            column_id: Some(adr_columns[0].id),
            decision_maker: Some("Dylan".into()),
            decision_date: Some(decision_date),
            created_by: user.id,
            updated_by: user.id,
        })
        .returning(Adr::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting adr");
    assert_eq!(adr.decision_date, Some(decision_date));

    // item_relationships: RelationshipType round-trips over the shared
    // UUID space (strategy -> initiative -> task, document supports).
    for (source, target, rel) in [
        (strategy.id, initiative.id, RelationshipType::Parent),
        (initiative.id, task.id, RelationshipType::Parent),
        (initiative.id, document.id, RelationshipType::Supports),
        (adr.id, task.id, RelationshipType::Blocks),
    ] {
        let edge: ItemRelationship = diesel::insert_into(s::item_relationships::table)
            .values(NewItemRelationship {
                source_id: source,
                target_id: target,
                relationship: rel,
            })
            .returning(ItemRelationship::as_returning())
            .get_result(&mut *conn)
            .await
            .expect("inserting relationship");
        assert_eq!(edge.relationship, rel);
    }
    let parent_edges: i64 = s::item_relationships::table
        .filter(s::item_relationships::relationship.eq(RelationshipType::Parent))
        .count()
        .get_result(&mut *conn)
        .await
        .expect("counting parent edges");
    assert_eq!(parent_edges, 2);

    // item_metadata: value against the provisioned priority definition.
    let meta: ItemMetadata = diesel::insert_into(s::item_metadata::table)
        .values(NewItemMetadata {
            item_id: task.id,
            metadata_definition_id: priority_def.id,
            value: "high".into(),
        })
        .returning(ItemMetadata::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting item metadata");
    assert_eq!(meta.value, "high");

    // board_member_capabilities: composite-PK grant (KAIROS-A-0006).
    let grant: BoardMemberCapability = diesel::insert_into(s::board_member_capabilities::table)
        .values(NewBoardMemberCapability {
            board_id: strategy_board.id,
            user_id: user.id,
            capability: "manage_*".into(),
            granted_by: user.id,
        })
        .returning(BoardMemberCapability::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting capability grant");
    assert_eq!(grant.capability, "manage_*");
    let grant_back: BoardMemberCapability = s::board_member_capabilities::table
        .find((strategy_board.id, user.id, "manage_*".to_string()))
        .select(BoardMemberCapability::as_select())
        .first(&mut *conn)
        .await
        .expect("selecting grant by composite key");
    assert_eq!(grant_back.granted_by, user.id);

    // item_history: append-only snapshot.
    let history: ItemHistory = diesel::insert_into(s::item_history::table)
        .values(NewItemHistory {
            item_id: strategy.id,
            version: 1,
            title: strategy.title.clone(),
            content: strategy.content.clone(),
            edited_by: user.id,
        })
        .returning(ItemHistory::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting item history");
    assert_eq!(history.version, 1);

    // activity_log: ActivityAction round-trip.
    let entry: ActivityLogEntry = diesel::insert_into(s::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: user.id,
            action: ActivityAction::Transition,
            entity_id: Some(strategy.id),
            entity_type: Some("strategy".into()),
            details: "column:Draft->Review".into(),
        })
        .returning(ActivityLogEntry::as_returning())
        .get_result(&mut *conn)
        .await
        .expect("inserting activity log entry");
    assert_eq!(entry.action, ActivityAction::Transition);

    // Unknown enum values coming FROM the database are rejected, not
    // defaulted (activity_log.action has no CHECK constraint, so a bad
    // writer could store anything).
    sql_query("INSERT INTO activity_log (actor_id, action, details) VALUES ($1, 'bogus', 'x')")
        .bind::<diesel::sql_types::Uuid, _>(user.id)
        .execute(&mut *conn)
        .await
        .expect("inserting raw bogus action");
    let err = s::activity_log::table
        .select(ActivityLogEntry::as_select())
        .load(&mut *conn)
        .await
        .expect_err("loading a row with an unknown action must fail");
    assert!(
        err.to_string()
            .contains("unknown ActivityAction value \"bogus\""),
        "unexpected error: {err}"
    );

    drop(conn);
    drop(pconn);
    drop(pool);
    drop_scratch_database(DB);
}

#[tokio::test]
async fn pool_isolation_interleaved() {
    const DB: &str = "kairos_pool_isolation_test";
    let url = scratch_database(DB, &["alpha", "beta"]);

    // Deliberately small pool: 2 connections serving 2 tenants interleaved
    // guarantees each physical connection is reused across tenants.
    let pool = TenantPool::new(&url, 2).await.expect("building pool");

    // Marker rows, one per tenant, inserted through the pinned pool.
    for slug in ["alpha", "beta"] {
        let mut conn = pool.tenant(slug).await.expect("tenant checkout");
        diesel::insert_into(s::teams::table)
            .values(NewTeam {
                name: format!("{slug}-team"),
                slug: format!("{slug}-team"),
                team_type: TeamType::StreamAligned,
            })
            .execute(&mut *conn)
            .await
            .expect("inserting marker team");
        conn.release().await.expect("explicit release");
    }

    async fn team_names(conn: &mut kairos_db::TenantConnection) -> Vec<String> {
        s::teams::table
            .order(s::teams::name.asc())
            .select(s::teams::name)
            .load::<String>(&mut **conn)
            .await
            .expect("loading team names")
    }

    // Interleaved checkouts: both tenants held simultaneously from the same
    // pool, queried back and forth. Rows must only ever come from the
    // pinned tenant.
    for round in 0..8 {
        let mut alpha = pool.tenant("alpha").await.expect("alpha checkout");
        let mut beta = pool.tenant("beta").await.expect("beta checkout");

        assert_eq!(
            show_search_path(&mut alpha).await,
            "org_alpha, public",
            "round {round}: alpha search_path"
        );
        assert_eq!(
            show_search_path(&mut beta).await,
            "org_beta, public",
            "round {round}: beta search_path"
        );

        let alpha_rows = team_names(&mut alpha).await;
        let beta_rows = team_names(&mut beta).await;
        // Query alpha again AFTER beta ran on the pool, same checkouts.
        let alpha_again = team_names(&mut alpha).await;

        assert_eq!(alpha_rows, ["alpha-team"], "round {round}: alpha leakage");
        assert_eq!(beta_rows, ["beta-team"], "round {round}: beta leakage");
        assert_eq!(alpha_again, ["alpha-team"], "round {round}: alpha drifted");

        // Alternate drop order so both connections cycle through both
        // tenants across rounds.
        if round % 2 == 0 {
            drop(alpha);
            drop(beta);
        } else {
            drop(beta);
            drop(alpha);
        }
    }

    // Tenant-created data is invisible from a public-pinned checkout: the
    // unqualified relation does not even resolve.
    let mut pconn = pool.public_conn().await.expect("public checkout");
    assert_eq!(show_search_path(&mut pconn).await, "public");
    let err = sql_query("SELECT count(*) FROM teams")
        .execute(&mut pconn)
        .await
        .expect_err("unqualified tenant table must not resolve from public");
    assert!(
        err.to_string()
            .contains("relation \"teams\" does not exist"),
        "unexpected error: {err}"
    );
    drop(pconn);
    drop(pool);

    // Reset-on-return, observed directly: with a single-connection pool the
    // raw (unpinned) checkout after dropping a TenantConnection is the SAME
    // backend, and its search_path must be back to public.
    let pool1 = TenantPool::new(&url, 1)
        .await
        .expect("building 1-conn pool");
    let mut conn = pool1.tenant("alpha").await.expect("alpha checkout");
    let pinned_pid = backend_pid(&mut conn).await;
    assert_eq!(show_search_path(&mut conn).await, "org_alpha, public");
    drop(conn); // Drop path: reset task runs before the pool gets it back.

    let mut raw = pool1
        .raw_pool()
        .get_owned()
        .await
        .expect("raw checkout after drop");
    assert_eq!(
        backend_pid(&mut raw).await,
        pinned_pid,
        "expected the same physical connection back from the 1-conn pool"
    );
    assert_eq!(
        show_search_path(&mut raw).await,
        "public",
        "search_path must be reset when a TenantConnection returns to the pool"
    );
    drop(raw);

    // Invalid slugs are refused before touching the pool.
    let err = pool1.tenant("ACME; DROP SCHEMA public").await;
    assert!(matches!(err, Err(PoolError::InvalidSlug(_))));

    // Sanity: tenant ids differ across schemas (shared UUID space, separate
    // tables) — belt-and-braces that the marker rows are distinct rows.
    let mut alpha = pool1.tenant("alpha").await.expect("alpha checkout");
    let alpha_ids: Vec<Uuid> = s::teams::table
        .select(s::teams::id)
        .load(&mut *alpha)
        .await
        .expect("alpha ids");
    alpha.release().await.expect("release");
    let mut beta = pool1.tenant("beta").await.expect("beta checkout");
    let beta_ids: Vec<Uuid> = s::teams::table
        .select(s::teams::id)
        .load(&mut *beta)
        .await
        .expect("beta ids");
    beta.release().await.expect("release");
    assert!(alpha_ids.iter().all(|id| !beta_ids.contains(id)));

    drop(pool1);
    drop_scratch_database(DB);
}
