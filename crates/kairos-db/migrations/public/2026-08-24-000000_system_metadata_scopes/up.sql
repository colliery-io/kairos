-- KAIROS-T-0078 (public side): entity-type scoping for system metadata
-- definitions, and retirement of the 'Document status' definition — the
-- document lifecycle becomes a typed column on documents (tenant side).
--
-- Scope semantics: NO rows for a definition = applies to every entity
-- type; rows = the allowed types. Guarded for re-runnability.

CREATE TABLE IF NOT EXISTS public.system_metadata_definition_scopes (
    metadata_definition_id UUID NOT NULL
        REFERENCES public.system_metadata_definitions(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL
        CHECK (entity_type IN ('strategy', 'initiative', 'task', 'document', 'adr')),
    PRIMARY KEY (metadata_definition_id, entity_type)
);

-- document_type: documents only (the UAT rule this ticket enforces).
-- complexity: everywhere EXCEPT initiatives — their native complexity
-- column is the source of truth there (the T-0066 duplication lesson).
-- priority: stays unscoped (applies to all).
INSERT INTO public.system_metadata_definition_scopes (metadata_definition_id, entity_type)
SELECT d.id, s.entity_type
FROM (VALUES
    ('document_type', 'document'),
    ('complexity', 'strategy'),
    ('complexity', 'task'),
    ('complexity', 'document'),
    ('complexity', 'adr')
) AS s(slug, entity_type)
JOIN public.system_metadata_definitions d ON d.slug = s.slug
ON CONFLICT DO NOTHING;

-- Retire the system 'Document status' definition: lifecycle is a typed
-- documents column now, never metadata. Template associations and enum
-- options follow via ON DELETE CASCADE where declared; delete explicitly
-- anyway so this never depends on the FK declarations.
DELETE FROM public.system_template_metadata stm
    USING public.system_metadata_definitions d
    WHERE d.id = stm.metadata_definition_id AND d.slug = 'status';
DELETE FROM public.system_metadata_enum_options seo
    USING public.system_metadata_definitions d
    WHERE d.id = seo.metadata_definition_id AND d.slug = 'status';
DELETE FROM public.system_metadata_definitions WHERE slug = 'status';
