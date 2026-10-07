//! A repository opts in to the hosted summaries of its tenant
//! (KAIROS-T-0340, COLLIERY-I-0611): the setting `code_index_summaries`.
//!
//! 1. A tenant with no hosted provider: `hosted` is refused with
//!    `CODE_INDEX_NO_HOSTED_PROVIDER`, and the text names the command.
//! 2. The admin sets Ollama Cloud; a member of the owner team sets `hosted`;
//!    the repository shows it on REST (the list and the detail).
//! 3. An unknown value is refused and named; a value that the repository
//!    has changes nothing.
//! 4. A member of another team cannot set it.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_repository_code_index_summaries_t0340_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types_auth::Secret;
use kairos_client::types_code_index::{
    PutCodeIndexSettings, PutSummaryProvider, PutVectorProvider,
};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::{
    CodeIndexSummaries, CreateRepositoryRequest, UpdateRepositoryRequest,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;

const SCRATCH_DB: &str = "kairos_repository_code_index_summaries_t0340_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn summaries(value: CodeIndexSummaries) -> UpdateRepositoryRequest {
    UpdateRepositoryRequest {
        code_index_summaries: Some(value),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_repository_opts_in_to_hosted_summaries() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let svc_token = user_token(&http, "svc").await;
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let mut config = base_config(&scratch_url);
    config.secrets_key = Some(SecretsKey::from_bytes([11u8; 32]));
    let server = spawn_server(app::router(app::state_with(config, pool, auth))).await;
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    for client in [&svc, &alice, &bob] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        let user_id = user_id(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    for (name, slug, prefix, member) in [
        ("Platform", "platform", "PLATFORM", "bob@kairos.test"),
        ("Web", "web", "WEB", "alice@kairos.test"),
    ] {
        let team = svc
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
        svc.add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: user_id(&mut conn, member).to_string(),
            },
        )
        .await
        .expect("team member");
    }
    let repo = bob
        .create_repository(&CreateRepositoryRequest {
            slug: Some("payments-api".into()),
            forge: "github".into(),
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: None,
            team: "platform".into(),
            description: None,
        })
        .await
        .expect("payments-api");
    assert_eq!(repo.code_index_summaries, CodeIndexSummaries::Embedded);

    // 1. No hosted provider: the opt-in is refused and names the command.
    let err = bob
        .update_repository("payments-api", &summaries(CodeIndexSummaries::Hosted))
        .await
        .expect_err("no hosted provider");
    match &err {
        Error::Other {
            status,
            code,
            message,
            details,
        } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "CODE_INDEX_NO_HOSTED_PROVIDER");
            assert!(
                message.contains("kairos admin code-index-settings set"),
                "{message}"
            );
            assert_eq!(details["field"], "code_index_summaries");
        }
        other => panic!("{other}"),
    }

    // 2. The admin sets Ollama Cloud; bob opts the repository in.
    svc.put_code_index_settings(&PutCodeIndexSettings {
        summary: PutSummaryProvider {
            provider: "ollama-cloud".into(),
            base_url: Some("https://ollama.com/v1".into()),
            model: Some("gemma4:31b".into()),
            region: None,
            secret: Some(Secret::new("ollama_t0340_key")),
        },
        vectors: PutVectorProvider {
            provider: "embedded".into(),
            ..Default::default()
        },
        concurrency: None,
    })
    .await
    .expect("the provider");
    let hosted = bob
        .update_repository("payments-api", &summaries(CodeIndexSummaries::Hosted))
        .await
        .expect("bob opts in");
    assert_eq!(hosted.code_index_summaries, CodeIndexSummaries::Hosted);
    let listed = alice.list_repositories(None).await.expect("the list");
    assert_eq!(
        listed
            .iter()
            .find(|r| r.slug == "payments-api")
            .expect("payments-api")
            .code_index_summaries,
        CodeIndexSummaries::Hosted
    );
    let detail = alice
        .get_repository("payments-api")
        .await
        .expect("the detail");
    assert_eq!(
        detail.repository.code_index_summaries,
        CodeIndexSummaries::Hosted
    );

    // 3. An unknown value is refused and named; the same value changes
    //    nothing.
    let raw = http
        .patch(format!("{}/api/repositories/payments-api", server.base_url))
        .bearer_auth(&bob_token)
        .header("X-Tenant", "acme")
        .json(&json!({ "code_index_summaries": "cloud" }))
        .send()
        .await
        .expect("send");
    assert_eq!(raw.status(), 422);
    let text = raw.text().await.unwrap();
    assert!(text.contains("cloud"), "{text}");
    let same = bob
        .update_repository("payments-api", &summaries(CodeIndexSummaries::Hosted))
        .await
        .expect("the same value");
    assert_eq!(
        same.updated_at, hosted.updated_at,
        "the same value writes nothing"
    );

    // 4. A member of another team cannot set it; back to embedded by bob.
    let err = alice
        .update_repository("payments-api", &summaries(CodeIndexSummaries::Embedded))
        .await
        .expect_err("alice is on web");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let back = bob
        .update_repository("payments-api", &summaries(CodeIndexSummaries::Embedded))
        .await
        .expect("back to embedded");
    assert_eq!(back.code_index_summaries, CodeIndexSummaries::Embedded);
}
