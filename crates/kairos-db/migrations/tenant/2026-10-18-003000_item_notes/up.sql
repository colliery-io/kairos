-- KAIROS-T-0630 (COLLIERY-I-0650): notes are rows of their own.
--
--   item_notes   One row for each note of an item: its number in the item
--                (`seq`, the n-3 of "note n-3 on ACME-T-0012"), its author,
--                its time, its markdown text, and the item that a promote
--                made from it (`promoted_to`).
--
-- Before this, `append_note` added a `## Note n-3 (date, name)` section to
-- the content of the item, and each note wrote a full copy of the content to
-- item_history. A note now writes one row, and no version of the content.
--
-- `item_id` has no foreign key: an item is a row of one of 5 tables, as for
-- item_history. A purge removes the rows of its item (kairos_db::purge).
-- `author_id` is the user (public.users) that wrote the note. A note that
-- this migration moves gets `author_name` when its name resolves to no user
-- of the tenant.
--
-- THE MOVE OF THE OLD NOTES
--
-- The block below moves each `## Note` section of each item into a row, and
-- removes it from the content. It reads the content as the old parser of
-- kairos_markdown::notes read it (removed in KAIROS-T-0743; the test
-- item_notes_migration holds the expected texts):
--
--   - a line `## Note n-3 (date, name)` or `## Note (date, name)` outside a
--     fenced code block starts a note;
--   - any other `## ` line outside a fence ends the note, and stays in the
--     content. So a note that a stray `## ` heading cut before this
--     migration stays cut: the rest stays in the content.
--
-- The id: a note keeps its id when no earlier note of the item has it. A
-- note without an id has the id of its position (n-1, n-2, ...), as
-- get_item showed it. A note with the id of an earlier note gets the next
-- free number, and a last line in its text names the old id.
--
-- The name: the user of the tenant whose display name, email, user name or
-- id is the name, when exactly one user matches. Else `author_name` keeps
-- the name as written.
--
-- The date: the time is midnight UTC of the date of the heading. A date that
-- is not YYYY-MM-DD gives the time of the item, and a line in the text
-- keeps the date as written.
--
-- A last line `Promoted to CODE.` sets `promoted_to` and leaves the text.
--
-- The move writes no version and no item_history row. The newest history
-- row of an item keeps the notes; the history is of the content.
--
-- Why SQL and not a step of the server at boot: the move runs in the
-- transaction of this migration, on each path that migrates a tenant (boot,
-- `migrate-tenants`, provision). No server reads a schema that has the table
-- but still has notes in the content.
--
-- SEARCH
--
-- item_search_vector gives the vector of an item: the title at weight A,
-- the content at weight B, and the text of the notes at weight B. An item
-- without notes keeps the vector that it had. A trigger on item_notes
-- writes the vector again for each new, changed or removed note.
-- item_notes_markdown gives the notes as `## Note` sections, for the text
-- that the embedding sweep reads.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, 3 functions, 1 trigger, 1 changed function. The content of
-- each item with notes loses its note sections, and its notes become rows.
-- No version changes.
--
-- Re-runnable: each statement has a guard. A second run finds no note in
-- the content, and changes nothing. A note that is in the content and also
-- a row with the same number and text is not written again.
CREATE TABLE IF NOT EXISTS item_notes (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id      UUID NOT NULL,
    seq          INTEGER NOT NULL CHECK (seq > 0),
    author_id    UUID,
    author_name  TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    text         TEXT NOT NULL,
    promoted_to  TEXT,
    agent_key_id UUID DEFAULT NULLIF(current_setting('kairos.agent_key', true), '')::uuid,
    CONSTRAINT item_notes_item_seq UNIQUE (item_id, seq),
    CONSTRAINT item_notes_author CHECK (author_id IS NOT NULL OR author_name IS NOT NULL)
);

-- The vector of an item (KAIROS-T-0624, with the notes). The schema is an
-- argument: a trigger names the schema of its table, not the search_path.
CREATE OR REPLACE FUNCTION item_search_vector(
    p_schema TEXT, p_item UUID, p_title TEXT, p_content TEXT
) RETURNS tsvector AS $$
DECLARE
    notes TEXT;
BEGIN
    EXECUTE format(
        'SELECT string_agg(text, E''\n\n'' ORDER BY seq) FROM %I.item_notes WHERE item_id = $1',
        p_schema
    ) INTO notes USING p_item;
    RETURN setweight(to_tsvector('english', coalesce(p_title, '')), 'A')
        || setweight(to_tsvector('english', coalesce(p_content, '')), 'B')
        || setweight(to_tsvector('english', coalesce(notes, '')), 'B');
END;
$$ LANGUAGE plpgsql STABLE;

-- The trigger function of the 5 item tables (2026-10-17-001500), now with
-- the notes.
CREATE OR REPLACE FUNCTION item_search_write() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        EXECUTE format(
            'DELETE FROM %I.item_search WHERE item_id = $1 AND entity_type = $2',
            TG_TABLE_SCHEMA
        ) USING OLD.id, TG_ARGV[0];
        RETURN OLD;
    END IF;
    EXECUTE format(
        'INSERT INTO %I.item_search (item_id, entity_type, tsv) '
        || 'VALUES ($1, $2, %I.item_search_vector($5, $1, $3, $4)) '
        || 'ON CONFLICT (item_id) DO UPDATE '
        || 'SET entity_type = EXCLUDED.entity_type, tsv = EXCLUDED.tsv',
        TG_TABLE_SCHEMA, TG_TABLE_SCHEMA
    ) USING NEW.id, TG_ARGV[0], NEW.title, NEW.content, TG_TABLE_SCHEMA::text;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- A new, changed or removed note writes the vector of its item again.
CREATE OR REPLACE FUNCTION item_notes_search_write() RETURNS trigger AS $$
DECLARE
    v_item UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        v_item := OLD.item_id;
    ELSE
        v_item := NEW.item_id;
    END IF;
    EXECUTE format(
        'UPDATE %1$I.item_search s '
        || 'SET tsv = %1$I.item_search_vector($2, s.item_id, u.title, u.content) '
        || 'FROM (SELECT title, content FROM %1$I.strategies WHERE id = $1 '
        || 'UNION ALL SELECT title, content FROM %1$I.initiatives WHERE id = $1 '
        || 'UNION ALL SELECT title, content FROM %1$I.tasks WHERE id = $1 '
        || 'UNION ALL SELECT title, content FROM %1$I.documents WHERE id = $1 '
        || 'UNION ALL SELECT title, content FROM %1$I.adrs WHERE id = $1) u '
        || 'WHERE s.item_id = $1',
        TG_TABLE_SCHEMA
    ) USING v_item, TG_TABLE_SCHEMA::text;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- The notes of an item as `## Note n-3 (2026-10-11, Robin)` sections, each
-- after an empty line, in the order of their numbers. The same text as
-- kairos_markdown::notes::render_notes. NULL for an item without notes.
CREATE OR REPLACE FUNCTION item_notes_markdown(p_item UUID) RETURNS TEXT AS $$
    SELECT string_agg(
        E'\n\n## Note n-' || n.seq || ' ('
            || to_char(n.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') || ', '
            || coalesce(nullif(btrim(u.display_name), ''), n.author_name, n.author_id::text)
            || ')'
            || CASE WHEN n.text = '' THEN '' ELSE E'\n\n' || n.text END
            || CASE WHEN n.promoted_to IS NULL THEN ''
                    ELSE E'\n\nPromoted to ' || n.promoted_to || '.' END,
        '' ORDER BY n.seq)
    FROM item_notes n
    LEFT JOIN public.users u ON u.id = n.author_id
    WHERE n.item_id = p_item;
$$ LANGUAGE sql STABLE;

-- The move of the old notes (see the header).
DO $move$
DECLARE
    v_schema   TEXT := current_schema();
    v_table    TEXT;
    v_item     RECORD;
    v_lines    TEXT[];
    v_line     TEXT;
    v_trimmed  TEXT;
    v_match    TEXT[];
    v_head     TEXT;
    v_inside   TEXT;
    v_date     TEXT;
    v_name     TEXT;
    v_in_fence BOOLEAN;
    v_open     BOOLEAN;
    v_exists   BOOLEAN;
    v_keep     TEXT[];
    -- The notes of one item, in order: id, date, name, text.
    v_ids      TEXT[];
    v_dates    TEXT[];
    v_names    TEXT[];
    v_texts    TEXT[];
    v_body     TEXT[];
    v_count    INTEGER;
    v_i        INTEGER;
    v_seq      INTEGER;
    v_next     INTEGER;
    v_used     INTEGER[];
    v_text     TEXT;
    v_promoted TEXT;
    v_last     TEXT;
    v_at       TIMESTAMPTZ;
    v_author   UUID;
    v_authors  UUID[];
    v_moved    INTEGER := 0;
    v_items    INTEGER := 0;
    v_renamed  INTEGER := 0;
BEGIN
    FOREACH v_table IN ARRAY ARRAY['strategies', 'initiatives', 'tasks', 'documents', 'adrs'] LOOP
        FOR v_item IN EXECUTE format(
            'SELECT id, content, created_at FROM %I.%I '
            || 'WHERE content ~ ''##[[:space:]]+Note'' ORDER BY id',
            v_schema, v_table
        ) LOOP
            -- Pass 1: the lines that stay, and the notes.
            v_lines := string_to_array(v_item.content, E'\n');
            v_keep := ARRAY[]::TEXT[];
            v_ids := ARRAY[]::TEXT[];
            v_dates := ARRAY[]::TEXT[];
            v_names := ARRAY[]::TEXT[];
            v_texts := ARRAY[]::TEXT[];
            v_body := ARRAY[]::TEXT[];
            v_in_fence := false;
            v_open := false;
            v_count := 0;
            FOREACH v_line IN ARRAY v_lines LOOP
                v_trimmed := btrim(v_line, E' \t\r\n\f\v');
                IF left(v_trimmed, 3) IN ('```', '~~~') THEN
                    v_in_fence := NOT v_in_fence;
                    IF v_open THEN
                        v_body := v_body || rtrim(v_line, E'\r');
                    ELSE
                        v_keep := v_keep || v_line;
                    END IF;
                    CONTINUE;
                END IF;
                IF NOT v_in_fence AND left(v_trimmed, 3) = '## ' THEN
                    -- Any `## ` heading ends the open note.
                    IF v_open THEN
                        v_texts := v_texts || btrim(array_to_string(v_body, E'\n'), E' \t\r\n\f\v');
                        v_open := false;
                    END IF;
                    v_match := regexp_match(
                        btrim(substr(v_trimmed, 4), E' \t\r\n\f\v'),
                        '^Note([^(]*)\((.*)\)$'
                    );
                    v_head := btrim(v_match[1], E' \t\r\n\f\v');
                    v_inside := v_match[2];
                    IF v_match IS NOT NULL
                        AND (v_head = '' OR v_head ~ '^n-[0-9]+$')
                        AND position(',' IN v_inside) > 0
                    THEN
                        v_date := btrim(split_part(v_inside, ',', 1), E' \t\r\n\f\v');
                        v_name := btrim(substr(v_inside, position(',' IN v_inside) + 1), E' \t\r\n\f\v');
                        IF v_date <> '' AND v_name <> '' THEN
                            v_count := v_count + 1;
                            v_ids := v_ids || CASE WHEN v_head = '' THEN 'n-' || v_count ELSE v_head END;
                            v_dates := v_dates || v_date;
                            v_names := v_names || v_name;
                            v_body := ARRAY[]::TEXT[];
                            v_open := true;
                            CONTINUE;
                        END IF;
                    END IF;
                    v_keep := v_keep || v_line;
                    CONTINUE;
                END IF;
                IF v_open THEN
                    v_body := v_body || rtrim(v_line, E'\r');
                ELSE
                    v_keep := v_keep || v_line;
                END IF;
            END LOOP;
            IF v_open THEN
                v_texts := v_texts || btrim(array_to_string(v_body, E'\n'), E' \t\r\n\f\v');
            END IF;
            CONTINUE WHEN v_count = 0;

            -- Pass 2: the numbers. The next free number is after the
            -- highest id of the content and of the rows of the item.
            EXECUTE format('SELECT coalesce(max(seq), 0) FROM %I.item_notes WHERE item_id = $1', v_schema)
                INTO v_next USING v_item.id;
            FOR v_i IN 1..v_count LOOP
                IF length(v_ids[v_i]) <= 11 THEN
                    v_next := greatest(v_next, substr(v_ids[v_i], 3)::INTEGER);
                END IF;
            END LOOP;
            v_used := ARRAY[]::INTEGER[];
            FOR v_i IN 1..v_count LOOP
                v_text := v_texts[v_i];
                -- A last line `Promoted to CODE.` is the promote mark.
                v_promoted := NULL;
                v_last := btrim(regexp_replace(v_text, '^.*\n', ''), E' \t\r\n\f\v');
                IF v_last ~ '^Promoted to [A-Za-z0-9-]+\.$' THEN
                    v_promoted := substr(v_last, 13, length(v_last) - 13);
                    v_text := btrim(regexp_replace(v_text, '[^\n]*$', ''), E' \t\r\n\f\v');
                END IF;
                v_seq := CASE WHEN length(v_ids[v_i]) <= 11
                              THEN substr(v_ids[v_i], 3)::INTEGER END;
                -- The same note as a row already: a second run.
                IF v_seq IS NOT NULL AND NOT v_seq = ANY (v_used) THEN
                    EXECUTE format(
                        'SELECT EXISTS (SELECT 1 FROM %I.item_notes '
                        || 'WHERE item_id = $1 AND seq = $2 AND text = $3)',
                        v_schema
                    ) INTO v_exists USING v_item.id, v_seq, v_text;
                    IF v_exists THEN
                        v_used := v_used || v_seq;
                        CONTINUE;
                    END IF;
                END IF;
                -- A number that an earlier note has, or a row has.
                IF v_seq IS NOT NULL AND NOT v_seq = ANY (v_used) THEN
                    EXECUTE format(
                        'SELECT EXISTS (SELECT 1 FROM %I.item_notes WHERE item_id = $1 AND seq = $2)',
                        v_schema
                    ) INTO v_exists USING v_item.id, v_seq;
                    IF v_exists THEN
                        v_seq := NULL;
                    END IF;
                ELSE
                    v_seq := NULL;
                END IF;
                IF v_seq IS NULL THEN
                    v_next := v_next + 1;
                    v_seq := v_next;
                    v_text := v_text
                        || CASE WHEN v_text = '' THEN '' ELSE E'\n\n' END
                        || 'Kairos changed the id of this note from ' || v_ids[v_i]
                        || ' to n-' || v_seq || ': an earlier note has the id '
                        || v_ids[v_i] || '.';
                    v_renamed := v_renamed + 1;
                END IF;
                v_used := v_used || v_seq;
                -- The time of the note.
                v_at := NULL;
                IF v_dates[v_i] ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' THEN
                    BEGIN
                        v_at := (v_dates[v_i] || ' 00:00:00+00')::TIMESTAMPTZ;
                    EXCEPTION WHEN others THEN
                        v_at := NULL;
                    END;
                END IF;
                IF v_at IS NULL THEN
                    v_at := v_item.created_at;
                    v_text := v_text
                        || CASE WHEN v_text = '' THEN '' ELSE E'\n\n' END
                        || 'The heading of this note gave the date "' || v_dates[v_i] || '".';
                END IF;
                -- The author: one user of the tenant, or the name.
                SELECT array_agg(DISTINCT u.id) INTO v_authors
                  FROM public.users u
                  JOIN public.organization_members m ON m.user_id = u.id
                  JOIN public.organizations o ON o.id = m.organization_id
                 WHERE 'org_' || o.slug = v_schema
                   AND (u.display_name = v_names[v_i] OR u.email = v_names[v_i]
                        OR u.user_name = v_names[v_i] OR u.id::text = v_names[v_i]);
                v_author := CASE WHEN cardinality(v_authors) = 1 THEN v_authors[1] END;
                EXECUTE format(
                    'INSERT INTO %I.item_notes '
                    || '(item_id, seq, author_id, author_name, created_at, text, promoted_to, agent_key_id) '
                    || 'VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)',
                    v_schema
                ) USING v_item.id, v_seq, v_author,
                        CASE WHEN v_author IS NULL THEN v_names[v_i] END,
                        v_at, v_text, v_promoted;
                v_moved := v_moved + 1;
            END LOOP;

            -- The content without the notes, and with no blank lines at
            -- the end. No version, no history row.
            EXECUTE format('UPDATE %I.%I SET content = $1 WHERE id = $2', v_schema, v_table)
                USING rtrim(array_to_string(v_keep, E'\n'), E' \t\r\n\f\v'), v_item.id;
            v_items := v_items + 1;
        END LOOP;
    END LOOP;
    RAISE NOTICE 'KAIROS-T-0630: % notes of % items moved to item_notes in %, % with a new id',
        v_moved, v_items, v_schema, v_renamed;
END
$move$;

-- After the move, so that the move rewrites each vector once (the update of
-- the content).
DROP TRIGGER IF EXISTS item_notes_search ON item_notes;
CREATE TRIGGER item_notes_search
    AFTER INSERT OR DELETE OR UPDATE OF text ON item_notes
    FOR EACH ROW EXECUTE FUNCTION item_notes_search_write();
