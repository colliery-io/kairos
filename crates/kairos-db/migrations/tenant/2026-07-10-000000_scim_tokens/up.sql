-- SCIM bearer tokens (KAIROS-T-0025, contract per KAIROS-A-0016).
--
-- Tenant-scoped by design: SCIM traffic is "tenant-scoped by the token,
-- not by subdomain" (A-0016), so each org's tokens live in its own
-- `org_{slug}` schema. Only the SHA-256 hash of the token is stored — the
-- secret (`kairos_scim_<slug>_<64-hex>`) is shown exactly once at creation
-- and never persisted.
CREATE TABLE scim_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,                    -- operator label ("okta-prod", ...)
    token_hash      TEXT NOT NULL UNIQUE,             -- hex SHA-256 of the full token string
    created_by      UUID NOT NULL,                    -- references public.users (org admin)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ                       -- set on revocation; row retained for audit
);
