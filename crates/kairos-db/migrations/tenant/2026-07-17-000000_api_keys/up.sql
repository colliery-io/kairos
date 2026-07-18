-- Service-account API keys (KAIROS-A-0017 / KAIROS-T-0057).
--
-- Tenant-scoped, mirroring `scim_tokens` (A-0016): each org's keys live in
-- its own `org_{slug}` schema, so this DDL is unqualified and runs under a
-- pinned search_path. Only the hex SHA-256 of the full key is stored — the
-- secret (`kairos_sk_<slug>_<hex>`) is shown exactly once at creation and
-- never persisted. `user_id` is the service-account principal
-- (`public.users`, kind='service_account'); no cross-schema FK, matching
-- `board_member_capabilities`.
CREATE TABLE api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL,                     -- the service account (public.users.id)
    name         TEXT NOT NULL,                     -- operator label ("ci-deploy", ...)
    token_hash   TEXT NOT NULL UNIQUE,              -- hex SHA-256 of the full key string
    prefix       TEXT NOT NULL,                     -- display-only leading chars (never the secret)
    created_by   UUID NOT NULL,                     -- the org admin (public.users.id) who minted it
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ,                       -- optional expiry; NULL = no expiry
    last_used_at TIMESTAMPTZ,                       -- best-effort last-seen; NULL until first use
    revoked_at   TIMESTAMPTZ                        -- set on revocation; row retained for audit
);

-- Listing a service account's keys.
CREATE INDEX idx_api_keys_user ON api_keys (user_id);
