-- Reverse of COLLIERY-T-3100: removes the retired codes. A read with a
-- retired code then finds nothing, and the sequences can issue a retired
-- code again.
DROP TRIGGER IF EXISTS strategies_refuse_retired_code ON strategies;
DROP TRIGGER IF EXISTS initiatives_refuse_retired_code ON initiatives;
DROP TRIGGER IF EXISTS tasks_refuse_retired_code ON tasks;
DROP TRIGGER IF EXISTS documents_refuse_retired_code ON documents;
DROP TRIGGER IF EXISTS adrs_refuse_retired_code ON adrs;
DROP FUNCTION IF EXISTS refuse_retired_short_code();
DROP TABLE IF EXISTS retired_codes;
