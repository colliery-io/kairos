-- KAIROS-T-0779: the setting goes.
ALTER TABLE boards DROP CONSTRAINT IF EXISTS boards_size_warning_check;
ALTER TABLE boards DROP COLUMN IF EXISTS size_warning;
