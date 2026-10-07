//! `/api/org/code-index-settings` (KAIROS-T-0339, COLLIERY-I-0611): where
//! the code index summaries and vectors of the tenant are made.
//!
//! - `GET`: the settings, with each secret as a status (set, by whom,
//!   when) and never the secret. Open tenant-wide: a member reads which
//!   provider the summaries of a repository go to.
//! - `PUT`: the full settings, by an organization admin. A secret that the
//!   body does not name keeps the stored secret; an empty secret removes
//!   it; a value replaces it, sealed with `KAIROS_SECRETS_KEY`
//!   ([`crate::secrets`]). A provider that needs a value refuses the call
//!   and names the field; an unknown provider or field is refused and named.
//!
//! The builder reads the row for each build (KAIROS-T-0341); this module
//! only stores it.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types_code_index as dto;
use kairos_db::code_index_settings::{
    self as settings, CONCURRENCY_RANGE, DEFAULT_CONCURRENCY, SUMMARY_PROVIDERS, SealedSecret,
    SecretChange, Settings, VECTOR_PROVIDERS,
};
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::secrets::SecretsKey;

/// The longest secret, URL, model and region that the settings take.
const MAX_LEN: usize = 4096;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/org/code-index-settings",
        get(get_settings).put(put_settings),
    )
}

/// The associated data of a sealed secret of the settings: the tenant and
/// the field, so that a ciphertext opens only in its own place.
pub fn settings_aad(tenant: &str, field: &str) -> Vec<u8> {
    format!("kairos/code-index-settings/v1\0{tenant}\0{field}").into_bytes()
}

fn status_of(
    set_by: Option<Uuid>,
    set_at: Option<chrono::DateTime<chrono::Utc>>,
) -> dto::SecretStatus {
    dto::SecretStatus {
        set: set_by.is_some(),
        set_by: set_by.map(|u| u.to_string()),
        set_at: set_at.map(|t| t.to_rfc3339()),
    }
}

/// The DTO of a row, or of the defaults when the tenant has no row.
pub(crate) fn dto_of(row: Option<Settings>) -> dto::CodeIndexSettings {
    let (row, stored) = match row {
        Some(row) => (row, true),
        None => (Settings::embedded(Uuid::nil()), false),
    };
    dto::CodeIndexSettings {
        summary: dto::SummaryProviderSettings {
            provider: row.summary_provider,
            base_url: row.summary_base_url,
            model: row.summary_model,
            region: row.summary_region,
            secret: status_of(row.summary_secret_set_by, row.summary_secret_set_at),
        },
        vectors: dto::VectorProviderSettings {
            provider: row.vector_provider,
            base_url: row.vector_base_url,
            model: row.vector_model,
            secret: status_of(row.vector_secret_set_by, row.vector_secret_set_at),
        },
        concurrency: row.concurrency,
        updated_by: stored.then(|| row.updated_by.to_string()),
        updated_at: stored.then(|| row.updated_at.to_rfc3339()),
    }
}

/// The provider settings of the code index of the tenant. The secrets are
/// a status, never the value.
#[utoipa::path(
    get,
    path = "/api/org/code-index-settings",
    tag = "organization",
    responses(
        (status = 200, description = "The settings", body = dto::CodeIndexSettings),
    ),
)]
pub(crate) async fn get_settings(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<dto::CodeIndexSettings>, ApiError> {
    let row = state
        .blocking
        .run(&tenant.slug, |conn| {
            settings::load(conn).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(dto_of(row)))
}

/// A field that a provider needs and the body does not give, as a refusal.
fn missing(provider: &str, field: &str, hint: &str) -> ApiError {
    ApiError::validation(format!("The provider {provider:?} needs {field}. {hint}"))
        .with_details(json!({ "field": field }))
}

/// A value that is too long, has a space or a control character.
fn check_text(field: &str, value: &str) -> Result<(), ApiError> {
    let bad = value.is_empty()
        || value.len() > MAX_LEN
        || value.chars().any(|c| c.is_whitespace() || c.is_control());
    if bad {
        return Err(ApiError::validation(format!(
            "The {field} is not correct. Use 1 to {MAX_LEN} characters and no space."
        ))
        .with_details(json!({ "field": field })));
    }
    Ok(())
}

fn check_url(field: &str, value: &str) -> Result<(), ApiError> {
    check_text(field, value)?;
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err(ApiError::validation(format!(
            "The {field} {value:?} is not an http or https URL. Send the base URL of the \
             endpoint, for example https://ollama.com/v1."
        ))
        .with_details(json!({ "field": field })));
    }
    Ok(())
}

/// The sealed secret after the body: kept, removed or set. `has_stored`
/// says whether a secret is stored now, so that a provider that needs one
/// is refused when the body removes it or none is there.
#[allow(clippy::too_many_arguments)]
fn secret_change(
    key: &Option<SecretsKey>,
    tenant: &str,
    field: &str,
    sent: Option<&kairos_client::types_auth::Secret>,
    provider_needs_one: bool,
    has_stored: bool,
    provider: &str,
    hint: &str,
) -> Result<SecretChange, ApiError> {
    let change = match sent {
        None => SecretChange::Keep,
        Some(secret) if secret.is_empty() => SecretChange::Remove,
        Some(secret) => {
            check_text(field, secret.expose())?;
            let Some(key) = key else {
                return Err(ApiError::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "SECRETS_NOT_CONFIGURED",
                    "This deployment has no KAIROS_SECRETS_KEY, so Kairos cannot keep a \
                     secret. An operator sets KAIROS_SECRETS_KEY to 32 random bytes in base64 \
                     and starts the server again.",
                )
                .with_details(json!({ "setting": "KAIROS_SECRETS_KEY", "field": field })));
            };
            let sealed = key.seal(&settings_aad(tenant, field), secret.expose().as_bytes());
            SecretChange::Set(SealedSecret {
                ciphertext: sealed.ciphertext,
                nonce: sealed.nonce,
                key_id: sealed.key_id,
            })
        }
    };
    let will_have = match &change {
        SecretChange::Keep => has_stored,
        SecretChange::Remove => false,
        SecretChange::Set(_) => true,
    };
    if provider_needs_one && !will_have {
        return Err(missing(provider, field, hint));
    }
    Ok(change)
}

/// The Bedrock credentials in one secret: `<access key id>:<secret access
/// key>` or with `:<session token>`. Pure.
pub(crate) fn bedrock_secret_fault(secret: &str) -> Option<String> {
    let parts: Vec<&str> = secret.split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty()) {
        return Some(
            "The secret of bedrock is <access key id>:<secret access key>, or with \
             :<session token> at the end. Each part is not empty."
                .to_string(),
        );
    }
    None
}

/// The ONE check and write of the settings, for REST and the CLI (through
/// REST). `key` is the key of the deployment, when it has one.
pub(crate) fn write_settings(
    conn: &mut PgConnection,
    tenant: &str,
    user: Uuid,
    key: &Option<SecretsKey>,
    body: dto::PutCodeIndexSettings,
) -> Result<Settings, ApiError> {
    let current = settings::load(conn).map_err(ApiError::internal)?;
    let summary = body.summary;
    if !SUMMARY_PROVIDERS.contains(&summary.provider.as_str()) {
        return Err(ApiError::validation(format!(
            "{:?} is not a provider of the summaries. The providers are: {}.",
            summary.provider,
            SUMMARY_PROVIDERS.join(", ")
        ))
        .with_details(json!({ "field": "summary.provider" })));
    }
    let vectors = body.vectors;
    if !VECTOR_PROVIDERS.contains(&vectors.provider.as_str()) {
        return Err(ApiError::validation(format!(
            "{:?} is not a provider of the vectors. The providers are: {}.",
            vectors.provider,
            VECTOR_PROVIDERS.join(", ")
        ))
        .with_details(json!({ "field": "vectors.provider" })));
    }
    let concurrency = body.concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    if !CONCURRENCY_RANGE.contains(&concurrency) {
        return Err(ApiError::validation(format!(
            "The concurrency {concurrency} is not in the range {} to {}.",
            CONCURRENCY_RANGE.start(),
            CONCURRENCY_RANGE.end()
        ))
        .with_details(json!({ "field": "concurrency" })));
    }

    // The summaries.
    let (summary_base_url, summary_model, summary_region) = match summary.provider.as_str() {
        "embedded" => (None, None, None),
        "ollama-cloud" => {
            let url = summary.base_url.clone().ok_or_else(|| {
                missing(
                    "ollama-cloud",
                    "summary.base_url",
                    "Send the base URL of the OpenAI-compatible endpoint, for example \
                     https://ollama.com/v1.",
                )
            })?;
            check_url("summary.base_url", &url)?;
            let model = summary.model.clone().ok_or_else(|| {
                missing(
                    "ollama-cloud",
                    "summary.model",
                    "Send the model name, for example gemma4:31b.",
                )
            })?;
            check_text("summary.model", &model)?;
            (Some(url), Some(model), None)
        }
        "bedrock" => {
            let region = summary.region.clone().ok_or_else(|| {
                missing(
                    "bedrock",
                    "summary.region",
                    "Send the AWS region, for example us-east-1.",
                )
            })?;
            check_text("summary.region", &region)?;
            let model = summary.model.clone().ok_or_else(|| {
                missing(
                    "bedrock",
                    "summary.model",
                    "Send the model id, for example anthropic.claude-3-5-haiku-20241022-v1:0.",
                )
            })?;
            check_text("summary.model", &model)?;
            if let Some(secret) = summary.secret.as_ref().filter(|s| !s.is_empty())
                && let Some(fault) = bedrock_secret_fault(secret.expose())
            {
                return Err(
                    ApiError::validation(fault).with_details(json!({ "field": "summary.secret" }))
                );
            }
            (None, Some(model), Some(region))
        }
        _ => unreachable!("checked above"),
    };
    let summary_needs_secret = summary.provider != "embedded";
    let summary_secret = secret_change(
        key,
        tenant,
        "summary.secret",
        summary.secret.as_ref(),
        summary_needs_secret,
        current
            .as_ref()
            .is_some_and(|c| c.summary_secret_set_by.is_some()),
        &summary.provider,
        if summary.provider == "bedrock" {
            "Send the AWS credentials as <access key id>:<secret access key>."
        } else {
            "Send the API key of the endpoint."
        },
    )?;

    // The vectors.
    let (vector_base_url, vector_model) = match vectors.provider.as_str() {
        "embedded" => (None, None),
        "remote" => {
            let url = vectors.base_url.clone().ok_or_else(|| {
                missing(
                    "remote",
                    "vectors.base_url",
                    "Send the base URL of the OpenAI-compatible embeddings endpoint.",
                )
            })?;
            check_url("vectors.base_url", &url)?;
            let model = vectors.model.clone().ok_or_else(|| {
                missing(
                    "remote",
                    "vectors.model",
                    "Send the model name of the embeddings.",
                )
            })?;
            check_text("vectors.model", &model)?;
            (Some(url), Some(model))
        }
        _ => unreachable!("checked above"),
    };
    let vector_secret = secret_change(
        key,
        tenant,
        "vectors.secret",
        vectors.secret.as_ref(),
        false,
        current
            .as_ref()
            .is_some_and(|c| c.vector_secret_set_by.is_some()),
        &vectors.provider,
        "",
    )?;

    settings::save(
        conn,
        settings::Update {
            summary_provider: summary.provider,
            summary_base_url,
            summary_model,
            summary_region,
            summary_secret,
            vector_provider: vectors.provider,
            vector_base_url,
            vector_model,
            vector_secret,
            concurrency,
            updated_by: user,
        },
    )
    .map_err(ApiError::internal)
}

/// Set the provider settings of the code index of the tenant (an
/// organization admin). The answer has each secret as a status.
#[utoipa::path(
    put,
    path = "/api/org/code-index-settings",
    tag = "organization",
    request_body = dto::PutCodeIndexSettings,
    responses(
        (status = 200, description = "The settings as Kairos keeps them now", body = dto::CodeIndexSettings),
        (status = 403, description = "Not an organization admin", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "An unknown provider or field, a value that a provider needs and the body does not give, or a value that is not correct", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "A secret was sent, and this deployment has no KAIROS_SECRETS_KEY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn put_settings(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    crate::body::ApiJson(body): crate::body::ApiJson<dto::PutCodeIndexSettings>,
) -> Result<Json<dto::CodeIndexSettings>, ApiError> {
    crate::api::meta::require_org_admin(&tenant)?;
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let key = state.config.secrets_key.clone();
    let row = state
        .blocking
        .run(&tenant.slug, move |conn| {
            write_settings(conn, &tenant_slug, user, &key, body)
        })
        .await?;
    Ok(Json(dto_of(Some(row))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bedrock_secret_has_two_or_three_parts() {
        assert_eq!(bedrock_secret_fault("AKIA:secret"), None);
        assert_eq!(bedrock_secret_fault("AKIA:secret:token"), None);
        assert!(bedrock_secret_fault("AKIA").is_some());
        assert!(bedrock_secret_fault("AKIA:").is_some());
        assert!(bedrock_secret_fault("a:b:c:d").is_some());
    }

    #[test]
    fn the_aad_names_the_tenant_and_the_field() {
        assert_eq!(
            settings_aad("acme", "summary.secret"),
            b"kairos/code-index-settings/v1\0acme\0summary.secret".to_vec()
        );
        assert_ne!(
            settings_aad("acme", "summary.secret"),
            settings_aad("acme", "vectors.secret")
        );
    }
}
