-- KAIROS-T-0700 (KAIROS-A-0069): `amends`, an edge from an ADR to an ADR.
-- The source changes a part of the target, and the rest of the target
-- stays in force. `supersedes` replaces all of it.
--
-- The CHECK on `item_relationships.relationship` takes the new value. The
-- new set holds each old value, so each row agrees with it. The table is
-- small (one row for each edge that a person or an agent makes).

ALTER TABLE item_relationships
    DROP CONSTRAINT IF EXISTS item_relationships_relationship_check;

ALTER TABLE item_relationships
    ADD CONSTRAINT item_relationships_relationship_check
    CHECK (relationship IN (
        'parent', 'supports', 'informs', 'supersedes', 'amends', 'blocks'
    ));
