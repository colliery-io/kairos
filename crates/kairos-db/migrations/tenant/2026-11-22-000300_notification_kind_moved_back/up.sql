-- KAIROS-T-0775 (COLLIERY-I-0678): the notification kind `moved_back`. A
-- person moved an initiative back, and its tasks went back to Backlog.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- No row changes. The CHECK of the kind of notifications is added NOT
-- VALID (KAIROS-A-0062): `notification_kind_moved_back_validate` validates
-- it. notification_settings is small: its CHECK is checked at once.
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided',
     'notice', 'watch', 'moved_back')) NOT VALID;
ALTER TABLE notification_settings DROP CONSTRAINT IF EXISTS notification_settings_kind_check;
ALTER TABLE notification_settings ADD CONSTRAINT notification_settings_kind_check CHECK (kind IN
    ('hand_off', 'blocker_cleared', 'cancelled', 'mention', 'p0_bug', 'delivery_failing',
     'agent_key_revoked', 'blocked_again', 'blocker_cancelled', 'claim_dropped',
     'claim_stale', 'key_expiring', 'proposal_decided',
     'notice', 'watch', 'moved_back'));
