//! The read token of a repository (COLLIERY-T-3105): the encryption, the
//! decryption, and the status that a read shows. The storage is
//! [`kairos_db::repository_credentials`]; the routes are
//! [`crate::api::org::repository_credentials`].
//!
//! The token is write-only. No surface gives it back: a read gives
//! [`status_dto`] (set or not, who set it and when, the last access check).
//! Only the builder of the base code index decrypts it ([`read_token`]), to
//! give it to git ([`crate::code_index::git`]).
//!
//! KAIROS-T-0431: the organization can keep one read token for each forge
//! host ([`kairos_db::org_read_tokens`]). [`fetch_token`] chooses the token
//! of a fetch: the token of the host of the repository when there is one,
//! else the token of the repository, else none. [`TokenSource`] names the
//! choice for the log, never the value.

use diesel::pg::PgConnection;
use kairos_client::types_repositories as dto;
use kairos_db::repository_credentials::{self, CredentialStatus};
use uuid::Uuid;

use crate::code_index::git::GitToken;
use crate::secrets::{OpenError, SecretsKey, credential_aad, org_read_token_aad};

/// The longest token that Kairos takes. A GitHub fine-grained token has 93
/// characters.
pub const MAX_TOKEN_LEN: usize = 1024;

/// Why [`read_token`] gave no token.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// The repository has a token, and the deployment has no key.
    #[error(
        "The repository has a read token, but this deployment has no KAIROS_SECRETS_KEY. \
         Set KAIROS_SECRETS_KEY to the key that encrypted the token, or set the token again."
    )]
    NoKey,
    /// The host has a token of the organization, and the deployment has no
    /// key (KAIROS-T-0431).
    #[error(
        "The organization has a read token for {0}, but this deployment has no \
         KAIROS_SECRETS_KEY. Set KAIROS_SECRETS_KEY to the key that encrypted the token, or \
         set the token again."
    )]
    OrgNoKey(String),
    /// The token does not decrypt.
    #[error(transparent)]
    Open(#[from] OpenError),
    /// The database failed.
    /// KAIROS-T-0697: only the log has the cause ([`token_refusal`] makes
    /// it a 500).
    #[error("The database gave an error. The server log has the cause.")]
    Database(#[from] diesel::result::Error),
}

/// The refusal of a stored token that the server cannot read: a 409 with
/// the code `CREDENTIAL_UNREADABLE` and the reason.
pub fn token_refusal(e: TokenError) -> crate::error::ApiError {
    match e {
        TokenError::Database(e) => crate::error::ApiError::internal(e),
        e => crate::error::ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "CREDENTIAL_UNREADABLE",
            e.to_string(),
        ),
    }
}

/// The decrypted read token of a repository. `None`: the repository has no
/// token.
pub fn read_token(
    conn: &mut PgConnection,
    tenant: &str,
    repository_id: Uuid,
    key: Option<&SecretsKey>,
) -> Result<Option<GitToken>, TokenError> {
    let Some(stored) = repository_credentials::load(conn, repository_id)? else {
        return Ok(None);
    };
    let key = key.ok_or(TokenError::NoKey)?;
    let plain = key.open(
        &credential_aad(tenant, repository_id),
        &stored.key_id,
        &stored.nonce,
        &stored.ciphertext,
    )?;
    let token = String::from_utf8(plain).map_err(|_| OpenError::Corrupt)?;
    Ok(Some(GitToken::new(token)))
}

/// The token that a fetch of a repository uses (KAIROS-T-0431). It names
/// the kind of token, never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// The read token of the organization for this host.
    Org { host: String },
    /// The read token of the repository.
    Repository,
    /// No token: the fetch has no credential.
    None,
}

impl std::fmt::Display for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenSource::Org { host } => write!(f, "the organization read token of {host}"),
            TokenSource::Repository => f.write_str("the read token of the repository"),
            TokenSource::None => f.write_str("no read token"),
        }
    }
}

/// The decrypted read token of the organization for `host`. `None`: the
/// host has no token.
pub fn org_token(
    conn: &mut PgConnection,
    tenant: &str,
    host: &str,
    key: Option<&SecretsKey>,
) -> Result<Option<GitToken>, TokenError> {
    let Some(stored) = kairos_db::org_read_tokens::load(conn, host)? else {
        return Ok(None);
    };
    let key = key.ok_or_else(|| TokenError::OrgNoKey(host.to_string()))?;
    let plain = key.open(
        &org_read_token_aad(tenant, host),
        &stored.key_id,
        &stored.nonce,
        &stored.ciphertext,
    )?;
    let token = String::from_utf8(plain).map_err(|_| OpenError::Corrupt)?;
    Ok(Some(GitToken::new(token)))
}

/// The token of a fetch of `repo` (KAIROS-T-0431): the read token of the
/// organization for the host of its `repo_url` when there is one, else the
/// read token of the repository, else none.
pub fn fetch_token(
    conn: &mut PgConnection,
    tenant: &str,
    repo: &kairos_db::models::repositories::Repository,
    key: Option<&SecretsKey>,
) -> Result<(Option<GitToken>, TokenSource), TokenError> {
    if let Some(host) = kairos_core::repositories::forge_host(&repo.repo_url)
        && let Some(token) = org_token(conn, tenant, &host, key)?
    {
        return Ok((Some(token), TokenSource::Org { host }));
    }
    Ok(match read_token(conn, tenant, repo.id, key)? {
        Some(token) => (Some(token), TokenSource::Repository),
        None => (None, TokenSource::None),
    })
}

/// The fault of a token that Kairos does not take, as a text for the
/// refusal. The text does not hold the token.
pub fn token_fault(token: &str) -> Option<String> {
    if token.is_empty() {
        return Some("The token is empty. Send the read token.".into());
    }
    if token.len() > MAX_TOKEN_LEN {
        return Some(format!(
            "The token has {} bytes. A token has {MAX_TOKEN_LEN} bytes or fewer.",
            token.len()
        ));
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some(
            "The token has a space, a line break or a control character. Send the token \
             only."
                .into(),
        );
    }
    None
}

/// The wire form of the status of a credential. `None`: no token.
pub fn status_dto(status: Option<&CredentialStatus>) -> dto::RepositoryCredential {
    match status {
        None => dto::RepositoryCredential::default(),
        Some(status) => dto::RepositoryCredential {
            set: true,
            set_by: Some(status.set_by.to_string()),
            set_by_name: status.set_by_name.clone(),
            set_at: Some(status.set_at.to_rfc3339()),
            last_checked_at: status.last_checked_at.map(|at| at.to_rfc3339()),
            last_check_ok: status.last_check_ok,
            last_check_error: status.last_check_error.clone(),
        },
    }
}

/// The write-back of a repository as a read shows it (KAIROS-T-0386).
pub fn write_back_dto(
    status: Option<&kairos_db::writeback::WriteTokenStatus>,
) -> dto::RepositoryWriteBack {
    match status {
        None => dto::RepositoryWriteBack::default(),
        Some(status) => dto::RepositoryWriteBack {
            enabled: true,
            set_by: Some(status.set_by.to_string()),
            set_by_name: status.set_by_name.clone(),
            set_at: Some(status.set_at.to_rfc3339()),
            last_write_at: status.last_write_at.map(|at| at.to_rfc3339()),
            last_write_ok: status.last_write_ok,
            last_write_error: status.last_write_error.clone(),
        },
    }
}

/// The write-back of one repository.
pub fn write_back_of(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<dto::RepositoryWriteBack, diesel::result::Error> {
    let statuses = kairos_db::writeback::token_statuses(conn, &[repository_id])?;
    Ok(write_back_dto(statuses.get(&repository_id)))
}

/// The status of the credential of one repository.
pub fn status_of(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<dto::RepositoryCredential, diesel::result::Error> {
    let statuses = repository_credentials::statuses(conn, &[repository_id])?;
    Ok(status_dto(statuses.get(&repository_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_must_be_one_word_of_text() {
        assert_eq!(token_fault("github_pat_11ABC"), None);
        for bad in ["", "two words", "line\n", "tab\t"] {
            let fault = token_fault(bad).expect(bad);
            if !bad.is_empty() {
                assert!(!fault.contains(bad), "{fault}");
            }
        }
        assert!(token_fault(&"a".repeat(MAX_TOKEN_LEN + 1)).is_some());
    }
}
