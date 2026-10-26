-- KAIROS-T-0700, down. The `amends` edges are lost: the old CHECK does not
-- take them.

DELETE FROM item_relationships WHERE relationship = 'amends';

ALTER TABLE item_relationships
    DROP CONSTRAINT IF EXISTS item_relationships_relationship_check;

ALTER TABLE item_relationships
    ADD CONSTRAINT item_relationships_relationship_check
    CHECK (relationship IN (
        'parent', 'supports', 'informs', 'supersedes', 'blocks'
    ));
