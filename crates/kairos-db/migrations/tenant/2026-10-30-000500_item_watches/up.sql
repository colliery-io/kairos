-- KAIROS-T-0736 (COLLIERY-I-0664, decision 8): a person watches an item,
-- with or without its children.
--
--   item_watches   One watch of one person (`user_id`) on one item
--                  (`item_id`). `with_children`: the children of the item
--                  (its `parent` edges, one level) tell the person too.
--
-- A coarse change of a watched item (moved, edited, note added, done)
-- makes a notification of the new kind `watch`. The CHECK keeps `notice`
-- (KAIROS-T-0737, migration 2026-10-30-000300) that runs before.
--
-- No foreign key on user_id: users are in the public schema. No foreign
-- key on item_id: an item is in one of 5 tables. A purge of an item
-- deletes its watches; an archive keeps them.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty. The CHECK of the kind of the 2 notification tables
-- accepts one more value. No row is changed.
--
-- notifications is a large table, so its CHECK is added NOT VALID. The next
-- migration validates it (KAIROS-A-0062). Each row is valid already: the
-- old list is a part of the new one.
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS item_watches (
    user_id       UUID NOT NULL,
    item_id       UUID NOT NULL,
    with_children BOOLEAN NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id)
);

CREATE INDEX IF NOT EXISTS item_watches_item_idx ON item_watches (item_id);

ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided',
     'notice', 'watch')) NOT VALID;
ALTER TABLE notification_settings DROP CONSTRAINT IF EXISTS notification_settings_kind_check;
ALTER TABLE notification_settings ADD CONSTRAINT notification_settings_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided',
     'notice', 'watch'));
