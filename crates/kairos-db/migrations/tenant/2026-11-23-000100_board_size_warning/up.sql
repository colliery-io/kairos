-- KAIROS-T-0779 (COLLIERY-I-0678): the size of an initiative past which
-- Kairos asks whether to split it. A setting of an initiative board.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new column on boards, 15 for each board. Only an initiative board
-- reads it. No other row is changed. boards is small: the CHECK is checked
-- at once.
--
-- Re-runnable.
ALTER TABLE boards ADD COLUMN IF NOT EXISTS size_warning INTEGER NOT NULL DEFAULT 15;
ALTER TABLE boards DROP CONSTRAINT IF EXISTS boards_size_warning_check;
ALTER TABLE boards ADD CONSTRAINT boards_size_warning_check CHECK (size_warning BETWEEN 1 AND 1000);
