//! Symbols from one file, through the vendored narsil parser, with what the
//! index adds to them: the container, the test mark and the tree hash.

use std::collections::HashMap;
use std::path::Path;

use kairos_narsil::parser::LanguageParser;
use kairos_narsil::symbols::{Symbol, SymbolKind};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Tree};

use crate::calls::{
    CallSite, Import, call_sites, go_package_and_imports, import_ranges, rust_macro_calls,
    rust_macro_ranges, rust_use_ranges, terraform_modules,
};
use crate::tokens;

/// The language of a file, from its extension: the languages of the index
/// (Rust, Python, TypeScript, JavaScript, Go, Terraform, with the names that
/// narsil uses) and SQL
/// (KAIROS-T-0350, read by [`crate::sql`]). A `.tf.json` file is JSON: it
/// has no language (KAIROS-T-0470).
pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1;
    match ext {
        "rs" => Some("rust"),
        "py" | "pyi" => Some("python"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "go" => Some("go"),
        "tf" | "tfvars" => Some("terraform"),
        "sql" => Some("sql"),
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
    /// The token vector of a function or a method (COLLIERY-T-1857).
    pub token_vector: Option<Vec<u32>>,
    /// The count of the tokens of the token vector.
    pub token_count: Option<u32>,
    /// Whether the token vector was made from the code: `false` when it came
    /// from the vectors that the index had for the tree hash.
    pub token_vector_made: bool,
}

/// What one file gives: its symbols, its call sites and, for Rust, the
/// ranges of its `use` declarations and the calls in the text of its macro
/// invocations.
#[derive(Debug, Clone, Default)]
pub struct FileExtract {
    pub symbols: Vec<Extracted>,
    pub calls: Vec<CallSite>,
    pub use_ranges: Vec<(usize, usize)>,
    pub macro_calls: Vec<CallSite>,
    /// The byte ranges of the Rust macro invocations (COLLIERY-T-2531).
    pub macro_ranges: Vec<(usize, usize)>,
    /// The package of a Go file (KAIROS-T-0469).
    pub package: Option<String>,
    /// The imports of a Go file (KAIROS-T-0469), or the module calls of a
    /// Terraform file with their sources (KAIROS-T-0471).
    pub imports: Vec<Import>,
    /// The ADR citations in the comments and the doc strings
    /// (KAIROS-T-0703).
    pub citations: Vec<crate::citations::Citation>,
}

/// Parse `content` and return its symbols in source order, and its call
/// sites. `test_file` marks each symbol as test code. `known` holds the token
/// vectors that the index has, by tree hash: a function with one of these
/// hashes reuses its vector.
pub fn extract(
    parser: &LanguageParser,
    path: &str,
    language: &str,
    content: &str,
    test_file: bool,
    known: &HashMap<String, (Vec<u32>, u32)>,
) -> Result<FileExtract, String> {
    if language == "sql" {
        return Ok(crate::sql::extract(content, test_file));
    }
    let parsed = parser
        .parse_file(Path::new(path), content)
        .map_err(|e| e.to_string())?;
    let tree = parsed.tree.ok_or_else(|| "no syntax tree".to_string())?;

    let mut symbols: Vec<Symbol> = parsed.symbols;
    if language == "go" {
        go_refine(&tree, &mut symbols, content);
    }
    // The kind and the container of each Terraform symbol, by its span and
    // its name (KAIROS-T-0470).
    let mut terraform: HashMap<(usize, usize, String), (&'static str, Option<String>)> =
        HashMap::new();
    if language == "terraform" {
        symbols = terraform_symbols(&tree, &symbols, content, &mut terraform);
    }
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
    let (use_ranges, macro_calls, macro_ranges) = if language == "rust" {
        (
            rust_use_ranges(&tree),
            rust_macro_calls(&tree, source),
            rust_macro_ranges(&tree),
        )
    } else {
        (import_ranges(language, &tree), Vec::new(), Vec::new())
    };
    let (package, imports) = match language {
        "go" => go_package_and_imports(&tree, source),
        "terraform" => (None, terraform_modules(&tree, source)),
        _ => (None, Vec::new()),
    };
    let symbols: Vec<Extracted> = symbols
        .iter()
        .map(|s| {
            let node = definition_node(&tree, s);
            let own = terraform.get(&(s.start_byte, s.end_byte, s.name.clone()));
            let kind = own.map_or_else(|| kind_name(&s.kind), |(kind, _)| *kind);
            let tree_hash = node.map_or_else(
                || hash_text(language, &source[s.start_byte..s.end_byte]),
                |node| tree_hash(language, node, source),
            );
            // The tokens depend only on the tree, so a known tree hash has
            // its vector already. A symbol with no definition node has none.
            let (token_vector, token_vector_made) = match node {
                Some(node) if tokens::has_vector(kind) => match known.get(&tree_hash) {
                    Some(v) => (Some(v.clone()), false),
                    None => (tokens::token_vector(node, source), true),
                },
                _ => (None, false),
            };
            let (token_vector, token_count) = token_vector.unzip();
            let container = match node {
                _ if own.is_some() => own.and_then(|(_, c)| c.clone()),
                Some(node) if language == "go" && kind == "method" => go_receiver(node, source),
                _ => None,
            };
            Extracted {
                name: s.name.clone(),
                container: if own.is_some() {
                    container
                } else {
                    container.or_else(|| container_of(&symbols, s))
                },
                kind,
                start_line: s.start_line,
                end_line: s.end_line,
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                signature: s.signature.clone(),
                tree_hash,
                is_test: test_file
                    || (language == "rust" && node.is_some_and(|n| in_rust_test_code(n, source)))
                    || (language == "go"
                        && kind == "function"
                        && path.ends_with("_test.go")
                        && is_go_test_name(&s.name)),
                token_vector,
                token_count,
                token_vector_made,
            }
        })
        .collect();
    let citations = crate::citations::in_tree(language, &tree, content);
    Ok(FileExtract {
        symbols,
        calls,
        use_ranges,
        macro_calls,
        macro_ranges,
        package,
        imports,
        citations,
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

/// The kind of a name that [`kind_name`] gives.
pub fn kind_from_name(name: &str) -> Option<&'static str> {
    const KINDS: [&str; 28] = [
        // The kinds of a SQL file (KAIROS-T-0350).
        "table",
        "index",
        "view",
        "trigger",
        // The kinds of a Terraform file (KAIROS-T-0470); a module call is
        // a `module`, a variable a `variable`.
        "resource",
        "data",
        "output",
        "local",
        "provider",
        "struct",
        "class",
        "enum",
        "interface",
        "trait",
        "type_alias",
        "function",
        "method",
        "constructor",
        "module",
        "namespace",
        "package",
        "constant",
        "variable",
        "field",
        "parameter",
        "implementation",
        "macro",
        "unknown",
    ];
    KINDS.into_iter().find(|kind| *kind == name)
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
/// the text of the leaves, with no comments, no whitespace and no trailing
/// comma before a closing bracket (a formatter adds one when it wraps a
/// line; `(x,)` and `(x)` still differ, as their node kinds differ). The same
/// code, formatted another way, gives the same hash. It is a part of the key
/// of the symbol's summary in the summary pool (COLLIERY-T-1850).
fn tree_hash(language: &str, node: Node<'_>, source: &[u8]) -> String {
    fn feed(hasher: &mut Sha256, node: Node<'_>, source: &[u8]) {
        if node.kind().contains("comment") {
            return;
        }
        if node.kind() == "," {
            let mut next = node.next_sibling();
            while let Some(n) = next.filter(|n| n.kind().contains("comment")) {
                next = n.next_sibling();
            }
            if next.is_some_and(|n| matches!(n.kind(), ")" | "]" | "}")) {
                return;
            }
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

pub(crate) fn hash_text(language: &str, text: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(language.as_bytes());
    hasher.update([0]);
    hasher.update(text);
    hex(hasher.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The Go symbols as the index keeps them (KAIROS-T-0468). narsil gives
/// each type spec the kind `type_alias`: a struct and an interface get their
/// own kind here. A type, const or var spec that is alone in its declaration
/// (`type S struct {…}`, not `type ( … )`) has the declaration as its span,
/// so its signature starts with the keyword. Each name of `const A, B = 1,
/// 2` is a symbol: the query gives the first only.
fn go_refine(tree: &Tree, symbols: &mut Vec<Symbol>, content: &str) {
    let mut more = Vec::new();
    for s in symbols.iter() {
        let Some(node) = definition_node(tree, s) else {
            continue;
        };
        if matches!(node.kind(), "const_spec" | "var_spec") {
            let mut cursor = node.walk();
            for name in node
                .children_by_field_name("name", &mut cursor)
                .filter(|n| n.kind() == "identifier")
                .skip(1)
            {
                more.push(Symbol {
                    name: content[name.start_byte()..name.end_byte()].to_string(),
                    ..s.clone()
                });
            }
        }
    }
    symbols.extend(more);
    for s in symbols.iter_mut() {
        let Some(node) = definition_node(tree, s) else {
            continue;
        };
        if node.kind() == "type_spec" {
            match node.child_by_field_name("type").map(|t| t.kind()) {
                Some("struct_type") => s.kind = SymbolKind::Struct,
                Some("interface_type") => s.kind = SymbolKind::Interface,
                _ => {}
            }
        }
        if !matches!(
            node.kind(),
            "type_spec" | "type_alias" | "const_spec" | "var_spec"
        ) {
            continue;
        }
        let Some(declaration) = node.parent().filter(|p| {
            p.kind().ends_with("_declaration") && {
                let mut cursor = p.walk();
                !p.children(&mut cursor).any(|c| c.kind() == "(")
            }
        }) else {
            continue;
        };
        s.start_byte = declaration.start_byte();
        s.start_line = declaration.start_position().row + 1;
        let text = &content[declaration.start_byte()..declaration.end_byte()];
        let first_line = text.find('\n').unwrap_or(text.len());
        s.signature = Some(text[..text.floor_char_boundary(first_line.min(200))].to_string());
    }
}

/// The Terraform symbols of the top-level blocks that narsil gives
/// (KAIROS-T-0470), with the kind and the container of each in `own`. The
/// name and the container make the address that Terraform code uses:
///
/// - `resource "aws_s3_bucket" "b"`: `b` in `aws_s3_bucket`, a `resource`;
/// - `data "aws_iam_policy" "p"`: `p` in `data.aws_iam_policy`, a `data`;
/// - `module "net"`: `net` in `module`, a `module`;
/// - `variable "region"`: `region` in `var`, a `variable`;
/// - each entry `name = …` of a `locals` block: `name` in `local`, a `local`;
/// - `output "id"`: `id`, an `output`; `provider "aws"`: `aws`, a `provider`.
///
/// The other blocks (`terraform`, `moved`, `import`, `check`) give none.
fn terraform_symbols(
    tree: &Tree,
    symbols: &[Symbol],
    content: &str,
    own: &mut HashMap<(usize, usize, String), (&'static str, Option<String>)>,
) -> Vec<Symbol> {
    let mut out = Vec::new();
    for s in symbols {
        let Some(block) = definition_node(tree, s) else {
            continue;
        };
        let labels = terraform_labels(block, content);
        let label = |i: usize| labels.get(i).cloned();
        let (name, kind, container) = match (s.name.as_str(), label(0), label(1)) {
            ("resource", Some(ty), Some(name)) => (name, "resource", Some(ty)),
            ("data", Some(ty), Some(name)) => (name, "data", Some(format!("data.{ty}"))),
            ("module", Some(name), _) => (name, "module", Some("module".to_string())),
            ("variable", Some(name), _) => (name, "variable", Some("var".to_string())),
            ("output", Some(name), _) => (name, "output", None),
            ("provider", Some(name), _) => (name, "provider", None),
            ("locals", _, _) => {
                let Some(body) = terraform_body(block) else {
                    continue;
                };
                let mut cursor = body.walk();
                for attribute in body
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() == "attribute")
                {
                    let Some(key) = attribute.named_child(0) else {
                        continue;
                    };
                    let text = &content[attribute.start_byte()..attribute.end_byte()];
                    let first_line = text.find('\n').unwrap_or(text.len());
                    let local = Symbol {
                        name: content[key.start_byte()..key.end_byte()].to_string(),
                        start_line: attribute.start_position().row + 1,
                        end_line: attribute.end_position().row + 1,
                        start_byte: attribute.start_byte(),
                        end_byte: attribute.end_byte(),
                        signature: Some(
                            text[..text.floor_char_boundary(first_line.min(200))].to_string(),
                        ),
                        ..s.clone()
                    };
                    own.insert(
                        (local.start_byte, local.end_byte, local.name.clone()),
                        ("local", Some("local".to_string())),
                    );
                    out.push(local);
                }
                continue;
            }
            _ => continue,
        };
        // The facts of the module map (KAIROS-T-0472): the source and the
        // version of a module call, and the `count` or `for_each` marker.
        let mut facts = Vec::new();
        if kind == "module" {
            for key in ["source", "version"] {
                if let Some(value) = terraform_attribute(block, content, key) {
                    facts.push(format!("{key} = \"{value}\""));
                }
            }
        }
        if matches!(kind, "resource" | "data" | "module") {
            for key in ["count", "for_each"] {
                if terraform_has_attribute(block, content, key) {
                    facts.push(key.to_string());
                }
            }
        }
        let signature = match (&s.signature, facts.is_empty()) {
            (Some(first), false) if first.trim_end().ends_with('{') => {
                Some(format!("{} {} }}", first.trim_end(), facts.join("; ")))
            }
            _ => s.signature.clone(),
        };
        let symbol = Symbol {
            name,
            signature,
            ..s.clone()
        };
        own.insert(
            (symbol.start_byte, symbol.end_byte, symbol.name.clone()),
            (kind, container),
        );
        out.push(symbol);
    }
    out
}

/// The labels of a Terraform block: `["aws_s3_bucket", "b"]` for
/// `resource "aws_s3_bucket" "b" {`.
pub(crate) fn terraform_labels(block: Node<'_>, content: &str) -> Vec<String> {
    let mut cursor = block.walk();
    block
        .named_children(&mut cursor)
        .skip(1)
        .take_while(|n| matches!(n.kind(), "string_lit" | "identifier"))
        .map(|n| {
            content[n.start_byte()..n.end_byte()]
                .trim_matches('"')
                .to_string()
        })
        .collect()
}

/// The text of the string attribute `key` of a Terraform block:
/// `./modules/bucket` for `source = "./modules/bucket"`.
pub(crate) fn terraform_attribute(block: Node<'_>, content: &str, key: &str) -> Option<String> {
    let body = terraform_body(block)?;
    let mut cursor = body.walk();
    let attribute = body.named_children(&mut cursor).find(|n| {
        n.kind() == "attribute"
            && n.named_child(0)
                .is_some_and(|k| &content[k.start_byte()..k.end_byte()] == key)
    })?;
    let value = attribute.named_child(1)?;
    let text = content[value.start_byte()..value.end_byte()].trim();
    (text.starts_with('"') && text.ends_with('"') && text.len() >= 2 && !text.contains("${"))
        .then(|| text[1..text.len() - 1].to_string())
}

/// Whether a Terraform block has the attribute `key`.
fn terraform_has_attribute(block: Node<'_>, content: &str, key: &str) -> bool {
    terraform_body(block).is_some_and(|body| {
        let mut cursor = body.walk();
        body.named_children(&mut cursor).any(|n| {
            n.kind() == "attribute"
                && n.named_child(0)
                    .is_some_and(|k| &content[k.start_byte()..k.end_byte()] == key)
        })
    })
}

/// The body of a Terraform block, if it has one.
pub(crate) fn terraform_body(block: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = block.walk();
    block
        .named_children(&mut cursor)
        .find(|n| n.kind() == "body")
}

/// The receiver type of a Go method: `S` for `func (s *S[T]) m()`. Go keeps
/// a method in the package of its type, so the name is enough.
fn go_receiver(node: Node<'_>, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    let parameter = receiver
        .named_children(&mut cursor)
        .find(|n| n.kind() == "parameter_declaration")?;
    let mut ty = parameter.child_by_field_name("type")?;
    loop {
        ty = match ty.kind() {
            "pointer_type" => ty.named_child(0)?,
            "generic_type" => ty.child_by_field_name("type")?,
            "parenthesized_type" => ty.named_child(0)?,
            "type_identifier" => {
                return Some(
                    String::from_utf8_lossy(&source[ty.start_byte()..ty.end_byte()]).into_owned(),
                );
            }
            _ => return None,
        };
    }
}

/// Whether a Go function name is one that `go test` runs: `Test`,
/// `Benchmark`, `Fuzz` or `Example`, alone or followed by a character that
/// is not a lower-case letter (`TestLoad`, not `Testify`).
fn is_go_test_name(name: &str) -> bool {
    ["Test", "Benchmark", "Fuzz", "Example"]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_lowercase()))
        })
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

    #[test]
    fn each_kind_name_reads_back() {
        use kairos_narsil::symbols::SymbolKind::*;
        for kind in [
            Struct,
            Class,
            Enum,
            Interface,
            Trait,
            TypeAlias,
            Function,
            Method,
            Constructor,
            Module,
            Namespace,
            Package,
            Constant,
            Variable,
            Field,
            Parameter,
            Implementation,
            Macro,
            Unknown,
        ] {
            assert_eq!(kind_from_name(kind_name(&kind)), Some(kind_name(&kind)));
        }
        assert_eq!(kind_from_name("no_such_kind"), None);
    }

    fn symbols(path: &str, content: &str) -> Vec<Extracted> {
        let parser = LanguageParser::new().unwrap();
        extract(
            &parser,
            path,
            language_of(path).unwrap(),
            content,
            false,
            &HashMap::new(),
        )
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
    fn the_tree_hash_ignores_a_trailing_comma() {
        let a = hash_of("a.py", "def f(path):\n    return g(path, 1)\n", "f");
        let b = hash_of(
            "a.py",
            "def f(\n    path,\n):\n    return g(\n        path,\n        1,\n    )\n",
            "f",
        );
        assert_eq!(a, b);
        let tuple = hash_of("a.py", "def f(x):\n    return (x,)\n", "f");
        let plain = hash_of("a.py", "def f(x):\n    return (x)\n", "f");
        assert_ne!(tuple, plain);
        let a = hash_of("a.rs", "fn f() { g(1, 2) }", "f");
        let b = hash_of(
            "a.rs",
            "fn f() {\n    g(\n        1,\n        2,\n    )\n}",
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

    /// KAIROS-T-0468: struct, interface, const and var are Go kinds.
    #[test]
    fn go_kinds_are_split() {
        let code = "package p\n\nconst A, B = 1, 2\n\nconst (\n\tC = iota\n)\n\nvar X int\n\nvar (\n\tY = 1\n)\n\ntype S[T any] struct{ x T }\n\ntype I interface{ M() }\n\ntype N = int\n\ntype (\n\tK int\n)\n\nfunc f() {\n\tvar z int\n\t_ = z\n}\n";
        let found: Vec<_> = symbols("p.go", code)
            .into_iter()
            .map(|s| {
                (
                    s.name,
                    s.kind,
                    s.start_line,
                    s.signature.unwrap_or_default(),
                )
            })
            .collect();
        let found: Vec<_> = found
            .iter()
            .map(|(n, k, l, sig)| (n.as_str(), *k, *l, sig.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("A", "constant", 3, "const A, B = 1, 2"),
                ("B", "constant", 3, "const A, B = 1, 2"),
                ("C", "constant", 6, "C = iota"),
                ("X", "variable", 9, "var X int"),
                ("Y", "variable", 12, "Y = 1"),
                ("S", "struct", 15, "type S[T any] struct{ x T }"),
                ("I", "interface", 17, "type I interface{ M() }"),
                ("N", "type_alias", 19, "type N = int"),
                ("K", "type_alias", 22, "K int"),
                ("f", "function", 25, "func f() {"),
            ]
        );
    }

    /// KAIROS-T-0468: a Go method has its receiver type as its container.
    #[test]
    fn a_go_method_has_its_receiver_as_container() {
        let code = "package p\n\nfunc (s *S[T]) M() {}\n\nfunc (n N) P() {}\n\nfunc (Q) R() {}\n";
        let found: Vec<_> = symbols("p.go", code)
            .into_iter()
            .map(|s| (s.name, s.container))
            .collect();
        assert_eq!(
            found,
            [
                ("M".to_string(), Some("S".to_string())),
                ("P".to_string(), Some("N".to_string())),
                ("R".to_string(), Some("Q".to_string())),
            ]
        );
    }

    /// KAIROS-T-0468: the functions that `go test` runs are tests, in a
    /// `_test.go` file only, also when no rule marks the file as a test.
    #[test]
    fn go_test_functions_are_marked() {
        let code = "package p\n\nfunc TestLoad(t *testing.T) {}\nfunc Test(t *testing.T) {}\nfunc BenchmarkLoad(b *testing.B) {}\nfunc FuzzLoad(f *testing.F) {}\nfunc Example() {}\nfunc ExampleLoad_second() {}\nfunc Testify() {}\nfunc helper() {}\nfunc (s S) TestM() {}\n";
        let marks = |path: &str| -> Vec<(String, bool)> {
            symbols(path, code)
                .into_iter()
                .map(|s| (s.name, s.is_test))
                .collect()
        };
        let expected = |test: bool| -> Vec<(String, bool)> {
            [
                ("TestLoad", test),
                ("Test", test),
                ("BenchmarkLoad", test),
                ("FuzzLoad", test),
                ("Example", test),
                ("ExampleLoad_second", test),
                ("Testify", false),
                ("helper", false),
                ("TestM", false),
            ]
            .iter()
            .map(|(n, t)| (n.to_string(), *t))
            .collect()
        };
        assert_eq!(marks("load_test.go"), expected(true));
        assert_eq!(marks("load.go"), expected(false));
    }

    /// KAIROS-T-0470: the symbols of a Terraform file.
    #[test]
    fn terraform_blocks_are_symbols() {
        let code = "terraform {\n  required_version = \">= 1.5\"\n}\n\nvariable \"region\" {\n  default = \"us-east-1\"\n}\n\nlocals {\n  name = \"app-${var.region}\"\n  tags = {\n    env = local.name\n  }\n}\n\nresource \"aws_s3_bucket\" \"b\" {\n  bucket = local.name\n}\n\ndata \"aws_iam_policy\" \"p\" {\n  name = \"x\"\n}\n\nmodule \"net\" {\n  source = \"./net\"\n}\n\noutput \"id\" {\n  value = module.net.id\n}\n\nprovider \"aws\" {\n  region = var.region\n}\n";
        let found: Vec<_> = symbols("main.tf", code)
            .into_iter()
            .map(|s| {
                (
                    s.container
                        .map_or(s.name.clone(), |c| format!("{c}.{}", s.name)),
                    s.kind,
                    (s.start_line, s.end_line),
                    s.signature.unwrap_or_default(),
                )
            })
            .collect();
        let found: Vec<_> = found
            .iter()
            .map(|(n, k, l, sig)| (n.as_str(), *k, *l, sig.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("var.region", "variable", (5, 7), "variable \"region\" {"),
                (
                    "local.name",
                    "local",
                    (10, 10),
                    "name = \"app-${var.region}\""
                ),
                ("local.tags", "local", (11, 13), "tags = {"),
                (
                    "aws_s3_bucket.b",
                    "resource",
                    (16, 18),
                    "resource \"aws_s3_bucket\" \"b\" {"
                ),
                (
                    "data.aws_iam_policy.p",
                    "data",
                    (20, 22),
                    "data \"aws_iam_policy\" \"p\" {"
                ),
                (
                    "module.net",
                    "module",
                    (24, 26),
                    "module \"net\" { source = \"./net\" }"
                ),
                ("id", "output", (28, 30), "output \"id\" {"),
                ("aws", "provider", (32, 34), "provider \"aws\" {"),
            ]
        );
        assert_eq!(language_of("prod.tfvars"), Some("terraform"));
        assert_eq!(language_of("main.tf.json"), None);
        assert!(symbols("prod.tfvars", "region = \"eu-west-1\"\n").is_empty());
    }

    #[test]
    fn a_method_has_its_impl_as_container() {
        let found = symbols("s.rs", "struct S;\nimpl S {\n    fn m(&self) {}\n}\n");
        let m = found.iter().find(|s| s.name == "m").unwrap();
        assert_eq!(m.container.as_deref(), Some("S"));
    }
}
