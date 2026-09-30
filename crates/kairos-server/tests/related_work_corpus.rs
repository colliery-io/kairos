//! Related work against a fixed corpus of real items, with the REAL local model
//! (COLLIERY-T-1840).
//!
//! `related_work.rs` proves the plumbing with the deterministic provider, which
//! cannot say that two texts are about the same thing. This file can: it embeds
//! 26 real items from the colliery tenant with `bge-small-en-v1.5-q` and checks
//! what comes back.
//!
//! The corpus is `fixtures/related_work_corpus.json`: the items that the
//! investigation of COLLIERY-T-1840 needed, as stored in the live tenant on
//! 2026-09-30 (importer footers included):
//!
//! - COLLIERY-T-1757 (the broker serves the web UI with an SPA fallback), the
//!   item asked about, with its parent initiative and the other console items
//!   of the same project (COLLIERY-A-0109, COLLIERY-T-1756, COLLIERY-T-1772,
//!   COLLIERY-I-0263);
//! - the prior art it missed: COLLIERY-I-0220 (Cloacina serves its UI from the
//!   server binary) and COLLIERY-T-0039 (Kairos serves its GUI at `/`);
//! - the unrelated items it proposed instead, most of them in the same
//!   repository;
//! - COLLIERY-T-1801 (a tenant schema bug) and the items it was already right
//!   about — the case that must not get worse.
//!
//! What the test holds T-1757 to: the unrelated proposals leave the first 5,
//! and the prior art in the other repositories is proposed (in the first 10).
//! It does not ask for the prior art in the first 5. The console items of the
//! same project are closer in meaning and fill the first places (T-0039 7th,
//! I-0220 10th on 2026-09-30), and Dylan chose on 2026-09-30 not to reserve a
//! place for another repository.
//!
//! One more item is written here rather than read from the tenant: a
//! **template twin** of COLLIERY-T-1757. It has the same headings, the same
//! parent line and the same short template bodies, and it is about something
//! else. It must not be in the first 5. And a reason may quote a heading only
//! when that section of the item has real content: before the fix the reasons
//! quoted a 137-character "Dependencies" section and a 95-character note.
//!
//! The model is loaded from `target/embed-cache` (or `KAIROS_EMBED_CACHE`) and
//! is fetched on the first run if it is missing (`angreal dev fetch-model` does
//! the same ahead of time).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::retrieval::RetrievalConfig;
use kairos_core::short_code::ItemType;
use kairos_db::items::{self, CreateAdr, CreateInitiative, CreateTask};
use kairos_db::models::enums::Forge;
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{BoardLevel, NewUser, RelationshipType, TaskType, WorkClass};
use kairos_db::{create_board, graph, provision_tenant, run_public_migrations, schema};
use kairos_embed::EmbeddingProvider;
use kairos_embed::local::{LocalConfig, LocalProvider};
use kairos_server::embedding::{EmbeddingService, RelatedWork};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_related_work_corpus_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database path segment");
    format!("{base}/{db}")
}

#[derive(serde::Deserialize)]
struct Corpus {
    items: Vec<FixtureItem>,
    edges: Vec<FixtureEdge>,
}

#[derive(serde::Deserialize, Clone)]
struct FixtureItem {
    code: String,
    entity_type: String,
    title: String,
    repository: Option<String>,
    archived: bool,
    content: String,
}

#[derive(serde::Deserialize)]
struct FixtureEdge {
    from: String,
    relationship: String,
    to: String,
}

/// The template twin: COLLIERY-T-1757's headings, parent line and template
/// bodies, about a different subject.
const TWIN_CODE: &str = "TEMPLATE-TWIN";
const TWIN_TITLE: &str = "agent rotates its log files at midnight and keeps seven";
const TWIN_CONTENT: &str = "# agent rotates its log files at midnight\n\n\
## Parent Initiative\n\n\
COLLIERY-I-0262 · decision COLLIERY-A-0109\n\n\
## Objective\n\n\
Make `brokkr-agent` rotate its log file at midnight and keep the last seven files, \
so that an agent that runs for months does not fill the disk of its node. Old files \
are compressed with gzip.\n\n\
### Type\n\
- [x] Feature — log rotation\n\n\
## Acceptance Criteria\n\n\
## Acceptance Criteria\n\n\
- [ ] Rotation works.\n\n\
## Implementation Notes\n\n\
### Technical Approach\n\
- TBD\n\n\
### Dependencies\n\
- None.\n\n\
### Risk Considerations\n\
- None.\n\n\
## Status Updates\n\n\
*To be added during implementation*\n";

fn load_corpus() -> Corpus {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/related_work_corpus.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).expect("the corpus fixture parses")
}

fn real_model() -> Arc<dyn EmbeddingProvider> {
    let cache_dir = std::env::var("KAIROS_EMBED_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/embed-cache")
        });
    let provider = LocalProvider::new(&LocalConfig {
        cache_dir,
        allow_download: true,
    })
    .expect("the local model loads (angreal dev fetch-model fills the cache)");
    Arc::new(provider)
}

/// The corpus, created in a scratch tenant. Maps a fixture code to the short
/// code the scratch tenant gave it, and back.
struct Seeded {
    code_of: HashMap<String, String>,
    fixture_of: HashMap<String, String>,
    content_of: HashMap<String, String>,
}

fn seed(conn: &mut PgConnection, corpus: &Corpus) -> Seeded {
    let alice = diesel::insert_into(schema::users::table)
        .values(NewUser {
            external_id: "dex|alice".into(),
            user_name: "dex|alice".into(),
            email: "alice@colliery.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(conn)
        .expect("alice");
    let team = {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(kairos_db::models::teams::NewTeam {
                name: "Colliery".to_string(),
                slug: "colliery-io".to_string(),
                team_type: kairos_db::models::TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result::<Uuid>(conn)
            .expect("team")
    };
    let delivery = create_board(
        conn,
        BoardLevel::Delivery,
        "Delivery",
        "colliery-io-delivery",
        Some(team),
        Some(alice),
    )
    .expect("delivery board")
    .id;
    let initiatives = create_board(
        conn,
        BoardLevel::Initiative,
        "Initiatives",
        "colliery-initiatives",
        None,
        Some(alice),
    )
    .expect("initiative board")
    .id;

    let mut repos: HashMap<String, Uuid> = HashMap::new();
    for slug in ["brokkr", "kairos", "cloacina"] {
        let repo = kairos_db::repositories::create(
            conn,
            NewRepository {
                slug: slug.to_string(),
                forge: Forge::Github,
                repo_full_name: format!("colliery-io/{slug}"),
                repo_url: format!("https://github.com/colliery-io/{slug}"),
                default_branch: "main".to_string(),
                team_id: team,
                description: String::new(),
                created_by: alice,
                updated_by: alice,
            },
        )
        .expect("repository");
        repos.insert(slug.to_string(), repo.id);
    }

    let mut all = corpus.items.clone();
    all.push(FixtureItem {
        code: TWIN_CODE.to_string(),
        entity_type: "task".to_string(),
        title: TWIN_TITLE.to_string(),
        repository: Some("brokkr".to_string()),
        archived: false,
        content: TWIN_CONTENT.to_string(),
    });

    let mut ids: HashMap<String, (Uuid, ItemType)> = HashMap::new();
    let mut seeded = Seeded {
        code_of: HashMap::new(),
        fixture_of: HashMap::new(),
        content_of: HashMap::new(),
    };
    for item in &all {
        let (id, code, kind) = match item.entity_type.as_str() {
            "task" => {
                let t = items::create_task(
                    conn,
                    CreateTask {
                        board_id: delivery,
                        column_id: None,
                        title: &item.title,
                        content: &item.content,
                        task_type: TaskType::Task,
                        work_class: WorkClass::Planned,
                        repository_id: item.repository.as_ref().map(|r| repos[r]),
                    },
                    alice,
                )
                .expect("task");
                (t.id, t.short_code, ItemType::Task)
            }
            "initiative" => {
                let i = items::create_initiative(
                    conn,
                    CreateInitiative {
                        board_id: initiatives,
                        column_id: None,
                        title: &item.title,
                        content: &item.content,
                        complexity: None,
                        bucket_type: None,
                    },
                    alice,
                )
                .expect("initiative");
                (i.id, i.short_code, ItemType::Initiative)
            }
            "adr" => {
                let a = items::create_adr(
                    conn,
                    CreateAdr {
                        board_id: None,
                        column_id: None,
                        title: &item.title,
                        content: &item.content,
                        decision_maker: None,
                        decision_date: None,
                    },
                    alice,
                )
                .expect("adr");
                (a.id, a.short_code, ItemType::Adr)
            }
            other => panic!("the corpus has no {other} items"),
        };
        ids.insert(item.code.clone(), (id, kind));
        seeded.code_of.insert(item.code.clone(), code.clone());
        seeded.fixture_of.insert(code.clone(), item.code.clone());
        seeded.content_of.insert(code, item.content.clone());
    }

    for edge in &corpus.edges {
        let relationship = match edge.relationship.as_str() {
            "parent" => RelationshipType::Parent,
            "blocks" => RelationshipType::Blocks,
            other => panic!("the corpus has no {other} edges"),
        };
        graph::link_items(
            conn,
            ids[&edge.from].0,
            ids[&edge.to].0,
            relationship,
            alice,
        )
        .expect("edge");
    }
    for item in all.iter().filter(|i| i.archived) {
        let (id, kind) = ids[&item.code];
        // A parent archived first takes its archived children with it.
        let live: bool =
            sql_query("SELECT (deleted_at IS NULL) AS live FROM entity_directory WHERE id = $1")
                .bind::<diesel::sql_types::Uuid, _>(id)
                .get_result::<Live>(conn)
                .expect("directory row")
                .live;
        if live {
            items::soft_delete_item(conn, kind, id, alice).expect("archiving");
        }
    }
    seeded
}

#[derive(QueryableByName)]
struct Live {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    live: bool,
}

fn show(seeded: &Seeded, found: &RelatedWork) {
    println!(
        "\n=== {} ({}) ===",
        seeded.fixture_of[&found.short_code],
        found.sources.note()
    );
    for (rank, p) in found.proposals.iter().enumerate() {
        println!(
            "  {:2}. {} {} — {}",
            rank + 1,
            seeded.fixture_of[&p.short_code],
            p.claim,
            p.title
        );
        println!("      {}", p.why);
    }
}

/// The quoted heading of a reason, if the reason quotes one.
fn quoted_heading(why: &str) -> Option<&str> {
    let start = why.find("(under \"")? + "(under \"".len();
    let end = why[start..].find("\")")?;
    Some(&why[start..start + end])
}

#[test]
fn related_work_finds_prior_art_across_repositories_on_a_real_corpus() {
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
    provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning");
    sql_query("SET search_path TO org_colliery, public")
        .execute(&mut conn)
        .expect("pinning");

    let corpus = load_corpus();
    let seeded = seed(&mut conn, &corpus);

    let service = EmbeddingService::new(real_model());
    let batch = service
        .refresh_batch(&mut conn, 100)
        .expect("embedding the corpus");
    assert_eq!(batch.items_seen, corpus.items.len() + 1, "{batch:?}");

    let wide = RetrievalConfig {
        limit: 10,
        ..RetrievalConfig::default()
    };
    let code = |fixture: &str| seeded.code_of[fixture].clone();

    // The full order, for a person who reads the output (--nocapture).
    let everything = RetrievalConfig {
        limit: 100,
        ..RetrievalConfig::default()
    };
    for (asked, watched) in [
        (
            "COLLIERY-T-1757",
            ["COLLIERY-I-0220", "COLLIERY-T-0039", TWIN_CODE],
        ),
        (
            "COLLIERY-T-1801",
            ["COLLIERY-A-0105", "COLLIERY-I-0236", "COLLIERY-T-1821"],
        ),
    ] {
        let all = service
            .related_work(&mut conn, &code(asked), &everything)
            .expect("the full order");
        for w in watched {
            let rank = all
                .proposals
                .iter()
                .position(|p| seeded.fixture_of[&p.short_code] == w)
                .map_or("not proposed".to_string(), |r| format!("rank {}", r + 1));
            println!("{asked}: {w} is at {rank} of {}", all.proposals.len());
        }
    }

    // ---- COLLIERY-T-1757: the prior art in other repositories ---------------
    let found = service
        .related_work(&mut conn, &code("COLLIERY-T-1757"), &wide)
        .expect("related work for T-1757");
    show(&seeded, &found);
    assert!(!found.sources.degraded(), "the corpus has vectors");
    let proposed: Vec<&str> = found
        .proposals
        .iter()
        .map(|p| seeded.fixture_of[&p.short_code].as_str())
        .collect();
    let top5 = &proposed[..proposed.len().min(5)];
    assert!(
        proposed.contains(&"COLLIERY-I-0220") || proposed.contains(&"COLLIERY-T-0039"),
        "the prior art (COLLIERY-I-0220 or COLLIERY-T-0039) is in the first 10: {proposed:?}"
    );
    // The first 5 before the fix, which had nothing to do with serving a UI.
    for unrelated in [
        "COLLIERY-T-1635",
        "COLLIERY-T-1523",
        "COLLIERY-T-1655",
        "COLLIERY-I-0238",
    ] {
        assert!(
            !top5.contains(&unrelated),
            "{unrelated} is about something else, and is not in the first 5: {top5:?}"
        );
    }

    // ---- a template heading does not decide a match -------------------------
    assert!(
        !top5.contains(&TWIN_CODE),
        "the template twin shares only headings and template text, and is not in the first 5: {top5:?}"
    );
    // Each quoted heading is one whose section has real content in the item
    // proposed, and not a heading over a line of codes or "None.".
    for p in &found.proposals {
        let Some(heading) = quoted_heading(&p.why) else {
            continue;
        };
        let content = &seeded.content_of[&p.short_code];
        let masked = kairos_core::embed_text::mask_import_footer(content);
        let has_content = kairos_core::chunk::chunk(&masked).iter().any(|c| {
            c.heading.as_deref() == Some(heading)
                && c.text.trim().chars().count() >= kairos_core::embed_text::MIN_MATCH_CHARS
        });
        assert!(
            has_content,
            "{} quotes \"{heading}\", and that section has no real content: {}",
            seeded.fixture_of[&p.short_code], p.why
        );
    }

    // ---- COLLIERY-T-1801: the good case stays good --------------------------
    let found = service
        .related_work(&mut conn, &code("COLLIERY-T-1801"), &wide)
        .expect("related work for T-1801");
    show(&seeded, &found);
    let top5: Vec<&str> = found
        .proposals
        .iter()
        .take(5)
        .map(|p| seeded.fixture_of[&p.short_code].as_str())
        .collect();
    assert!(
        top5.contains(&"COLLIERY-A-0105") || top5.contains(&"COLLIERY-I-0236"),
        "the schema-per-tenant ADR or its initiative is still in the first 5: {top5:?}"
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch");
}
