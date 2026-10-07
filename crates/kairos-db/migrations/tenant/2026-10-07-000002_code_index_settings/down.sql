-- Reverse of KAIROS-T-0339: removes the provider settings of the code
-- index. The tenant then uses the embedded model again.
DROP TABLE IF EXISTS code_index_settings;
