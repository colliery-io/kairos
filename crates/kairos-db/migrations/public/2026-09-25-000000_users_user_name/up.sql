-- KAIROS-T-0184 items 1 and 4: `userName` stops being `external_id`.
--
-- Both defects are one root cause. SCIM served `userName` AND `externalId` from
-- the single `users.external_id` column, which is the OIDC `sub` that logins join
-- on (middleware/auth.rs). Conflating them produced:
--
--   1. `userName` had to be immutable, because changing it would change the
--      login key. In the common configuration where `userName` is the login
--      email and `sub` is an opaque id, an IdP has a legitimate reason to send
--      `userName` on every PUT — and got 400 every time, for ever. The sync
--      never converged, and a better error message would not have changed that.
--   4. A `userName eq "..."` filter searched `external_id`, so an IdP filtering
--      by login email against opaque subs found nothing — including during its
--      own reconciliation sweeps, where finding nothing means "absent", which
--      means re-create.
--
-- They are different things in SCIM and are now different columns:
--
--   users.external_id -- the OIDC `sub`. Logins join on it. Still immutable
--                        through SCIM, and that is not a limitation to work
--                        around: re-keying a live identity through a
--                        provisioning PUT would lock the person out.
--   users.user_name   -- SCIM `userName`. Mutable, and what a userName filter
--                        searches.
--
-- Backfilled from `external_id`, so every existing row serves exactly the
-- `userName` it served before this migration and no integration observes a
-- change. JIT login (no SCIM involved) also sets `user_name = sub`, which keeps
-- the value unique for free and keeps today's behaviour for users SCIM never saw.
ALTER TABLE users ADD COLUMN IF NOT EXISTS user_name TEXT;

UPDATE users SET user_name = external_id WHERE user_name IS NULL;

ALTER TABLE users ALTER COLUMN user_name SET NOT NULL;

-- SCIM declares `userName` as `uniqueness: "server"`, and `users` is
-- deployment-wide, so server-wide is the right scope. `external_id` is already
-- UNIQUE, so backfilling from it cannot violate this.
CREATE UNIQUE INDEX IF NOT EXISTS users_user_name_key ON users (user_name);
