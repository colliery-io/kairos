"""Helpers of scripts/migrate-metis-to-kairos.py.

- metis: read a Metis archive.
- remap: the pure parts of `--codes remap` (codes, references, footer, order).
- api: the HTTP and MCP client, with retries and a request count.
- run_remap: plan, apply and verify of `--codes remap`.
"""
