//! The refresh path against a real database and a real provider
//! (KAIROS-T-0190).
//!
//! The unit tests in `kairos_server::embedding` can only check the bookkeeping.
//! What matters is the behaviour the design exists for — that unchanged text is
//! never sent to a model, that an append to one section costs one embedding
//! rather than a whole document, and that a model change invalidates everything
//! rather than silently comparing across vector spaces. All of that needs the
//! store, so it lives here.
//!
//! The provider is `kairos_embed::DeterministicProvider`: stable, free and
//! instant, which is exactly what an assertion about *how many* embeddings
//! happened needs. It cannot say anything about whether retrieval finds the
//! right thing — that is KAIROS-T-0189's measurements, against a real model.

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::embeddings::{counts, metadata_for, pending_primary};
use kairos_db::items::{self, CreateTask};
use kairos_db::models::{BoardLevel, NewUser, TaskType, WorkClass};
use kairos_db::{create_board, provision_tenant, run_public_migrations, schema};
use kairos_embed::{DeterministicProvider, EmbeddingProvider};
use kairos_server::embedding::EmbeddingService;

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_embedding_service_test";

/// A document with a heading per section, so chunking produces a known count.
fn document(status: &str) -> String {
    format!(
        "## Objective\n\nMake refunds round after tax.\n\n\
         ## Implementation Notes\n\nThe total is computed in the wrong order.\n\n\
         ## Status Updates\n\n{status}\n"
    )
}

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database path segment");
    format!("{base}/{db_name}")
}

#[test]
fn refreshing_embeds_only_what_changed() {
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url)
        .unwrap_or_else(|e| panic!("cannot connect to compose postgres at {admin_url}: {e}"));
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch database");

    let url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&url).expect("connecting");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
            user_name: "dex|alice".into(),
            email: "alice@acme.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("inserting alice");
    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("creating the board")
    .id;

    let task = items::create_task(
        &mut conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title: "Refund rounds the wrong way",
            content: &document("Investigating."),
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            team_id: None,
            repository_id: None,
        },
        alice,
    )
    .expect("creating the task");

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let service = EmbeddingService::new(provider);
    let model = service.model().clone();

    let one = |conn: &mut PgConnection| {
        let items = pending_primary(conn, &model, 10, 0).expect("pending");
        let item = items
            .into_iter()
            .find(|i| i.id == task.id)
            .expect("the task is pending");
        let metadata = metadata_for(conn, &[task.id]).expect("metadata");
        let pairs: Vec<(String, String)> = metadata.into_iter().map(|(_, l, v)| (l, v)).collect();
        service
            .refresh_item(conn, &item, &pairs)
            .expect("refreshing")
    };

    // ---- first pass: everything is new ------------------------------------
    let first = one(&mut conn);
    assert!(first.primary_embedded, "the primary vector is new");
    assert_eq!(first.chunks_total, 3, "three sections, three chunks");
    assert_eq!(first.chunks_embedded, 3, "all three are new");
    assert_eq!(first.embedded(), 4, "primary + three chunks");

    // ---- second pass, nothing changed: no model call at all ---------------
    let second = one(&mut conn);
    assert!(
        !second.did_work(),
        "unchanged text must not reach the model: {second:?}"
    );
    assert_eq!(second.chunks_total, 3, "but the chunks are still counted");

    // ---- append to ONE section --------------------------------------------
    // The commonest write an agent makes, and the whole reason chunking exists.
    items::update_item_content(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        task.id,
        items::ContentUpdate {
            new_title: None,
            new_content: &document("Investigating.\n\nFound it: rounding is applied pre-tax."),
            expected_version: task.version,
        },
        alice,
    )
    .expect("appending to Status Updates");

    let third = one(&mut conn);
    assert_eq!(
        third.chunks_embedded, 1,
        "one section moved, so one embedding — not three: {third:?}"
    );
    assert_eq!(third.chunks_total, 3);
    assert!(
        !third.primary_embedded,
        "the opening prose did not change, so the primary vector is untouched"
    );

    // ---- a title change DOES move the primary vector ----------------------
    let reloaded = pending_primary(&mut conn, &model, 10, 0)
        .expect("pending")
        .into_iter()
        .find(|i| i.id == task.id)
        .expect("the task");
    let version = sql_query("SELECT version FROM tasks WHERE id = $1")
        .bind::<diesel::sql_types::Uuid, _>(task.id)
        .get_result::<VersionRow>(&mut conn)
        .expect("version")
        .version;
    items::update_item_content(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        task.id,
        items::ContentUpdate {
            new_title: Some("Refund rounding is applied before tax"),
            new_content: &reloaded.content,
            expected_version: version,
        },
        alice,
    )
    .expect("retitling");

    let fourth = one(&mut conn);
    assert!(
        fourth.primary_embedded,
        "the title is part of the composed text, so it moved"
    );
    assert_eq!(
        fourth.chunks_embedded, 0,
        "but the body did not, so no chunk was re-embedded: {fourth:?}"
    );

    // ---- a different model invalidates everything -------------------------
    let other: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::new(128));
    let other_service = EmbeddingService::new(other);
    let c = counts(&mut conn, other_service.model()).expect("counts");
    assert_eq!(
        c.embedded, 0,
        "vectors from another model do not count as embedded"
    );
    let batch = other_service
        .refresh_batch(&mut conn, 10)
        .expect("refreshing under a new model");
    assert_eq!(batch.items_changed, 1, "the item is redone from scratch");
    assert_eq!(
        batch.texts_embedded, 4,
        "primary + three chunks, because a hash from another vector space \
         cannot be trusted: {batch:?}"
    );

    // ---- and a batch that is already current does nothing ------------------
    let again = other_service
        .refresh_batch(&mut conn, 10)
        .expect("second batch");
    assert_eq!(again.items_seen, 1);
    assert_eq!(again.items_changed, 0);
    assert_eq!(again.texts_embedded, 0, "no work, no calls: {again:?}");
}

#[derive(diesel::QueryableByName)]
struct VersionRow {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    version: i32,
}

/// A sweep must reach past its first page (KAIROS-T-0193).
///
/// The regression test for a bug no smaller run could find. `refresh_batch` took
/// the first page by short code every time, so once those were current it
/// reported "nothing to do" for ever and items after them were never embedded at
/// all. It survived every earlier test because every earlier fixture was smaller
/// than one page.
#[test]
fn a_sweep_reaches_items_beyond_the_first_page() {
    const SCRATCH: &str = "kairos_sweep_paging_test";
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url)
        .unwrap_or_else(|e| panic!("cannot connect to compose postgres at {admin_url}: {e}"));
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch");
    sql_query(format!("CREATE DATABASE {SCRATCH}"))
        .execute(&mut admin)
        .expect("creating scratch");
    let url = with_database(&admin_url, SCRATCH);

    let mut conn = PgConnection::establish(&url).expect("connecting");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning");
    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
            user_name: "dex|alice".into(),
            email: "alice@acme.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("alice");
    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("board")
    .id;

    // Three pages' worth at the page size used below.
    const PAGE: i64 = 5;
    for i in 0..(PAGE * 3) {
        items::create_task(
            &mut conn,
            CreateTask {
                board_id: board,
                column_id: None,
                title: &format!("Item {i:03}"),
                content: &document(&format!("Body {i:03}")),
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("task");
    }

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let service = EmbeddingService::new(provider);
    let model = service.model().clone();

    // Repeated pages from offset 0 — exactly what the sweep used to do. It must
    // converge now, because never-embedded rows sort to the front.
    for _ in 0..10 {
        if service
            .refresh_batch(&mut conn, PAGE)
            .unwrap()
            .items_changed
            == 0
        {
            break;
        }
    }
    let c = counts(&mut conn, &model).expect("counts");
    assert_eq!(
        c.missing(),
        0,
        "every item is embedded, not just the first page: {c:?}"
    );

    // And paging explicitly reaches the far end: a third page exists and is
    // disjoint from the first, which offset-0-only never returns.
    let far = kairos_db::embeddings::pending_primary(&mut conn, &model, PAGE, PAGE * 2)
        .expect("the third page");
    assert_eq!(far.len() as i64, PAGE, "a third page exists");
    let first =
        kairos_db::embeddings::pending_primary(&mut conn, &model, PAGE, 0).expect("the first page");
    assert!(
        far.iter().all(|f| !first.iter().any(|x| x.id == f.id)),
        "the third page is disjoint from the first — the cursor really moves"
    );
}

/// The background refresher embeds work that arrives after it started
/// (KAIROS-T-0190).
///
/// This is the claim that "embedding happens off the write path" rests on: a
/// create returns immediately, and something else notices. Asserted by creating
/// an item while the sweep is running and waiting for it to be picked up —
/// there is no queue to inspect, because staleness is derived rather than
/// recorded, so the only honest test is to watch the world change.
#[tokio::test]
async fn the_refresher_picks_up_work_created_after_it_started() {
    const SCRATCH: &str = "kairos_refresher_test";
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url)
        .unwrap_or_else(|e| panic!("cannot connect to compose postgres at {admin_url}: {e}"));
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch");
    sql_query(format!("CREATE DATABASE {SCRATCH}"))
        .execute(&mut admin)
        .expect("creating scratch");
    let url = with_database(&admin_url, SCRATCH);

    let mut conn = PgConnection::establish(&url).expect("connecting");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning");
    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
            user_name: "dex|alice".into(),
            email: "alice@acme.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("alice");
    let board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        Some(alice),
    )
    .expect("board")
    .id;

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let service = Arc::new(EmbeddingService::new(provider));
    let model = service.model().clone();
    let blocking = kairos_server::blocking::BlockingTenantPool::new(&url, 4);

    let sweeper = tokio::spawn(kairos_server::embedding::run_refresher(
        blocking,
        Arc::clone(&service),
        std::time::Duration::from_millis(200),
        25,
    ));

    // The write. It does not wait for a model, and nothing here asks it to.
    items::create_task(
        &mut conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title: "Filed while the sweeper was running",
            content: &document("Just filed."),
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            team_id: None,
            repository_id: None,
        },
        alice,
    )
    .expect("creating the task");

    // Wait for the sweep to notice, with a bound so a failure is a failure
    // rather than a hang.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let embedded = loop {
        let c = counts(&mut conn, &model).expect("counts");
        if c.embedded > 0 {
            break c.embedded;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the refresher never embedded the new item"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert_eq!(embedded, 1, "the item the sweep found");

    sweeper.abort();
}
