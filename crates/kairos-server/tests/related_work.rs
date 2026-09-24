//! Related-work proposals against a real database (KAIROS-T-0191).
//!
//! The fusion and the claims are unit-tested pure in `kairos_core::retrieval`.
//! What needs a database is everything around them: that the queries return what
//! the fusion expects, that a chunk's heading survives to become a citation, that
//! an already-linked item is not proposed, and that with no vectors the whole
//! thing still answers — from text — and says that it did.
//!
//! The provider is deterministic, so "these two are similar" is not a claim this
//! file can make. It uses **identical text** to force similarity, which the hash
//! provider reproduces exactly. Whether retrieval finds genuinely related work is
//! KAIROS-T-0189's measurements against a real model, and the seeded tenant's own
//! distribution in KAIROS-T-0190.

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::retrieval::{Claim, RetrievalConfig};
use kairos_db::items::{self, CreateInitiative, CreateTask};
use kairos_db::models::{BoardLevel, NewUser, RelationshipType, TaskType, WorkClass};
use kairos_db::{create_board, graph, provision_tenant, run_public_migrations, schema};
use kairos_embed::{DeterministicProvider, EmbeddingProvider};
use kairos_server::embedding::EmbeddingService;

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_related_work_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database path segment");
    format!("{base}/{db}")
}

/// A document whose MIDDLE section is the part shared between fixtures.
///
/// The surrounding sections are deliberately unique per document. The first
/// version made the `## Objective` text identical everywhere, and the
/// deterministic provider — which hashes bytes — duly matched every document to
/// every other on that section and cited "Objective". A true result about a
/// fixture that said nothing.
fn doc(unique: &str, shared: &str) -> String {
    format!(
        "## Objective\n\nSomething about {unique}, which nothing else says.\n\n\
         ## Implementation Notes\n\n{shared}\n\n\
         ## Status Updates\n\nNothing yet for {unique}.\n"
    )
}

#[test]
fn related_work_proposes_and_degrades() {
    let admin_url = admin_database_url();
    let mut admin = PgConnection::establish(&admin_url)
        .unwrap_or_else(|e| panic!("cannot connect to compose postgres at {admin_url}: {e}"));
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch");
    let url = with_database(&admin_url, SCRATCH_DB);

    let mut conn = PgConnection::establish(&url).expect("connecting");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning");

    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
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
    let init_board = create_board(
        &mut conn,
        BoardLevel::Initiative,
        "Initiatives",
        "initiatives",
        None,
        Some(alice),
    )
    .expect("initiative board")
    .id;

    let task = |conn: &mut PgConnection, title: &str, content: &str| {
        items::create_task(
            conn,
            CreateTask {
                board_id: board,
                column_id: None,
                title,
                content,
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("task")
    };

    // The item asked about.
    let subject = task(
        &mut conn,
        "Refund rounds before tax",
        &doc(
            "the subject",
            "The refund total is rounded before tax instead of after.",
        ),
    );
    // Identical detail: the deterministic provider reproduces it exactly, so
    // this is a duplicate by construction rather than by judgement.
    let twin = task(
        &mut conn,
        "Rounding applied too early on refunds",
        &doc(
            "the twin",
            "The refund total is rounded before tax instead of after.",
        ),
    );
    // Same, but already finished — prior art.
    let old = task(
        &mut conn,
        "Historic rounding fix",
        &doc(
            "the old one",
            "The refund total is rounded before tax instead of after.",
        ),
    );
    // Same, but already linked — nothing to propose.
    let linked = task(
        &mut conn,
        "Known related rounding work",
        &doc(
            "the linked one",
            "The refund total is rounded before tax instead of after.",
        ),
    );
    // Unrelated.
    let _noise = task(
        &mut conn,
        "Dark mode toggle",
        &doc("dark mode", "A settings page switch."),
    );

    graph::link_items(
        &mut conn,
        subject.id,
        linked.id,
        RelationshipType::Blocks,
        alice,
    )
    .expect("linking");
    items::soft_delete_item(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        old.id,
        alice,
    )
    .expect("archiving the old one");

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let service = EmbeddingService::new(provider);
    let config = RetrievalConfig::default();

    // ---- with no vectors at all: still answers, and says how ---------------
    let degraded = service
        .related_work(&mut conn, &subject.short_code, &config)
        .expect("a lexical-only answer");
    assert!(
        degraded.sources.degraded(),
        "no vectors have been written yet"
    );
    assert!(
        !degraded.proposals.is_empty(),
        "rule 7: it answers from text rather than returning nothing"
    );
    assert!(
        degraded
            .proposals
            .iter()
            .all(|p| p.why.contains("Text only")),
        "and every proposal admits it: {:?}",
        degraded.proposals[0].why
    );

    // ---- embed everything, then ask again ----------------------------------
    let batch = service.refresh_batch(&mut conn, 100).expect("embedding");
    assert!(batch.items_changed >= 5, "{batch:?}");

    let out = service
        .related_work(&mut conn, &subject.short_code, &config)
        .expect("a hybrid answer");
    assert!(!out.sources.degraded(), "vectors are available now");
    assert_eq!(out.short_code, subject.short_code);

    let codes: Vec<&str> = out
        .proposals
        .iter()
        .map(|p| p.short_code.as_str())
        .collect();
    assert!(
        codes.contains(&twin.short_code.as_str()),
        "the duplicate is found: {codes:?}"
    );
    assert!(
        !codes.contains(&linked.short_code.as_str()),
        "the already-linked item is not proposed — there is no edge to draw: {codes:?}"
    );
    assert!(
        !codes.contains(&subject.short_code.as_str()),
        "and it does not propose itself: {codes:?}"
    );

    // Prior art: the archived twin, claimed as such.
    let prior = out
        .proposals
        .iter()
        .find(|p| p.short_code == old.short_code)
        .expect("the archived item is found — A-0020 kept it searchable");
    assert_eq!(prior.claim, Claim::PriorArt);
    assert!(prior.why.contains("finished or put away"), "{}", prior.why);

    // The citation: a chunk heading, echoed verbatim, one of the document's real
    // ones.
    //
    // WHICH section is cited cannot be asserted here, and the reason is worth
    // knowing. The probe is the item's PRIMARY vector compared against other
    // items' CHUNK vectors — which is right for a real model, where both live in
    // one space, and meaningless for a provider that hashes bytes. So the
    // nearest chunk under this provider is arbitrary. What this checks is the
    // plumbing: that a heading survives the query, the fusion and the sentence.
    // That the RIGHT section is cited is `the_matched_heading_is_cited_verbatim`
    // in kairos_core::retrieval, where it can be stated exactly.
    let headings = ["Objective", "Implementation Notes", "Status Updates"];
    assert!(
        out.proposals.iter().any(|p| headings
            .iter()
            .any(|h| p.why.contains(&format!("under \"{h}\"")))),
        "a real section is cited: {:?}",
        out.proposals.iter().map(|p| &p.why).collect::<Vec<_>>()
    );

    // Bounded, and the same question twice gives the same answer.
    assert!(out.proposals.len() <= config.limit);
    let again = service
        .related_work(&mut conn, &subject.short_code, &config)
        .expect("asked twice");
    assert_eq!(
        codes,
        again
            .proposals
            .iter()
            .map(|p| p.short_code.as_str())
            .collect::<Vec<_>>(),
        "deterministic"
    );

    // ---- siblings read as duplicates, not dependencies ---------------------
    let parent = items::create_initiative(
        &mut conn,
        CreateInitiative {
            board_id: init_board,
            column_id: None,
            title: "Billing correctness",
            content: "Umbrella.",
            complexity: None,
            bucket_type: None,
        },
        alice,
    )
    .expect("parent");
    for child in [subject.id, twin.id] {
        graph::link_items(&mut conn, parent.id, child, RelationshipType::Parent, alice)
            .expect("parenting");
    }
    service.refresh_batch(&mut conn, 100).expect("re-embedding");
    let siblings = service
        .related_work(&mut conn, &subject.short_code, &config)
        .expect("after parenting");
    let twin_now = siblings
        .proposals
        .iter()
        .find(|p| p.short_code == twin.short_code)
        .expect("still found");
    assert_eq!(
        twin_now.claim,
        Claim::NearDuplicate,
        "sharing a parent changes the claim from dependency to duplicate"
    );
    assert!(twin_now.why.contains("same parent"), "{}", twin_now.why);

    // ---- an unknown short code is an error, not an empty answer ------------
    service
        .related_work(&mut conn, "DEMO-T-9999", &config)
        .expect_err("an unknown item must not read as 'nothing is related'");
}

/// Eyeball real retrieval against the seeded `demo` tenant, with the **real**
/// model (KAIROS-T-0191).
///
/// `#[ignore]` because it asserts almost nothing: it needs a seeded, embedded
/// tenant that only a developer's machine has, and its value is in being read
/// rather than in passing. The deterministic tests above prove the mechanism;
/// this shows whether the answers are any good, which no fake can.
///
/// ```sh
/// angreal db seed && angreal db backfill-embeddings
/// KAIROS_EMBED_CACHE=$PWD/target/embed-cache \
///   cargo test -p kairos-server --test related_work -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a seeded, embedded demo tenant; run deliberately to read the output"]
fn show_real_proposals_for_the_demo_tenant() {
    let url = admin_database_url();
    let mut conn = PgConnection::establish(&url).expect("connecting to the dev database");
    sql_query("SET search_path TO org_demo, public")
        .execute(&mut conn)
        .expect("pinning org_demo");

    let config = kairos_embed::EmbedConfig::from_env().expect("embed config");
    let provider = config
        .build()
        .expect("building the provider")
        .expect("a provider — set KAIROS_EMBED_CACHE to target/embed-cache");
    let service = EmbeddingService::new(provider);

    for code in ["DEMO-T-0002", "DEMO-T-0004", "DEMO-I-0001"] {
        match service.related_work(&mut conn, code, &RetrievalConfig::default()) {
            Ok(found) => {
                println!("\n=== {} ({}) ===", found.short_code, found.sources.note());
                for p in &found.proposals {
                    println!("  [{}] {} — {}", p.claim, p.short_code, p.title);
                    println!("      {}", p.why);
                }
            }
            Err(e) => println!("\n=== {code}: {e}"),
        }
    }
}
