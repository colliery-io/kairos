//! The standing regression guard for KAIROS-A-0020's third rule: **default
//! listings hide archived work.** Rules 1 and 2 (archived work stays
//! retrievable and searchable when asked for) are proved elsewhere; this
//! file proves the half that nothing else would notice breaking.
//!
//! Why it exists (KAIROS-T-0156). Until that task, two database views —
//! `entity_directory` and `searchable_items` — filtered `deleted_at IS
//! NULL` inside their own bodies, and everything downstream inherited
//! "archived means absent" without asking for it. That is also why
//! `--include-deleted` was a silent no-op next to a text query: the view
//! had already dropped the rows the flag was meant to widen to. T-0156
//! moved the predicate out of the views and into every call site, which
//! buys the opt-in — at the price of a failure mode with no symptom. A
//! consumer that forgets its `deleted_at IS NULL` does not error; a board
//! just quietly grows rows that should not be on it.
//!
//! So this test archives one item of every shape and then walks EVERY
//! default read surface asserting it is absent:
//!
//! - the board (`GET /api/boards/{id}/items`), for the four board-bound
//!   families;
//! - all five family list endpoints, including `total` — a list that
//!   hides the row but counts it is still a leak, just a subtler one;
//! - `POST /api/search` with no `include_deleted`, over all three
//!   capabilities (`q`, `filter`, `traverse`).
//!
//! **One surface is deliberately the other way round** (KAIROS-T-0158):
//! an item's relationship list NAMES its archived neighbours, marked.
//! That is not a listing of archived work, it is the record of a LIVE
//! item, and dropping an endpoint from it shrinks the answer to "what did
//! this contain?" without saying so. Section 4 below pins that contract
//! from this side, so anyone who re-tightens the join to make this file
//! green again finds out here what they are undoing.
//!
//! And, to keep the assertion honest about WHICH property is being
//! tested, it checks the same rows ARE reachable by short code
//! (KAIROS-A-0020 rule 1). A test that only proved absence would pass just
//! as well against a hard delete, which is precisely the behaviour this
//! initiative is removing.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres + real Dex. Owns the uniquely named scratch database
//! `kairos_archived_hidden_t0156_test`; the shared `kairos` database is
//! never touched (shared-services discipline).

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, Pagination,
};
use kairos_client::types_search::{
    SearchFilter, SearchRequest, SearchTraverse, SearchTraverseFrom,
};
use kairos_client::{EntityKind, KairosClient};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, RelationshipType};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_archived_hidden_t0156_test";

/// The word every seeded item carries, so one `q` reaches all five
/// families at once.
const MARKER: &str = "zarquon";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn board_id(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("DTO id is a UUID")
}

/// Every short code on a board, across all four board-bound families and
/// every column.
async fn board_short_codes(client: &KairosClient, board: Uuid) -> Vec<String> {
    let items = client
        .board_items(&board.to_string())
        .await
        .expect("board items");
    let mut codes: Vec<String> = Vec::new();
    for column in &items.columns {
        codes.extend(column.strategies.iter().map(|i| i.short_code.clone()));
        codes.extend(column.initiatives.iter().map(|i| i.short_code.clone()));
        codes.extend(column.tasks.iter().map(|i| i.short_code.clone()));
        codes.extend(column.adrs.iter().map(|i| i.short_code.clone()));
    }
    codes.sort();
    codes
}

/// Every short code a search request returns, across the five groups.
async fn search_short_codes(client: &KairosClient, request: SearchRequest) -> Vec<String> {
    let body = client.search(&request).await.expect("search");
    let mut codes: Vec<String> = Vec::new();
    codes.extend(body.results.strategies.iter().map(|i| i.short_code.clone()));
    codes.extend(
        body.results
            .initiatives
            .iter()
            .map(|i| i.short_code.clone()),
    );
    codes.extend(body.results.tasks.iter().map(|i| i.short_code.clone()));
    codes.extend(body.results.documents.iter().map(|i| i.short_code.clone()));
    codes.extend(body.results.adrs.iter().map(|i| i.short_code.clone()));
    codes.sort();
    codes
}

#[tokio::test]
async fn archived_work_is_absent_from_every_default_listing() {
    // --- scratch database + tenant ----------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // --- live server + one org-admin client (svc bypasses ABAC) ------------
    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth,
    ));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");

    let _ = svc.list_tasks(Pagination::default()).await;
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting svc admin membership");

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning seeding connection to the tenant schema");
    kairos_db::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Delivery",
        "delivery",
        None,
        None,
    )
    .expect("creating the delivery board");

    let strategy_board = board_id(&mut conn, BoardLevel::Strategy);
    let initiative_board = board_id(&mut conn, BoardLevel::Initiative);
    let delivery_board = board_id(&mut conn, BoardLevel::Delivery);
    let adr_board = board_id(&mut conn, BoardLevel::Adr);

    // --- seed a KEEP and a DOOMED item in every family --------------------
    // Both carry MARKER so one `q` reaches all ten, and the pairs sit on the
    // same boards so "absent" can never be confused with "was never there".
    let mut keep = Vec::new();
    let mut doomed = Vec::new();

    for (label, bucket) in [("keep", &mut keep), ("doomed", &mut doomed)] {
        let strategy = svc
            .create_strategy(&CreateStrategyRequest {
                board_id: strategy_board.to_string(),
                column_id: None,
                title: format!("{MARKER} strategy {label}"),
                content: format!("# {MARKER}"),
                hypothesis: None,
            })
            .await
            .expect("creating strategy");
        let initiative = svc
            .create_initiative(&CreateInitiativeRequest {
                board_id: initiative_board.to_string(),
                column_id: None,
                title: format!("{MARKER} initiative {label}"),
                content: format!("# {MARKER}"),
                complexity: None,
                bucket_type: None,
            })
            .await
            .expect("creating initiative");
        let task = svc
            .create_task(&CreateTaskRequest {
                board_id: Some(delivery_board.to_string()),
                repository: None,
                column_id: None,
                title: format!("{MARKER} task {label}"),
                content: format!("# {MARKER}"),
                task_type: None,
                work_class: None,
                team_id: None,
            })
            .await
            .expect("creating task");
        let adr = svc
            .create_adr(&CreateAdrRequest {
                board_id: Some(adr_board.to_string()),
                column_id: None,
                title: format!("{MARKER} adr {label}"),
                content: format!("# {MARKER}"),
                decision_maker: None,
                decision_date: None,
            })
            .await
            .expect("creating adr");
        // A document hangs off its own family's parent, never the other
        // pair's — otherwise archiving `doomed` would cascade into `keep`.
        let document = svc
            .create_document(&CreateDocumentRequest {
                title: format!("{MARKER} document {label}"),
                content: Some(format!("# {MARKER}")),
                template_id: None,
                parent_short_code: Some(initiative.short_code.clone()),
            })
            .await
            .expect("creating document");

        bucket.push((EntityKind::Strategy, strategy.short_code, strategy.id));
        bucket.push((EntityKind::Initiative, initiative.short_code, initiative.id));
        bucket.push((EntityKind::Task, task.short_code, task.id));
        bucket.push((EntityKind::Document, document.short_code, document.id));
        bucket.push((EntityKind::Adr, adr.short_code, adr.id));
    }

    // A `parent` edge from each KEEP strategy to its own initiative gives
    // the traverse capability something to walk, and the relationships
    // endpoint a neighbour to hydrate through `entity_directory`.
    let keep_strategy = keep[0].1.clone();
    let keep_initiative = keep[1].1.clone();
    graph::link_items(
        &mut conn,
        uuid(&keep[0].2),
        uuid(&keep[1].2),
        RelationshipType::Parent,
        svc_id,
    )
    .expect("linking keep strategy -> keep initiative");
    // ...and one from the KEEP strategy to the DOOMED initiative, so the
    // archived row is a live item's neighbour: a LIVE strategy with one
    // live and one archived child. Section 4 reads both ends of that —
    // the list names both children, the rollup counts one.
    graph::link_items(
        &mut conn,
        uuid(&keep[0].2),
        uuid(&doomed[1].2),
        RelationshipType::Parent,
        svc_id,
    )
    .expect("linking keep strategy -> doomed initiative");

    // --- everything is visible BEFORE the archive -------------------------
    // Establishes the baseline: each absence asserted below is a change
    // this archive caused, not a fixture that never landed.
    let before = search_short_codes(
        &svc,
        SearchRequest {
            q: Some(MARKER.to_string()),
            ..Default::default()
        },
    )
    .await;
    for (_, code, _) in keep.iter().chain(doomed.iter()) {
        assert!(
            before.contains(code),
            "{code} must be searchable before it is archived"
        );
    }

    // --- archive the DOOMED five (no cascade: no edges among them) --------
    for (kind, code, _) in &doomed {
        match kind {
            EntityKind::Strategy => {
                svc.delete_strategy(code).await.expect("archiving strategy");
            }
            EntityKind::Initiative => {
                svc.delete_initiative(code)
                    .await
                    .expect("archiving initiative");
            }
            EntityKind::Task => {
                svc.delete_task(code).await.expect("archiving task");
            }
            EntityKind::Document => {
                svc.delete_document(code).await.expect("archiving document");
            }
            EntityKind::Adr => {
                svc.delete_adr(code).await.expect("archiving adr");
            }
        }
    }
    let archived: Vec<String> = doomed.iter().map(|(_, code, _)| code.clone()).collect();
    let live: Vec<String> = keep.iter().map(|(_, code, _)| code.clone()).collect();

    // === 1. boards ========================================================
    for board in [strategy_board, initiative_board, delivery_board, adr_board] {
        let codes = board_short_codes(&svc, board).await;
        for code in &archived {
            assert!(
                !codes.contains(code),
                "archived {code} is still on board {board}: {codes:?}"
            );
        }
    }
    // The KEEP items are all still on their boards — so the assertions above
    // are about archiving, not about an empty board.
    let mut on_boards = Vec::new();
    for board in [strategy_board, initiative_board, delivery_board, adr_board] {
        on_boards.extend(board_short_codes(&svc, board).await);
    }
    for (kind, code, _) in &keep {
        if *kind != EntityKind::Document {
            assert!(
                on_boards.contains(code),
                "live {code} vanished from its board: {on_boards:?}"
            );
        }
    }

    // === 2. the five family lists ========================================
    // `total` is asserted alongside the page: a list that hides the row but
    // still counts it leaks the same fact, one number at a time.
    let page = Pagination {
        limit: Some(200),
        offset: Some(0),
    };
    let listed: Vec<(String, i64)> = {
        let mut rows = Vec::new();
        let s = svc.list_strategies(page).await.expect("list strategies");
        rows.extend(s.items.iter().map(|i| (i.short_code.clone(), s.total)));
        let i = svc.list_initiatives(page).await.expect("list initiatives");
        rows.extend(i.items.iter().map(|x| (x.short_code.clone(), i.total)));
        let t = svc.list_tasks(page).await.expect("list tasks");
        rows.extend(t.items.iter().map(|x| (x.short_code.clone(), t.total)));
        let d = svc.list_documents(page).await.expect("list documents");
        rows.extend(d.items.iter().map(|x| (x.short_code.clone(), d.total)));
        let a = svc.list_adrs(page).await.expect("list adrs");
        rows.extend(a.items.iter().map(|x| (x.short_code.clone(), a.total)));
        rows
    };
    let listed_codes: Vec<String> = listed.iter().map(|(code, _)| code.clone()).collect();
    for code in &archived {
        assert!(
            !listed_codes.contains(code),
            "archived {code} is still in its family list: {listed_codes:?}"
        );
    }
    for code in &live {
        assert!(
            listed_codes.contains(code),
            "live {code} fell out of its family list: {listed_codes:?}"
        );
    }
    for family_total in [
        svc.list_strategies(page).await.expect("strategies").total,
        svc.list_initiatives(page).await.expect("initiatives").total,
        svc.list_tasks(page).await.expect("tasks").total,
        svc.list_documents(page).await.expect("documents").total,
        svc.list_adrs(page).await.expect("adrs").total,
    ] {
        assert_eq!(
            family_total, 1,
            "each family list counts its ONE live row, never the archived one"
        );
    }

    // === 3. search without the flag ======================================
    // All three capabilities, since each reaches the directory by a
    // different route: `q` through `searchable_items`, `filter` through
    // per-family hydration, `traverse` through the recursive walk plus
    // type resolution.
    let q_only = search_short_codes(
        &svc,
        SearchRequest {
            q: Some(MARKER.to_string()),
            ..Default::default()
        },
    )
    .await;
    let filter_only = search_short_codes(
        &svc,
        SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec![
                    "strategy".into(),
                    "initiative".into(),
                    "task".into(),
                    "document".into(),
                    "adr".into(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await;
    let traverse_only = search_short_codes(
        &svc,
        SearchRequest {
            traverse: Some(SearchTraverse {
                from: SearchTraverseFrom {
                    short_code: Some(keep_strategy.clone()),
                    id: None,
                },
                relationships: vec!["parent".into()],
                direction: "outbound".into(),
                depth: Some(3),
            }),
            ..Default::default()
        },
    )
    .await;
    for (surface, codes) in [
        ("q", &q_only),
        ("filter", &filter_only),
        ("traverse", &traverse_only),
    ] {
        for code in &archived {
            assert!(
                !codes.contains(code),
                "archived {code} surfaced in search by {surface}: {codes:?}"
            );
        }
    }
    assert!(
        q_only.contains(&keep_strategy),
        "the live strategy must still match `q`: {q_only:?}"
    );
    assert!(
        traverse_only.contains(&keep_initiative),
        "traverse must still reach the live child: {traverse_only:?}"
    );

    // === 4. relationships: the ONE surface that goes the other way =======
    // Until KAIROS-T-0158 this leg asserted the opposite — that the
    // archived child did not hydrate through `entity_directory` — and it
    // was right to, because nothing else made the inherited filter
    // observable. T-0158 changed the contract, not the coverage: the KEEP
    // strategy has two children, one archived, and BOTH must come back,
    // with `archived_at` telling them apart.
    //
    // Rule 3 is not weakened by this. A relationship list is not a
    // listing of archived work; it is the record of a live item, and the
    // archived row appears there only because that live item points at
    // it. Sections 1-3 above still own rule 3, and none of them moved.
    let rels = svc
        .relationships(EntityKind::Strategy, &keep_strategy)
        .await
        .expect("relationships of the live strategy");
    let neighbours: Vec<(String, bool)> = rels
        .outgoing
        .iter()
        .flat_map(|group| group.items.iter())
        .map(|n| (n.short_code.clone(), n.archived_at.is_some()))
        .collect();
    let archived_initiative = doomed[1].1.clone();
    assert_eq!(
        neighbours,
        vec![
            (keep_initiative.clone(), false),
            (archived_initiative.clone(), true),
        ],
        "a live item lists BOTH children — the archived one MARKED, never \
         silently dropped (KAIROS-T-0158)"
    );
    // The marker is the whole contract: an unmarked archived neighbour
    // would be worse than a missing one, because a reader would act on it.
    let marker = rels
        .outgoing
        .iter()
        .flat_map(|group| group.items.iter())
        .find(|n| n.short_code == archived_initiative)
        .and_then(|n| n.archived_at.clone())
        .expect("the archived neighbour carries archived_at");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&marker).is_ok(),
        "archived_at is an RFC 3339 instant, like every other archived marker: {marker}"
    );
    // ...and the rollup still refuses to count it (ADR-20 rule 5): two
    // children listed, one child's worth of progress.
    let progress = svc
        .children_progress(EntityKind::Strategy, &keep_strategy)
        .await
        .expect("children progress of the live strategy");
    assert_eq!(
        progress.total, 1,
        "progress counts LIVE children only, though the list names both: {progress:?}"
    );

    // === 5. …and none of that is erasure =================================
    // KAIROS-A-0020 rule 1. Without this, every assertion above would pass
    // just as happily against a hard delete — which is the behaviour this
    // initiative exists to remove, not to enshrine.
    for (kind, code, _) in &doomed {
        let archived_at = match kind {
            EntityKind::Strategy => svc.get_strategy(code).await.expect("get").archived_at,
            EntityKind::Initiative => svc.get_initiative(code).await.expect("get").archived_at,
            EntityKind::Task => svc.get_task(code).await.expect("get").archived_at,
            EntityKind::Document => svc.get_document(code).await.expect("get").archived_at,
            EntityKind::Adr => svc.get_adr(code).await.expect("get").archived_at,
        };
        assert!(
            archived_at.is_some(),
            "{code} is hidden from the listings AND still readable, marked archived"
        );
    }

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
