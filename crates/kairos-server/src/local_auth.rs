//! Local-account passwords and sessions — the pure half (KAIROS-T-0201,
//! KAIROS-I-0018).
//!
//! Kairos required an OIDC issuer to let a person in. This module is the
//! foundation for letting a small deployment do without one: hashing and
//! verifying a password, and minting and parsing the bearer token a successful
//! login returns.
//!
//! # Two hashes, on purpose
//!
//! There are two secrets here and they want **opposite** treatment, which is the
//! single most important thing to understand before changing anything:
//!
//! | Secret | Entropy | Hash | Why |
//! |---|---|---|---|
//! | session token | 32 random bytes | SHA-256 | Brute force is infeasible regardless, so the hash only needs to be one-way and fast — it runs on every authenticated request |
//! | password | whatever a person chose | argon2id | Low entropy, so the hash itself must be expensive enough that guessing a stolen dump is impractical |
//!
//! Making these consistent with each other would be a mistake in one direction
//! or the other: argon2 on every request is a denial-of-service against
//! yourself, and SHA-256 on a password is a dictionary attack waiting for a
//! database leak. `service_accounts::auth` uses SHA-256 for the same reason the
//! session token does.
//!
//! # No I/O
//!
//! Per KAIROS-A-0009 everything here is a function over its arguments, unit
//! tested without a database. Storage lives in `kairos_db::local_auth`.

use argon2::password_hash::phc::PasswordHash;
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use kairos_db::tenant::is_valid_slug;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// The fixed session-token prefix. Distinct from `service_accounts::auth`'s
/// `kairos_sk_` so `is_session_token` and `is_api_key` can never both claim one
/// bearer — asserted in the tests, because the two prefixes differing by a
/// single character is exactly the kind of thing a later rename breaks.
pub const SESSION_PREFIX: &str = "kairos_ss_";

/// 32 random bytes → 64 hex characters, matching the API-key secret length.
pub const SESSION_SECRET_LEN: usize = 64;

/// argon2id parameters (KAIROS-T-0201).
///
/// These are a **deployment trade-off**, not a default to accept unexamined.
/// Too cheap is crackable from a stolen dump; too expensive is a
/// denial-of-service against your own login endpoint, because every attempt —
/// including every wrong one — costs the server this much memory and time.
///
/// Chosen: the OWASP Argon2id recommendation of **19 MiB, 2 iterations, 1 lane**.
/// That configuration is deliberately the one tuned for a *server* handling
/// concurrent logins rather than the higher-memory variants meant for disk
/// encryption. At 19 MiB a burst of ten concurrent login attempts costs about
/// 190 MiB transiently, which the rate limiter in KAIROS-T-0202 is what keeps
/// bounded — the two decisions are related and should move together.
///
/// The full PHC string is stored rather than a bare digest, so these parameters
/// travel with each hash. Raising them later can then re-hash on next successful
/// login instead of invalidating everybody's password.
const ARGON2_M_COST: u32 = 19 * 1024;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

/// Why a password could not be hashed or verified.
///
/// Note what is NOT here: "wrong password" is not an error, it is `Ok(false)`.
/// Conflating the two invites a caller to treat a malformed stored hash as a
/// failed login, which would silently lock a user out rather than alerting
/// anyone.
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    /// The password could not be hashed (argon2 configuration or OS randomness).
    #[error("could not hash password: {0}")]
    Hash(String),
    /// The stored value is not a parseable PHC string — corruption or a manual
    /// edit, never a wrong password.
    #[error("stored password hash is malformed: {0}")]
    MalformedStoredHash(String),
}

fn hasher() -> Argon2<'static> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, None)
        .expect("the compile-time argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a password for storage. Returns a PHC string
/// (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`).
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    // `hash_password` generates the salt itself from the OS CSPRNG in this
    // version of `password-hash`, so there is no salt to pass and no way to get
    // it wrong by reusing one.
    hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| PasswordError::Hash(e.to_string()))
}

/// Verify a password against a stored PHC string.
///
/// `Ok(false)` is a wrong password. An `Err` means the stored hash could not be
/// read at all, which is an operational problem rather than a failed login.
pub fn verify_password(password: &str, stored: &str) -> Result<bool, PasswordError> {
    let parsed =
        PasswordHash::new(stored).map_err(|e| PasswordError::MalformedStoredHash(e.to_string()))?;
    Ok(hasher()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Spend the same work as a real verification, and return false.
///
/// Called when there is no user, or when the user has no password because they
/// are OIDC-only. Without it, "no such account" returns in microseconds while a
/// wrong password takes ~50ms, and that difference **is** an account-enumeration
/// oracle — an attacker learns which addresses exist by timing the endpoint,
/// however careful the response body is.
pub fn verify_against_dummy(password: &str) -> bool {
    // A fixed hash of a value nobody can present. Hashing at the same cost is
    // the point; the comparison always fails.
    const DUMMY: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHlzYWx0eXNhbHR5$\
                         5rMUxPe7VGYCXUP7pDbxhP0H2jpvpqu5yXDgRDN0Nqk";
    matches!(verify_password(password, DUMMY), Ok(true))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a session token for `slug`: `kairos_ss_<slug>_<64-hex>`.
///
/// The tenant is embedded for the same reason an API key embeds it: the bearer
/// then carries its own tenant and does not need one resolved from the `Host`.
pub fn generate_session_token(slug: &str) -> String {
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    format!("{SESSION_PREFIX}{slug}_{}", hex(&secret))
}

/// The value stored at rest: hex SHA-256 of the full token. Correct here and
/// wrong for a password — see the module docs before "fixing" the difference.
pub fn hash_session_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Split a presented token into `(slug, secret)`, or `None` if it is not
/// shaped like one.
pub fn parse_session_token(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix(SESSION_PREFIX)?;
    let (slug, secret) = rest.rsplit_once('_')?;
    let secret_ok = secret.len() == SESSION_SECRET_LEN
        && secret
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (secret_ok && is_valid_slug(slug)).then_some((slug, secret))
}

/// True if the bearer looks like a session token, so `require_auth` takes the
/// session path rather than OIDC. As with API keys, a prefixed bearer is ALWAYS
/// a session attempt — an invalid one 401s rather than falling through.
pub fn is_session_token(token: &str) -> bool {
    token.starts_with(SESSION_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_accounts::auth::is_api_key;

    #[test]
    fn a_password_verifies_against_its_own_hash_and_nothing_else() {
        let stored = hash_password("correct horse battery staple").expect("hash");
        assert!(verify_password("correct horse battery staple", &stored).unwrap());
        assert!(!verify_password("Correct horse battery staple", &stored).unwrap());
        assert!(!verify_password("", &stored).unwrap());
    }

    #[test]
    fn the_stored_hash_carries_its_parameters() {
        // The whole reason for storing PHC rather than a bare digest: raising the
        // cost later can re-hash on next login instead of invalidating everyone.
        let stored = hash_password("whatever").expect("hash");
        assert!(stored.starts_with("$argon2id$v=19$"), "{stored}");
        assert!(stored.contains("m=19456,t=2,p=1"), "{stored}");
    }

    #[test]
    fn the_same_password_hashes_differently_every_time() {
        // A per-hash salt. Equal hashes would mean two users with the same
        // password are visibly identical in a dump.
        let a = hash_password("same").expect("hash");
        let b = hash_password("same").expect("hash");
        assert_ne!(a, b);
        assert!(verify_password("same", &a).unwrap());
        assert!(verify_password("same", &b).unwrap());
    }

    #[test]
    fn a_malformed_stored_hash_is_an_error_not_a_wrong_password() {
        // This distinction matters operationally: treating corruption as a failed
        // login locks a user out silently instead of telling anyone.
        for junk in ["", "not-a-phc-string", "$argon2id$garbage"] {
            assert!(
                matches!(
                    verify_password("x", junk),
                    Err(PasswordError::MalformedStoredHash(_))
                ),
                "{junk:?} should not read as a wrong password"
            );
        }
    }

    #[test]
    fn the_dummy_verification_does_work_and_always_fails() {
        // It must parse — a malformed dummy would return Err and skip the work,
        // which silently restores the timing oracle it exists to close.
        assert!(!verify_against_dummy("anything"));
        assert!(
            verify_password("anything", "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHlzYWx0eXNhbHR5$5rMUxPe7VGYCXUP7pDbxhP0H2jpvpqu5yXDgRDN0Nqk").is_ok(),
            "the dummy hash must be a PARSEABLE PHC string, or no work is done"
        );
    }

    #[test]
    fn a_session_token_round_trips() {
        let token = generate_session_token("acme");
        assert!(is_session_token(&token));
        let (slug, secret) = parse_session_token(&token).expect("parses");
        assert_eq!(slug, "acme");
        assert_eq!(secret.len(), SESSION_SECRET_LEN);
    }

    #[test]
    fn session_tokens_are_not_api_keys_and_api_keys_are_not_sessions() {
        // The prefixes differ by one character, so this is exactly the assertion
        // a later rename would break. Both directions, deliberately.
        let session = generate_session_token("acme");
        let key = crate::service_accounts::auth::generate_key("acme");
        assert!(is_session_token(&session) && !is_api_key(&session));
        assert!(is_api_key(&key) && !is_session_token(&key));
        assert!(parse_session_token(&key).is_none());
    }

    #[test]
    fn a_malformed_session_token_does_not_parse() {
        for bad in [
            "kairos_ss_acme_short",
            "kairos_ss_acme_ZZZZ",
            "kairos_ss_BADSLUG_0000000000000000000000000000000000000000000000000000000000000000",
            "kairos_ss_nosecret",
            "bearer-looking-jwt.eyJ.x",
        ] {
            assert!(parse_session_token(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn hashing_a_token_is_stable_and_one_way() {
        let token = generate_session_token("acme");
        assert_eq!(hash_session_token(&token), hash_session_token(&token));
        assert_eq!(hash_session_token(&token).len(), 64);
        assert!(!hash_session_token(&token).contains(&token));
    }
}
