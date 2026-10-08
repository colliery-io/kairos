//! Symbols of a SQL file (KAIROS-T-0350): the statements that define a
//! thing, read statement by statement with no grammar. The SQL of a
//! migration is regular, and the index wants the names, the kinds and the
//! places: `CREATE TABLE payments (` gives the table `payments`, the lines
//! of the statement and its first line as the signature. An `ALTER TABLE`
//! is a symbol of the table too, so a search for a table finds each
//! migration that changes it. A `DROP` defines nothing and gives none.
//!
//! The kinds are `table`, `index`, `view`, `function`, `type_alias` and
//! `trigger`. The hash of a statement is the hash of its text with the
//! whitespace collapsed, so a formatting change keeps the key of its
//! summary.

use crate::extract::{Extracted, FileExtract, hash_text};

/// The symbols of the SQL `content`.
pub fn extract(content: &str, test_file: bool) -> FileExtract {
    let mut symbols = Vec::new();
    for statement in statements(content) {
        let Some((kind, name)) = definition(statement.text) else {
            continue;
        };
        let signature = statement
            .text
            .lines()
            .next()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        let collapsed: String = statement
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        symbols.push(Extracted {
            name,
            container: None,
            kind,
            start_line: statement.start_line,
            end_line: statement.end_line,
            start_byte: statement.start,
            end_byte: statement.end,
            signature,
            tree_hash: hash_text("sql", collapsed.as_bytes()),
            is_test: test_file,
            token_vector: None,
            token_count: None,
            token_vector_made: false,
        });
    }
    FileExtract {
        symbols,
        ..FileExtract::default()
    }
}

/// One statement: its text from the first word to the `;` (or the end), and
/// its place.
struct Statement<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    start_line: usize,
    end_line: usize,
}

/// The statements of `content`, split at each `;` that is outside a string,
/// a quoted name, a comment or a dollar-quoted body.
fn statements(content: &str) -> Vec<Statement<'_>> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        let rest = &content[i..];
        // Comments and quotes are skipped whole.
        if rest.starts_with("--") {
            i += rest.find('\n').map_or(rest.len(), |n| n + 1);
            continue;
        }
        if rest.starts_with("/*") {
            i += rest.find("*/").map_or(rest.len(), |n| n + 2);
            continue;
        }
        if let Some(tag) = dollar_tag(rest) {
            let after = &rest[tag.len()..];
            i += tag.len()
                + after
                    .find(tag.as_str())
                    .map_or(after.len(), |n| n + tag.len());
            continue;
        }
        let c = bytes[i];
        if c == b'\'' || c == b'"' {
            let after = &rest[1..];
            i += 1 + after.find(c as char).map_or(after.len(), |n| n + 1);
            continue;
        }
        if c == b';' {
            if let Some(s) = start.take() {
                out.push(statement(content, s, i + 1));
            }
            i += 1;
            continue;
        }
        if start.is_none() && !c.is_ascii_whitespace() {
            start = Some(i);
        }
        i += 1;
    }
    if let Some(s) = start
        && content[s..].trim().chars().any(|c| !c.is_whitespace())
    {
        out.push(statement(content, s, content.len()));
    }
    out
}

fn statement(content: &str, start: usize, end: usize) -> Statement<'_> {
    let text = content[start..end].trim_end();
    let end = start + text.len();
    let line_of = |at: usize| content[..at].matches('\n').count() + 1;
    Statement {
        text,
        start,
        end,
        start_line: line_of(start),
        end_line: line_of(end.saturating_sub(1).max(start)),
    }
}

/// A dollar quote at the start of `rest`: `$$` or `$tag$`.
fn dollar_tag(rest: &str) -> Option<String> {
    if !rest.starts_with('$') {
        return None;
    }
    let end = rest[1..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(|n| n + 1)?;
    (rest.as_bytes()[end] == b'$').then(|| rest[..=end].to_string())
}

/// The kind and the name that a statement defines, or `None` for a
/// statement that defines nothing (`DROP`, `INSERT`, `GRANT`, …).
fn definition(text: &str) -> Option<(&'static str, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let upper: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
    let mut i = 0;
    let verb = upper.first()?.as_str();
    match verb {
        "CREATE" => {
            i += 1;
            while i < upper.len()
                && matches!(
                    upper[i].as_str(),
                    "OR" | "REPLACE"
                        | "TEMP"
                        | "TEMPORARY"
                        | "UNIQUE"
                        | "UNLOGGED"
                        | "MATERIALIZED"
                )
            {
                i += 1;
            }
        }
        "ALTER" => i += 1,
        _ => return None,
    }
    let object = upper.get(i)?.as_str();
    let kind = match (verb, object) {
        (_, "TABLE") => "table",
        ("CREATE", "INDEX") => "index",
        ("CREATE", "VIEW") => "view",
        ("CREATE", "FUNCTION" | "PROCEDURE") => "function",
        ("CREATE", "TYPE" | "DOMAIN") => "type_alias",
        ("CREATE", "TRIGGER") => "trigger",
        _ => return None,
    };
    i += 1;
    while i < upper.len()
        && matches!(
            upper[i].as_str(),
            "IF" | "NOT" | "EXISTS" | "ONLY" | "CONCURRENTLY"
        )
    {
        i += 1;
    }
    let raw = words.get(i)?;
    let name: String = raw
        .split('(')
        .next()
        .unwrap_or(raw)
        .trim_end_matches(';')
        .replace('"', "");
    if name.is_empty() {
        return None;
    }
    Some((kind, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATION: &str = "\
-- The payments of a tenant; a 'quoted;' comment.
CREATE TABLE IF NOT EXISTS payments (
    id    UUID PRIMARY KEY,
    note  TEXT NOT NULL DEFAULT 'none;'
);

CREATE UNIQUE INDEX payments_by_note ON payments (note);

CREATE OR REPLACE FUNCTION touch_payment() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now(); -- a ; in the body
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER payments_touch BEFORE UPDATE ON payments
    FOR EACH ROW EXECUTE FUNCTION touch_payment();

ALTER TABLE payments ADD COLUMN day DATE;

DROP TABLE old_payments;
";

    #[test]
    fn each_definition_is_a_symbol_and_a_drop_is_none() {
        let found = extract(MIGRATION, false);
        let names: Vec<(&str, &str)> = found
            .symbols
            .iter()
            .map(|s| (s.kind, s.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("table", "payments"),
                ("index", "payments_by_note"),
                ("function", "touch_payment"),
                ("trigger", "payments_touch"),
                ("table", "payments"),
            ]
        );
    }

    #[test]
    fn the_place_and_the_signature_are_those_of_the_statement() {
        let found = extract(MIGRATION, false);
        let table = &found.symbols[0];
        assert_eq!((table.start_line, table.end_line), (2, 5));
        assert_eq!(
            table.signature.as_deref(),
            Some("CREATE TABLE IF NOT EXISTS payments (")
        );
        assert!(MIGRATION[table.start_byte..table.end_byte].ends_with(");"));
        let function = &found.symbols[2];
        assert_eq!((function.start_line, function.end_line), (9, 14));
    }

    #[test]
    fn a_formatting_change_keeps_the_hash() {
        let a = extract("CREATE TABLE t (\n  id INT\n);", false);
        let b = extract("CREATE   TABLE t ( id INT );", false);
        assert_eq!(a.symbols[0].tree_hash, b.symbols[0].tree_hash);
        let c = extract("CREATE TABLE t ( id TEXT );", false);
        assert_ne!(a.symbols[0].tree_hash, c.symbols[0].tree_hash);
    }

    #[test]
    fn a_tagged_dollar_body_is_one_statement() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql;\nCREATE TABLE t (id INT);";
        let found = extract(sql, false);
        assert_eq!(found.symbols.len(), 2);
        assert_eq!(found.symbols[1].start_line, 2);
    }
}
