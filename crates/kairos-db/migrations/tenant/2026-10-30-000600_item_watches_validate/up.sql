-- KAIROS-T-0736: validate the CHECK of the kind of notifications that the
-- migration before added NOT VALID (KAIROS-A-0062). VALIDATE reads each row
-- and does not block a write.
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_kind_check;
