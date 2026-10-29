-- Reverse of KAIROS-T-0718: the review state of each task is lost, and the
-- rules that fire on `review_changed` go. The tasks and their notes stay.
DELETE FROM dispatch_rules WHERE trigger = 'review_changed';
ALTER TABLE dispatch_rules DROP CONSTRAINT IF EXISTS dispatch_rules_trigger_check;
ALTER TABLE dispatch_rules ADD CONSTRAINT dispatch_rules_trigger_check
    CHECK (trigger IN ('entered_column', 'unblocked')
           AND (trigger = 'unblocked' OR column_id IS NOT NULL));
DROP TABLE IF EXISTS task_review;
