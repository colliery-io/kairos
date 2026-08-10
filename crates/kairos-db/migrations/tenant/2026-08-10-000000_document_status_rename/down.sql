-- Reverse the KAIROS-T-0066 rename (same guard discipline).
UPDATE metadata_definitions
SET name = 'Status'
WHERE slug = 'status'
  AND is_system_default
  AND name = 'Document status';
