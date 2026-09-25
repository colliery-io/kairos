//! Edge proposals against a real database (KAIROS-T-0192).
//!
//! The pure helpers are unit-tested in `kairos_db::proposals`. What needs a
//! database is everything that matters: that confirming really creates the edge
//! through the **existing** graph path (and so inherits its cycle check), that a
//! rejection is kept rather than erased, that an agent cannot bury the signal
//! under its own output, and that a decided proposal cannot be decided twice.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::graph::GraphError;
use kairos_db::models::{BoardLevel, NewUser, RelationshipType, TaskType, WorkClass};
use kairos_db::proposals::{
    MAX_PENDING_PER_ITEM, NewProposal, ProposalError, confirm, pending_for_item, propose, reject,
    stats,
};
use kairos_db::{create_board, items, provision_tenant, run_public_migrations, schema};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_proposals_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database path segment");
    format!("{base}/{db}")
}

#[test]
fn edge_proposal_lifecycle() {
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
            user_name: "dex|alice".into(),
            email: "alice@acme.test".into(),
            display_name: "Alice".into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("alice");
    let agent = diesel::insert_into(schema::users::table)
        .values(kairos_db::models::public::NewServiceAccountUser {
            external_id: "svc|agent".into(),
            user_name: "svc|agent".into(),
            email: "agent@acme.test".into(),
            display_name: "Agent".into(),
            kind: kairos_db::models::public::USER_KIND_SERVICE_ACCOUNT.into(),
        })
        .returning(schema::users::id)
        .get_result::<Uuid>(&mut conn)
        .expect("agent");

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
    let task = |conn: &mut PgConnection, title: &str| {
        items::create_task(
            conn,
            items::CreateTask {
                board_id: board,
                column_id: None,
                title,
                content: "…",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                team_id: None,
                repository_id: None,
            },
            alice,
        )
        .expect("task")
    };
    let a = task(&mut conn, "A");
    let b = task(&mut conn, "B");
    let c = task(&mut conn, "C");

    let evidence = |score: f32| NewProposal {
        source_id: a.id,
        target_id: b.id,
        relationship: RelationshipType::Blocks,
        claim: "possible dependency",
        why: "Reads as being about the same thing, and nothing in the graph joins them.",
        score,
    };

    // ---- propose -----------------------------------------------------------
    let p = propose(&mut conn, evidence(0.03), agent).expect("proposing");
    assert_eq!(p.state, "pending");
    assert_eq!(p.proposed_by, agent);
    assert!(p.decided_by.is_none() && p.decided_at.is_none());
    assert!(
        p.why.contains("nothing in the graph joins them"),
        "the evidence is kept verbatim, so a reviewer sees what the agent saw"
    );

    // Visible from BOTH ends: a proposal concerns two items, and a human looking
    // at either should see it.
    assert_eq!(pending_for_item(&mut conn, a.id).unwrap().len(), 1);
    assert_eq!(pending_for_item(&mut conn, b.id).unwrap().len(), 1);
    assert!(pending_for_item(&mut conn, c.id).unwrap().is_empty());

    // ---- an agent cannot bury the signal under its own output --------------
    let again = propose(&mut conn, evidence(0.9), agent);
    assert!(
        matches!(again, Err(ProposalError::AlreadyPending)),
        "re-proposing the same pair is work already done, not a second proposal"
    );

    // Only parent and blocks are proposable.
    let editorial = propose(
        &mut conn,
        NewProposal {
            relationship: RelationshipType::Supports,
            ..evidence(0.1)
        },
        agent,
    );
    assert!(matches!(editorial, Err(ProposalError::NotProposable(_))));

    // ---- an agent proposes but does not decide (rule 6) --------------------
    // Enforced in the service, not in the surfaces: a rule implemented in two
    // handlers is a rule the third handler will not have.
    assert!(
        matches!(
            confirm(&mut conn, p.id, agent),
            Err(ProposalError::NotHuman)
        ),
        "a service account confirming its own proposal is the whole thing rule 6 \
         exists to prevent"
    );
    assert!(matches!(
        reject(&mut conn, p.id, agent),
        Err(ProposalError::NotHuman)
    ));

    // ---- reject is recorded, not erased ------------------------------------
    let rejected = reject(&mut conn, p.id, alice).expect("rejecting");
    assert_eq!(rejected.state, "rejected");
    assert_eq!(rejected.decided_by, Some(alice));
    assert!(rejected.decided_at.is_some());
    assert!(
        pending_for_item(&mut conn, a.id).unwrap().is_empty(),
        "no longer pending"
    );
    let s = stats(&mut conn).expect("stats");
    assert_eq!((s.pending, s.confirmed, s.rejected), (0, 0, 1));
    assert_eq!(
        s.confirm_rate(),
        Some(0.0),
        "a feature nobody agrees with should read as zero, not as absent"
    );

    // A rejected pair CAN be proposed again — a rejection judges the evidence at
    // the time, and better evidence deserves another hearing.
    let second = propose(&mut conn, evidence(0.9), agent).expect("re-proposing after rejection");

    // ---- a decision is not revisited ---------------------------------------
    assert!(matches!(
        reject(&mut conn, p.id, alice),
        Err(ProposalError::AlreadyDecided { .. })
    ));
    assert!(matches!(
        confirm(&mut conn, p.id, alice),
        Err(ProposalError::AlreadyDecided { .. })
    ));

    // ---- confirm creates the real edge -------------------------------------
    let confirmed = confirm(&mut conn, second.id, alice).expect("confirming");
    assert_eq!(confirmed.state, "confirmed");
    assert_eq!(confirmed.decided_by, Some(alice));
    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let edge = sql_query(
        "SELECT count(*)::bigint AS count FROM item_relationships \
         WHERE source_id = $1 AND target_id = $2 AND relationship = 'blocks'",
    )
    .bind::<diesel::sql_types::Uuid, _>(a.id)
    .bind::<diesel::sql_types::Uuid, _>(b.id)
    .get_result::<Count>(&mut conn)
    .expect("looking for the edge")
    .count;
    assert_eq!(edge, 1, "the edge really exists now");
    let s = stats(&mut conn).expect("stats");
    assert_eq!((s.pending, s.confirmed, s.rejected), (0, 1, 1));
    assert_eq!(s.confirm_rate(), Some(0.5));

    // ---- the graph's OWN checks are inherited, not re-implemented ----------
    // Two of them, both refused at confirmation time rather than at proposal
    // time: an agent proposing something is a suggestion, and the graph is the
    // thing that gets to say no.

    // 1. A cycle. a blocks b already (confirmed above), so b blocks a closes it.
    let loop_proposal = propose(
        &mut conn,
        NewProposal {
            source_id: b.id,
            target_id: a.id,
            relationship: RelationshipType::Blocks,
            claim: "possible dependency",
            why: "…",
            score: 0.5,
        },
        agent,
    )
    .expect("proposing is allowed — the graph is consulted on confirmation");
    let refused = confirm(&mut conn, loop_proposal.id, alice)
        .expect_err("confirming a cycle must be refused");
    assert!(
        matches!(
            refused,
            ProposalError::Graph(GraphError::CycleDetected { .. })
        ),
        "by the graph's own cycle check, not a second copy: {refused}"
    );
    // The refusal leaves it undecided: a refused confirmation is not a decision,
    // and the proposal is still there for a human to reject properly.
    let still = pending_for_item(&mut conn, b.id).expect("pending");
    assert!(
        still
            .iter()
            .any(|p| p.id == loop_proposal.id && p.state == "pending"),
        "still waiting: {still:?}"
    );
    reject(&mut conn, loop_proposal.id, alice).expect("rejected on its merits instead");

    // 2. The rule matrix. `parent` does not run task -> task, and an agent that
    // proposes one gets the same refusal a human would.
    let wrong_shape = propose(
        &mut conn,
        NewProposal {
            source_id: c.id,
            target_id: a.id,
            relationship: RelationshipType::Parent,
            claim: "possible duplicate",
            why: "…",
            score: 0.5,
        },
        agent,
    )
    .expect("proposing an ill-shaped edge is allowed");
    let refused =
        confirm(&mut conn, wrong_shape.id, alice).expect_err("task -> task parent must be refused");
    assert!(
        matches!(refused, ProposalError::Graph(GraphError::Rule(_))),
        "by the existing rule matrix: {refused}"
    );
    reject(&mut conn, wrong_shape.id, alice).expect("cleaning up");

    // ---- the cap ------------------------------------------------------------
    let hub = task(&mut conn, "Hub");
    for i in 0..MAX_PENDING_PER_ITEM {
        let other = task(&mut conn, &format!("Spoke {i}"));
        propose(
            &mut conn,
            NewProposal {
                source_id: hub.id,
                target_id: other.id,
                relationship: RelationshipType::Blocks,
                claim: "possible dependency",
                why: "…",
                score: 0.1,
            },
            agent,
        )
        .unwrap_or_else(|e| panic!("proposal {i} should fit under the cap: {e}"));
    }
    let overflow = task(&mut conn, "One too many");
    let capped = propose(
        &mut conn,
        NewProposal {
            source_id: hub.id,
            target_id: overflow.id,
            relationship: RelationshipType::Blocks,
            claim: "possible dependency",
            why: "…",
            score: 0.1,
        },
        agent,
    );
    assert!(
        matches!(capped, Err(ProposalError::TooManyPending { limit, .. }) if limit == MAX_PENDING_PER_ITEM),
        "an agent loop must not bury the signal under its own output: {capped:?}"
    );
}
