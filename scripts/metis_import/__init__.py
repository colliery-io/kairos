"""Helpers of scripts/migrate-metis-to-kairos.py.

- metis: read a Metis archive.
- remap: the pure parts of `--codes remap` and `--codes keep` (codes,
  references, footer, order, the codes that keep their number).
- api: the HTTP and MCP client, with retries and a request count.
- run_remap: plan, apply and verify of the two modes.
"""
