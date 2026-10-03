//! The encryption of the secrets that Kairos keeps in the database
//! (COLLIERY-T-3105): today, the read token of a repository.
//!
//! AES-256-GCM (`ring`), with the key of the deployment setting
//! `KAIROS_SECRETS_KEY` (32 bytes, base64). Each encryption has a new random
//! nonce of 12 bytes. The caller gives associated data that names the row
//! (for a token: the tenant slug and the repository id), so a ciphertext
//! that is copied to a different row does not decrypt.
//!
//! Each row also keeps the [`SecretsKey::id`] of the key that encrypted it: a
//! short fingerprint, not the key. When the deployment has a different key
//! now, [`SecretsKey::open`] says so ([`OpenError::OtherKey`]), and the
//! operator knows to set the secret again. A decryption that fails with the
//! same key is [`OpenError::Corrupt`]: the row was changed or moved.

use base64::Engine as _;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The name of the setting, for the texts that name it.
pub const SECRETS_KEY_VAR: &str = "KAIROS_SECRETS_KEY";

/// The key of `KAIROS_SECRETS_KEY`. Its `Debug` shows only the fingerprint.
#[derive(Clone)]
pub struct SecretsKey {
    key: [u8; 32],
    id: String,
}

impl std::fmt::Debug for SecretsKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsKey").field("id", &self.id).finish()
    }
}

/// A secret after [`SecretsKey::seal`].
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    /// The ciphertext and the tag of 16 bytes.
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    /// The [`SecretsKey::id`] of the key.
    pub key_id: String,
}

/// Why [`SecretsKey::open`] gave no secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// A different key encrypted the secret.
    #[error("The token was set with a different KAIROS_SECRETS_KEY. Set the token again.")]
    OtherKey,
    /// The key is the same, but the ciphertext does not decrypt: the row was
    /// changed, or it is the row of a different repository.
    #[error("The stored token does not decrypt. Set the token again.")]
    Corrupt,
}

impl SecretsKey {
    /// The key from its base64 text. The error says what is wrong, with no
    /// part of the value.
    pub fn from_base64(raw: &str) -> Result<Self, String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .map_err(|_| {
                "it is not base64. Make a key with `openssl rand -base64 32`".to_string()
            })?;
        let key: [u8; 32] = bytes.try_into().map_err(|bytes: Vec<u8>| {
            format!(
                "it has {} bytes, and it must have 32 bytes. Make a key with `openssl rand \
                 -base64 32`",
                bytes.len()
            )
        })?;
        Ok(Self::from_bytes(key))
    }

    /// The key from its 32 bytes.
    pub fn from_bytes(key: [u8; 32]) -> Self {
        let digest = Sha256::new()
            .chain_update(b"kairos-secrets-key-id\0")
            .chain_update(key)
            .finalize();
        let id = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
        Self { key, id }
    }

    /// The fingerprint of the key: 12 hex characters. It does not tell the
    /// key.
    pub fn id(&self) -> &str {
        &self.id
    }

    fn cipher(&self) -> LessSafeKey {
        // A key of 32 bytes is always a correct AES-256 key.
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &self.key).expect("a 32-byte AES key"))
    }

    /// Encrypt `secret`, bound to `aad`.
    pub fn seal(&self, aad: &[u8], secret: &[u8]) -> Sealed {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .expect("the system random source works");
        let mut in_out = secret.to_vec();
        self.cipher()
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                &mut in_out,
            )
            .expect("AES-GCM encrypts a short secret");
        Sealed {
            ciphertext: in_out,
            nonce: nonce.to_vec(),
            key_id: self.id.clone(),
        }
    }

    /// Decrypt a secret that [`Self::seal`] encrypted with `aad`.
    pub fn open(
        &self,
        aad: &[u8],
        key_id: &str,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, OpenError> {
        if key_id != self.id {
            return Err(OpenError::OtherKey);
        }
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| OpenError::Corrupt)?;
        let mut in_out = ciphertext.to_vec();
        let plain = self
            .cipher()
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                &mut in_out,
            )
            .map_err(|_| OpenError::Corrupt)?;
        Ok(plain.to_vec())
    }
}

/// The associated data of the read token of a repository: the tenant and
/// the repository, so that the ciphertext decrypts only in its own row.
pub fn credential_aad(tenant: &str, repository_id: Uuid) -> Vec<u8> {
    format!("kairos/repository-credential/v1\0{tenant}\0{repository_id}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> SecretsKey {
        SecretsKey::from_bytes([byte; 32])
    }

    #[test]
    fn a_sealed_secret_opens_with_its_key_and_its_row() {
        let k = key(7);
        let repo = Uuid::new_v4();
        let aad = credential_aad("acme", repo);
        let sealed = k.seal(&aad, b"github_pat_secret");
        assert_eq!(sealed.key_id, k.id());
        assert_eq!(sealed.nonce.len(), 12);
        assert!(
            !sealed
                .ciphertext
                .windows(b"github_pat_secret".len())
                .any(|w| w == b"github_pat_secret"),
            "the ciphertext must not hold the secret"
        );
        let opened = k
            .open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext)
            .expect("opens");
        assert_eq!(opened, b"github_pat_secret");
        // A new nonce for each seal.
        assert_ne!(k.seal(&aad, b"github_pat_secret").nonce, sealed.nonce);
    }

    #[test]
    fn a_secret_of_a_different_row_or_tenant_does_not_open() {
        let k = key(7);
        let repo = Uuid::new_v4();
        let sealed = k.seal(&credential_aad("acme", repo), b"tok");
        let other_repo = credential_aad("acme", Uuid::new_v4());
        let other_tenant = credential_aad("globex", repo);
        for aad in [other_repo, other_tenant] {
            assert_eq!(
                k.open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext),
                Err(OpenError::Corrupt)
            );
        }
    }

    #[test]
    fn a_changed_key_says_set_it_again() {
        let repo = Uuid::new_v4();
        let aad = credential_aad("acme", repo);
        let sealed = key(7).seal(&aad, b"tok");
        let err = key(8)
            .open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext)
            .unwrap_err();
        assert_eq!(err, OpenError::OtherKey);
        assert!(err.to_string().contains("Set the token again"), "{err}");
        assert!(err.to_string().contains("KAIROS_SECRETS_KEY"), "{err}");
    }

    #[test]
    fn the_key_is_32_bytes_of_base64() {
        let good = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
        let k = SecretsKey::from_base64(&format!(" {good}\n")).expect("good key");
        assert_eq!(k.id().len(), 12);
        assert_eq!(k.id(), SecretsKey::from_bytes([1u8; 32]).id());
        assert_ne!(k.id(), key(2).id());
        let short = base64::engine::general_purpose::STANDARD.encode([1u8; 16]);
        let err = SecretsKey::from_base64(&short).unwrap_err();
        assert!(err.contains("16 bytes"), "{err}");
        let err = SecretsKey::from_base64("not base64!").unwrap_err();
        assert!(err.contains("not base64"), "{err}");
        // The debug text has no key bytes.
        assert!(!format!("{k:?}").contains("[1, 1"), "{k:?}");
    }
}
