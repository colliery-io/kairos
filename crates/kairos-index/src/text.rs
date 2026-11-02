//! The text rank of the code search (KAIROS-T-0423): the SQLite FTS5 table
//! `symbol_text`, with the Porter stemmer, ranked by BM25.
//!
//! Each symbol that the search reads (a kind of [`SUMMARIZED_KINDS`]) has one
//! row, with the symbol id as its rowid. The columns hold the words of
//! [`words`]: the identifiers are cut at `_`, `::` and at a change to a
//! capital before FTS5 reads them, so `parseDocument` matches "parse". The
//! tokenizer keeps `_` in a token, so `parse_document` is one token too and
//! the whole name matches. The Porter stemmer makes "parsed" and "parse" one
//! term.
//!
//! - `name`: the name with its container.
//! - `signature`: the signature.
//! - `summary`: the summary from the pool, when the symbol is linked to one.
//! - `path`: the path of the file, only for a symbol with no summary (code
//!   that waits for its summary is found by its file too, KAIROS-T-0352).
//!
//! The table has no content of its own (`content=''`): it is an index of
//! words only. [`refresh`] writes it again from `symbols` and `summaries`.
//! Each write that changes the text of a symbol calls it: a build of the
//! structure, the link step of a summary run, and each copy of pool rows
//! into an index (`copy_pool`, `add_pool`, `assemble`). `split` calls it
//! after it removes the pool, so a structure in the store holds the words
//! of its names only.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::{IndexError, SUMMARIZED_KINDS};

/// The table. `IF NOT EXISTS`: [`refresh`] makes it in an index that has
/// none.
pub(crate) const TABLE: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS symbol_text USING fts5(
    name, signature, summary, path,
    content = '',
    tokenize = "porter unicode61 tokenchars '_'"
);
"#;

/// The BM25 weights of the columns, in the order of [`TABLE`]: a word of
/// the name counts more than a word of the summary.
const WEIGHTS: [f64; 4] = [3.0, 1.5, 1.0, 1.0];

/// Words that tell nothing about code.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "does", "for", "from", "how", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "this", "to", "what", "when", "where", "which",
    "who", "with",
];

/// Write the table again from `symbols` and `summaries`. Make it first if
/// the index has none. Call it in the transaction of the write.
pub(crate) fn refresh(conn: &Connection) -> Result<(), IndexError> {
    conn.execute_batch(TABLE)?;
    conn.execute(
        "INSERT INTO symbol_text (symbol_text) VALUES ('delete-all')",
        [],
    )?;
    let kinds = SUMMARIZED_KINDS
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut read = conn.prepare(&format!(
        "SELECT s.id, s.name, s.container, s.language, s.signature, f.path, m.summary
         FROM symbols s JOIN files f ON f.id = s.file_id
         LEFT JOIN summaries m ON m.key = s.summary_key
         WHERE s.kind IN ({kinds})"
    ))?;
    let mut insert = conn.prepare(
        "INSERT INTO symbol_text (rowid, name, signature, summary, path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut rows = read.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let name: String = r.get(1)?;
        let container: Option<String> = r.get(2)?;
        let language: String = r.get(3)?;
        let signature: Option<String> = r.get(4)?;
        let path: String = r.get(5)?;
        let summary: Option<String> = r.get(6)?;
        let qualified = match container {
            Some(c) if language == "rust" => format!("{c}::{name}"),
            Some(c) => format!("{c}.{name}"),
            None => name,
        };
        let path = if summary.is_none() {
            joined(&path)
        } else {
            String::new()
        };
        insert.execute(params![
            id,
            joined(&qualified),
            joined(signature.as_deref().unwrap_or("")),
            joined(summary.as_deref().unwrap_or("")),
            path,
        ])?;
    }
    Ok(())
}

/// The symbols that match `query`, as (symbol id, score), best first. The
/// score is the BM25 of FTS5 with [`WEIGHTS`], with the sign turned: a
/// higher score is a better match. A query with no word matches nothing.
pub(crate) fn rank(conn: &Connection, query: &str) -> Result<Vec<(i64, f64)>, IndexError> {
    let mut seen = HashSet::new();
    let terms: Vec<String> = words(query)
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .map(|w| format!("\"{w}\""))
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT rowid, -bm25(symbol_text, {}, {}, {}, {}) FROM symbol_text
         WHERE symbol_text MATCH ?1",
        WEIGHTS[0], WEIGHTS[1], WEIGHTS[2], WEIGHTS[3]
    ))?;
    let mut found: Vec<(i64, f64)> = stmt
        .query_map([terms.join(" OR ")], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    found.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(found)
}

/// [`words`] of `text`, with a space between them.
fn joined(text: &str) -> String {
    words(text).join(" ")
}

/// The words of a text for the text rank: lower case, with each identifier
/// also cut at `_`, `::` and at a change to a capital (`enqueue_all` gives
/// `enqueue_all`, `enqueue` and `all`). Stop words and single letters go.
pub(crate) fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if token.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut previous_lower = false;
        for c in token.chars() {
            if c == '_' {
                parts.push(std::mem::take(&mut current));
                previous_lower = false;
                continue;
            }
            if c.is_uppercase() && previous_lower {
                parts.push(std::mem::take(&mut current));
            }
            previous_lower = c.is_lowercase() || c.is_ascii_digit();
            current.extend(c.to_lowercase());
        }
        parts.push(current);
        let parts: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let whole = token.to_lowercase();
        if parts.len() > 1 {
            out.push(whole);
        }
        out.extend(parts);
    }
    out.retain(|w| w.chars().count() > 1 && !STOP_WORDS.contains(&w.as_str()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An index with the tables that [`refresh`] reads, and one symbol for
    /// each (name, signature, summary).
    fn index(symbols: &[(&str, &str, Option<&str>)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::prepare(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (id, path, language, decision, rule, rule_origin, size, longest_line, content_hash)
             VALUES (1, 'src/lib.rs', 'rust', 'source', 'r', 'default', 1, 1, 'h')",
            [],
        )
        .unwrap();
        for (i, (name, signature, summary)) in symbols.iter().enumerate() {
            let key = summary.map(|_| format!("k{i}"));
            if let (Some(key), Some(summary)) = (&key, summary) {
                conn.execute(
                    "INSERT INTO summaries (key, level, summary, model) VALUES (?1, 'symbol', ?2, 'fake/fixed')",
                    params![key, summary],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO symbols (id, file_id, name, kind, language, start_line, end_line,
                                      start_byte, end_byte, signature, tree_hash, is_test, summary_key)
                 VALUES (?1, 1, ?2, 'function', 'rust', 1, 1, 0, 1, ?3, 'h', 0, ?4)",
                params![i as i64 + 1, name, signature, key],
            )
            .unwrap();
        }
        refresh(&conn).unwrap();
        conn
    }

    fn ids(conn: &Connection, query: &str) -> Vec<i64> {
        rank(conn, query)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    #[test]
    fn the_bundled_sqlite_has_fts5_with_the_porter_stemmer() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(x, tokenize = 'porter unicode61');
             INSERT INTO t (x) VALUES ('documents are parsed');",
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM t WHERE t MATCH 'document parse'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn words_cut_identifiers() {
        assert_eq!(
            words("enqueue_all(items) -> Queue: the HttpClient"),
            [
                "enqueue_all",
                "enqueue",
                "all",
                "items",
                "queue",
                "httpclient",
                "http",
                "client"
            ]
        );
    }

    #[test]
    fn an_inflected_word_matches_its_stem() {
        let conn = index(&[
            ("parse_document", "fn parse_document(text: &str)", None),
            ("validate", "fn validate(doc: &Doc)", None),
            ("render", "fn render()", None),
        ]);
        assert_eq!(ids(&conn, "parsed documents"), [1]);
        assert_eq!(ids(&conn, "validated"), [2]);
        assert_eq!(ids(&conn, "validation"), [2]);
        assert!(ids(&conn, "the of").is_empty());
    }

    #[test]
    fn the_parts_and_the_whole_of_an_identifier_match() {
        let conn = index(&[
            ("HttpClient", "struct HttpClient", None),
            ("enqueue_all", "fn enqueue_all()", None),
        ]);
        assert_eq!(ids(&conn, "http client"), [1]);
        assert_eq!(ids(&conn, "httpclient"), [1]);
        assert_eq!(ids(&conn, "enqueued"), [2]);
        assert_eq!(ids(&conn, "enqueue_all"), [2]);
    }

    #[test]
    fn a_word_of_the_name_counts_more_than_a_word_of_the_summary() {
        let conn = index(&[
            ("render", "fn render()", Some("Writes the queue as text.")),
            ("queue", "fn queue()", Some("Writes the rows as text.")),
        ]);
        assert_eq!(ids(&conn, "queue"), [2, 1]);
    }

    #[test]
    fn the_symbol_with_the_rare_word_ranks_first() {
        let conn = index(&[
            ("a", "", Some("Gives the newest number.")),
            ("b", "", Some("Gives the number.")),
            ("c", "", Some("Gives the number of rows.")),
        ]);
        assert_eq!(ids(&conn, "newest number")[0], 1);
        assert!(ids(&conn, "queue").is_empty());
    }

    #[test]
    fn a_symbol_with_no_summary_is_found_by_its_path() {
        let conn = index(&[("f", "fn f()", None), ("g", "fn g()", Some("Does g."))]);
        assert_eq!(ids(&conn, "lib"), [1]);
    }

    #[test]
    fn a_refresh_follows_the_symbols_and_the_summaries() {
        let conn = index(&[("parse", "fn parse()", None)]);
        assert!(ids(&conn, "tokens").is_empty());
        conn.execute_batch(
            "INSERT INTO summaries (key, level, summary, model)
                 VALUES ('n', 'symbol', 'Reads the tokens.', 'fake/fixed');
             UPDATE symbols SET summary_key = 'n' WHERE id = 1;",
        )
        .unwrap();
        refresh(&conn).unwrap();
        assert_eq!(ids(&conn, "token"), [1]);
        conn.execute("DELETE FROM symbols", []).unwrap();
        refresh(&conn).unwrap();
        assert!(ids(&conn, "parse").is_empty());
    }
}
