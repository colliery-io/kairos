//! KAIROS-T-0733 — dictionaries: the system template `dictionary`, one live
//! dictionary for each owner board, and the terms of a team on the
//! organization dictionary.
//!
//! - a new tenant has the template, and the migration
//!   `dictionary_template` gives it to a tenant that has none, one time,
//!   with the text of the system default,
//! - a second dictionary on a board is refused and the first is named: at
//!   create, at a change of the owner board, and at a restore,
//! - the team entry wins over the organization entry for the same word,
//! - a defined term passes the vocabulary check, and a word that is in no
//!   list fails it,
//! - a bad row is given with its document and its line.
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
use kairos_db::dictionaries::{self, DictionaryError};
use kairos_db::items::{ItemError, restore_item, set_document_board};
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    CreateDocument, create_board, create_document, provision_tenant, run_public_migrations,
    soft_delete_item,
};
use kairos_plain::Layer;

static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_dictionaries_t0733");

const UP: &str = include_str!("../migrations/tenant/2026-10-30-000100_dictionary_template/up.sql");

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
    platform: Uuid,
    platform_board: Uuid,
    data_board: Uuid,
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
    let data = team("Data", "data");
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
    let data_board = board(data, "data");

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
            platform,
            platform_board,
            data_board,
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
fn dictionaries_of_a_team_and_of_the_organization() {
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

    // --- no dictionary: a team has no terms -------------------------------
    let empty = dictionaries::team_dictionary(&mut f.conn, f.platform).expect("no dictionary");
    assert!(empty.terms.is_empty() && empty.problems.is_empty());
    assert_eq!((empty.organization, empty.team), (None, None));

    // --- the organization and the team -------------------------------------
    let strategy = f.strategy;
    let (_, org_code) = dictionary(
        &mut f,
        strategy,
        "| production | noun | Code that runs for the users. |\n\
         | board | noun | A set of columns. |\n",
    )
    .expect("the organization dictionary");
    let platform_board = f.platform_board;
    let (team_dict, team_code) = dictionary(
        &mut f,
        platform_board,
        "| Production | noun | The work that the team gives to its users. |\n\
         | production document | noun | A document that the team uses for its work. |\n\
         | lane | noun |\n",
    )
    .expect("the team dictionary");

    // --- a second dictionary on a board is refused and the first named ----
    let err = dictionary(&mut f, platform_board, "| card | noun | A task. |\n")
        .expect_err("a second dictionary on the board");
    match &err {
        ItemError::DictionaryExists { board, existing } => {
            assert_eq!(board, "platform");
            assert_eq!(existing, &team_code);
        }
        other => panic!("expected DictionaryExists, got {other}"),
    }
    assert!(err.to_string().contains(&team_code), "{err}");
    // A document with no template is not a dictionary: no refusal.
    create_document(
        &mut f.conn,
        CreateDocument {
            board_id: f.platform_board,
            title: "Notes",
            content: Some("free text"),
            template_id: None,
        },
        f.alice,
    )
    .expect("a plain document on the same board");

    // A change of the owner board to a board with a dictionary is refused.
    let data_board = f.data_board;
    let (data_dict, _) =
        dictionary(&mut f, data_board, "| row | noun | A line. |\n").expect("data dictionary");
    let err = set_document_board(&mut f.conn, data_dict, f.platform_board, f.alice)
        .expect_err("the board has a dictionary");
    assert!(
        matches!(&err, ItemError::DictionaryExists { existing, .. } if *existing == team_code),
        "{err}"
    );

    // An archived dictionary does not come back next to a live one.
    soft_delete_item(&mut f.conn, ItemType::Document, team_dict, f.alice).expect("archive");
    set_document_board(&mut f.conn, data_dict, f.platform_board, f.alice)
        .expect("the board has no live dictionary now");
    let blocked = restore_item(&mut f.conn, ItemType::Document, team_dict, f.alice)
        .expect("restore")
        .expect_err("a live dictionary is on the board");
    assert!(
        blocked.missing.iter().any(|m| m.contains("dictionary")),
        "{blocked:?}"
    );
    // Put the team dictionary back for the rest of the test.
    set_document_board(&mut f.conn, data_dict, f.data_board, f.alice).expect("move back");
    restore_item(&mut f.conn, ItemType::Document, team_dict, f.alice)
        .expect("restore")
        .expect("no live dictionary on the board now");

    // --- the team entry wins over the organization entry --------------------
    let resolved = dictionaries::team_dictionary(&mut f.conn, f.platform).expect("terms");
    assert_eq!(resolved.organization.as_deref(), Some(org_code.as_str()));
    assert_eq!(resolved.team.as_deref(), Some(team_code.as_str()));
    let terms: Vec<(&str, &str, Layer)> = resolved
        .terms
        .iter()
        .map(|t| (t.entry.word.as_str(), t.entry.meaning.as_str(), t.layer))
        .collect();
    assert_eq!(
        terms,
        [
            ("board", "A set of columns.", Layer::Organization),
            (
                "Production",
                "The work that the team gives to its users.",
                Layer::Team
            ),
            (
                "production document",
                "A document that the team uses for its work.",
                Layer::Team
            ),
        ]
    );
    // A bad row is given with its document and its line.
    assert_eq!(resolved.problems.len(), 1, "{:?}", resolved.problems);
    assert_eq!(resolved.problems[0].short_code, team_code);
    assert_eq!(resolved.problems[0].problem.line, 7);

    // --- the vocabulary hook: a defined term passes, an undefined word fails
    let vocabulary = resolved.vocabulary(["the", "is", "a"]);
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

    // Another team has the organization terms only.
    let data_team: Uuid = {
        use kairos_db::schema::teams;
        teams::table
            .filter(teams::slug.eq("data"))
            .select(teams::id)
            .first(&mut f.conn)
            .expect("data")
    };
    let data = dictionaries::team_dictionary(&mut f.conn, data_team).expect("data terms");
    let words: Vec<&str> = data.words().collect();
    assert_eq!(words, ["board", "production", "row"]);
    assert_eq!(
        data.terms
            .iter()
            .find(|t| t.entry.word == "production")
            .map(|t| t.layer),
        Some(Layer::Organization)
    );

    // An unknown team is an error.
    assert!(matches!(
        dictionaries::team_dictionary(&mut f.conn, Uuid::new_v4()),
        Err(DictionaryError::TeamNotFound(_))
    ));
}
