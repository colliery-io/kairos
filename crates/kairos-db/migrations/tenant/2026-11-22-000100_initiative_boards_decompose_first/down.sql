-- KAIROS-T-0773: back to Ready before Decompose, on each initiative board
-- that has the new default flow exactly. Other boards stay.
DO $$
DECLARE
    b RECORD;
    names TEXT[];
    pairs TEXT[];
    c_design UUID; c_ready UUID; c_decompose UUID; c_active UUID;
BEGIN
    FOR b IN SELECT id FROM boards WHERE board_level = 'initiative' LOOP
        SELECT array_agg(name ORDER BY position) INTO names
        FROM board_columns WHERE board_id = b.id;
        SELECT array_agg(f.name || ' -> ' || t.name ORDER BY f.name || ' -> ' || t.name) INTO pairs
        FROM board_transitions x
        JOIN board_columns f ON f.id = x.from_column_id
        JOIN board_columns t ON t.id = x.to_column_id
        WHERE x.board_id = b.id;
        IF names IS DISTINCT FROM ARRAY['Discovery','Design','Decompose','Ready','Active','Monitoring','Completed']
           OR pairs IS DISTINCT FROM ARRAY[
               'Active -> Monitoring', 'Decompose -> Ready', 'Design -> Decompose',
               'Discovery -> Design', 'Monitoring -> Completed', 'Ready -> Active']
        THEN
            CONTINUE;
        END IF;
        SELECT id INTO c_design FROM board_columns WHERE board_id = b.id AND name = 'Design';
        SELECT id INTO c_ready FROM board_columns WHERE board_id = b.id AND name = 'Ready';
        SELECT id INTO c_decompose FROM board_columns WHERE board_id = b.id AND name = 'Decompose';
        SELECT id INTO c_active FROM board_columns WHERE board_id = b.id AND name = 'Active';
        UPDATE board_columns SET position = -1, updated_at = now() WHERE id = c_decompose;
        UPDATE board_columns SET position = 2, updated_at = now() WHERE id = c_ready;
        UPDATE board_columns SET position = 3, updated_at = now() WHERE id = c_decompose;
        DELETE FROM board_transitions
        WHERE board_id = b.id AND (
            (from_column_id = c_design AND to_column_id = c_decompose) OR
            (from_column_id = c_decompose AND to_column_id = c_ready) OR
            (from_column_id = c_ready AND to_column_id = c_active));
        INSERT INTO board_transitions (board_id, from_column_id, to_column_id) VALUES
            (b.id, c_design, c_ready),
            (b.id, c_ready, c_decompose),
            (b.id, c_decompose, c_active);
    END LOOP;
END $$;
