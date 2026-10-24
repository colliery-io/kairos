-- KAIROS-T-0681 (KAIROS-A-0065): a request_key makes a retried MCP write
-- safe. One row for each caller and key. The tool writes the row in the
-- transaction of the write, before the write, and stores the answer text
-- in it before the commit. So a row commits only with its write, and a
-- second call with the same key returns the stored answer.
--
--   user_id      The person of the call (`public.users.id`, by
--                documentation only, no foreign key: the same as
--                `activity_log.actor_id`).
--   agent_key    The agent key of the call (`public.api_keys.id`), NULL
--                for OIDC, a password session or a service account.
--   request_key  The key that the client chose.
--   tool         The tool of the first call.
--   fingerprint  SHA-256 of the tool name and the canonical JSON of the
--                arguments without `request_key`.
--   answer       The text of the answer of the first call. NULL only
--                inside the transaction of the first call.
--
-- A key is free again 24 hours after created_at. The retention pass
-- deletes the old rows; a read ignores a row past the window.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new empty table and 2 indexes on it. No row of another table is
-- read or changed, so the migration takes no lock on a large table
-- (KAIROS-A-0062).
CREATE TABLE IF NOT EXISTS request_keys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL,
    agent_key   UUID,
    request_key TEXT NOT NULL,
    tool        TEXT NOT NULL,
    fingerprint BYTEA NOT NULL,
    answer      TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT request_keys_key_check
        CHECK (request_key ~ '^[A-Za-z0-9._:-]{8,128}$')
);

-- One row for each caller and key. A NULL agent key is one caller (the
-- person with no agent key), so the index uses the nil UUID for it.
CREATE UNIQUE INDEX IF NOT EXISTS request_keys_caller_key
    ON request_keys (
        user_id,
        COALESCE(agent_key, '00000000-0000-0000-0000-000000000000'::uuid),
        request_key
    );

-- The retention pass deletes by age.
CREATE INDEX IF NOT EXISTS request_keys_created_at ON request_keys (created_at);
