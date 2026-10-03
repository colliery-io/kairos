//! The read token of a repository (COLLIERY-T-3105): the encryption, the
//! decryption, and the status that a read shows. The storage is
//! [`kairos_db::repository_credentials`]; the routes are
//! [`crate::api::org::repository_credentials`].
//!
//! The token is write-only. No surface gives it back: a read gives
//! [`status_dto`] (set or not, who set it and when, the last access check).
//! Only the builder of the base code index decrypts it ([`read_token`]), to
//! give it to git ([`crate::code_index::git`]).

use diesel::pg::PgConnection;
use kairos_client::types_repositories as dto;
use kairos_db::repository_credentials::{self, CredentialStatus};
use uuid::Uuid;

use crate::code_index::git::GitToken;
use crate::secrets::{OpenError, SecretsKey, credential_aad};

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
    /// The token does not decrypt.
    #[error(transparent)]
    Open(#[from] OpenError),
    /// The database failed.
    #[error("The database gave an error: {0}.")]
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

/// The fault of a token that Kairos does not take, as a text for the
/// refusal. The text does not hold the token.
pub fn token_fault(token: &str) -> Option<String> {
    if token.is_empty() {
        return Some("The token is empty. Send the read token of the repository.".into());
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
