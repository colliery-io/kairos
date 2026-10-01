"""Build a report. Which `load` it calls depends on the import that works."""

try:
    from .csv_loader import load
except ImportError:
    from .json_loader import load


class Report:
    def __init__(self, rows):
        self.rows = rows

    def render(self):
        return "\n".join(str(row) for row in self.rows)


def build_report(path):
    rows = load(path)
    return Report(rows).render()
