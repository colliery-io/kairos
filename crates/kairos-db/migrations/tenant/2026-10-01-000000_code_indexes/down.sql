-- Reverse of COLLIERY-T-1853: removes the code indexes and their summary
-- pools. A checkout keeps its local index; the server builds no base index
-- until the tables are there again and an index is uploaded.
DROP TABLE IF EXISTS code_indexes;
DROP TABLE IF EXISTS code_index_summaries;
