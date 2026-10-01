//! Symbols from one file, through the vendored narsil parser, with what the
//! index adds to them: the container, the test mark and the tree hash.

use std::path::Path;

use kairos_narsil::parser::LanguageParser;
use kairos_narsil::symbols::{Symbol, SymbolKind};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Tree};

use crate::calls::{CallSite, call_sites, rust_use_ranges};

/// The language of a file, from its extension: the languages of the index
/// (Rust, Python, TypeScript, Go), with the names that narsil uses.
pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1;
    match ext {
        "rs" => Some("rust"),
        "py" | "pyi" => Some("python"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "go" => Some("go"),
        _ => None,
    }
}

/// A symbol, ready for the index.
#[derive(Debug, Clone)]
pub struct Extracted {
    pub name: String,
    pub container: Option<String>,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub signature: Option<String>,
    pub tree_hash: String,
    pub is_test: bool,
}

/// What one file gives: its symbols, its call sites and, for Rust, the
/// ranges of its `use` declarations.
#[derive(Debug, Clone, Default)]
pub struct FileExtract {
    pub symbols: Vec<Extracted>,
    pub calls: Vec<CallSite>,
    pub use_ranges: Vec<(usize, usize)>,
}

/// Parse `content` and return its symbols in source order, and its call
/// sites. `test_file` marks each symbol as test code.
pub fn extract(
    parser: &LanguageParser,
    path: &str,
    language: &str,
    content: &str,
    test_file: bool,
) -> Result<FileExtract, String> {
    let parsed = parser
        .parse_file(Path::new(path), content)
        .map_err(|e| e.to_string())?;
    let tree = parsed.tree.ok_or_else(|| "no syntax tree".to_string())?;

    let mut symbols: Vec<Symbol> = parsed.symbols;
    symbols.sort_by(|a, b| {
        (
            a.start_byte,
            std::cmp::Reverse(a.end_byte),
            kind_name(&a.kind),
            &a.name,
        )
            .cmp(&(
                b.start_byte,
                std::cmp::Reverse(b.end_byte),
                kind_name(&b.kind),
                &b.name,
            ))
    });
    symbols.dedup_by(|a, b| {
        a.start_byte == b.start_byte
            && a.end_byte == b.end_byte
            && a.kind == b.kind
            && a.name == b.name
    });

    let source = content.as_bytes();
    let calls = call_sites(language, &tree, source);
    let use_ranges = if language == "rust" {
        rust_use_ranges(&tree)
    } else {
        Vec::new()
    };
    let symbols: Vec<Extracted> = symbols
        .iter()
        .map(|s| {
            let node = definition_node(&tree, s);
            Extracted {
                name: s.name.clone(),
                container: container_of(&symbols, s),
                kind: kind_name(&s.kind),
                start_line: s.start_line,
                end_line: s.end_line,
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                signature: s.signature.clone(),
                tree_hash: node.map_or_else(
                    || hash_text(language, &source[s.start_byte..s.end_byte]),
                    |node| tree_hash(language, node, source),
                ),
                is_test: test_file
                    || (language == "rust" && node.is_some_and(|n| in_rust_test_code(n, source))),
            }
        })
        .collect();
    Ok(FileExtract {
        symbols,
        calls,
        use_ranges,
    })
}

/// The kind in snake case. Stable: it is stored in the index.
pub fn kind_name(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Struct => "struct",
        SymbolKind::Class => "class",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        SymbolKind::Trait => "trait",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Package => "package",
        SymbolKind::Constant => "constant",
        SymbolKind::Variable => "variable",
        SymbolKind::Field => "field",
        SymbolKind::Parameter => "parameter",
        SymbolKind::Implementation => "implementation",
        SymbolKind::Macro => "macro",
        SymbolKind::Unknown => "unknown",
    }
}

/// The name of the smallest other symbol whose span holds this one.
fn container_of(symbols: &[Symbol], s: &Symbol) -> Option<String> {
    symbols
        .iter()
        .filter(|o| {
            o.start_byte <= s.start_byte
                && o.end_byte >= s.end_byte
                && (o.start_byte, o.end_byte) != (s.start_byte, s.end_byte)
        })
        .min_by_key(|o| o.end_byte - o.start_byte)
        .map(|o| o.name.clone())
}

fn definition_node<'t>(tree: &'t Tree, s: &Symbol) -> Option<Node<'t>> {
    let node = tree
        .root_node()
        .descendant_for_byte_range(s.start_byte, s.end_byte)?;
    (node.start_byte() == s.start_byte && node.end_byte() == s.end_byte).then_some(node)
}

/// The hash of the normalized syntax tree of a symbol: the node kinds and
/// the text of the leaves, with no comments and no whitespace. The same code,
/// formatted another way, gives the same hash. It keys the symbol's summary
/// in the summary pool (COLLIERY-T-1850).
fn tree_hash(language: &str, node: Node<'_>, source: &[u8]) -> String {
    fn feed(hasher: &mut Sha256, node: Node<'_>, source: &[u8]) {
        if node.kind().contains("comment") {
            return;
        }
        hasher.update(b"(");
        hasher.update(node.kind().as_bytes());
        if node.child_count() == 0 {
            hasher.update([0x1f]);
            hasher.update(&source[node.start_byte()..node.end_byte()]);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            feed(hasher, child, source);
        }
        hasher.update(b")");
    }
    let mut hasher = Sha256::new();
    hasher.update(language.as_bytes());
    hasher.update([0]);
    feed(&mut hasher, node, source);
    hex(hasher.finalize().as_slice())
}

fn hash_text(language: &str, text: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(language.as_bytes());
    hasher.update([0]);
    hasher.update(text);
    hex(hasher.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a Rust item is test code: it, or an item that holds it, has a
/// `#[test]`, `#[…::test]` or `#[cfg(test)]` attribute.
fn in_rust_test_code(node: Node<'_>, source: &[u8]) -> bool {
    let mut current = Some(node);
    while let Some(item) = current {
        let mut sibling = item.prev_sibling();
        while let Some(s) = sibling {
            match s.kind() {
                "attribute_item" => {
                    let text: String =
                        String::from_utf8_lossy(&source[s.start_byte()..s.end_byte()])
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect();
                    if text == "#[test]" || text.ends_with("::test]") || text.contains("cfg(test)")
                    {
                        return true;
                    }
                }
                "line_comment" | "block_comment" => {}
                _ => break,
            }
            sibling = s.prev_sibling();
        }
        current = item.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(path: &str, content: &str) -> Vec<Extracted> {
        let parser = LanguageParser::new().unwrap();
        extract(&parser, path, language_of(path).unwrap(), content, false)
            .unwrap()
            .symbols
    }

    fn hash_of(path: &str, content: &str, name: &str) -> String {
        symbols(path, content)
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
            .tree_hash
    }

    #[test]
    fn the_tree_hash_ignores_whitespace_and_comments() {
        let a = hash_of("a.rs", "fn f(x: u32) -> u32 { x + 1 }", "f");
        let b = hash_of(
            "b.rs",
            "fn f(x: u32) -> u32 {\n    // add one\n    x + 1\n}\n",
            "f",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn the_tree_hash_changes_with_the_code() {
        let a = hash_of("a.rs", "fn f(x: u32) -> u32 { x + 1 }", "f");
        let b = hash_of("a.rs", "fn f(x: u32) -> u32 { x + 2 }", "f");
        assert_ne!(a, b);
    }

    #[test]
    fn the_tree_hash_does_not_depend_on_the_place() {
        let a = hash_of("a.py", "def f(x):\n    return x\n", "f");
        let b = hash_of("b.py", "import os\n\n\ndef f(x):\n    return x\n", "f");
        assert_eq!(a, b);
    }

    #[test]
    fn rust_test_code_is_marked() {
        let found = symbols(
            "lib.rs",
            "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}\n\n#[tokio::test]\nasync fn async_case() {}\n",
        );
        let marks: Vec<_> = found.iter().map(|s| (s.name.as_str(), s.is_test)).collect();
        assert_eq!(
            marks,
            [
                ("real", false),
                ("tests", true),
                ("it_works", true),
                ("async_case", true)
            ]
        );
    }

    #[test]
    fn a_method_has_its_impl_as_container() {
        let found = symbols("s.rs", "struct S;\nimpl S {\n    fn m(&self) {}\n}\n");
        let m = found.iter().find(|s| s.name == "m").unwrap();
        assert_eq!(m.container.as_deref(), Some("S"));
    }
}
