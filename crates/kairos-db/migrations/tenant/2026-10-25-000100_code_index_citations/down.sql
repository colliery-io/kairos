-- KAIROS-T-0703, down. The cited codes of the stored indexes are lost;
-- `get_repository` lists no cited ADR until the table is there again and
-- each repository is indexed again.
DROP TABLE IF EXISTS code_index_citations;
