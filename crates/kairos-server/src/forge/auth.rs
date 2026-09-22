//! Webhook credential derivation (KAIROS-T-0097, design in
//! KAIROS-I-0009).
//!
//! # Nothing secret is stored
//!
//! GitHub verifies deliveries with `X-Hub-Signature-256` (HMAC-SHA256 of
//! the raw body) and GitLab compares `X-Gitlab-Token` to a shared secret.
//! **Both need the plaintext at verification time**, so the API-key trick
//! of storing only a hash ([`crate::service_accounts::auth`]) does not
//! apply.
//!
//! Rather than keeping recoverable secrets at rest, this module DERIVES
//! them from the deployment signing key and the connection id:
//!
//! ```text
//! secret = hex(HMAC-SHA256(KAIROS_WEBHOOK_SIGNING_KEY, "webhook:" || connection_id))
//! ```
//!
//! The secret is shown exactly once at connection creation (like an API
//! key), recomputed on every delivery, and never persisted. Rotation is
//! therefore a NEW CONNECTION ID — there is no stored value to rotate.
//!
//! Compromising the database alone yields no webhook secrets; compromising
//! the signing key yields all of them, which is the same blast radius as
//! any deployment-wide secret and is documented as such.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The webhook secret for one connection. Deterministic: the same
/// `(signing_key, connection_id)` always yields the same secret, which is
/// what lets verification recompute it without storage.
pub fn derive_secret(signing_key: &str, connection_id: Uuid) -> String {
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(b"webhook:");
    mac.update(connection_id.to_string().as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// GitHub's `X-Hub-Signature-256` value for a body under this secret:
/// `sha256=<hex>`. Used to verify deliveries, and by tests to sign them.
pub fn github_signature(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    format!("sha256={}", hex(&mac.finalize().into_bytes()))
}

/// Constant-time string comparison — never `==` on a credential, so a
/// timing side-channel cannot reveal a prefix.
pub fn secure_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic_and_key_dependent() {
        let id = Uuid::new_v4();
        assert_eq!(derive_secret("key-a", id), derive_secret("key-a", id));
        // A different key or a different connection yields a different
        // secret — the property rotation-by-new-id depends on.
        assert_ne!(derive_secret("key-a", id), derive_secret("key-b", id));
        assert_ne!(
            derive_secret("key-a", id),
            derive_secret("key-a", Uuid::new_v4())
        );
        // 32 bytes hex-encoded.
        assert_eq!(derive_secret("key-a", id).len(), 64);
    }

    #[test]
    fn github_signature_matches_the_documented_shape() {
        let sig = github_signature("secret", b"{}");
        assert!(sig.starts_with("sha256="));
        assert_eq!(sig.len(), "sha256=".len() + 64);
        // Body-sensitive.
        assert_ne!(sig, github_signature("secret", b"{ }"));
        assert_ne!(sig, github_signature("other", b"{}"));
    }

    #[test]
    fn secure_eq_matches_equality_semantics() {
        assert!(secure_eq("abc", "abc"));
        assert!(!secure_eq("abc", "abd"));
        assert!(!secure_eq("abc", "ab"));
        assert!(secure_eq("", ""));
    }
}
