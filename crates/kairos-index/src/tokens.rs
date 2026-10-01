//! The token vector of a function (COLLIERY-T-1857): a MinHash signature of
//! its tokens, for the `near` kind of repeated code.
//!
//! The tokens come from the syntax tree, so whitespace and comments do not
//! count. A copy usually renames its local names and changes a few lines, so:
//!
//! - a local name (`identifier`) becomes `$id`;
//! - a string becomes `$str`, and a number becomes `$num`;
//! - a name that tells what the code uses stays: a field, a method, a
//!   property, a type, a path (`u32::from`), a called function, a macro, and
//!   each name that starts with a capital letter;
//! - commas do not count. So the tokens depend only on the tree hash of the
//!   symbol (which ignores a trailing comma), and a build reuses the vector
//!   of each tree hash that the index has.
//!
//! The vector is the MinHash of the shingles of [`SHINGLE`] tokens. Each
//! shingle is counted with its occurrence (the second `$id = $id` is another
//! element than the first), so the signature estimates the Jaccard
//! similarity of the 2 multisets of shingles. All hashes are fixed (FNV-1a
//! and splitmix64), so a vector stays the same on each machine and in each
//! release. A change here changes the stored vectors: it needs a new schema
//! version.
//!
//! narsil's `embeddings.rs` (TF-IDF) is not used: its vectors depend on the
//! vocabulary and the IDF of all documents, so one new symbol changes each
//! vector, and they cannot be stored for each symbol.

use std::collections::HashMap;

use tree_sitter::Node;

/// The values of one signature.
pub const SIGNATURE_LEN: usize = 128;

/// The tokens of one shingle.
pub const SHINGLE: usize = 4;

/// The kinds of symbol that get a token vector, and that `duplicates` reads.
pub const VECTOR_KINDS: &[&str] = &["function", "method", "constructor"];

/// Whether a symbol of this kind gets a token vector.
pub fn has_vector(kind: &str) -> bool {
    VECTOR_KINDS.contains(&kind)
}

/// The token vector of the symbol at `node` and its count of tokens, or
/// `None` if it has no tokens.
pub fn token_vector(node: Node<'_>, source: &[u8]) -> Option<(Vec<u32>, u32)> {
    let mut tokens = Vec::new();
    collect(node, source, &mut tokens);
    let count = u32::try_from(tokens.len()).unwrap_or(u32::MAX);
    signature(&tokens).map(|v| (v, count))
}

/// The MinHash signature of the shingles of `tokens`.
fn signature(tokens: &[String]) -> Option<Vec<u32>> {
    if tokens.is_empty() {
        return None;
    }
    let width = SHINGLE.min(tokens.len());
    let mut seen: HashMap<u64, u64> = HashMap::new();
    let mut elements = Vec::with_capacity(tokens.len());
    for window in tokens.windows(width) {
        let mut hash = FNV_OFFSET;
        for token in window {
            hash = fnv(hash, token.as_bytes());
            hash = fnv(hash, &[0x1f]);
        }
        let n = seen.entry(hash).or_insert(0);
        *n += 1;
        elements.push(mix(hash ^ mix(*n)));
    }
    let mut out = vec![u32::MAX; SIGNATURE_LEN];
    for (i, slot) in out.iter_mut().enumerate() {
        let seed = mix(0x4b41_4952_4f53_0000 + i as u64);
        for &e in &elements {
            let h = (mix(e ^ seed) >> 32) as u32;
            if h < *slot {
                *slot = h;
            }
        }
    }
    Some(out)
}

/// The share of the places where 2 signatures agree: an estimate of the
/// Jaccard similarity of the 2 multisets of shingles.
pub fn similarity(a: &[u32], b: &[u32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).filter(|(x, y)| x == y).count() as f64 / a.len() as f64
}

/// The bytes of a signature in the index: little-endian u32.
pub fn to_blob(vector: &[u32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn from_blob(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn collect(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
    let kind = node.kind();
    if kind.contains("comment") || kind == "," {
        return;
    }
    if is_string(kind) {
        out.push("$str".into());
        return;
    }
    if is_number(kind) {
        out.push("$num".into());
        return;
    }
    if node.child_count() == 0 {
        let text = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]);
        if text.trim().is_empty() {
            return;
        }
        if is_name(kind) && !kept(node, &text) {
            out.push("$id".into());
        } else {
            out.push(text.into_owned());
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, source, out);
    }
}

fn is_string(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "concatenated_string" | "template_string" | "char_literal" | "rune_literal"
    ) || kind.ends_with("string_literal")
}

fn is_number(kind: &str) -> bool {
    matches!(
        kind,
        "integer_literal"
            | "float_literal"
            | "integer"
            | "float"
            | "number"
            | "int_literal"
            | "imaginary_literal"
    )
}

fn is_name(kind: &str) -> bool {
    kind == "identifier" || kind.ends_with("_identifier")
}

/// Whether a name stays as it is: it tells what the code uses, not how the
/// copy named its own values.
fn kept(node: Node<'_>, text: &str) -> bool {
    if matches!(
        node.kind(),
        "field_identifier"
            | "property_identifier"
            | "private_property_identifier"
            | "type_identifier"
            | "package_identifier"
    ) {
        return true;
    }
    if text.chars().next().is_some_and(char::is_uppercase) {
        return true;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| parent.child_by_field_name(name) == Some(node);
    match parent.kind() {
        // A called function: `load(path)`, `len(row)`.
        "call_expression" | "call" => is_field("function"),
        // A Python attribute: `handle.read`.
        "attribute" => is_field("attribute"),
        // A macro: `format!`.
        "macro_invocation" => is_field("macro"),
        // A Rust path: `std::fs::read`, `u32::from`.
        k => k.starts_with("scoped_"),
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The finalizer of splitmix64.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_narsil::parser::LanguageParser;

    /// The tokens and the vector of the first function of `code`.
    fn of(path: &str, code: &str) -> (Vec<String>, Vec<u32>) {
        let parser = LanguageParser::new().unwrap();
        let parsed = parser.parse_file(std::path::Path::new(path), code).unwrap();
        let tree = parsed.tree.unwrap();
        let s = parsed
            .symbols
            .iter()
            .find(|s| matches!(s.kind, kairos_narsil::symbols::SymbolKind::Function))
            .unwrap();
        let node = tree
            .root_node()
            .descendant_for_byte_range(s.start_byte, s.end_byte)
            .unwrap();
        let mut tokens = Vec::new();
        collect(node, code.as_bytes(), &mut tokens);
        let (vector, count) = token_vector(node, code.as_bytes()).unwrap();
        assert_eq!(count as usize, tokens.len());
        (tokens, vector)
    }

    #[test]
    fn local_names_and_literals_are_normalized() {
        let (tokens, _) = of(
            "a.py",
            "def total(rows):\n    # a comment\n    return len(rows) + rows.count(\"x\", 2)\n",
        );
        assert_eq!(
            tokens.join(" "),
            "def $id ( $id ) : return len ( $id ) + $id . count ( $str $num )"
        );
        let (tokens, _) = of(
            "a.rs",
            "fn f(data: &[u8]) -> Checksum { let s = u32::from(data[0]); Checksum { sum: s.wrapping_add(1) } }",
        );
        assert_eq!(
            tokens.join(" "),
            "fn $id ( $id : & [ u8 ] ) -> Checksum { let $id = u32 :: from ( $id [ $num ] ) ; \
             Checksum { sum : $id . wrapping_add ( $num ) } }"
        );
    }

    #[test]
    fn a_renamed_copy_has_the_same_vector() {
        let (_, a) = of(
            "a.py",
            "def f(rows):\n    for row in rows:\n        print(row)\n",
        );
        let (_, b) = of(
            "b.py",
            "def g(items):\n    for item in items:\n        print(item)\n",
        );
        assert_eq!(similarity(&a, &b), 1.0);
    }

    #[test]
    fn other_code_has_a_low_similarity() {
        let (_, a) = of(
            "a.py",
            "def f(rows):\n    out = []\n    for row in rows:\n        out.append(row.strip())\n    return out\n",
        );
        let (_, b) = of(
            "b.py",
            "def g(path):\n    with open(path) as handle:\n        return json.loads(handle.read())\n",
        );
        assert!(similarity(&a, &b) < 0.2, "{}", similarity(&a, &b));
    }

    #[test]
    fn the_signature_is_fixed() {
        // A stored vector must not change between builds or releases.
        let (_, v) = of("a.rs", "fn f() -> u32 { 1 }");
        assert_eq!(v.len(), SIGNATURE_LEN);
        assert_eq!(from_blob(&to_blob(&v)), v);
        assert_eq!(v[..4], FIRST_VALUES);
    }

    /// The first 4 values of the signature of `fn f() -> u32 { 1 }`.
    const FIRST_VALUES: [u32; 4] = [179_852_798, 54_558_710, 1_448_572_054, 1_336_483_082];
}
