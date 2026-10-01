"""Load rows from a JSON file."""

import json


def load(path):
    with open(path) as handle:
        return json.loads(handle.read())
