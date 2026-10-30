-- KAIROS-T-0731 (COLLIERY-I-0664, decision 3): the check of the typed
-- sections of each document, stored.
--
--   section_checks   One row for each document whose template has typed
--                    sections and that the server checked. `violations` is
--                    the number of problems (0: the document passes);
--                    `failures` lists each section that does not pass, as
--                    JSON. `version` is the version of the document that
--                    the server checked. `stale`: the template sections or
--                    a dictionary changed, and the periodic pass
--                    `section-checks` checks the document again.
--
-- Each content write of a document writes its row. The periodic pass
-- writes the rows of the documents that no write checked (the old
-- documents). Nothing is refused: the row is a mark.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty. No row of another table changes.
--
-- The foreign key to documents is added NOT VALID, so the ALTER does not
-- read documents. The next migration validates it (KAIROS-A-0062).
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS section_checks (
    document_id UUID PRIMARY KEY,
    version     INTEGER NOT NULL,
    violations  INTEGER NOT NULL CHECK (violations >= 0),
    failures    JSONB NOT NULL DEFAULT '[]'::jsonb,
    stale       BOOLEAN NOT NULL DEFAULT false,
    checked_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS section_checks_stale_idx ON section_checks (document_id)
    WHERE stale;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'section_checks_document_id_fkey'
           AND conrelid = 'section_checks'::regclass
    ) THEN
        ALTER TABLE section_checks
            ADD CONSTRAINT section_checks_document_id_fkey
            FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
            NOT VALID;
    END IF;
END $$;
