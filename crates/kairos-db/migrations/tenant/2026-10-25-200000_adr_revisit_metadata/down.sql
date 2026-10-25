-- KAIROS-T-0705, down. The values of the three definitions on ADRs are
-- lost (item_metadata and the scopes go with ON DELETE CASCADE).
DELETE FROM metadata_definitions
WHERE slug IN ('revisit_when', 'revisit_on', 'accepted_commit')
  AND is_system_default;
