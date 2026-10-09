//! The call sites of one file, from its syntax tree: the called name, how
//! the code writes it, and its place. `edges` gives each call site a target.

use tree_sitter::{Node, Tree};

/// One call in the code.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CallSite {
    /// The called name: the last part of the path (`push` in `a.b.push(x)`).
    pub name: String,
    /// The path as the code writes it (`self.items.push`), or only the name
    /// when the part before the name is not a plain path (`f(x).push`).
    pub written: String,
    /// The byte range of the called name.
    pub start_byte: usize,
    pub end_byte: usize,
    /// The place of the called name, from 1. The column counts bytes.
    pub line: usize,
    pub col: usize,
}

/// The call sites of a file, in source order. `language` is a name that
/// `extract::language_of` gives. A Rust macro call is not a call site: its
/// arguments are tokens for tree-sitter. SCIP resolves the calls in a macro
/// that it expands, and [`rust_macro_calls`] finds the others in the text.
pub fn call_sites(language: &str, tree: &Tree, source: &[u8]) -> Vec<CallSite> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if let Some(name) = called_name(language, node) {
            out.push(site(name, node, source));
        }
        // Children in reverse, so that the stack gives source order.
        let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

/// The byte ranges of the Rust `use` declarations of a file. A SCIP
/// reference in one of them is not a call.
pub fn rust_use_ranges(tree: &Tree) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "use_declaration" {
            out.push((node.start_byte(), node.end_byte()));
            continue;
        }
        stack.extend(node.children(&mut cursor));
    }
    out
}

/// The byte ranges of the Rust macro invocations of a file, outermost only.
/// A SCIP definition in one of them is a function that the macro makes
/// (COLLIERY-T-2531).
pub fn rust_macro_ranges(tree: &Tree) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "macro_invocation" {
            out.push((node.start_byte(), node.end_byte()));
            continue;
        }
        stack.extend(node.children(&mut cursor));
    }
    out.sort_unstable();
    out
}

/// The calls in the text of each Rust macro invocation, in source order:
/// narsil's fallback (`macro_text_calls`, COLLIERY-T-1851). It finds
/// `name(`, `obj.method(` and `Type::method(` in the tokens, with no code
/// for one library. A name in a string literal of the macro is not a call.
/// `edges` gives such a call a name class only where SCIP resolved nothing.
///
/// The body (the right side of each rule) of a `macro_rules!` definition is
/// read the same way: SCIP gives no reference there, and the call has the
/// macro as its caller (KAIROS-T-0353). A `$name(` metavariable and a
/// `fn name(` definition in the body are not calls.
pub fn rust_macro_calls(tree: &Tree, source: &[u8]) -> Vec<CallSite> {
    let mut out: Vec<CallSite> = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    let starts: Vec<usize> = std::iter::once(0)
        .chain(
            source
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\n')
                .map(|(i, _)| i + 1),
        )
        .collect();
    while let Some(node) = stack.pop() {
        match node.kind() {
            "macro_invocation" => {
                text_calls(
                    source,
                    node.start_byte(),
                    node.end_byte(),
                    &starts,
                    false,
                    &mut out,
                );
                continue;
            }
            "macro_definition" => {
                let mut rules = node.walk();
                for rule in node
                    .children(&mut rules)
                    .filter(|n| n.kind() == "macro_rule")
                {
                    if let Some(body) = rule.child_by_field_name("right") {
                        text_calls(
                            source,
                            body.start_byte(),
                            body.end_byte(),
                            &starts,
                            true,
                            &mut out,
                        );
                    }
                }
                continue;
            }
            _ => {}
        }
        let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out.sort_by_key(|c| c.start_byte);
    out.dedup_by_key(|c| c.start_byte);
    out
}

/// The calls in `source[start..end]`, the text of a macro. `body` is true
/// for the body of a `macro_rules!` definition.
fn text_calls(
    source: &[u8],
    start: usize,
    end: usize,
    starts: &[usize],
    body: bool,
    out: &mut Vec<CallSite>,
) {
    let Ok(text) = std::str::from_utf8(&source[start..end]) else {
        return;
    };
    let strings = string_ranges(text);
    for (offset, name) in kairos_narsil::callgraph::macro_text_calls(text) {
        if strings.iter().any(|&(s, e)| s <= offset && offset < e) {
            continue;
        }
        if body {
            let before = text[..offset].trim_end();
            if text[..offset].ends_with('$')
                || before.ends_with("fn") && !before[..before.len() - 2].ends_with(is_ident_char)
            {
                continue;
            }
        }
        let path_start = text[..offset]
            .rfind(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | ':' | '.')))
            .map_or(0, |i| i + 1);
        let written = text[path_start..offset + name.len()]
            .trim_start_matches([':', '.'])
            .to_string();
        let byte = start + offset;
        let line = starts.partition_point(|&s| s <= byte);
        out.push(CallSite {
            name: name.to_string(),
            written,
            start_byte: byte,
            end_byte: byte + name.len(),
            line,
            col: byte - starts[line - 1] + 1,
        });
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte ranges of the string literals in the text of a Rust macro:
/// `"..."` with escapes, and raw strings `r"..."`, `r#"..."#`.
fn string_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // A char literal: '"' or '\'' or 'x'. A lifetime has no closing quote.
            b'\'' => {
                let end = if bytes.get(i + 1) == Some(&b'\\') {
                    i + 3
                } else {
                    i + 2
                };
                i = if bytes.get(end) == Some(&b'\'') {
                    end + 1
                } else {
                    i + 1
                };
            }
            b'"' => {
                // The hashes of a raw string, before the quote.
                let mut hashes = 0;
                while i > hashes && bytes[i - 1 - hashes] == b'#' {
                    hashes += 1;
                }
                let raw = i > hashes && bytes[i - 1 - hashes] == b'r';
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if !raw && bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"'
                        && bytes[i + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|b| **b == b'#')
                            .count()
                            == hashes
                    {
                        break;
                    }
                    i += 1;
                }
                out.push((start, (i + 1).min(bytes.len())));
                i += 1 + hashes;
            }
            _ => i += 1,
        }
    }
    out
}

/// The node of the called name, if `node` is a call.
fn called_name<'t>(language: &str, node: Node<'t>) -> Option<Node<'t>> {
    let function = match (language, node.kind()) {
        ("rust", "call_expression") => node.child_by_field_name("function")?,
        ("python", "call") => node.child_by_field_name("function")?,
        ("typescript" | "tsx" | "javascript", "call_expression") => {
            node.child_by_field_name("function")?
        }
        ("typescript" | "tsx" | "javascript", "new_expression") => {
            node.child_by_field_name("constructor")?
        }
        ("go", "call_expression") => node.child_by_field_name("function")?,
        _ => return None,
    };
    name_of(function)
}

/// The last name of a callee expression: `f`, `a::f`, `a.f`, `f::<T>`.
fn name_of(function: Node<'_>) -> Option<Node<'_>> {
    match function.kind() {
        "identifier" => Some(function),
        // Rust
        "scoped_identifier" => function.child_by_field_name("name"),
        "field_expression" => function.child_by_field_name("field"),
        "generic_function" => name_of(function.child_by_field_name("function")?),
        // Python
        "attribute" => function.child_by_field_name("attribute"),
        // TypeScript
        "member_expression" => function.child_by_field_name("property"),
        // Go
        "selector_expression" => function.child_by_field_name("field"),
        _ => None,
    }
}

fn site(name: Node<'_>, call: Node<'_>, source: &[u8]) -> CallSite {
    let name_text =
        String::from_utf8_lossy(&source[name.start_byte()..name.end_byte()]).into_owned();
    // The callee expression ends at the name (a Rust turbofish follows it).
    let function = call
        .child_by_field_name(if call.kind() == "new_expression" {
            "constructor"
        } else {
            "function"
        })
        .unwrap_or(name);
    let path: String = String::from_utf8_lossy(&source[function.start_byte()..name.end_byte()])
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let plain = path.len() <= 120
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | ':'));
    let written = if plain { path } else { name_text.clone() };
    CallSite {
        name: name_text,
        written,
        start_byte: name.start_byte(),
        end_byte: name.end_byte(),
        line: name.start_position().row + 1,
        col: name.start_position().column + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_narsil::parser::LanguageParser;
    use std::path::Path;

    fn sites(path: &str, language: &str, code: &str) -> Vec<(String, String, usize)> {
        let parser = LanguageParser::new().unwrap();
        let tree = parser.parse_to_tree(Path::new(path), code).unwrap();
        call_sites(language, &tree, code.as_bytes())
            .into_iter()
            .map(|s| (s.name, s.written, s.line))
            .collect()
    }

    fn own(v: &[(&str, &str, usize)]) -> Vec<(String, String, usize)> {
        v.iter()
            .map(|(a, b, l)| (a.to_string(), b.to_string(), *l))
            .collect()
    }

    #[test]
    fn rust_calls_in_source_order_with_no_macro_calls() {
        let code = "fn f() {\n    let q = Queue::new();\n    q.push(g(1));\n    assert_eq!(h(), 2);\n    x::<u8>();\n}\n";
        assert_eq!(
            sites("a.rs", "rust", code),
            own(&[
                ("new", "Queue::new", 2),
                ("push", "q.push", 3),
                ("g", "g", 3),
                ("x", "x", 5),
            ])
        );
    }

    #[test]
    fn a_receiver_that_is_not_a_path_is_left_out() {
        let code = "def f(line):\n    return line.rstrip('x').split(',')\n";
        assert_eq!(
            sites("a.py", "python", code),
            own(&[("split", "split", 2), ("rstrip", "line.rstrip", 2)])
        );
    }

    #[test]
    fn typescript_new_and_member_calls() {
        let code = "function f() {\n  const c = new Cart();\n  this.items.push(1);\n}\n";
        assert_eq!(
            sites("a.ts", "typescript", code),
            own(&[("Cart", "Cart", 2), ("push", "this.items.push", 3)])
        );
    }

    #[test]
    fn go_selector_calls() {
        let code = "package p\nfunc f() {\n\tfmt.Println(g())\n}\n";
        assert_eq!(
            sites("a.go", "go", code),
            own(&[("Println", "fmt.Println", 3), ("g", "g", 3)])
        );
    }

    #[test]
    fn calls_in_the_text_of_a_rust_macro() {
        let parser = LanguageParser::new().unwrap();
        let code = "fn f() {\n    m!(g(1), \"h(2)\", Queue::new(), self.items.len());\n    println!(r#\"x(\"#, k ( 3 ));\n}\n";
        let tree = parser.parse_to_tree(Path::new("a.rs"), code).unwrap();
        let got: Vec<_> = rust_macro_calls(&tree, code.as_bytes())
            .into_iter()
            .map(|c| (c.name, c.written, c.line, c.col))
            .collect();
        assert_eq!(
            got,
            [
                ("g".to_string(), "g".to_string(), 2, 8),
                ("new".to_string(), "Queue::new".to_string(), 2, 29),
                ("len".to_string(), "self.items.len".to_string(), 2, 47),
                ("k".to_string(), "k".to_string(), 3, 23),
            ]
        );
    }

    /// KAIROS-T-0353: the calls in the body of a `macro_rules!` definition,
    /// with no metavariable, no `fn` definition and no call of the matcher.
    #[test]
    fn calls_in_a_macro_body_are_found() {
        let parser = LanguageParser::new().unwrap();
        let code = "macro_rules! m {\n    ($name:ident) => {\n        pub fn $name(id: u32) -> bool {\n            check(id) && $name(1) && rules::valid(id)\n        }\n    };\n}\n";
        let tree = parser.parse_to_tree(Path::new("a.rs"), code).unwrap();
        let got: Vec<(String, String, usize)> = rust_macro_calls(&tree, code.as_bytes())
            .into_iter()
            .map(|c| (c.name, c.written, c.line))
            .collect();
        assert_eq!(
            got,
            [
                ("check".to_string(), "check".to_string(), 4),
                ("valid".to_string(), "rules::valid".to_string(), 4),
            ]
        );
    }

    #[test]
    fn rust_macro_invocations_are_found() {
        let parser = LanguageParser::new().unwrap();
        let code = "make!(f, 1);\nfn g() {\n    vec![1];\n}\n";
        let tree = parser.parse_to_tree(Path::new("a.rs"), code).unwrap();
        let got: Vec<&str> = rust_macro_ranges(&tree)
            .into_iter()
            .map(|(s, e)| &code[s..e])
            .collect();
        assert_eq!(got, ["make!(f, 1)", "vec![1]"]);
    }

    #[test]
    fn rust_use_declarations_are_found() {
        let parser = LanguageParser::new().unwrap();
        let code = "use a::b;\nfn f() { use c::d; }\n";
        let tree = parser.parse_to_tree(Path::new("a.rs"), code).unwrap();
        assert_eq!(rust_use_ranges(&tree).len(), 2);
    }
}
