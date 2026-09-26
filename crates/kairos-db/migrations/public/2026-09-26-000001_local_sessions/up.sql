-- KAIROS-T-0201: sessions minted by a password login (KAIROS-I-0018).
--
-- Deliberately the same shape as `api_keys`, because the validation path is the
-- same shape: hash the presented bearer, look it up, check it is neither expired
-- nor revoked. Only the SHA-256 of the full token is stored, so a leaked dump
-- yields nothing usable — and SHA-256 is correct here precisely because the token
-- is 32 bytes of OS randomness, unlike the password in `users.password_hash`.
--
-- PUBLIC, not tenant-scoped, which is a change from this migration's first draft
-- (KAIROS-T-0203 found it). `api_keys` is tenant-scoped because its key embeds a
-- slug, and it embeds a slug because the script using it has no other way to say
-- which org it means. A session has one: the browser is already at
-- `acme.kairos.example`, so ordinary tenant resolution applies. What a session
-- actually replaces is an OIDC token, and an OIDC token is deployment-wide.
--
-- Two things follow, and they are the reason for the change:
--
--   * One login, not one per org. A person in two organizations logs in once,
--     exactly as they would with an issuer. Membership is still enforced per
--     request by the tenant middleware, so nothing is widened.
--   * Revoking every session a user has is ONE statement. Changing a password
--     must revoke that user's other sessions — otherwise a password change after
--     a suspected compromise leaves the attacker logged in, which is the one
--     thing the user is trying to prevent. Tenant-scoped, that guarantee would
--     have had to fan out across every `org_*` schema the user belongs to, and a
--     guarantee that has to iterate is a guarantee that will one day miss a row.
CREATE TABLE IF NOT EXISTS local_sessions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash    TEXT NOT NULL UNIQUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL,
    last_used_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);

-- Validation is one indexed lookup on every authenticated request, so the hash
-- carries the UNIQUE above, and this covers the revoke-all-for-a-user path that
-- a password change needs.
CREATE INDEX IF NOT EXISTS idx_local_sessions_user ON local_sessions (user_id);
