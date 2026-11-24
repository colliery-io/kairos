-- KAIROS-T-0773 (COLLIERY-I-0678): each initiative board that still has the
-- old default flow puts Decompose before Ready:
--
--   old  Discovery → Design → Ready → Decompose → Active → Monitoring → Completed
--   new  Discovery → Design → Decompose → Ready → Active → Monitoring → Completed
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- A board changes only when its columns, in order, and its transitions are
-- the old default exactly. Ready and Decompose swap positions; the
-- transitions Design → Ready, Ready → Decompose and Decompose → Active
-- become Design → Decompose, Decompose → Ready and Ready → Active. Each
-- item stays in its column. A board that differs (someone changed it) is
-- left as it is, and a NOTICE names it.
--
-- Re-runnable: a board with the new order no longer matches.
DO $$
DECLARE
    b RECORD;
    names TEXT[];
    pairs TEXT[];
    c_design UUID; c_ready UUID; c_decompose UUID; c_active UUID;
BEGIN
    FOR b IN SELECT id, slug FROM boards WHERE board_level = 'initiative' LOOP
        SELECT array_agg(name ORDER BY position) INTO names
        FROM board_columns WHERE board_id = b.id;
        SELECT array_agg(f.name || ' -> ' || t.name ORDER BY f.name || ' -> ' || t.name) INTO pairs
        FROM board_transitions x
        JOIN board_columns f ON f.id = x.from_column_id
        JOIN board_columns t ON t.id = x.to_column_id
        WHERE x.board_id = b.id;

        IF names = ARRAY['Discovery','Design','Decompose','Ready','Active','Monitoring','Completed'] THEN
            CONTINUE;  -- the new order already
        END IF;
        IF names IS DISTINCT FROM ARRAY['Discovery','Design','Ready','Decompose','Active','Monitoring','Completed']
           OR pairs IS DISTINCT FROM ARRAY[
               'Active -> Monitoring', 'Decompose -> Active', 'Design -> Ready',
               'Discovery -> Design', 'Monitoring -> Completed', 'Ready -> Decompose']
        THEN
            RAISE NOTICE 'KAIROS-T-0773: initiative board % is not the old default; left as it is', b.slug;
            CONTINUE;
        END IF;

        SELECT id INTO c_design FROM board_columns WHERE board_id = b.id AND name = 'Design';
        SELECT id INTO c_ready FROM board_columns WHERE board_id = b.id AND name = 'Ready';
        SELECT id INTO c_decompose FROM board_columns WHERE board_id = b.id AND name = 'Decompose';
        SELECT id INTO c_active FROM board_columns WHERE board_id = b.id AND name = 'Active';

        -- UNIQUE (board_id, position): swap through a free position.
        UPDATE board_columns SET position = -1, updated_at = now() WHERE id = c_ready;
        UPDATE board_columns SET position = 2, updated_at = now() WHERE id = c_decompose;
        UPDATE board_columns SET position = 3, updated_at = now() WHERE id = c_ready;

        DELETE FROM board_transitions
        WHERE board_id = b.id AND (
            (from_column_id = c_design AND to_column_id = c_ready) OR
            (from_column_id = c_ready AND to_column_id = c_decompose) OR
            (from_column_id = c_decompose AND to_column_id = c_active));
        INSERT INTO board_transitions (board_id, from_column_id, to_column_id) VALUES
            (b.id, c_design, c_decompose),
            (b.id, c_decompose, c_ready),
            (b.id, c_ready, c_active);
    END LOOP;
END $$;
