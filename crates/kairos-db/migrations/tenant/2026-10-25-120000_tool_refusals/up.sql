-- KAIROS-T-0694 (KAIROS-A-0068): the count of the MCP refusals. One row for
-- each tool call that ends in a tool error. The row has no argument value
-- and no person: only what a count by tool and code needs.
--
--   tool         The name of the tool of the call.
--   code         The code of the refusal (VALIDATION, NOT_FOUND, ...).
--   field        The `field` of the details of the refusal, or NULL.
--   caller_kind  person, agent_key or service_account.
--   created_at   When the server refused the call.
--
-- The retention pass deletes the rows older than 90 days.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new empty table and 1 index on it. No row of another table is read or
-- changed, so the migration takes no lock on a large table (KAIROS-A-0062).
CREATE TABLE IF NOT EXISTS tool_refusals (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool        TEXT NOT NULL,
    code        TEXT NOT NULL,
    field       TEXT,
    caller_kind TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT tool_refusals_caller_kind_check
        CHECK (caller_kind IN ('person', 'agent_key', 'service_account'))
);

-- The report and the retention pass read by age.
CREATE INDEX IF NOT EXISTS tool_refusals_created_at ON tool_refusals (created_at);
