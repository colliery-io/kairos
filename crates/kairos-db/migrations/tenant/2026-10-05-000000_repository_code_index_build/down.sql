-- Reverse of KAIROS-T-0318: removes the opt-out of the code index builder.
-- The builder then builds each repository again.
ALTER TABLE repositories DROP COLUMN IF EXISTS code_index_build;
