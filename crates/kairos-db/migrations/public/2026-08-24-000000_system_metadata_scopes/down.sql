-- Reverse KAIROS-T-0078 (public side): restore the 'Document status'
-- system definition and drop the scopes table. Enum options and template
-- associations are restored to the seeded shape.
INSERT INTO public.system_metadata_definitions (name, slug, field_type)
VALUES ('Document status', 'status', 'enum')
ON CONFLICT (slug) DO NOTHING;

INSERT INTO public.system_metadata_enum_options (metadata_definition_id, value, position)
SELECT d.id, o.value, o.position
FROM (VALUES
    ('status', 'draft', 0),
    ('status', 'review', 1),
    ('status', 'approved', 2)
) AS o(slug, value, position)
JOIN public.system_metadata_definitions d ON d.slug = o.slug
ON CONFLICT (metadata_definition_id, value) DO NOTHING;

INSERT INTO public.system_template_metadata (template_id, metadata_definition_id, default_value, required)
SELECT t.id, d.id, a.default_value, false
FROM (VALUES
    ('prd', 'status', 'draft'),
    ('system_context', 'status', 'draft'),
    ('architecture_framing', 'status', 'draft')
) AS a(template_slug, def_slug, default_value)
JOIN public.system_templates t ON t.slug = a.template_slug
JOIN public.system_metadata_definitions d ON d.slug = a.def_slug
ON CONFLICT (template_id, metadata_definition_id) DO NOTHING;

DROP TABLE IF EXISTS public.system_metadata_definition_scopes;
