-- KAIROS-T-0320 (COLLIERY-I-0602): an initiative or a strategy can name a
-- team by hand, before it has tasks.
--
-- The link is a row of `item_impacts` with `target_kind = 'team'`. The
-- table was made for this (COLLIERY-T-0269 kept `target_kind` in each row
-- so that a later target kind is a new value), and COLLIERY-I-0019
-- decision 3 names the link to an organizational unit `impacts`. This
-- amends decision 13 of COLLIERY-I-0019 ("an initiative has no `impacts`
-- link of its own"): an initiative and a strategy can impact a team.
--
-- The pairs that a row can have:
--
--   item_type              target_kind
--   document, adr          repository
--   initiative, strategy   team
--
-- No row is changed: each row that exists is a document or an ADR that
-- impacts a repository, which the new CHECK permits.
--
-- Re-runnable: each constraint is dropped first.
ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_item_type_check;
ALTER TABLE item_impacts
    ADD CONSTRAINT item_impacts_item_type_check
    CHECK (item_type IN ('document', 'adr', 'initiative', 'strategy'));

ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_target_kind_check;
ALTER TABLE item_impacts
    ADD CONSTRAINT item_impacts_target_kind_check
    CHECK (target_kind IN ('repository', 'team'));

ALTER TABLE item_impacts DROP CONSTRAINT IF EXISTS item_impacts_kind_pair_check;
ALTER TABLE item_impacts
    ADD CONSTRAINT item_impacts_kind_pair_check
    CHECK (
        (target_kind = 'repository' AND item_type IN ('document', 'adr'))
        OR (target_kind = 'team' AND item_type IN ('initiative', 'strategy'))
    );
