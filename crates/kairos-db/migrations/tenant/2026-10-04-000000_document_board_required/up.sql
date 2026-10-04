-- COLLIERY-T-3109 (COLLIERY-I-0407): each document has an owner board.
--
-- Dylan decided on 2026-10-03: a document must have an owner board, and
-- there is no default. A create with no board is refused, and the owner
-- board of a document cannot be removed. The code of a document has the
-- prefix of its owner board (COLLIERY-T-3099). So `documents.board_id`
-- becomes NOT NULL.
--
-- Until now a document could name no board. Its authorization board was
-- then the board of its earliest `supports` parent (COLLIERY-T-0269), and
-- its code had the prefix of the tenant.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- No row changes. The migration does not choose an owner for a document:
-- that is a decision of the tenant, and the board decides the prefix of
-- the code. So when one or more documents have no board, the migration
-- refuses, names them, and changes nothing. The operator gives each one a
-- board (PATCH /api/documents/{code}/board, with `rename` when the code
-- must get the prefix of the board) and runs the migration again.
--
-- ARCHIVED DOCUMENTS COUNT. A NOT NULL column holds each row, live or
-- archived, and a restore makes an archived document live again. An
-- archived document with no board also stops the migration: restore it,
-- give it a board, and archive it again (a write to an archived item is
-- refused, so the board cannot change while it is archived). The migration
-- does not take the board of the earliest `supports` parent for it: the
-- tenant decides each owner board, as it did for the live documents.
--
-- Re-runnable: SET NOT NULL on a column that is NOT NULL changes nothing.
DO $$
DECLARE
    missing BIGINT;
    archived BIGINT;
    codes TEXT;
BEGIN
    SELECT count(*), count(*) FILTER (WHERE deleted_at IS NOT NULL)
      INTO missing, archived
      FROM documents
     WHERE board_id IS NULL;
    IF missing > 0 THEN
        SELECT string_agg(
                   short_code || CASE WHEN deleted_at IS NOT NULL THEN ' (archived)' ELSE '' END,
                   ', ' ORDER BY short_code)
          INTO codes
          FROM (SELECT short_code, deleted_at
                  FROM documents
                 WHERE board_id IS NULL
                 ORDER BY short_code
                 LIMIT 50) AS first_codes;
        RAISE EXCEPTION
            'COLLIERY-T-3109: % documents have no owner board (% of them archived). Each document must have an owner board. Give each one a board with PATCH /api/documents/{code}/board. Restore an archived document first, and archive it again after. Then run the migration again. The first documents with no board: %',
            missing, archived, codes;
    END IF;
END
$$;

ALTER TABLE documents ALTER COLUMN board_id SET NOT NULL;
