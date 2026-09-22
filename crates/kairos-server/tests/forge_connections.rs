//! Integration test for the KAIROS-T-0097 forge-connection surface
//! (design in KAIROS-I-0009), through the typed `kairos_client` against
//! the booted production router on a real port.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. Owns the uniquely named scratch
//! database `kairos_forge_t0097_test` (shared-services discipline).
//!
//! Cast:
//! - `svc`   — org ADMIN: the only role allowed to write connections,
//! - `alice` — org member: reads open, writes 403.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types_forge::CreateForgeConnectionRequest;
use kairos_client::types_org::CreateTeamRequest;
use kairos_client::types_repositories::{CreateRepositoryRequest, UpdateRepositoryRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::forge::auth::derive_secret;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_forge_t0097_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn rejection<T: std::fmt::Debug>(result: Result<T, Error>) -> Error {
    match result {
        Ok(value) => panic!("expected an API rejection, got success: {value:?}"),
        Err(err) => err,
    }
}

#[tokio::test]
async fn forge_connection_lifecycle_against_live_stack() {
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

    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
    let svc_token = user_token(&http, "svc").await;

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let config = base_config(&scratch_url);
    let signing_key = config
        .webhook_signing_key
        .clone()
        .expect("the test config configures the forge integration");
    let router = app::router(app::state_with(config, pool.clone(), auth.clone()));
    let server = spawn_server(router).await;
    let alice = server.client(&alice_token, "acme");
    let svc = server.client(&svc_token, "acme");

    // JIT-provision then grant membership (alice member, svc admin).
    for client in [&alice, &svc] {
        let err = rejection(client.list_forge_connections().await);
        assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    for (user_id, role) in [(alice_id, OrgRole::Member), (svc_id, OrgRole::Admin)] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }

    // =======================================================================
    // Writes are org-admin only; reads open tenant-wide
    // =======================================================================
    // KAIROS-T-0106 (A-0019): a connection is the webhook wiring OF a
    // registered repository — register it (under its one owning team)
    // first, then connect by slug.
    let platform = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await
        .expect("owning team");
    let payments = svc
        .create_repository(&CreateRepositoryRequest {
            slug: None,
            forge: "github".into(),
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: None,
            team: platform.id.clone(),
            description: None,
        })
        .await
        .expect("registering the repository");
    assert_eq!(
        payments.slug, "acme-payments-api",
        "slug derives from the full name"
    );
    assert!(!payments.has_webhook);
    let request = CreateForgeConnectionRequest {
        repository: payments.slug.clone(),
    };
    let err = rejection(alice.create_forge_connection(&request).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let err = rejection(
        svc.create_forge_connection(&CreateForgeConnectionRequest {
            repository: "nope".into(),
        })
        .await,
    );
    assert!(
        matches!(err, Error::NotFound { .. }),
        "an unregistered repository cannot be connected: {err}"
    );

    let created = svc
        .create_forge_connection(&request)
        .await
        .expect("org admin connects a repository");
    assert_eq!(created.connection.forge, "github");
    assert_eq!(created.connection.repository.slug, payments.slug);
    assert_eq!(
        created.connection.repository.repo_full_name,
        "acme/payments-api"
    );
    assert_eq!(
        created.connection.repository.team_id, platform.id,
        "ownership is the repository's"
    );
    assert!(
        svc.get_repository(&payments.slug)
            .await
            .expect("repo detail")
            .repository
            .has_webhook,
        "the directory shows the webhook"
    );

    // The delivery URL carries forge, tenant, and connection id — the
    // routing a webhook has instead of an auth stack.
    assert_eq!(
        created.webhook_url,
        format!(
            "https://kairos.test/webhooks/github/acme/{}",
            created.connection.id
        )
    );
    // The secret is DERIVED, so it is reproducible from the key + id and
    // was never stored.
    let connection_uuid: Uuid = created.connection.id.parse().expect("uuid");
    assert_eq!(
        created.webhook_secret,
        derive_secret(&signing_key, connection_uuid)
    );

    // …and it is never returned again by any read.
    let listed = alice
        .list_forge_connections()
        .await
        .expect("reads are open tenant-wide");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.connection.id);
    let fetched = alice
        .get_forge_connection(&created.connection.id)
        .await
        .expect("single read");
    assert_eq!(fetched.repository.repo_full_name, "acme/payments-api");

    // =======================================================================
    // Prerequisites are checked BEFORE any write (KAIROS-T-0116)
    // =======================================================================
    // A second server on the same database with no KAIROS_PUBLIC_URL: the
    // connect must 501 and persist NOTHING (it used to persist, then 501).
    let mut unconfigured = base_config(&scratch_url);
    unconfigured.public_url = None;
    let dark = spawn_server(app::router(app::state_with(
        unconfigured,
        pool.clone(),
        auth.clone(),
    )))
    .await;
    let dark_svc = dark.client(&svc_token, "acme");
    let second = svc
        .create_repository(&CreateRepositoryRequest {
            slug: Some("ledger".into()),
            forge: "github".into(),
            repo_full_name: "acme/ledger".into(),
            repo_url: "https://github.com/acme/ledger".into(),
            default_branch: None,
            team: platform.id.clone(),
            description: None,
        })
        .await
        .expect("a second repo");
    let err = rejection(
        dark_svc
            .create_forge_connection(&CreateForgeConnectionRequest {
                repository: second.slug.clone(),
            })
            .await,
    );
    assert!(
        matches!(&err, Error::Other { status: 501, .. }),
        "unconfigured public URL is a 501: {err}"
    );
    assert!(
        !svc.get_repository(&second.slug)
            .await
            .expect("detail")
            .repository
            .has_webhook,
        "nothing was persisted before the 501"
    );
    drop(dark);

    // =======================================================================
    // One live connection per repo; validation
    // =======================================================================
    let err = rejection(svc.create_forge_connection(&request).await);
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    // An `other`-forge repository owns tasks but has no webhook dialect.
    let plain = svc
        .create_repository(&CreateRepositoryRequest {
            slug: Some("wiki".into()),
            forge: "other".into(),
            repo_full_name: "acme/wiki".into(),
            repo_url: "https://wiki.acme.test".into(),
            default_branch: None,
            team: platform.id.clone(),
            description: None,
        })
        .await
        .expect("an other-forge repo");
    let err = rejection(
        svc.create_forge_connection(&CreateForgeConnectionRequest {
            repository: plain.slug.clone(),
        })
        .await,
    );
    assert!(matches!(err, Error::Validation { .. }), "{err}");

    // =======================================================================
    // Ownership lives on the repository: re-home it there, never here
    // =======================================================================
    let team = svc
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            team_type: None,
        })
        .await
        .expect("team to re-home to");
    let updated = svc
        .update_repository(
            &payments.slug,
            &UpdateRepositoryRequest {
                team: Some(team.id.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("re-homing the repo to another team");
    assert_eq!(updated.team.id, team.id);
    let fetched = alice
        .get_forge_connection(&created.connection.id)
        .await
        .expect("connection follows its repository");
    assert_eq!(fetched.repository.team_id, team.id);
    // A repository with a live connection cannot be deleted.
    let err = rejection(svc.delete_repository(&payments.slug).await);
    assert!(matches!(err, Error::Conflict { .. }), "{err}");
    // A plain member (on neither team) cannot re-home it.
    let err = rejection(
        alice
            .update_repository(
                &payments.slug,
                &UpdateRepositoryRequest {
                    team: Some(platform.id.clone()),
                    ..Default::default()
                },
            )
            .await,
    );
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // =======================================================================
    // Rotation mints a NEW id (and therefore a new URL and secret)
    // =======================================================================
    let rotated = svc
        .rotate_forge_connection(&created.connection.id)
        .await
        .expect("rotating");
    assert_ne!(rotated.connection.id, created.connection.id);
    assert_ne!(rotated.webhook_secret, created.webhook_secret);
    assert_ne!(rotated.webhook_url, created.webhook_url);
    // Same repository, and still exactly one live connection for it.
    assert_eq!(
        rotated.connection.repository.repo_full_name,
        "acme/payments-api"
    );
    assert_eq!(
        rotated.connection.repository.id,
        created.connection.repository.id
    );
    let listed = alice.list_forge_connections().await.expect("after rotate");
    assert_eq!(listed.len(), 1, "the old connection is gone: {listed:?}");
    assert_eq!(listed[0].id, rotated.connection.id);
    // The old id no longer resolves.
    let err = rejection(alice.get_forge_connection(&created.connection.id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");

    // =======================================================================
    // Disconnect
    // =======================================================================
    let err = rejection(alice.delete_forge_connection(&rotated.connection.id).await);
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let deleted = svc
        .delete_forge_connection(&rotated.connection.id)
        .await
        .expect("disconnecting");
    assert!(deleted.deleted);
    assert!(
        alice
            .list_forge_connections()
            .await
            .expect("after delete")
            .is_empty()
    );
    let err = rejection(svc.delete_forge_connection(&rotated.connection.id).await);
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    // The repo is free again after disconnecting (the partial unique index).
    svc.create_forge_connection(&request)
        .await
        .expect("reconnecting a disconnected repository");

    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
