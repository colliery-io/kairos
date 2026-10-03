-- COLLIERY-T-3099 (COLLIERY-I-0407): each board has its own short-code
-- prefix, and the numbers come from a sequence for each prefix and type.
--
-- Before this migration, each item of a tenant got the prefix of the tenant
-- (`COLLIERY-T-0012`), and the numbers came from one sequence for each type
-- (`seq_task_code`, ...), shared by the whole tenant.
--
-- THE DATA
--
--   boards.code_prefix     The prefix of the board. It matches
--                          ^[A-Z][A-Z0-9]{1,9}$. An admin sets it when the
--                          board is created, and it does not change.
--   short_code_sequences   One row for each (prefix, type letter): the last
--                          number that was given. The next code of the
--                          pair has the number after it.
--
-- UNIQUENESS: each pair (prefix, type) is unique among the live boards. Each
-- level of board holds different types (strategy: S; initiative: I;
-- delivery: T and D; adr: A), so a unique index on (code_prefix,
-- board_level) of the live boards gives that rule. Boards can thus share a
-- prefix when they hold different types: `initiatives`, `adrs`, `strategy`
-- and `colliery-io-delivery` all have `COLLIERY`. A document of a board of a
-- different level takes the prefix of its board too. It shares the sequence
-- of that prefix, so its code is unique.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- No short code changes. Each board gets a prefix by this rule:
--
--   1. THE TENANT PREFIX is the slug of the tenant in capitals, with only
--      letters and digits, cut to 10 characters (`colliery` -> `COLLIERY`).
--   2. For each level, ONE live board keeps the tenant prefix: the board
--      that holds the most live items of that level (tasks for a delivery
--      board). If two boards hold the same number, the oldest board. Its
--      items have the tenant prefix already, so its sequence continues.
--   3. Each other live board, oldest first, gets a new prefix:
--      a. A delivery board whose tasks came from Metis gets the Metis prefix
--         that occurs most in the footers of its live tasks ("Its Metis code
--         was GQLITE-T-0008."). If two prefixes occur the same number of
--         times, the first in alphabetical order.
--      b. Each other board gets a prefix from its slug: capitals, letters
--         and digits only, no digit at the start, 10 characters at most.
--      c. If a live board of the same level has that prefix, a number goes
--         at the end (`ABC2`, `ABC3`, ...).
--      The existing items of such a board keep their codes. (On the tenant
--      colliery, the tasks of `skadi`, `crt` and `graphqlite` keep their
--      COLLIERY codes until the re-key of COLLIERY-T-3103.)
--   4. A deleted board gets the prefix from its slug. The uniqueness rule
--      is for the live boards only.
--
-- Each row of `short_code_sequences` starts at the highest number of the
-- existing codes with its prefix and type. The rows of the tenant prefix
-- also start at the last value of the old sequence of the type, so no
-- number that the old sequence gave is given again. Each live board gets a
-- row for each type that it holds (0 when no code exists). A later task
-- (COLLIERY-T-3103) can set a row after a re-key.
--
-- The old sequences `seq_*_code` stay. No code reads them now. The down
-- migration moves them past the numbers that were given here.
--
-- Re-runnable: each statement has a guard. The second run finds each board
-- with a prefix and each row of `short_code_sequences` there, and changes
-- nothing.

ALTER TABLE boards ADD COLUMN IF NOT EXISTS code_prefix TEXT;

CREATE TABLE IF NOT EXISTS short_code_sequences (
    code_prefix  TEXT NOT NULL,
    item_type    TEXT NOT NULL CHECK (item_type IN ('S', 'I', 'T', 'D', 'A')),
    last_number  BIGINT NOT NULL DEFAULT 0 CHECK (last_number >= 0),
    PRIMARY KEY (code_prefix, item_type)
);

DO $$
DECLARE
    tenant_prefix TEXT;
    board         RECORD;
    base          TEXT;
    candidate     TEXT;
    number        INTEGER;
BEGIN
    -- 1. The tenant prefix, from the schema `org_<slug>`.
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

    -- 2. For each level, the live board with the most live items keeps the
    -- tenant prefix. Not when a live board of that level has it already
    -- (a second run).
    FOR board IN
        SELECT DISTINCT ON (b.board_level) b.id, b.board_level
          FROM boards b
          LEFT JOIN LATERAL (
                SELECT count(*) AS items
                  FROM (
                        SELECT 1 FROM strategies s
                         WHERE b.board_level = 'strategy' AND s.board_id = b.id
                           AND s.deleted_at IS NULL
                        UNION ALL
                        SELECT 1 FROM initiatives i
                         WHERE b.board_level = 'initiative' AND i.board_id = b.id
                           AND i.deleted_at IS NULL
                        UNION ALL
                        SELECT 1 FROM tasks t
                         WHERE b.board_level = 'delivery' AND t.board_id = b.id
                           AND t.deleted_at IS NULL
                        UNION ALL
                        SELECT 1 FROM adrs a
                         WHERE b.board_level = 'adr' AND a.board_id = b.id
                           AND a.deleted_at IS NULL
                       ) held
               ) counted ON true
         WHERE b.deleted_at IS NULL
           AND b.code_prefix IS NULL
         ORDER BY b.board_level, counted.items DESC, b.created_at, b.id
    LOOP
        IF NOT EXISTS (
            SELECT 1 FROM boards o
             WHERE o.board_level = board.board_level
               AND o.code_prefix = tenant_prefix
               AND o.deleted_at IS NULL
        ) THEN
            UPDATE boards SET code_prefix = tenant_prefix WHERE id = board.id;
        END IF;
    END LOOP;

    -- 3. and 4. Each other board, oldest first.
    FOR board IN
        SELECT b.id, b.slug, b.board_level, b.deleted_at
          FROM boards b
         WHERE b.code_prefix IS NULL
         ORDER BY b.created_at, b.id
    LOOP
        base := NULL;
        IF board.board_level = 'delivery' AND board.deleted_at IS NULL THEN
            SELECT footer.prefix INTO base
              FROM (
                    SELECT substring(t.content FROM
                               'Its Metis code was ([A-Z][A-Z0-9]{1,9})-[STIDA]-[0-9]+') AS prefix
                      FROM tasks t
                     WHERE t.board_id = board.id
                       AND t.deleted_at IS NULL
                   ) footer
             WHERE footer.prefix IS NOT NULL
             GROUP BY footer.prefix
             ORDER BY count(*) DESC, footer.prefix
             LIMIT 1;
        END IF;
        IF base IS NULL THEN
            base := left(ltrim(upper(regexp_replace(board.slug, '[^A-Za-z0-9]', '', 'g')),
                               '0123456789'), 10);
            IF base = '' THEN
                base := 'X';
            END IF;
            IF length(base) < 2 THEN
                base := rpad(base, 2, '0');
            END IF;
        END IF;

        candidate := base;
        IF board.deleted_at IS NULL THEN
            number := 2;
            WHILE EXISTS (
                SELECT 1 FROM boards o
                 WHERE o.board_level = board.board_level
                   AND o.code_prefix = candidate
                   AND o.deleted_at IS NULL
            ) LOOP
                candidate := left(base, 10 - length(number::text)) || number;
                number := number + 1;
            END LOOP;
        END IF;

        UPDATE boards SET code_prefix = candidate WHERE id = board.id;
        RAISE NOTICE 'COLLIERY-T-3099: the board "%" has the prefix %.', board.slug, candidate;
    END LOOP;

    -- The sequences. ON CONFLICT DO NOTHING: a row that is there (a second
    -- run, or a row that a later task set) does not change.
    INSERT INTO short_code_sequences (code_prefix, item_type, last_number)
    SELECT pair.code_prefix, pair.item_type, max(pair.last_number)
      FROM (
            -- The highest number of each (prefix, type) of the existing codes.
            SELECT substring(codes.short_code FROM '^([A-Z][A-Z0-9]*)-[STIDA]-[0-9]+$')
                       AS code_prefix,
                   substring(codes.short_code FROM '^[A-Z][A-Z0-9]*-([STIDA])-[0-9]+$')
                       AS item_type,
                   substring(codes.short_code FROM '-([0-9]+)$')::bigint AS last_number
              FROM (
                    SELECT short_code FROM strategies
                    UNION ALL SELECT short_code FROM initiatives
                    UNION ALL SELECT short_code FROM tasks
                    UNION ALL SELECT short_code FROM documents
                    UNION ALL SELECT short_code FROM adrs
                   ) codes
             WHERE codes.short_code ~ '^[A-Z][A-Z0-9]*-[STIDA]-[0-9]{1,18}$'
            -- The last values of the old sequences, for the tenant prefix.
            UNION ALL
            SELECT tenant_prefix, 'S',
                   CASE WHEN is_called THEN last_value ELSE last_value - 1 END
              FROM seq_strategy_code
            UNION ALL
            SELECT tenant_prefix, 'I',
                   CASE WHEN is_called THEN last_value ELSE last_value - 1 END
              FROM seq_initiative_code
            UNION ALL
            SELECT tenant_prefix, 'T',
                   CASE WHEN is_called THEN last_value ELSE last_value - 1 END
              FROM seq_task_code
            UNION ALL
            SELECT tenant_prefix, 'D',
                   CASE WHEN is_called THEN last_value ELSE last_value - 1 END
              FROM seq_document_code
            UNION ALL
            SELECT tenant_prefix, 'A',
                   CASE WHEN is_called THEN last_value ELSE last_value - 1 END
              FROM seq_adr_code
            -- A row for each type that each live board holds.
            UNION ALL
            SELECT b.code_prefix, held.item_type, 0
              FROM boards b
              JOIN (VALUES ('strategy', 'S'), ('initiative', 'I'), ('delivery', 'T'),
                           ('delivery', 'D'), ('adr', 'A')) AS held(board_level, item_type)
                ON held.board_level = b.board_level
             WHERE b.deleted_at IS NULL
           ) pair
     GROUP BY pair.code_prefix, pair.item_type
        ON CONFLICT (code_prefix, item_type) DO NOTHING;
END
$$;

ALTER TABLE boards ALTER COLUMN code_prefix SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'boards_code_prefix_rule'
           AND conrelid = 'boards'::regclass
    ) THEN
        ALTER TABLE boards
            ADD CONSTRAINT boards_code_prefix_rule
            CHECK (code_prefix ~ '^[A-Z][A-Z0-9]{1,9}$');
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS boards_live_code_prefix_key
    ON boards (code_prefix, board_level)
    WHERE deleted_at IS NULL;
