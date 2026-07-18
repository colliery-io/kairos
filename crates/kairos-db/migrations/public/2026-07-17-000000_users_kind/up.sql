-- Service-account principals (KAIROS-A-0017 / KAIROS-T-0057).
--
-- `kind` distinguishes ordinary OIDC-backed humans from native service
-- accounts (machine principals authenticated by an API key, not by login).
-- Existing rows are humans. A service account is a `users` row with
-- kind='service_account' and a synthetic `external_id` ('svc:<uuid>') that
-- can never collide with a real OIDC subject.
ALTER TABLE users
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'human'
        CHECK (kind IN ('human', 'service_account'));
