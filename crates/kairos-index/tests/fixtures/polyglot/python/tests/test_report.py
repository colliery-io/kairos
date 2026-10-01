from polyglot.report import Report, build_report


def test_render_joins_rows():
    assert Report(["a", "b"]).render() == "a\nb"


def test_build_report_reads_the_file(tmp_path):
    path = tmp_path / "rows.csv"
    path.write_text("a,b\n")
    assert build_report(str(path)) == "['a', 'b']"
