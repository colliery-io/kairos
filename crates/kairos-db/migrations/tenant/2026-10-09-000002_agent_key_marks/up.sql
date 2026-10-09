-- KAIROS-T-0359 (KAIROS-A-0024, decision 3): history and activity mark a
-- change made by an agent.
--
--   activity_log.agent_key_id        the agent key of the request that
--   item_history.agent_key_id        wrote the row (api_keys.id), or NULL.
--   team_page_history.agent_key_id   The actor column still names the
--                                    person: the agent acts as the person.
--
-- The default reads the session setting kairos.agent_key. The server sets
-- it on each checkout of a connection: the id of the agent key of the
-- request, or '' for no agent key (kairos-server, blocking.rs). A
-- connection that never set it (a background job, a migration, psql) gets
-- NULL. So no insert names the column.
--
-- No foreign key: the mark stays when the key is revoked or deleted.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 3 new columns. Each row that is there gets NULL: no row is changed, and
-- the actor of a row stays as it was.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.
ALTER TABLE activity_log
    ADD COLUMN IF NOT EXISTS agent_key_id UUID
    DEFAULT NULLIF(current_setting('kairos.agent_key', true), '')::uuid;
ALTER TABLE item_history
    ADD COLUMN IF NOT EXISTS agent_key_id UUID
    DEFAULT NULLIF(current_setting('kairos.agent_key', true), '')::uuid;
ALTER TABLE team_page_history
    ADD COLUMN IF NOT EXISTS agent_key_id UUID
    DEFAULT NULLIF(current_setting('kairos.agent_key', true), '')::uuid;
