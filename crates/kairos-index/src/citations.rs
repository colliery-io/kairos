//! The ADR citations of the code (KAIROS-T-0703): each ADR short code in a
//! comment, a doc string or the name of a test.
//!
//! - **An ADR short code** is a board prefix, `-A-` and a number of 4 digits
//!   or more: `KAIROS-A-0065`. The prefix is an upper-case letter, then
//!   upper-case letters and digits. A letter, a digit, `_` or `-` just
//!   before the code, or a letter, a digit or `_` just after it, makes it
//!   part of a longer word: it is not a citation.
//! - **The contexts:**
//!   - `doc`: a doc comment (Rust `///`, `//!`, `/** */`, `/*! */`; a `/**`
//!     comment in TypeScript and JavaScript; a Go comment on the line just
//!     above a declaration) or a Python doc string (a string that is the
//!     first statement of a module, a class or a function).
//!   - `comment`: each other comment. In SQL, `--` and `/* */`.
//!   - `test`: the name of a test symbol, in snake case:
//!     `retry_is_safe_kairos_a_0065` cites `KAIROS-A-0065`.
//! - **The enclosing symbol** is the smallest symbol that holds the
//!   citation. A doc comment outside each symbol documents the item below
//!   it, so that item is its symbol. The build gives the symbol (see
//!   [`crate::build`]); this module gives the places.
//! - A string that is not a doc string is not read: a code in a string is
//!   data, not a citation.

use tree_sitter::{Node, Tree};

use crate::edges::Sym;
use crate::extract::FileExtract;

/// One citation in the text of a file, before the build gives it its
/// symbol.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Citation {
    /// The ADR short code, as `KAIROS-A-0065`.
    pub code: String,
    /// The byte of the code in the file.
    pub byte: usize,
    /// From 1.
    pub line: usize,
    /// From 1, in characters.
    pub col: usize,
    /// `comment` or `doc`. A test name has `test`; [`in_test_name`] gives it.
    pub context: String,
    /// The bytes of the item that a doc comment outside each symbol
    /// documents: the start and the end.
    pub documents: Option<(usize, usize)>,
}

/// An ADR that the code cites, with the count of its places and of its
/// files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CitedAdr {
    pub code: String,
    pub places: u32,
    pub files: u32,
}

/// Each ADR that the index at `conn` cites, by code.
pub(crate) fn cited(conn: &rusqlite::Connection) -> Result<Vec<CitedAdr>, crate::IndexError> {
    let mut stmt = conn.prepare(
        "SELECT code, count(*), count(DISTINCT file_id) FROM adr_citations
         GROUP BY code ORDER BY code",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CitedAdr {
            code: r.get(0)?,
            places: r.get(1)?,
            files: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A citation with its symbol, ready to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    pub code: String,
    pub symbol_id: Option<i64>,
    pub line: usize,
    pub col: usize,
    pub context: String,
}

/// The citations of one parsed file with their symbols: the comments and
/// the doc strings of `found`, and the names of its test symbols. `symbols`
/// are the symbols of `found` in the same order, with their ids. By line
/// and column.
pub(crate) fn of_file(found: &FileExtract, text: &str, symbols: &[Sym]) -> Vec<Placed> {
    let mut out: Vec<Placed> = found
        .citations
        .iter()
        .map(|c| Placed {
            code: c.code.clone(),
            symbol_id: symbol_of(symbols, c.byte, c.documents),
            line: c.line,
            col: c.col,
            context: c.context.clone(),
        })
        .collect();
    for (s, sym) in found.symbols.iter().zip(symbols) {
        if !s.is_test {
            continue;
        }
        let codes = in_test_name(&s.name);
        if codes.is_empty() {
            continue;
        }
        // The place of the name in the text of the symbol.
        let byte = text
            .get(s.start_byte..s.end_byte)
            .and_then(|body| body.find(s.name.as_str()))
            .map_or(s.start_byte, |n| s.start_byte + n);
        let line_start = text[..byte].rfind('\n').map_or(0, |n| n + 1);
        for code in codes {
            out.push(Placed {
                code,
                symbol_id: Some(sym.id),
                line: text[..byte].matches('\n').count() + 1,
                col: text[line_start..byte].chars().count() + 1,
                context: "test".to_string(),
            });
        }
    }
    out.sort_by(|a, b| (a.line, a.col, &a.code).cmp(&(b.line, b.col, &b.code)));
    out
}

/// The symbol of a citation at `byte`: the item that a doc comment
/// documents (the largest symbol that starts after the citation and in the
/// item), or else the smallest symbol that holds the byte.
fn symbol_of(symbols: &[Sym], byte: usize, documents: Option<(usize, usize)>) -> Option<i64> {
    if let Some((_, end)) = documents
        && let Some(s) = symbols
            .iter()
            .filter(|s| s.start_byte > byte && s.start_byte < end && s.end_byte <= end)
            .min_by_key(|s| (s.start_byte, std::cmp::Reverse(s.end_byte)))
    {
        return Some(s.id);
    }
    symbols
        .iter()
        .filter(|s| s.start_byte <= byte && byte < s.end_byte)
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| s.id)
}

/// The ADR short codes in `text`: the byte of each, and the code.
pub fn codes_in(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(n) = text[i..].find("-A-") {
        let dash = i + n;
        i = dash + 3;
        // The prefix: upper-case letters and digits, back from the dash,
        // with an upper-case letter first.
        let mut start = dash;
        while start > 0
            && (bytes[start - 1].is_ascii_uppercase() || bytes[start - 1].is_ascii_digit())
        {
            start -= 1;
        }
        while start < dash && !bytes[start].is_ascii_uppercase() {
            start += 1;
        }
        if start == dash {
            continue;
        }
        if start > 0 {
            let before = bytes[start - 1];
            if before.is_ascii_alphanumeric() || before == b'_' || before == b'-' {
                continue;
            }
        }
        let mut end = dash + 3;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end - (dash + 3) < 4 {
            continue;
        }
        if end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            continue;
        }
        out.push((start, text[start..end].to_string()));
        i = end;
    }
    out
}

/// Whether `text` is one ADR short code and nothing else: `KAIROS-A-0065`.
pub fn is_adr_code(text: &str) -> bool {
    matches!(codes_in(text).as_slice(), [(0, code)] if code.len() == text.len())
}

/// The ADR short codes in the name of a test, in snake case:
/// `kairos_a_0065` gives `KAIROS-A-0065`. The prefix is the word before
/// `_a_`.
pub fn in_test_name(name: &str) -> Vec<String> {
    let words: Vec<&str> = name.split('_').collect();
    let mut out = Vec::new();
    for w in 1..words.len().saturating_sub(1) {
        let (prefix, a, number) = (words[w - 1], words[w], words[w + 1]);
        let prefix_ok = prefix
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && prefix.chars().all(|c| c.is_ascii_alphanumeric());
        let number_ok = number.len() >= 4 && number.chars().all(|c| c.is_ascii_digit());
        if prefix_ok && a.eq_ignore_ascii_case("a") && number_ok {
            out.push(format!("{}-A-{number}", prefix.to_ascii_uppercase()));
        }
    }
    out
}

/// The citations in the comments and the doc strings of a parsed file.
pub fn in_tree(language: &str, tree: &Tree, source: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind().contains("comment") {
            let text = &source[node.start_byte()..node.end_byte()];
            let doc = is_doc_comment(language, node, text);
            // An inner doc comment documents the item that holds it.
            let inner = text.starts_with("//!") || text.starts_with("/*!");
            let documents = if doc && !inner {
                documented(node)
            } else {
                None
            };
            push_codes(
                &mut out,
                source,
                node.start_byte(),
                text,
                if doc { "doc" } else { "comment" },
                documents,
            );
            continue;
        }
        if language == "python" && node.kind() == "string" {
            if is_python_doc_string(node) {
                let text = &source[node.start_byte()..node.end_byte()];
                push_codes(&mut out, source, node.start_byte(), text, "doc", None);
            }
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out.sort_by_key(|c| c.byte);
    out
}

/// The citations in the comments of a SQL file: `--` to the end of the
/// line, and `/* */`. A string, a quoted name and a dollar-quoted body are
/// skipped.
pub fn in_sql(source: &str) -> Vec<Citation> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &source[i..];
        let skip = if rest.starts_with("--") {
            let len = rest.find('\n').unwrap_or(rest.len());
            push_codes(&mut out, source, i, &rest[..len], "comment", None);
            len
        } else if rest.starts_with("/*") {
            let len = rest.find("*/").map_or(rest.len(), |n| n + 2);
            push_codes(&mut out, source, i, &rest[..len], "comment", None);
            len
        } else if let Some(tag) = crate::sql::dollar_tag(rest) {
            let after = &rest[tag.len()..];
            tag.len()
                + after
                    .find(tag.as_str())
                    .map_or(after.len(), |n| n + tag.len())
        } else if bytes[i] == b'\'' || bytes[i] == b'"' {
            let after = &rest[1..];
            1 + after.find(bytes[i] as char).map_or(after.len(), |n| n + 1)
        } else {
            rest.chars().next().map_or(1, char::len_utf8)
        };
        i += skip;
    }
    out
}

fn push_codes(
    out: &mut Vec<Citation>,
    source: &str,
    at: usize,
    text: &str,
    context: &str,
    documents: Option<(usize, usize)>,
) {
    for (offset, code) in codes_in(text) {
        let byte = at + offset;
        let line_start = source[..byte].rfind('\n').map_or(0, |n| n + 1);
        out.push(Citation {
            code,
            byte,
            line: source[..byte].matches('\n').count() + 1,
            col: source[line_start..byte].chars().count() + 1,
            context: context.to_string(),
            documents,
        });
    }
}

/// Whether the comment `node` with the text `text` is a doc comment.
fn is_doc_comment(language: &str, node: Node, text: &str) -> bool {
    match language {
        "rust" => {
            (text.starts_with("///") && !text.starts_with("////"))
                || text.starts_with("//!")
                || (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
                || text.starts_with("/*!")
        }
        "typescript" | "tsx" | "javascript" => {
            text.starts_with("/**") && !text.starts_with("/***") && text != "/**/"
        }
        // A Go doc comment is on the line just above a declaration, or
        // above another comment that is.
        "go" => {
            let mut next = node.next_sibling();
            let mut end_row = node.end_position().row;
            while let Some(n) = next {
                if n.start_position().row != end_row + 1 {
                    return false;
                }
                if !n.kind().contains("comment") {
                    return n.kind().ends_with("declaration");
                }
                end_row = n.end_position().row;
                next = n.next_sibling();
            }
            false
        }
        _ => false,
    }
}

/// The bytes of the item that the doc comment `node` documents: the next
/// sibling that is not a comment and not an attribute. None before an
/// inner attribute.
fn documented(node: Node) -> Option<(usize, usize)> {
    let mut next = node.next_sibling();
    while let Some(n) = next {
        let kind = n.kind();
        if kind.contains("comment") || kind == "attribute_item" || kind == "decorator" {
            next = n.next_sibling();
            continue;
        }
        if kind == "inner_attribute_item" {
            return None;
        }
        return Some((n.start_byte(), n.end_byte()));
    }
    None
}

/// Whether the Python string `node` is a doc string: the first statement of
/// a module, a class or a function.
fn is_python_doc_string(node: Node) -> bool {
    let Some(statement) = node.parent().filter(|p| p.kind() == "expression_statement") else {
        return false;
    };
    if statement.named_child_count() != 1 {
        return false;
    }
    let Some(body) = statement.parent() else {
        return false;
    };
    let first = {
        let mut cursor = body.walk();
        body.named_children(&mut cursor)
            .find(|c| !c.kind().contains("comment"))
    };
    if first.is_none_or(|f| f.id() != statement.id()) {
        return false;
    }
    match body.kind() {
        "module" => true,
        "block" => body
            .parent()
            .is_some_and(|p| matches!(p.kind(), "function_definition" | "class_definition")),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_found_with_its_byte() {
        assert_eq!(
            codes_in("// see KAIROS-A-0065 and GQLITE-A-0012."),
            vec![
                (7, "KAIROS-A-0065".to_string()),
                (25, "GQLITE-A-0012".to_string())
            ]
        );
    }

    #[test]
    fn a_code_in_a_longer_word_is_not_a_citation() {
        assert!(codes_in("_KAIROS-A-0065").is_empty());
        assert!(codes_in("x-KAIROS-A-0065").is_empty());
        assert!(codes_in("KAIROS-A-0065x").is_empty());
        assert!(codes_in("KAIROS-A-065").is_empty());
        assert!(codes_in("KAIROS-T-0065").is_empty());
        assert!(codes_in("kairos-A-0065").is_empty());
        assert!(codes_in("-A-0065").is_empty());
        assert_eq!(codes_in("(KAIROS-A-12345)")[0].1, "KAIROS-A-12345");
    }

    #[test]
    fn a_test_name_cites_in_snake_case() {
        assert_eq!(
            in_test_name("retry_is_safe_kairos_a_0065"),
            vec!["KAIROS-A-0065".to_string()]
        );
        assert!(in_test_name("make_a_list").is_empty());
        assert!(in_test_name("a_0065").is_empty());
        assert!(in_test_name("kairos_a_065").is_empty());
    }

    #[test]
    fn a_split_keeps_the_cited_adrs_and_an_update_reads_a_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/lib.rs"),
            "/// KAIROS-A-0065\npub fn f() {}\n// KAIROS-A-0065, GQLITE-A-0002\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/b.rs"), "// KAIROS-A-0065\n").unwrap();
        let db = dir.path().join("index.sqlite");
        let report = crate::build_structure(&repo, &db).unwrap();
        assert_eq!(report.citations, 4);
        let split = crate::store::split(&db).unwrap();
        assert_eq!(
            split.citations,
            vec![
                CitedAdr {
                    code: "GQLITE-A-0002".into(),
                    places: 1,
                    files: 1
                },
                CitedAdr {
                    code: "KAIROS-A-0065".into(),
                    places: 3,
                    files: 2
                },
            ]
        );
        // The structure keeps each place.
        let out = dir.path().join("assembled.sqlite");
        crate::store::assemble(&split.structure_gz, [], &out).unwrap();
        let index = crate::Index::open(&out).unwrap();
        let places: Vec<(String, u32, String, Option<String>)> = index
            .adr_citations("KAIROS-A-0065")
            .unwrap()
            .into_iter()
            .map(|c| (c.file, c.line, c.context, c.symbol.map(|s| s.name)))
            .collect();
        assert_eq!(
            places,
            vec![
                ("src/b.rs".into(), 1, "comment".into(), None),
                ("src/lib.rs".into(), 1, "doc".into(), Some("f".into())),
                ("src/lib.rs".into(), 3, "comment".into(), None),
            ]
        );
        // An update reads the changed file again and keeps the other.
        std::fs::write(repo.join("src/b.rs"), "// nothing\n").unwrap();
        crate::update_structure(&repo, &db, &crate::UpdateOptions::default()).unwrap();
        let index = crate::Index::open(&db).unwrap();
        assert_eq!(index.adr_citations("KAIROS-A-0065").unwrap().len(), 2);
        assert!(is_adr_code("KAIROS-A-0065"));
        assert!(!is_adr_code("KAIROS-A-0065 "));
        assert!(!is_adr_code("Queue"));
    }

    #[test]
    fn sql_comments_cite_and_strings_do_not() {
        let found = in_sql(
            "-- KAIROS-A-0062: no long lock.\nSELECT 'KAIROS-A-0001';\n/* COLLIERY-A-0023 */\n",
        );
        let codes: Vec<(&str, usize, usize)> = found
            .iter()
            .map(|c| (c.code.as_str(), c.line, c.col))
            .collect();
        assert_eq!(
            codes,
            vec![("KAIROS-A-0062", 1, 4), ("COLLIERY-A-0023", 3, 4)]
        );
    }
}
