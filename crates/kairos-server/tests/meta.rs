//! Integration test for the KAIROS-T-0020 endpoint families —
//! relationships, item metadata, metadata definitions, templates, content
//! history, and the activity log — through the typed
//! `kairos_client::KairosClient` against the booted production router on a
//! real port (contracts per KAIROS-S-0005 / A-0003 / A-0004 / A-0006).
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer (tokens via the password grant, see
//! `tests/common/mod.rs`). For isolation the test owns the uniquely named
//! scratch database `kairos_meta_t0020_test`; the shared `kairos` database
//! is never touched (shared-services discipline).
//!
//! Cast (per KAIROS-A-0006):
//! - `svc`   — org ADMIN: the only role allowed to write relationships,
//!   metadata definitions, and templates,
//! - `alice` — org member with `manage_initiatives`/`manage_documents` on
//!   the initiative board and `manage_tasks` on the delivery board,
//! - `bob`   — org member with NO grants (the 403 matrix; reads only).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use reqwest::Method;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    CreateDocumentRequest, CreateInitiativeRequest, CreateTaskRequest, Pagination,
    UpdateContentRequest,
};
use kairos_client::types_meta::{
    ActivityQuery, CreateMetadataDefinitionRequest, CreateRelationshipRequest,
    CreateTemplateRequest, ItemMetadataResponse, MetadataValue, RelationshipGroup,
    TemplateMetadataEntry, UpdateMetadataDefinitionRequest, UpdateMetadataRequest,
    UpdateTemplateRequest,
};
use kairos_client::{EntityKind, Error};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, abac, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_meta_t0020_test";

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The tenant board of a level.
fn board_id(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

/// Unwrap an expected API rejection (panics on success).
fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

/// The group of `relationship` in one direction of a relationships
/// response, if present.
fn rel_group<'a>(
    groups: &'a [RelationshipGroup],
    relationship: &str,
) -> Option<&'a RelationshipGroup> {
    groups.iter().find(|g| g.relationship == relationship)
}

/// The metadata value entry for `slug` in an item-metadata response, if
/// present.
fn metadata_value<'a>(body: &'a ItemMetadataResponse, slug: &str) -> Option<&'a MetadataValue> {
    body.values.iter().find(|value| value.slug == slug)
}

/// A one-entry metadata PATCH body (`None` clears the slug).
fn metadata_patch(slug: &str, value: Option<&str>) -> UpdateMetadataRequest {
    UpdateMetadataRequest {
        values: BTreeMap::from([(slug.to_string(), value.map(str::to_string))]),
    }
}

#[tokio::test]
async fn meta_endpoints_against_live_stack() {
    // --- scratch database + tenant ------------------------------------------
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

    // --- live server + typed clients ------------------------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
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
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // --- JIT-provision the three users, then grant membership ----------------
    for client in [&alice, &bob, &svc] {
        let err = rejection(client.activity(&ActivityQuery::default()).await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
        assert_eq!(err.code(), Some("MEMBERSHIP_REQUIRED"), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user_id, role) in [
        (alice_id, OrgRole::Member),
        (bob_id, OrgRole::Member),
        (svc_id, OrgRole::Admin),
    ] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }

    // --- tenant-schema seeding (delivery board, alice's grants) --------------
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
    let initiative_board = board_id(&mut conn, BoardLevel::Initiative);
    let delivery_board = board_id(&mut conn, BoardLevel::Delivery);
    for (board, capability) in [
        (initiative_board, "manage_initiatives"),
        (initiative_board, "manage_documents"),
        (delivery_board, "manage_tasks"),
    ] {
        abac::grant_capability(&mut conn, board, alice_id, capability, svc_id)
            .expect("granting alice capability");
    }

    // --- fixture items via the API -------------------------------------------
    let initiative = alice
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.to_string(),
            column_id: None,
            title: "Ship M2".into(),
            content: "scope".into(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("creating initiative");
    let initiative_code = initiative.short_code.clone();

    let mut task_codes = Vec::new();
    for title in ["Wire endpoints", "Write tests"] {
        let task = alice
            .create_task(&CreateTaskRequest {
                board_id: Some(delivery_board.to_string()),
                repository_id: None,
                column_id: None,
                title: title.into(),
                content: "c1".into(),
                task_type: None,
                work_class: None,
                team_id: None,
            })
            .await
            .expect("creating task");
        task_codes.push((task.short_code.clone(), task.id.clone()));
    }
    let (t1_code, t1_id) = task_codes[0].clone();
    let (t2_code, _) = task_codes[1].clone();

    let document = alice
        .create_document(&CreateDocumentRequest {
            title: "Spec".into(),
            content: Some("spec".into()),
            template_id: None,
            parent_short_code: Some(initiative_code.clone()),
        })
        .await
        .expect("creating document");
    let doc_code = document.short_code.clone();

    // ==========================================================================
    // Relationships: org-admin writes, typed 422s, grouped both-direction GET
    // ==========================================================================
    // Non-admin cannot write a NON-collaborative edge (A-0006: relationships
    // are tenant-wide config; KAIROS-T-0111 carves out parent/blocks for
    // members who manage either end or authored the source).
    let err = rejection(
        alice
            .create_relationship(&CreateRelationshipRequest {
                source_short_code: initiative_code.clone(),
                target_short_code: t1_code.clone(),
                relationship: "informs".into(),
            })
            .await,
    );
    match &err {
        Error::Forbidden { code, details, .. } => {
            assert_eq!(code, "FORBIDDEN");
            assert_eq!(details["required_role"], "admin");
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // A member who manages both boards links initiative -> task (parent) —
    // the collaborative edge (KAIROS-T-0111).
    let parent_request = CreateRelationshipRequest {
        source_short_code: initiative_code.clone(),
        target_short_code: t1_code.clone(),
        relationship: "parent".into(),
    };
    let edge = alice
        .create_relationship(&parent_request)
        .await
        .expect("linking initiative -> task");
    assert_eq!(edge.relationship, "parent");

    // Duplicate edge -> 422 ALREADY_LINKED.
    let err = rejection(svc.create_relationship(&parent_request).await);
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "ALREADY_LINKED");
        }
        other => panic!("expected 422 ALREADY_LINKED, got {other}"),
    }

    // Type-rule matrix violation (task cannot parent an initiative).
    let err = rejection(
        svc.create_relationship(&CreateRelationshipRequest {
            source_short_code: t1_code.clone(),
            target_short_code: initiative_code.clone(),
            relationship: "parent".into(),
        })
        .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "RELATIONSHIP_RULE");
        }
        other => panic!("expected 422 RELATIONSHIP_RULE, got {other}"),
    }

    // Cycle: t1 blocks t2, then t2 blocks t1 -> 422 CYCLE_DETECTED.
    let blocks_edge = svc
        .create_relationship(&CreateRelationshipRequest {
            source_short_code: t1_code.clone(),
            target_short_code: t2_code.clone(),
            relationship: "blocks".into(),
        })
        .await
        .expect("linking t1 blocks t2");
    let blocks_edge_id = blocks_edge.id.clone();
    let err = rejection(
        svc.create_relationship(&CreateRelationshipRequest {
            source_short_code: t2_code.clone(),
            target_short_code: t1_code.clone(),
            relationship: "blocks".into(),
        })
        .await,
    );
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "CYCLE_DETECTED");
        }
        other => panic!("expected 422 CYCLE_DETECTED, got {other}"),
    }

    // Unknown endpoint / unknown relationship value -> 422 VALIDATION.
    let err = rejection(
        svc.create_relationship(&CreateRelationshipRequest {
            source_short_code: "ACME-T-9999".into(),
            target_short_code: t1_code.clone(),
            relationship: "blocks".into(),
        })
        .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );
    let err = rejection(
        svc.create_relationship(&CreateRelationshipRequest {
            source_short_code: t1_code.clone(),
            target_short_code: t2_code.clone(),
            relationship: "buddies".into(),
        })
        .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // GET is open tenant-wide (bob), grouped by type, both directions.
    let body = bob
        .relationships(EntityKind::Task, &t1_code)
        .await
        .expect("t1 relationships");
    assert_eq!(body.short_code, t1_code);
    let incoming_parent = rel_group(&body.incoming, "parent").expect("incoming parent group");
    assert_eq!(incoming_parent.items[0].short_code, initiative_code);
    assert_eq!(incoming_parent.items[0].entity_type, "initiative");
    assert_eq!(incoming_parent.items[0].title, "Ship M2");
    let outgoing_blocks = rel_group(&body.outgoing, "blocks").expect("outgoing blocks group");
    assert_eq!(outgoing_blocks.items[0].short_code, t2_code);
    assert_eq!(
        outgoing_blocks.items[0].relationship_id, blocks_edge_id,
        "edge ids are attached for DELETE"
    );

    // The initiative sees the same edges from the other side, plus the
    // supports edge the document create wrote.
    let body = bob
        .relationships(EntityKind::Initiative, &initiative_code)
        .await
        .expect("initiative relationships");
    let outgoing_parent = rel_group(&body.outgoing, "parent").expect("outgoing parent group");
    assert_eq!(outgoing_parent.items[0].short_code, t1_code);
    let outgoing_supports = rel_group(&body.outgoing, "supports").expect("outgoing supports group");
    assert_eq!(outgoing_supports.items[0].short_code, doc_code);
    assert_eq!(outgoing_supports.items[0].entity_type, "document");

    // ==========================================================================
    // Children progress (KAIROS-T-0080): rollup over parent-edge children
    // ==========================================================================
    // The initiative parents exactly t1; the supports edge to the document
    // never counts. t1 sits in the entry column (not done), and the seeded
    // delivery board carries a done-flagged Completed column.
    let progress = bob
        .children_progress(EntityKind::Initiative, &initiative_code)
        .await
        .expect("initiative children progress");
    assert_eq!(progress.short_code, initiative_code);
    assert_eq!((progress.done, progress.total), (0, 1));
    assert!(
        progress.has_done_columns,
        "seeded Completed is done-flagged"
    );
    assert_eq!(progress.by_column.len(), 1);
    assert!(!progress.by_column[0].is_done);
    assert_eq!(progress.by_column[0].count, 1);

    // Walk t1 to Completed (Backlog -> Todo -> Active -> Completed per the
    // seeded transitions); the rollup follows.
    let board = bob
        .get_board(&delivery_board.to_string())
        .await
        .expect("delivery board detail");
    let column_id_of = |name: &str| {
        board
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name} exists"))
            .id
            .clone()
    };
    // svc is the org admin — transition_items is not among alice's grants
    // in this fixture.
    for column in ["Todo", "Active", "Completed"] {
        svc.transition_task(&t1_code, &column_id_of(column))
            .await
            .unwrap_or_else(|e| panic!("moving t1 to {column}: {e}"));
    }
    let progress = bob
        .children_progress(EntityKind::Initiative, &initiative_code)
        .await
        .expect("progress after completion");
    assert_eq!((progress.done, progress.total), (1, 1));
    assert!(progress.by_column[0].is_done, "the one bucket is Completed");

    // A leaf reports an empty rollup, not an error.
    let progress = bob
        .children_progress(EntityKind::Task, &t1_code)
        .await
        .expect("leaf children progress");
    assert_eq!((progress.done, progress.total), (0, 0));
    assert!(progress.by_column.is_empty());

    // Family/short-code mismatch -> 404, exactly like relationships.
    let err = rejection(
        bob.children_progress(EntityKind::Task, &initiative_code)
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // ==========================================================================
    // Focal subgraph endpoint (KAIROS-T-0088): nodes + edges + depth
    // ==========================================================================
    // From t1 at default depth (2): the initiative parent (depth 1), the
    // blocked t2 (depth 1), and both edges among the visible set.
    let body = bob
        .get_item_graph(EntityKind::Task, &t1_code, None)
        .await
        .expect("t1 subgraph");
    assert_eq!(body.focus, t1_code);
    assert_eq!(body.depth, 2);
    let node = |code: &str| body.nodes.iter().find(|n| n.short_code == code);
    let focus = node(&t1_code).expect("focus present");
    assert_eq!((focus.depth, focus.entity_type.as_str()), (0, "task"));
    // Workflow status is the board COLUMN NAME (t1 was just completed).
    assert_eq!(focus.status, "Completed");
    assert_eq!(node(&initiative_code).expect("parent").depth, 1);
    assert_eq!(node(&t2_code).expect("blocked").depth, 1);
    let has_edge = |src: &str, tgt: &str, rel: &str| {
        let id_of = |code: &str| node(code).map(|n| n.id.clone()).unwrap_or_default();
        body.edges.iter().any(|e| {
            e.source_id == id_of(src) && e.target_id == id_of(tgt) && e.relationship == rel
        })
    };
    assert!(has_edge(&initiative_code, &t1_code, "parent"), "{body:?}");
    assert!(has_edge(&t1_code, &t2_code, "blocks"), "{body:?}");
    // degree powers the +N badge: t1 touches initiative + t2.
    assert_eq!(focus.degree, 2);

    // depth=1 bounds the walk; an oversized depth clamps (200 -> the cap)
    // rather than erroring.
    let near = bob
        .get_item_graph(EntityKind::Task, &t1_code, Some(1))
        .await
        .expect("depth 1");
    assert_eq!(near.depth, 1);
    assert!(near.nodes.len() >= 3, "focus + neighbors: {near:?}");
    let clamped = bob
        .get_item_graph(EntityKind::Task, &t1_code, Some(200))
        .await
        .expect("clamped depth");
    assert_eq!(clamped.depth, 10, "MAX_TRAVERSE_DEPTH cap");

    // Family mismatch and dead refs -> 404 like the sibling reads.
    let err = rejection(
        bob.get_item_graph(EntityKind::Task, &initiative_code, None)
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // The same edges roll up as board-card badge counts (KAIROS-T-0091):
    // t1 blocks t2; the initiative has no blocks edges, so no entry.
    let board = bob
        .board_items(&delivery_board.to_string())
        .await
        .expect("delivery board items");
    let t1_counts = board
        .blocks_summary
        .get(&t1_code)
        .expect("t1 has a blocks entry");
    assert_eq!(
        (t1_counts.blocked_by, t1_counts.blocks),
        (0, 1),
        "{board:?}"
    );
    let t2_counts = board
        .blocks_summary
        .get(&t2_code)
        .expect("t2 has a blocks entry");
    assert_eq!(
        (t2_counts.blocked_by, t2_counts.blocks),
        (1, 0),
        "{board:?}"
    );
    assert!(
        !board.blocks_summary.contains_key(&initiative_code),
        "no blocks edges, no entry (and the initiative is off this board)"
    );

    // Family/short-code mismatch and unknown family -> 404. (The unknown
    // family is not expressible in the typed surface: raw probe.)
    let err = rejection(bob.relationships(EntityKind::Task, &initiative_code).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let (status, body) = bob
        .raw_request(
            Method::GET,
            &format!("/api/widgets/{t1_code}/relationships"),
            None,
        )
        .await
        .expect("raw unknown-family probe");
    assert_eq!(status, 404, "{body}");

    // DELETE: a member with no manage on either end and no authorship (bob)
    // is 403 even for a collaborative edge (KAIROS-T-0111); admin removes;
    // edge disappears; repeat 404.
    let err = rejection(bob.delete_relationship(&blocks_edge_id).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let body = svc
        .delete_relationship(&blocks_edge_id)
        .await
        .expect("deleting blocks edge");
    assert_eq!(body.id, blocks_edge_id);
    let body = bob
        .relationships(EntityKind::Task, &t1_code)
        .await
        .expect("t1 relationships after delete");
    assert!(
        rel_group(&body.outgoing, "blocks").is_none(),
        "blocks edge removed: {body:?}"
    );
    let err = rejection(svc.delete_relationship(&blocks_edge_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // ==========================================================================
    // Item metadata: A-0003 typed validation + A-0006 capability gating
    // ==========================================================================
    // Valid enum value (alice holds manage_tasks on the delivery board).
    let body = alice
        .update_metadata(
            EntityKind::Task,
            &t1_code,
            &metadata_patch("priority", Some("high")),
        )
        .await
        .expect("setting priority");
    let priority = metadata_value(&body, "priority").expect("priority value");
    assert_eq!(priority.value, "high");
    assert_eq!(priority.field_type, "enum");

    // Reads are open tenant-wide.
    let body = bob
        .metadata(EntityKind::Task, &t1_code)
        .await
        .expect("bob reads metadata");
    assert_eq!(
        metadata_value(&body, "priority")
            .expect("priority value")
            .value,
        "high"
    );

    // Enum membership rejection names the allowed values.
    let err = rejection(
        alice
            .update_metadata(
                EntityKind::Task,
                &t1_code,
                &metadata_patch("priority", Some("urgent")),
            )
            .await,
    );
    match &err {
        Error::Validation {
            status: 422,
            message,
            ..
        } => assert!(message.contains("low, medium, high, critical"), "{message}"),
        other => panic!("expected 422 Validation, got {other}"),
    }

    // Unknown definition slug -> 422.
    let err = rejection(
        alice
            .update_metadata(
                EntityKind::Task,
                &t1_code,
                &metadata_patch("no_such_field", Some("x")),
            )
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // KAIROS-T-0078: entity-type scoping is enforced on the write path —
    // document_type is documents-only, so setting it on a TASK is 422
    // even with the capability in hand.
    let err = rejection(
        alice
            .update_metadata(
                EntityKind::Task,
                &t1_code,
                &metadata_patch("document_type", Some("prd")),
            )
            .await,
    );
    match &err {
        Error::Validation {
            status, message, ..
        } => {
            assert_eq!(*status, 422);
            assert!(
                message.contains("does not apply to task items"),
                "{message}"
            );
        }
        other => panic!("expected 422 Validation, got {other}"),
    }
    // …while the same write on a DOCUMENT is fine.
    alice
        .update_metadata(
            EntityKind::Document,
            &doc_code,
            &metadata_patch("document_type", Some("prd")),
        )
        .await
        .expect("document_type on a document is in scope");

    // The definitions catalog filters by entity type: tasks never see
    // document_type; documents do; complexity excludes initiatives.
    let for_tasks = bob
        .list_metadata_definitions_for(Pagination::default(), Some("task"))
        .await
        .expect("task catalog");
    let slugs: Vec<&str> = for_tasks.items.iter().map(|d| d.slug.as_str()).collect();
    assert!(slugs.contains(&"priority") && slugs.contains(&"complexity"));
    assert!(!slugs.contains(&"document_type"), "{slugs:?}");
    let for_initiatives = bob
        .list_metadata_definitions_for(Pagination::default(), Some("initiative"))
        .await
        .expect("initiative catalog");
    let slugs: Vec<&str> = for_initiatives
        .items
        .iter()
        .map(|d| d.slug.as_str())
        .collect();
    assert!(
        !slugs.contains(&"complexity"),
        "complexity excludes initiatives (native column): {slugs:?}"
    );
    let err = rejection(
        bob.list_metadata_definitions_for(Pagination::default(), Some("widget"))
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // bob holds no capability on the delivery board -> 403 naming it.
    let err = rejection(
        bob.update_metadata(
            EntityKind::Task,
            &t1_code,
            &metadata_patch("priority", Some("low")),
        )
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_tasks"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // Documents gate via the parent's board (A-0006 inheritance).
    alice
        .update_metadata(
            EntityKind::Document,
            &doc_code,
            &metadata_patch("priority", Some("low")),
        )
        .await
        .expect("alice sets document metadata via inherited capability");
    let err = rejection(
        bob.update_metadata(
            EntityKind::Document,
            &doc_code,
            &metadata_patch("priority", Some("low")),
        )
        .await,
    );
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_documents"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }

    // ==========================================================================
    // Document lifecycle (KAIROS-T-0078): a typed column, not metadata
    // ==========================================================================
    // Born draft; free transitions; gated like every other document write
    // (manage_documents via the parent's board); never a version bump.
    let doc = alice
        .get_document(&doc_code)
        .await
        .expect("reading the document");
    assert_eq!(doc.lifecycle, "draft", "documents are born draft");
    let before_version = doc.version;
    let published = alice
        .set_document_lifecycle(&doc_code, "published")
        .await
        .expect("alice publishes");
    assert_eq!(published.lifecycle, "published");
    assert_eq!(
        published.version, before_version,
        "lifecycle never bumps the content version"
    );
    // Free transitions: straight back to draft is legal.
    let drafted = alice
        .set_document_lifecycle(&doc_code, "draft")
        .await
        .expect("free transition back to draft");
    assert_eq!(drafted.lifecycle, "draft");
    let err = rejection(bob.set_document_lifecycle(&doc_code, "published").await);
    match &err {
        Error::Forbidden { capability, .. } => {
            assert_eq!(capability.as_deref(), Some("manage_documents"));
        }
        other => panic!("expected Forbidden, got {other}"),
    }
    let err = rejection(alice.set_document_lifecycle(&doc_code, "retired").await);
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // ==========================================================================
    // Metadata definitions: CRUD, org-admin gating, in-use delete rejection
    // ==========================================================================
    // Non-admin cannot create definitions.
    let due_request = CreateMetadataDefinitionRequest {
        name: "Due Date".into(),
        slug: "due".into(),
        field_type: "date".into(),
        enum_options: vec![],
        entity_types: vec![],
    };
    let err = rejection(alice.create_metadata_definition(&due_request).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // Admin creates a date definition; date values then parse-validate.
    let due_def = svc
        .create_metadata_definition(&due_request)
        .await
        .expect("creating due definition");
    let due_def_id = due_def.id.clone();
    let body = alice
        .update_metadata(
            EntityKind::Task,
            &t1_code,
            &metadata_patch("due", Some("2026-08-01")),
        )
        .await
        .expect("setting due date");
    assert_eq!(
        metadata_value(&body, "due").expect("due value").value,
        "2026-08-01"
    );
    let err = rejection(
        alice
            .update_metadata(
                EntityKind::Task,
                &t1_code,
                &metadata_patch("due", Some("next Tuesday")),
            )
            .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // null clears a value.
    let body = alice
        .update_metadata(
            EntityKind::Task,
            &t1_code,
            &metadata_patch("priority", None),
        )
        .await
        .expect("clearing priority");
    assert!(
        metadata_value(&body, "priority").is_none(),
        "priority cleared: {body:?}"
    );

    // List (open read) carries the seeded defaults with ordered options.
    let body = bob
        .list_metadata_definitions(Pagination::default())
        .await
        .expect("listing definitions");
    let listed_priority = body
        .items
        .iter()
        .find(|item| item.slug == "priority")
        .expect("seeded priority definition");
    assert_eq!(listed_priority.field_type, "enum");
    assert!(listed_priority.is_system_default);
    assert_eq!(
        listed_priority.enum_options,
        ["low", "medium", "high", "critical"]
    );
    let document_type_def_id = body
        .items
        .iter()
        .find(|item| item.slug == "document_type")
        .map(|item| item.id.clone())
        .expect("seeded document_type definition");

    // Option rules: enum without options, non-enum with options, bad type.
    for bad in [
        CreateMetadataDefinitionRequest {
            name: "Sev".into(),
            slug: "sev".into(),
            field_type: "enum".into(),
            enum_options: vec![],
            entity_types: vec![],
        },
        CreateMetadataDefinitionRequest {
            name: "Repo".into(),
            slug: "repo2".into(),
            field_type: "string".into(),
            enum_options: vec!["x".into()],
            entity_types: vec![],
        },
        CreateMetadataDefinitionRequest {
            name: "Odd".into(),
            slug: "odd".into(),
            field_type: "blob".into(),
            enum_options: vec![],
            entity_types: vec![],
        },
    ] {
        let err = rejection(svc.create_metadata_definition(&bad).await);
        assert!(
            matches!(err, Error::Validation { status: 422, .. }),
            "{bad:?}: {err}"
        );
    }

    // Full definition lifecycle: create -> get -> patch -> delete.
    let color_def = svc
        .create_metadata_definition(&CreateMetadataDefinitionRequest {
            name: "Team Color".into(),
            slug: "team_color".into(),
            field_type: "enum".into(),
            enum_options: vec!["red".into(), "blue".into()],
            entity_types: vec![],
        })
        .await
        .expect("creating team_color definition");
    let color_def_id = color_def.id.clone();
    assert_eq!(color_def.enum_options, ["red", "blue"]);
    let body = bob
        .get_metadata_definition(&color_def_id)
        .await
        .expect("reading team_color definition");
    assert_eq!(body.slug, "team_color");
    let body = svc
        .update_metadata_definition(
            &color_def_id,
            &UpdateMetadataDefinitionRequest {
                name: Some("Team Colour".into()),
                slug: None,
                enum_options: Some(vec!["red".into(), "blue".into(), "green".into()]),
                entity_types: None,
            },
        )
        .await
        .expect("updating team_color definition");
    assert_eq!(body.name, "Team Colour");
    assert_eq!(body.enum_options, ["red", "blue", "green"]);
    svc.delete_metadata_definition(&color_def_id)
        .await
        .expect("deleting team_color definition");
    let err = rejection(bob.get_metadata_definition(&color_def_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // In-use deletes are refused: `due` has an item value, `document_type`
    // is carried by the seeded templates.
    let err = rejection(svc.delete_metadata_definition(&due_def_id).await);
    match &err {
        Error::Conflict { code, details, .. } => {
            assert_eq!(code, "DEFINITION_IN_USE");
            assert_eq!(details["item_values"], 1);
        }
        other => panic!("expected 409 DEFINITION_IN_USE, got {other}"),
    }
    let err = rejection(svc.delete_metadata_definition(&document_type_def_id).await);
    match &err {
        Error::Conflict { code, .. } => assert_eq!(code, "DEFINITION_IN_USE"),
        other => panic!("expected 409 DEFINITION_IN_USE, got {other}"),
    }

    // ==========================================================================
    // Templates: CRUD, org-admin gating, create-from-template still stamps
    // ==========================================================================
    // List (open) carries the seeded system templates.
    let body = bob
        .list_templates(Pagination::default())
        .await
        .expect("listing templates");
    let prd_id = body
        .items
        .iter()
        .find(|item| item.slug == "prd")
        .map(|item| item.id.clone())
        .expect("seeded prd template");

    // Detail = content + associated definitions with defaults/required.
    let prd = bob.get_template(&prd_id).await.expect("prd detail");
    assert!(prd.content.contains("# "), "prd starter content: {prd:?}");
    let doc_type_field = prd
        .metadata
        .iter()
        .find(|field| field.slug == "document_type")
        .expect("prd carries document_type");
    assert_eq!(doc_type_field.default_value.as_deref(), Some("prd"));
    assert_eq!(doc_type_field.field_type, "enum");
    assert!(
        doc_type_field.enum_options.contains(&"prd".to_string()),
        "{prd:?}"
    );

    // Writes are org-admin only.
    let runbook_request = CreateTemplateRequest {
        name: "Runbook".into(),
        slug: "runbook".into(),
        content: "# Runbook\n".into(),
        metadata: vec![TemplateMetadataEntry {
            definition_slug: "priority".into(),
            default_value: Some("high".into()),
            required: true,
        }],
    };
    let err = rejection(alice.create_template(&runbook_request).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let runbook = svc
        .create_template(&runbook_request)
        .await
        .expect("creating runbook template");
    let runbook_id = runbook.id.clone();
    assert_eq!(runbook.metadata[0].slug, "priority");
    assert_eq!(runbook.metadata[0].default_value.as_deref(), Some("high"));
    assert!(runbook.metadata[0].required);

    // Invalid default value / unknown definition slug -> 422.
    let err = rejection(
        svc.create_template(&CreateTemplateRequest {
            name: "Bad".into(),
            slug: "bad-default".into(),
            content: String::new(),
            metadata: vec![TemplateMetadataEntry {
                definition_slug: "priority".into(),
                default_value: Some("urgent".into()),
                required: false,
            }],
        })
        .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );
    let err = rejection(
        svc.create_template(&CreateTemplateRequest {
            name: "Bad".into(),
            slug: "bad-slug".into(),
            content: String::new(),
            metadata: vec![TemplateMetadataEntry {
                definition_slug: "no_such_definition".into(),
                default_value: None,
                required: false,
            }],
        })
        .await,
    );
    assert!(
        matches!(err, Error::Validation { status: 422, .. }),
        "{err}"
    );

    // Create-from-template still stamps content + metadata defaults
    // (KAIROS-A-0003 via the T-0018 document create).
    let stamped = alice
        .create_document(&CreateDocumentRequest {
            title: "Deploy runbook".into(),
            content: None,
            template_id: Some(runbook_id.clone()),
            parent_short_code: Some(initiative_code.clone()),
        })
        .await
        .expect("creating document from runbook template");
    assert_eq!(stamped.content, "# Runbook\n");
    let stamped_code = stamped.short_code.clone();
    let body = bob
        .metadata(EntityKind::Document, &stamped_code)
        .await
        .expect("stamped document metadata");
    assert_eq!(
        metadata_value(&body, "priority")
            .expect("stamped priority")
            .value,
        "high"
    );

    // PATCH updates content; the association list survives untouched.
    let body = svc
        .update_template(
            &runbook_id,
            &UpdateTemplateRequest {
                name: None,
                slug: None,
                content: Some("# Runbook v2\n".into()),
                metadata: None,
            },
        )
        .await
        .expect("updating runbook template");
    assert_eq!(body.content, "# Runbook v2\n");
    assert_eq!(body.metadata[0].slug, "priority");

    // DELETE is a hard delete; stamped documents keep their content and
    // metadata, template_id goes NULL (DDL SET NULL).
    svc.delete_template(&runbook_id)
        .await
        .expect("deleting runbook template");
    let err = rejection(bob.get_template(&runbook_id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let body = bob
        .get_document(&stamped_code)
        .await
        .expect("stamped document survives template delete");
    assert_eq!(body.template_id, None);
    assert_eq!(body.content, "# Runbook\n");

    // ==========================================================================
    // Content history: version list + specific snapshots (A-0004)
    // ==========================================================================
    alice
        .update_task(
            &t1_code,
            &UpdateContentRequest {
                title: Some("Wire endpoints v2".into()),
                content: "c2".into(),
                version: 1,
            },
        )
        .await
        .expect("edit v1 -> v2");
    let body = alice
        .update_task(
            &t1_code,
            &UpdateContentRequest {
                title: None,
                content: "c3".into(),
                version: 2,
            },
        )
        .await
        .expect("edit v2 -> v3");
    assert_eq!(body.version, 3);

    // Version list: newest first, includes the v1 create baseline.
    let body = bob
        .history(EntityKind::Task, &t1_code, None, None)
        .await
        .expect("history list");
    assert_eq!(body.total, 3);
    let versions: Vec<i32> = body.items.iter().map(|item| item.version).collect();
    assert_eq!(versions, vec![3, 2, 1]);
    assert_eq!(body.items[0].edited_by, alice_id.to_string());
    assert!(!body.items[0].edited_at.is_empty(), "{body:?}");

    // Pagination.
    let body = bob
        .history(EntityKind::Task, &t1_code, Some(1), Some(1))
        .await
        .expect("history page");
    assert_eq!(body.total, 3);
    assert_eq!(body.limit, 1);
    assert_eq!(body.offset, 1);
    assert_eq!(body.items[0].version, 2);

    // Specific version -> full snapshot.
    let body = bob
        .history_snapshot(EntityKind::Task, &t1_code, 2)
        .await
        .expect("v2 snapshot");
    assert_eq!(body.version, 2);
    assert_eq!(body.title, "Wire endpoints v2");
    assert_eq!(body.content, "c2");

    // Unknown version / unknown short code -> 404.
    let err = rejection(bob.history_snapshot(EntityKind::Task, &t1_code, 99).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let err = rejection(
        bob.history(EntityKind::Task, "ACME-T-9999", None, None)
            .await,
    );
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // ==========================================================================
    // Activity log: combinable filters + pagination (S-0005)
    // ==========================================================================
    // entity_id: the task's create row plus the three children-progress
    // walk transitions above (content edits go to item_history, metadata
    // writes are unversioned/unlogged per A-0004). Newest first.
    let body = bob
        .activity(&ActivityQuery {
            entity_id: Some(t1_id.clone()),
            ..ActivityQuery::default()
        })
        .await
        .expect("entity_id filter");
    assert_eq!(body.total, 4, "{body:?}");
    assert!(
        body.items[..3]
            .iter()
            .all(|item| item.action == "transition"),
        "{body:?}"
    );
    assert_eq!(body.items[3].action, "create");
    assert_eq!(body.items[3].entity_type.as_deref(), Some("task"));

    // action filter: the parent + blocks links (svc) and the two document
    // supports edges (alice) are relationship_add rows.
    let body = bob
        .activity(&ActivityQuery {
            action: Some("relationship_add".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("action filter");
    assert_eq!(body.total, 4, "{body:?}");
    assert!(
        body.items
            .iter()
            .all(|item| item.entity_id.is_none() && item.details.starts_with("relationship:")),
        "{body:?}"
    );

    // Combined actor + action filters.
    let body = bob
        .activity(&ActivityQuery {
            action: Some("relationship_add".into()),
            actor_id: Some(svc_id.to_string()),
            ..ActivityQuery::default()
        })
        .await
        .expect("combined filters");
    assert_eq!(
        body.total, 1,
        "svc linked blocks (alice linked parent, KAIROS-T-0111): {body:?}"
    );
    let body = bob
        .activity(&ActivityQuery {
            action: Some("relationship_add".into()),
            actor_id: Some(alice_id.to_string()),
            ..ActivityQuery::default()
        })
        .await
        .expect("combined filters, alice");
    assert_eq!(
        body.total, 3,
        "alice: two document supports edges + the parent edge: {body:?}"
    );
    let body = bob
        .activity(&ActivityQuery {
            action: Some("relationship_remove".into()),
            actor_id: Some(alice_id.to_string()),
            ..ActivityQuery::default()
        })
        .await
        .expect("combined filters, empty");
    assert_eq!(body.total, 0, "alice removed nothing: {body:?}");
    let body = bob
        .activity(&ActivityQuery {
            action: Some("relationship_remove".into()),
            actor_id: Some(svc_id.to_string()),
            ..ActivityQuery::default()
        })
        .await
        .expect("combined filters, one row");
    assert_eq!(body.total, 1, "{body:?}");

    // since: the far future filters everything out; the epoch nothing.
    let body = bob
        .activity(&ActivityQuery {
            since: Some("2999-01-01T00:00:00Z".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("far-future since");
    assert_eq!(body.total, 0);
    let body = bob
        .activity(&ActivityQuery {
            since: Some("1970-01-01T00:00:00Z".into()),
            limit: Some(2),
            ..ActivityQuery::default()
        })
        .await
        .expect("epoch since with limit");
    assert!(body.total > 2, "{body:?}");
    assert_eq!(body.items.len(), 2);
    assert_eq!(body.limit, 2);

    // Malformed filter values -> 422 VALIDATION.
    for query in [
        ActivityQuery {
            since: Some("yesterday".into()),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            action: Some("frobnicate".into()),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            entity_id: Some("not-a-uuid".into()),
            ..ActivityQuery::default()
        },
        ActivityQuery {
            actor_id: Some("not-a-uuid".into()),
            ..ActivityQuery::default()
        },
    ] {
        let err = rejection(bob.activity(&query).await);
        assert!(
            matches!(err, Error::Validation { status: 422, .. }),
            "{query:?}: {err}"
        );
    }

    // --- teardown -------------------------------------------------------------
    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
