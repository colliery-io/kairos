-- KAIROS-T-0720 (COLLIERY-I-0662): a dispatch rule fires when the claim of a
-- task is released (the trigger `claim_released`): a release, or the end of
-- the claim on a move out of the claims column. Not a hand-off. The
-- delivery carries the live state of the task. The rule can name a column
-- that the task must be in, as an `unblocked` rule can.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- The CHECK of dispatch_rules.trigger allows one more value. No row is
-- changed.
--
-- Re-runnable: each statement has a guard.
ALTER TABLE dispatch_rules DROP CONSTRAINT IF EXISTS dispatch_rules_trigger_check;
ALTER TABLE dispatch_rules ADD CONSTRAINT dispatch_rules_trigger_check
    CHECK (trigger IN ('entered_column', 'unblocked', 'review_changed', 'claim_released')
           AND (trigger <> 'entered_column' OR column_id IS NOT NULL));
