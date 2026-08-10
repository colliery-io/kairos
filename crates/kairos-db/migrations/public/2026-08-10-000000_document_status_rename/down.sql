-- Reverse the KAIROS-T-0066 rename (same guard discipline).
UPDATE public.system_metadata_definitions
SET name = 'Status'
WHERE slug = 'status'
  AND name = 'Document status';
