#!/usr/bin/env python3
"""Unit tests for refusal-check.py (KAIROS-T-0690). Stdlib only; run with
`python3 -m unittest scripts/test_refusal_check.py` (or `angreal test unit`).

The tests drive the check with small sources. The last test runs it on the
repository, so a new refusal from library text fails here too."""

import importlib.util
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "refusal_check", os.path.join(HERE, "refusal-check.py")
)
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)

SERVER = "crates/kairos-server/src/x.rs"


def found(source, rel=SERVER):
    return check.refusals(rel, source)


class LibraryText(unittest.TestCase):
    def test_the_error_of_a_library_call_is_library_text(self):
        source = """
fn f(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|e| ApiError::validation(format!("Bad id: {e}.")))
}
"""
        (refusal,) = found(source)
        self.assertTrue(refusal["lib"])

    def test_an_own_error_is_not_library_text(self):
        source = """
fn f(e: BoardError) -> ApiError {
    match e {
        e @ BoardError::Done { .. } => ApiError::unprocessable("TASK_DONE", e.to_string()),
    }
}
"""
        (refusal,) = found(source)
        self.assertEqual(refusal["lib"], [])

    def test_serde_message_and_html(self):
        source = """
fn f(serde_message: &str) -> ApiError {
    let a = ApiError::conflict(format!("No: {serde_message}."));
    ApiError::conflict("The page is <html> here.")
}
"""
        refusals = found(source)
        self.assertEqual(len(refusals), 2)
        self.assertTrue(all(r["lib"] for r in refusals))

    def test_a_foreign_field_of_an_error_attribute(self):
        source = """
pub enum E {
    #[error("Cannot write {path}: {source}.")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("No index at {path}.")]
    NoIndex { path: PathBuf },
}
"""
        write, no_index = found(source, "crates/kairos-cli/src/index/error.rs")
        self.assertTrue(write["lib"])
        self.assertEqual(no_index["lib"], [])


class Field(unittest.TestCase):
    def test_a_validation_needs_a_field(self):
        source = """
fn f() -> Result<(), ApiError> {
    Err(ApiError::validation("No title. Send `title`."))?;
    Err(ApiError::validation("No title.").with_details(json!({ "field": "title" })))?;
    Err(ApiError::new(StatusCode::BAD_REQUEST, "NOT_FOUND", "None."))?;
    Err(ApiError::conflict("Stale."))
}
"""
        refusals = found(source)
        self.assertEqual([r["field"] for r in refusals], [False, True, False, True])

    def test_a_parameter_a_header_and_a_fault_count_as_a_field(self):
        """KAIROS-T-0698: `parameter` for a query parameter and `header`
        for a header (errors.md); KAIROS-T-0697: `fault.details(...)`."""
        source = """
fn f() -> Result<(), ApiError> {
    Err(ApiError::validation("Bad limit.").with_details(json!({ "parameter": "limit" })))?;
    Err(ApiError::validation("No type.").with_details(json!({ "header": "Content-Type" })))?;
    Err(ApiError::new(s, "VALIDATION", m).with_details(fault.details(path)))?;
    Err(ApiError::validation("Bad.").with_details(json!({ "argument": "x" })))
}
"""
        refusals = found(source)
        self.assertEqual([r["field"] for r in refusals], [True, True, True, False])

    def test_a_test_module_is_not_read(self):
        source = """
#[cfg(test)]
mod tests {
    fn f() { ApiError::validation(format!("{e}")); }
}
"""
        self.assertEqual(found(source), [])


class NextCall(unittest.TestCase):
    def test_a_text_with_a_verb_or_a_tool_names_a_next_call(self):
        tools = {"get_item"}
        self.assertTrue(check.names_next_call("No item. Send the short code.", tools))
        self.assertTrue(check.names_next_call("Read it with get_item.", tools))
        self.assertTrue(check.names_next_call("Not here. GET /api/teams lists them.", tools))
        self.assertFalse(check.names_next_call("The item is not there.", tools))


class Repository(unittest.TestCase):
    def test_the_repository_is_not_worse_than_the_baseline(self):
        import contextlib
        import io

        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = check.main([])
        self.assertEqual(code, 0, err.getvalue())


if __name__ == "__main__":
    unittest.main()
