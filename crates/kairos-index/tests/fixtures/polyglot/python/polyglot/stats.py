"""Count the rows of a report. `tally.py` has a copy of `summarize_rows`
with other names and one changed line (COLLIERY-T-1857)."""


def summarize_rows(rows):
    count = 0
    cells = 0
    longest = []
    for row in rows:
        count += 1
        cells += len(row)
        if len(row) > len(longest):
            longest = row
    average = cells / count if count else 0
    return {"count": count, "cells": cells, "longest": longest, "average": average}
