from polyglot.stats import summarize_rows


def test_summary_counts_the_rows():
    rows = [["a", "b"], ["c"]]
    summary = summarize_rows(rows)
    assert summary["count"] == 2
    assert summary["cells"] == 3
    assert summary["longest"] == ["a", "b"]


def test_summary_counts_the_rows_again():
    rows = [["a", "b"], ["c"]]
    summary = summarize_rows(rows)
    assert summary["count"] == 2
    assert summary["cells"] == 3
    assert summary["longest"] == ["a", "b"]
