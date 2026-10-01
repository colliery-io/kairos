"""Load rows from a CSV file."""


def load(path):
    with open(path) as handle:
        return [_split(line) for line in handle]


def _split(line):
    return line.rstrip("\n").split(",")
