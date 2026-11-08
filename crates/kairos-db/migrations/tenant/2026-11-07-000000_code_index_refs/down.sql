-- KAIROS-T-0750, down. The refs of the stored indexes are lost; the list
-- of the indexes shows `code_indexes.ref_name` only until the table is
-- there again and the sweep writes the rows.
DROP TABLE IF EXISTS code_index_refs;
