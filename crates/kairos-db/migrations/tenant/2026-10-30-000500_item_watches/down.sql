-- KAIROS-T-0736, down. Each watch is lost, and the notifications of the
-- kind watch and the settings of that kind.
DELETE FROM notifications WHERE kind = 'watch';
DELETE FROM notification_settings WHERE kind = 'watch';
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided', 'notice'));
ALTER TABLE notification_settings DROP CONSTRAINT IF EXISTS notification_settings_kind_check;
ALTER TABLE notification_settings ADD CONSTRAINT notification_settings_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided', 'notice'));
DROP TABLE IF EXISTS item_watches;
