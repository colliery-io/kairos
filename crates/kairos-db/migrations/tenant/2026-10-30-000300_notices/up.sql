-- KAIROS-T-0737 (COLLIERY-I-0664, decision 9): notices. A person, or an
-- agent key of a person, sends a notice to a person or to each member of a
-- team, with an optional item link.
--
--   notices        One row for each notice that was sent: the sender, the
--                  agent key (NULL for a person with no key), the target
--                  (a person or a team), the item, the text. The rate limit
--                  counts these rows.
--   notice_mutes   The agent keys that a person muted. A notice from a
--                  muted key, or from a key that replaced a muted key by a
--                  rotation, makes no notification for that person.
--
-- notifications.agent_key_id: the agent key that made the notification
-- (NULL: a person, or Kairos). The bell names the key.
--
-- No foreign key to users: users are in the public schema (the rule of
-- 2026-10-09-000003_task_claims).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 2 new tables, empty. notifications gets one column with no default, so
-- Postgres writes no row (a change of the catalog only). The CHECK of the
-- kind of the 2 notification tables accepts one more value; no row is
-- changed. notifications is a large table, so its CHECK is added NOT VALID.
-- The next migration validates it (KAIROS-A-0062). Each row is valid
-- already: the old list is a part of the new one.
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS notices (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sender_id    UUID NOT NULL,
    agent_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    to_user_id   UUID,
    to_team_id   UUID REFERENCES teams(id) ON DELETE SET NULL,
    item_id      UUID,
    text         TEXT NOT NULL,
    told         INTEGER NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS notices_key_created ON notices (agent_key_id, created_at);
CREATE INDEX IF NOT EXISTS notices_sender_created ON notices (sender_id, created_at);

CREATE TABLE IF NOT EXISTS notice_mutes (
    user_id    UUID NOT NULL,
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, api_key_id)
);

ALTER TABLE notifications ADD COLUMN IF NOT EXISTS agent_key_id UUID;

ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided', 'notice')) NOT VALID;
ALTER TABLE notification_settings DROP CONSTRAINT IF EXISTS notification_settings_kind_check;
ALTER TABLE notification_settings ADD CONSTRAINT notification_settings_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided', 'notice'));
