-- KAIROS-T-0359, down. The marks of the changes made by an agent are lost;
-- the actor of each row stays.
ALTER TABLE team_page_history DROP COLUMN IF EXISTS agent_key_id;
ALTER TABLE item_history DROP COLUMN IF EXISTS agent_key_id;
ALTER TABLE activity_log DROP COLUMN IF EXISTS agent_key_id;
