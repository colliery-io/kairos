-- KAIROS-T-0705, KAIROS-A-0070: "Revisit when" and the commit at
-- acceptance of an ADR are system metadata, scoped to ADRs.
--
--   revisit_when     string  The condition for a new look at the ADR.
--   revisit_on       date    The day after which `kairos adrs check` flags it.
--   accepted_commit  string  The main commit when the ADR moved to Decided.
--
-- A new tenant gets the same rows from public.system_metadata_definitions
-- at provisioning (kairos-db tenant.rs). That copy runs after this
-- migration and skips a slug that is there.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 3 rows in metadata_definitions and 3 in metadata_definition_scopes. A
-- definition of the tenant with the same slug stays as it is, and gets no
-- scope from this migration. No other row is changed.
--
-- Re-runnable: each insert has ON CONFLICT DO NOTHING.
INSERT INTO metadata_definitions (name, slug, field_type, is_system_default) VALUES
    ('Revisit when', 'revisit_when', 'string', true),
    ('Revisit on', 'revisit_on', 'date', true),
    ('Accepted commit', 'accepted_commit', 'string', true)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO metadata_definition_scopes (metadata_definition_id, entity_type)
SELECT d.id, 'adr'
FROM metadata_definitions d
WHERE d.slug IN ('revisit_when', 'revisit_on', 'accepted_commit')
  AND d.is_system_default
ON CONFLICT DO NOTHING;
