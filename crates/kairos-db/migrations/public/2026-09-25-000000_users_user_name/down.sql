-- Drops the column, which returns SCIM to serving `userName` from
-- `external_id` — and returns both defects with it. Any `userName` an IdP
-- changed since the up migration ran is lost, because there is nowhere else it
-- was ever stored.
DROP INDEX IF EXISTS users_user_name_key;
ALTER TABLE users DROP COLUMN IF EXISTS user_name;
