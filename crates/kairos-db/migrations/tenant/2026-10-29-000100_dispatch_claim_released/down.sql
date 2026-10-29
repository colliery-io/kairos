-- Reverse of KAIROS-T-0720: the rules that fire on `claim_released` go.
DELETE FROM dispatch_rules WHERE trigger = 'claim_released';
ALTER TABLE dispatch_rules DROP CONSTRAINT IF EXISTS dispatch_rules_trigger_check;
ALTER TABLE dispatch_rules ADD CONSTRAINT dispatch_rules_trigger_check
    CHECK (trigger IN ('entered_column', 'unblocked', 'review_changed')
           AND (trigger <> 'entered_column' OR column_id IS NOT NULL));
