//! KAIROS-T-0733, KAIROS-A-0079 — the dictionary of the organization: the
//! system template `dictionary`, one dictionary for the organization on the
//! board `strategy`, and no team dictionaries.
//!
//! - a new tenant has the template, and the migration
//!   `dictionary_template` gives it to a tenant that has none, one time,
//!   with the text of the system default,
//! - a dictionary on a team board is refused and the refusal names the
//!   board `strategy`: at create, at a change of the owner board, and at a
//!   restore,
//! - a second dictionary on the board `strategy` is refused and the first
//!   is named: at create and at a restore,
//! - `organization_dictionary` gives the terms and the bad rows, and the
//!   check of the typed sections takes its terms (`dictionary_terms`),
//! - a defined term passes the vocabulary check, and a word that is in no
//!   list fails it.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its own
//! scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::dictionaries;
use kairos_db::items::{ItemError, restore_item, set_document_board};
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    CreateDocument, create_board, create_document, provision_tenant, run_public_migrations,
    soft_delete_item,
};

static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_dictionaries_t0733");

const UP: &str = include_str!("../migrations/tenant/2026-10-30-000700_dictionary_template/up.sql");

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

struct Fixture {
    conn: PgConnection,
    alice: Uuid,
    dictionary_template: Uuid,
    strategy: Uuid,
    platform_board: Uuid,
}

fn fixture() -> (Fixture, scratch::ScratchGuard) {
    let admin_url = scratch::admin_database_url();
    let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!("cannot connect to compose postgres at {admin_url}: {e} (is the stack up?)")
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch database");
    let guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let mut conn = PgConnection::establish(&with_database(&admin_url, &SCRATCH_DB))
        .expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let alice = {
        use kairos_db::schema::users;
        diesel::insert_into(users::table)
            .values(NewUser {
                external_id: "dex|alice".into(),
                user_name: "alice".into(),
                email: "alice@acme.test".into(),
                display_name: "Alice".into(),
            })
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("inserting user")
            .id
    };
    let mut team = |name: &str, slug: &str| -> Uuid {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(NewTeam {
                name: name.into(),
                slug: slug.into(),
                team_type: TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result(&mut conn)
            .expect("inserting team")
    };
    let platform = team("Platform", "platform");
    let mut board = |team: Uuid, slug: &str| -> Uuid {
        create_board(
            &mut conn,
            BoardLevel::Delivery,
            slug,
            slug,
            kairos_db::CodePrefix::FromSlug,
            Some(team),
            Some(alice),
        )
        .expect("delivery board")
        .id
    };
    let platform_board = board(platform, "platform");

    let (dictionary_template, strategy) = {
        use kairos_db::schema::{boards, templates};
        let template: Uuid = templates::table
            .filter(templates::slug.eq("dictionary"))
            .filter(templates::is_system_default.eq(true))
            .select(templates::id)
            .first(&mut conn)
            .expect("a new tenant has the template dictionary");
        let strategy: Uuid = boards::table
            .filter(boards::slug.eq("strategy"))
            .select(boards::id)
            .first(&mut conn)
            .expect("the board strategy");
        (template, strategy)
    };
    (
        Fixture {
            conn,
            alice,
            dictionary_template,
            strategy,
            platform_board,
        },
        guard,
    )
}

fn dictionary(f: &mut Fixture, board: Uuid, rows: &str) -> Result<(Uuid, String), ItemError> {
    let content =
        format!("# Dictionary\n\n| Word | Part of speech | Meaning |\n|---|---|---|\n{rows}");
    create_document(
        &mut f.conn,
        CreateDocument {
            board_id: board,
            title: "Dictionary",
            content: Some(&content),
            template_id: Some(f.dictionary_template),
        },
        f.alice,
    )
    .map(|d| (d.id, d.short_code))
}

#[test]
fn one_dictionary_for_the_organization() {
    let (mut f, _guard) = fixture();

    // --- the template, and the migration for a tenant that has none -------
    #[derive(QueryableByName)]
    struct Content {
        #[diesel(sql_type = diesel::sql_types::Text)]
        content: String,
    }
    let system: Content =
        sql_query("SELECT content FROM public.system_templates WHERE slug = 'dictionary'")
            .get_result(&mut f.conn)
            .expect("the system default");
    assert!(
        system
            .content
            .contains("| Word | Part of speech | Meaning |")
    );
    sql_query("DELETE FROM templates WHERE slug = 'dictionary'")
        .execute(&mut f.conn)
        .expect("removing the template");
    f.conn.batch_execute(UP).expect("the migration");
    f.conn.batch_execute(UP).expect("the migration, again");
    let copies: Vec<Content> =
        sql_query("SELECT content FROM templates WHERE slug = 'dictionary' AND is_system_default")
            .load(&mut f.conn)
            .expect("the template of the migration");
    assert_eq!(copies.len(), 1, "one template after two runs");
    assert_eq!(
        copies[0].content, system.content,
        "the text of the system default"
    );
    f.dictionary_template = {
        use kairos_db::schema::templates;
        templates::table
            .filter(templates::slug.eq("dictionary"))
            .select(templates::id)
            .first(&mut f.conn)
            .expect("the template")
    };

    // --- no dictionary: no terms -------------------------------------------
    let empty = dictionaries::organization_dictionary(&mut f.conn).expect("no dictionary");
    assert_eq!(empty, dictionaries::OrganizationDictionary::default());

    // --- a dictionary on a team board is refused, and names `strategy` ----
    let platform_board = f.platform_board;
    let err = dictionary(
        &mut f,
        platform_board,
        "| lane | noun | The kind of work. |\n",
    )
    .expect_err("a dictionary on a team board");
    match &err {
        ItemError::DictionaryNotOnStrategy { board } => assert_eq!(board, "platform"),
        other => panic!("expected DictionaryNotOnStrategy, got {other}"),
    }
    assert!(err.to_string().contains("the board strategy"), "{err}");

    // --- the dictionary of the organization -------------------------------
    let strategy = f.strategy;
    let (org_dict, org_code) = dictionary(
        &mut f,
        strategy,
        "| production | noun | Code that runs for the users. |\n\
         | Board | noun | A set of columns. |\n\
         | production document | noun | A document that a team uses for its work. |\n\
         | lane | noun |\n",
    )
    .expect("the organization dictionary");

    // A second dictionary on the board `strategy` is refused, and the
    // first is named.
    let err = dictionary(&mut f, strategy, "| card | noun | A task. |\n")
        .expect_err("a second dictionary");
    match &err {
        ItemError::DictionaryExists { board, existing } => {
            assert_eq!(board, "strategy");
            assert_eq!(existing, &org_code);
        }
        other => panic!("expected DictionaryExists, got {other}"),
    }
    // A document with no template is not a dictionary: no refusal.
    create_document(
        &mut f.conn,
        CreateDocument {
            board_id: f.strategy,
            title: "Notes",
            content: Some("free text"),
            template_id: None,
        },
        f.alice,
    )
    .expect("a plain document on the same board");

    // A change of the owner board of the dictionary to a team board is
    // refused, and names `strategy`.
    let err = set_document_board(&mut f.conn, org_dict, f.platform_board, f.alice)
        .expect_err("a dictionary leaves the board strategy");
    assert!(
        matches!(&err, ItemError::DictionaryNotOnStrategy { board } if board == "platform"),
        "{err}"
    );

    // --- restore -------------------------------------------------------------
    // A dictionary on a team board from before KAIROS-A-0079 (written here
    // by SQL) does not come back.
    let old_team_dict = create_document(
        &mut f.conn,
        CreateDocument {
            board_id: f.platform_board,
            title: "Old team dictionary",
            content: Some("| Word | Part of speech | Meaning |\n|---|---|---|\n"),
            template_id: None,
        },
        f.alice,
    )
    .expect("a document")
    .id;
    sql_query("UPDATE documents SET template_id = $1 WHERE id = $2")
        .bind::<diesel::sql_types::Uuid, _>(f.dictionary_template)
        .bind::<diesel::sql_types::Uuid, _>(old_team_dict)
        .execute(&mut f.conn)
        .expect("the old team dictionary");
    soft_delete_item(&mut f.conn, ItemType::Document, old_team_dict, f.alice).expect("archive");
    let blocked = restore_item(&mut f.conn, ItemType::Document, old_team_dict, f.alice)
        .expect("restore")
        .expect_err("a dictionary on a team board");
    assert!(
        blocked
            .missing
            .iter()
            .any(|m| m.contains("the board strategy") && m.contains("platform")),
        "{blocked:?}"
    );

    // An archived dictionary does not come back next to a live one.
    soft_delete_item(&mut f.conn, ItemType::Document, org_dict, f.alice).expect("archive");
    let (second, _) = dictionary(&mut f, strategy, "| card | noun | A task. |\n")
        .expect("no live dictionary on the board now");
    let blocked = restore_item(&mut f.conn, ItemType::Document, org_dict, f.alice)
        .expect("restore")
        .expect_err("a live dictionary is on the board");
    assert!(
        blocked.missing.iter().any(|m| m.contains("dictionary")),
        "{blocked:?}"
    );
    // Put the first dictionary back for the rest of the test.
    soft_delete_item(&mut f.conn, ItemType::Document, second, f.alice).expect("archive");
    restore_item(&mut f.conn, ItemType::Document, org_dict, f.alice)
        .expect("restore")
        .expect("no live dictionary on the board now");

    // --- the terms and the bad rows of the organization dictionary --------
    let read = dictionaries::organization_dictionary(&mut f.conn).expect("terms");
    assert_eq!(read.short_code.as_deref(), Some(org_code.as_str()));
    let terms: Vec<(&str, &str)> = read
        .terms
        .iter()
        .map(|t| (t.word.as_str(), t.meaning.as_str()))
        .collect();
    assert_eq!(
        terms,
        [
            ("Board", "A set of columns."),
            ("production", "Code that runs for the users."),
            (
                "production document",
                "A document that a team uses for its work."
            ),
        ]
    );
    // A bad row is given with its line.
    assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
    assert_eq!(read.problems[0].line, 8);

    // The check of the typed sections takes the same terms, for a
    // document on any board.
    assert_eq!(
        kairos_db::section_checks::dictionary_terms(&mut f.conn).expect("terms"),
        ["Board", "production", "production document"]
    );

    // --- the vocabulary hook: a defined term passes, an undefined word fails
    let vocabulary = read.vocabulary(["the", "is", "a"]);
    assert!(
        vocabulary
            .check_text("The production document is a board.")
            .is_empty(),
        "each word is in the base list or is a term"
    );
    let found = vocabulary.check_text("The production document is a widget.");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule, "STE-V1");
    assert!(found[0].message.contains("widget"), "{}", found[0].message);
}
