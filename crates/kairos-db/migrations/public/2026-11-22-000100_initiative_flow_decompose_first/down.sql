-- KAIROS-T-0773: back to Ready before Decompose, when the row has the new
-- default.
UPDATE public.system_board_defaults
SET columns = E'Discovery\nDesign\nReady\nDecompose\nActive\nMonitoring\nCompleted',
    transitions = E'Discovery -> Design\nDesign -> Ready\nReady -> Decompose\nDecompose -> Active\nActive -> Monitoring\nMonitoring -> Completed'
WHERE board_level = 'initiative'
  AND columns = E'Discovery\nDesign\nDecompose\nReady\nActive\nMonitoring\nCompleted'
  AND transitions = E'Discovery -> Design\nDesign -> Decompose\nDecompose -> Ready\nReady -> Active\nActive -> Monitoring\nMonitoring -> Completed';
