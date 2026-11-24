-- KAIROS-T-0773 (COLLIERY-I-0678): the default initiative flow puts
-- Decompose before Ready. A new initiative board takes it (create_board,
-- kairos-db boards.rs).
--
-- WHAT THIS DOES TO THE DATA
--
-- The initiative row of system_board_defaults changes only when it still
-- has the old default. A default that an operator changed stays.
UPDATE public.system_board_defaults
SET columns = E'Discovery\nDesign\nDecompose\nReady\nActive\nMonitoring\nCompleted',
    transitions = E'Discovery -> Design\nDesign -> Decompose\nDecompose -> Ready\nReady -> Active\nActive -> Monitoring\nMonitoring -> Completed'
WHERE board_level = 'initiative'
  AND columns = E'Discovery\nDesign\nReady\nDecompose\nActive\nMonitoring\nCompleted'
  AND transitions = E'Discovery -> Design\nDesign -> Ready\nReady -> Decompose\nDecompose -> Active\nActive -> Monitoring\nMonitoring -> Completed';
