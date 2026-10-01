"""A copy of `summarize_rows` from `stats.py`, with other names and one
changed line: the average is rounded."""


def tally_records(records):
    total = 0
    fields = 0
    widest = []
    for record in records:
        total += 1
        fields += len(record)
        if len(record) > len(widest):
            widest = record
    average = round(fields / total, 2) if total else 0
    return {"count": total, "cells": fields, "longest": widest, "average": average}
