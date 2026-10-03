-- COLLIERY-T-3100 (COLLIERY-I-0407): retired short codes.
--
--   retired_codes   One row for each short code that an item had and does
--                   not have now (a move with a rename, COLLIERY-T-3101; a
--                   re-import, COLLIERY-T-3103). A read with the code finds
--                   the item, and the answer names its current code.
--
-- `item_id` has no foreign key. An item is a row of one of 5 tables, and an
-- operator can remove an item for real (COLLIERY-T-3103). The code must stay
-- retired after that, so the row stays.
--
-- A retired code is never issued again. kairos_db::items::next_short_code
-- skips it, and the trigger below refuses it on each item table, so a code
-- that is written directly (an import that keeps its codes) cannot take it
-- either. The trigger reads `retired_codes` of the schema of the table, not
-- of the search_path.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty, and 1 trigger on each of the 5 item tables. No row of
-- a table that the tenant has is changed or deleted.
--
-- Re-runnable: each statement has a guard, and the second run changes
-- nothing.
CREATE TABLE IF NOT EXISTS retired_codes (
    code       TEXT PRIMARY KEY CHECK (code ~ '^[A-Z][A-Z0-9]*-[SITDA]-[0-9]{4,}$'),
    item_id    UUID NOT NULL,
    retired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason     TEXT NOT NULL CHECK (length(btrim(reason)) > 0)
);
CREATE INDEX IF NOT EXISTS idx_retired_codes_item ON retired_codes (item_id);

CREATE OR REPLACE FUNCTION refuse_retired_short_code() RETURNS trigger AS $$
DECLARE
    retired BOOLEAN;
BEGIN
    EXECUTE format(
        'SELECT EXISTS (SELECT 1 FROM %I.retired_codes WHERE code = $1)',
        TG_TABLE_SCHEMA
    ) INTO retired USING NEW.short_code;
    IF retired THEN
        RAISE EXCEPTION 'COLLIERY-T-3100: the short code % is retired, and it is never issued again', NEW.short_code
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'retired_short_code';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS strategies_refuse_retired_code ON strategies;
CREATE TRIGGER strategies_refuse_retired_code
    BEFORE INSERT OR UPDATE OF short_code ON strategies
    FOR EACH ROW EXECUTE FUNCTION refuse_retired_short_code();
DROP TRIGGER IF EXISTS initiatives_refuse_retired_code ON initiatives;
CREATE TRIGGER initiatives_refuse_retired_code
    BEFORE INSERT OR UPDATE OF short_code ON initiatives
    FOR EACH ROW EXECUTE FUNCTION refuse_retired_short_code();
DROP TRIGGER IF EXISTS tasks_refuse_retired_code ON tasks;
CREATE TRIGGER tasks_refuse_retired_code
    BEFORE INSERT OR UPDATE OF short_code ON tasks
    FOR EACH ROW EXECUTE FUNCTION refuse_retired_short_code();
DROP TRIGGER IF EXISTS documents_refuse_retired_code ON documents;
CREATE TRIGGER documents_refuse_retired_code
    BEFORE INSERT OR UPDATE OF short_code ON documents
    FOR EACH ROW EXECUTE FUNCTION refuse_retired_short_code();
DROP TRIGGER IF EXISTS adrs_refuse_retired_code ON adrs;
CREATE TRIGGER adrs_refuse_retired_code
    BEFORE INSERT OR UPDATE OF short_code ON adrs
    FOR EACH ROW EXECUTE FUNCTION refuse_retired_short_code();
