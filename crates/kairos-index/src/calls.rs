//! The call sites of one file, from its syntax tree: the called name, how
//! the code writes it, and its place. `edges` gives each call site a target.

use tree_sitter::{Node, Tree};

/// One call in the code.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// arguments are tokens for tree-sitter, and SCIP resolves the calls in it.
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

/// The node of the called name, if `node` is a call.
fn called_name<'t>(language: &str, node: Node<'t>) -> Option<Node<'t>> {
    let function = match (language, node.kind()) {
        ("rust", "call_expression") => node.child_by_field_name("function")?,
        ("python", "call") => node.child_by_field_name("function")?,
        ("typescript" | "tsx", "call_expression") => node.child_by_field_name("function")?,
        ("typescript" | "tsx", "new_expression") => node.child_by_field_name("constructor")?,
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
    fn rust_use_declarations_are_found() {
        let parser = LanguageParser::new().unwrap();
        let code = "use a::b;\nfn f() { use c::d; }\n";
        let tree = parser.parse_to_tree(Path::new("a.rs"), code).unwrap();
        assert_eq!(rust_use_ranges(&tree).len(), 2);
    }
}
