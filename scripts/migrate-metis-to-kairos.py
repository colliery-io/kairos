#!/usr/bin/env python3
"""Copy a Metis work record into a Kairos tenant.

Two modes, one pipeline (scripts/metis_import/run_remap.py). `--codes` has no
default, so a run always names its mode.

- `--codes keep` (COLLIERY-T-3104) keeps the Metis numbers on the boards of
  the team. An item keeps its number when the board that gives its code has
  the prefix `--prefix`: the tasks, the documents (each document has the
  delivery board as its owner board, also a document that supports a
  parent, COLLIERY-T-3109), and the ADRs on a team ADR board with that
  prefix. `SKADI-T-0577` stays `SKADI-T-0577`; a specification
  `SKADI-S-0003` becomes the document `SKADI-D-0003`. Each other item (an
  initiative on the shared board `initiatives`) gets the next code of its
  board. When 2 documents want one code, the first keeps it and the
  other gets the next free number; its footer says why. Keep mode needs an
  organization admin (KAIROS_KEY or KAIROS_ADMIN_TOKEN), because it sets the
  code sequence of the board before each create.
- `--codes remap` gives each item the next code of its board.

In the two modes, the references in the text change to the new codes after
each item exists, and a provenance footer keeps the Metis code.

The 2026-09-26 keep mode (one sequence for the tenant, an empty tenant, the
text not changed) is gone: board prefixes (COLLIERY-I-0407) replaced it.

`--prefix` is the code prefix of the delivery board. The run stops if the
board has a different prefix.

Usage:
    KAIROS_KEY=kairos_sk_... [KAIROS_ADMIN_TOKEN=kairos_ss_...] \\
      scripts/migrate-metis-to-kairos.py --codes keep \\
        --url http://127.0.0.1:8080 --metis ~/Code/skadi/.metis \\
        --repository skadi --prefix SKADI \\
        --delivery-board skadi --adr-board skadi-adrs \\
        [--also-map ~/kairos-import/crt/import-state.json,...] \\
        [--content-dir ~/kairos-import/skadi/ste] plan|apply|verify

The state goes to ~/kairos-import/<repository>/import-state.json (--state).
A state file is for one mode, one prefix, one repository and one URL.
plan reads only the files, and the boards too when --url is given. apply
refuses a URL that is not 127.0.0.1 or localhost unless
--i-mean-the-live-deployment is given.
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from metis_import import run_remap  # noqa: E402
from metis_import.api import Stop, is_local  # noqa: E402


def parser():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("mode", choices=["plan", "apply", "verify"])
    p.add_argument("--codes", choices=["keep", "remap"], required=True,
                   help="keep: keep the Metis numbers on the boards with --prefix. "
                        "remap: the next code of each board")
    p.add_argument("--url", help="required, except for plan")
    p.add_argument("--metis", default=".metis")
    p.add_argument("--repository", required=True)
    p.add_argument("--prefix", required=True, help="the code prefix of the delivery board")
    p.add_argument("--delivery-board", required=True)
    p.add_argument("--initiative-board", default="initiatives")
    p.add_argument("--adr-board", default="adrs")
    p.add_argument("--limit", type=int, default=0, help="stop after this many creates (0 = all)")
    p.add_argument("--state", help="the state file (default ~/kairos-import/<repository>/import-state.json)")
    p.add_argument("--also-map", help="state files of earlier runs, separated by commas")
    p.add_argument("--content-dir", help="directory of staged texts <OLD-CODE>.md")
    p.add_argument("--wiki-links", choices=["plain", "link"], default="plain",
                   help="[[CODE]] becomes the plain new code, or a markdown link")
    p.add_argument("--tenant", help="send this X-Tenant header (a deployment with more than one tenant)")
    p.add_argument("--i-mean-the-live-deployment", action="store_true",
                   help="permit apply against a URL that is not 127.0.0.1 or localhost")
    return p


def main():
    args = parser().parse_args()
    sys.stdout.reconfigure(line_buffering=True)
    if args.mode == "apply" and args.url and not is_local(args.url) \
            and not args.i_mean_the_live_deployment:
        raise Stop("apply against %s is refused. That URL is not 127.0.0.1 or localhost. "
                   "Add --i-mean-the-live-deployment to write to it." % args.url)
    sys.exit(run_remap.main(args))


if __name__ == "__main__":
    try:
        main()
    except Stop as e:
        print("STOP: %s" % e, file=sys.stderr)
        sys.exit(2)
