-- KAIROS-T-0762, down. Who added each watch is lost; the watches stay.
ALTER TABLE item_watches DROP COLUMN IF EXISTS added_by;
