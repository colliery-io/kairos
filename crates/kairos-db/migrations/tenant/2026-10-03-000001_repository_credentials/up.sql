-- COLLIERY-T-3105 (COLLIERY-I-0264): the read token of a repository, for
-- the builder of the base code index. Kairos keeps the token encrypted.
--
--   repository_credentials   One row for each repository that has a token.
--                            The row holds the AES-256-GCM ciphertext of
--                            the token, never the token. The key is the
--                            deployment setting KAIROS_SECRETS_KEY; it is
--                            not in the database.
--
-- `ciphertext` holds the encrypted token and the tag of 16 bytes. `nonce` is
-- 12 random bytes, new for each write. The associated data of the
-- encryption is the tenant slug and the repository id, so a ciphertext that
-- is copied to a different row does not decrypt. `key_id` is a short
-- fingerprint of the key that encrypted the row: a changed key gives a clear
-- error, not a failed decryption. `set_by` is a user of public.users; it has
-- no foreign key, as `created_by` of `repositories` has none.
--
-- A delete of a repository only sets `deleted_at`, so the delete of the
-- repository row (the cascade) does not occur in normal use. The code that
-- deletes a repository deletes its credential too.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty. No row of a table that the tenant has is changed or
-- deleted.
--
-- Re-runnable: the statement has a guard, and the second run changes
-- nothing.
CREATE TABLE IF NOT EXISTS repository_credentials (
    repository_id    UUID PRIMARY KEY REFERENCES repositories (id) ON DELETE CASCADE,
    ciphertext       BYTEA NOT NULL CHECK (octet_length(ciphertext) > 16),
    nonce            BYTEA NOT NULL CHECK (octet_length(nonce) = 12),
    key_id           TEXT NOT NULL,
    set_by           UUID NOT NULL,
    set_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_checked_at  TIMESTAMPTZ,
    last_check_ok    BOOLEAN,
    last_check_error TEXT
);
