-- Drops every local password. Any account that had no OIDC identity becomes
-- unreachable, which is the honest consequence: there is nowhere else the
-- password was stored.
ALTER TABLE users DROP COLUMN IF EXISTS password_hash;
