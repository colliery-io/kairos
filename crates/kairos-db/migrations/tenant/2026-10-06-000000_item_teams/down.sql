-- Reverse of KAIROS-T-0320: removes the links to a team, then puts back
-- the CHECKs of COLLIERY-T-0269 (a document or an ADR impacts a
-- repository, and nothing more).
DELETE FROM item_impacts WHERE target_kind = 'team';

ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_kind_pair_check;

ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_target_kind_check;
ALTER TABLE item_impacts
    ADD CONSTRAINT item_impacts_target_kind_check
    CHECK (target_kind IN ('repository'));

ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_item_type_check;
ALTER TABLE item_impacts
    ADD CONSTRAINT item_impacts_item_type_check
    CHECK (item_type IN ('document', 'adr'));
