-- Dropping the extension would drop every `vector` column with it (CASCADE) or
-- fail (RESTRICT) while the tenant embedding tables exist. Either is worse than
-- leaving an unused extension installed, and `down` is expected to be safe.
-- The tenant tables' own down migration removes the columns; the extension
-- itself is left in place deliberately.
--
-- DROP EXTENSION IF EXISTS vector;
SELECT 1;
