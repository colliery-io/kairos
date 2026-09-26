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

/// The shortest password Kairos will store (KAIROS-T-0204).
///
/// Twelve, and **no composition rules**. A length floor is the one requirement that
/// reliably buys entropy; "one upper, one digit, one symbol" mostly buys
/// `Password1!`, which is in every wordlist, while making people write the password
/// down. Current NIST guidance says the same: require length, check against known
/// breached values if you can, and stop there.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Check a password is long enough, counting CHARACTERS rather than bytes.
///
/// `.len()` on a `str` is bytes, which would let a 12-byte four-character CJK
/// password through and reject a 11-character ASCII one — the opposite of the
/// intent both times.
///
/// Trailing and leading whitespace is NOT trimmed: it is part of the password a
/// person chose, and trimming it here while not trimming it at login would lock
/// them out of the account they just created.
pub fn validate_password(password: &str) -> Result<(), PasswordTooShort> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(PasswordTooShort {
            minimum: MIN_PASSWORD_LEN,
            got: len,
        });
    }
    Ok(())
}

/// A password below [`MIN_PASSWORD_LEN`].
///
/// Carries the minimum and the length given, so the message can be specific. That
/// is safe here and only here: this is a password being SET by someone who already
/// knows it, not one being guessed.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("password must be at least {minimum} characters ({got} given)")]
pub struct PasswordTooShort {
    /// [`MIN_PASSWORD_LEN`].
    pub minimum: usize,
    /// How many characters were given.
    pub got: usize,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a session token: `kairos_ss_<64-hex>`.
///
/// **No tenant in it**, unlike an API key, and that is the difference worth
/// understanding. A key embeds its slug because the script presenting it has no
/// other way to say which organization it means. A session has one — the browser
/// is already at `acme.kairos.example` — so the token stays a bare secret and the
/// tenant middleware resolves the org as it does for an OIDC token, which is what
/// a session actually stands in for.
///
/// The consequence is the useful one: a person in two organizations logs in once,
/// and revoking every session they hold is a single statement rather than a sweep
/// across schemas (KAIROS-T-0203).
pub fn generate_session_token() -> String {
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    format!("{SESSION_PREFIX}{}", hex(&secret))
}

/// The value stored at rest: hex SHA-256 of the full token. Correct here and
/// wrong for a password — see the module docs before "fixing" the difference.
pub fn hash_session_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// The secret part of a presented token, or `None` if it is not shaped like one.
///
/// Shape is checked before the database is touched, so a bearer that cannot
/// possibly be a session costs no query. It is NOT a security check — the hash
/// lookup is — it is a cheap way to reject noise.
pub fn parse_session_token(token: &str) -> Option<&str> {
    let secret = token.strip_prefix(SESSION_PREFIX)?;
    let ok = secret.len() == SESSION_SECRET_LEN
        && secret
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    ok.then_some(secret)
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
    fn the_password_floor_counts_characters_not_bytes() {
        // 11 characters is short; 12 is not.
        assert!(validate_password("elevenchars").is_err());
        assert!(validate_password("twelvechars!").is_ok());

        // Four CJK characters are 12 BYTES. Counting bytes would accept this and
        // reject an 11-character ASCII password — wrong in both directions.
        let four_cjk = "\u{6f22}\u{5b57}\u{6f22}\u{5b57}";
        assert_eq!(four_cjk.len(), 12, "12 bytes");
        assert_eq!(four_cjk.chars().count(), 4, "4 characters");
        assert!(
            validate_password(four_cjk).is_err(),
            "must count characters"
        );
    }

    #[test]
    fn the_floor_message_is_specific_because_the_setter_knows_the_password() {
        let err = validate_password("short").expect_err("too short");
        assert_eq!(err.minimum, MIN_PASSWORD_LEN);
        assert_eq!(err.got, 5);
        assert!(err.to_string().contains("at least 12"), "{err}");
    }

    #[test]
    fn whitespace_is_part_of_the_password() {
        // Trimming here while not trimming at login would lock someone out of the
        // account they had just created.
        let padded = "  spaces count  ";
        assert!(validate_password(padded).is_ok());
        let hash = hash_password(padded).expect("hash");
        assert!(verify_password(padded, &hash).expect("verify"));
        assert!(
            !verify_password(padded.trim(), &hash).expect("verify"),
            "the trimmed form is a different password"
        );
    }

    #[test]
    fn a_session_token_round_trips() {
        let token = generate_session_token();
        assert!(is_session_token(&token));
        let secret = parse_session_token(&token).expect("parses");
        assert_eq!(secret.len(), SESSION_SECRET_LEN);
        assert!(!token.contains("acme"), "a session carries no tenant");
    }

    #[test]
    fn session_tokens_are_not_api_keys_and_api_keys_are_not_sessions() {
        // The prefixes differ by one character, so this is exactly the assertion
        // a later rename would break. Both directions, deliberately.
        let session = generate_session_token();
        let key = crate::service_accounts::auth::generate_key("acme");
        assert!(is_session_token(&session) && !is_api_key(&session));
        assert!(is_api_key(&key) && !is_session_token(&key));
        assert!(parse_session_token(&key).is_none());
    }

    #[test]
    fn a_malformed_session_token_does_not_parse() {
        for bad in [
            "kairos_ss_short",
            &format!("kairos_ss_{}", "Z".repeat(64)), // non-hex
            &format!("kairos_ss_{}", "a".repeat(63)), // one short
            &format!("kairos_ss_{}", "a".repeat(65)), // one long
            "kairos_ss_",
            "bearer-looking-jwt.eyJ.x",
        ] {
            let bad: &str = bad;
            assert!(parse_session_token(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn hashing_a_token_is_stable_and_one_way() {
        let token = generate_session_token();
        assert_eq!(hash_session_token(&token), hash_session_token(&token));
        assert_eq!(hash_session_token(&token).len(), 64);
        assert!(!hash_session_token(&token).contains(&token));
    }
}
