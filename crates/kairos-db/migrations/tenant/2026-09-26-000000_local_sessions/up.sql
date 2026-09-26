-- KAIROS-T-0201: sessions minted by a password login (KAIROS-I-0018).
--
-- Deliberately the same shape as `api_keys`, because the validation path is the
-- same shape: hash the presented bearer, look it up, check it is neither expired
-- nor revoked. Only the SHA-256 of the full token is stored, so a leaked dump
-- yields nothing usable — and SHA-256 is correct here precisely because the token
-- is 32 bytes of OS randomness, unlike the password in `users.password_hash`.
--
-- Tenant-scoped, again like `api_keys`: the token embeds its tenant, so it
-- carries its own scope rather than relying on a Host header to supply one.
CREATE TABLE IF NOT EXISTS local_sessions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL,
    last_used_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);

-- Validation is one indexed lookup on every authenticated request, so the hash
-- carries the UNIQUE above and this covers the revoke-all-for-a-user path that
-- a password change needs.
CREATE INDEX IF NOT EXISTS idx_local_sessions_user ON local_sessions (user_id);
