-- Removes the prefix of each board and the sequences for each prefix and
-- type. Each item then gets the prefix of the tenant again, with a number
-- from the old sequence of its type.
--
-- First, each old sequence moves past the last number that this migration
-- gave for the tenant prefix, so the old sequence does not give a code
-- that an item has. The codes of the items do not change.

DO $$
DECLARE
    tenant_prefix TEXT;
    pair          RECORD;
BEGIN
    IF to_regclass('short_code_sequences') IS NULL THEN
        RETURN;
    END IF;
    tenant_prefix := upper(regexp_replace(
        regexp_replace(current_schema()::text, '^org_', ''),
        '[^A-Za-z0-9]', '', 'g'));
    tenant_prefix := left(ltrim(tenant_prefix, '0123456789'), 10);
    IF tenant_prefix = '' THEN
        tenant_prefix := 'X';
    END IF;
    IF length(tenant_prefix) < 2 THEN
        tenant_prefix := rpad(tenant_prefix, 2, '0');
    END IF;

    FOR pair IN
        SELECT s.item_type, s.last_number
          FROM short_code_sequences s
         WHERE s.code_prefix = tenant_prefix
           AND s.last_number > 0
    LOOP
        PERFORM setval(
            CASE pair.item_type
                WHEN 'S' THEN 'seq_strategy_code'
                WHEN 'I' THEN 'seq_initiative_code'
                WHEN 'T' THEN 'seq_task_code'
                WHEN 'D' THEN 'seq_document_code'
                ELSE 'seq_adr_code'
            END,
            GREATEST(
                pair.last_number,
                CASE pair.item_type
                    WHEN 'S' THEN (SELECT last_value FROM seq_strategy_code)
                    WHEN 'I' THEN (SELECT last_value FROM seq_initiative_code)
                    WHEN 'T' THEN (SELECT last_value FROM seq_task_code)
                    WHEN 'D' THEN (SELECT last_value FROM seq_document_code)
                    ELSE (SELECT last_value FROM seq_adr_code)
                END
            ));
    END LOOP;
END
$$;

DROP INDEX IF EXISTS boards_live_code_prefix_key;
ALTER TABLE boards DROP CONSTRAINT IF EXISTS boards_code_prefix_rule;
ALTER TABLE boards DROP COLUMN IF EXISTS code_prefix;
DROP TABLE IF EXISTS short_code_sequences;
