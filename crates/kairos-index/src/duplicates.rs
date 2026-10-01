//! Repeated code (COLLIERY-T-1857): groups of functions that are the same
//! code, nearly the same code, or the same idea in other code.
//!
//! | Kind | From | Score |
//! |---|---|---|
//! | `exact` | the same tree hash: the same code with other whitespace or comments | 1 |
//! | `near` | the token vectors (see `tokens`): a copy with renamed names or a few changed lines | the estimated Jaccard similarity of the shingles |
//! | `same-idea` | the summary vectors: the same job in other code | the cosine of the 2 summary vectors |
//!
//! Only functions and methods are compared: a type or an `impl` block holds
//! them, and would give each group again. A `near` pair has 2 symbols of one
//! language; a `same-idea` pair can have 2 languages. A `same-idea` pair is
//! left out when the 2 symbols are already in one `exact` or `near` group.
//!
//! A group joins each pair over the threshold that shares a symbol, and its
//! score is the lowest score of these pairs.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::query::{SYMBOL_COLUMNS, SYMBOL_FROM, symbol_info};
use crate::tokens::{self, VECTOR_KINDS};
use crate::{Index, IndexError, SymbolInfo};

/// The default smallest symbol, in lines.
pub const DEFAULT_MIN_LINES: u32 = 5;

/// The lowest estimated Jaccard similarity of a `near` pair. Calibrated on
/// Kairos at `6ffd848` (COLLIERY-T-1857): 93 groups at 0.7 (69 at 0.8), and
/// 9 of 10 groups checked by hand were repeated code. A renamed copy with one
/// changed line of 10 scores about 0.73. See `COLLIERY-I-0264`, "Repeated code
/// on Kairos, 2026-10-01".
pub const NEAR_THRESHOLD: f64 = 0.7;

/// The lowest cosine of the summary vectors of a `same-idea` pair. Not
/// calibrated: no index of Kairos had summaries when it was set.
pub const SAME_IDEA_THRESHOLD: f64 = 0.92;

/// The fewest tokens of a function that a `near` pair can have. A smaller
/// function is mostly its signature and one call, and on Kairos such
/// functions matched with no common code (`chat` and
/// `only_delivery_board_note`: one `format!` of a string each). An exact copy
/// of any size is still found.
pub const MIN_NEAR_TOKENS: u32 = 30;

/// The rows of a signature in one band of the LSH that finds the `near`
/// candidates: 32 bands of 4. A pair with a similarity of 0.5 is a
/// candidate with a probability of 0.87, one of 0.7 with 0.9995.
const BAND_ROWS: usize = 4;

/// A kind of repeated code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DuplicateKind {
    /// The same normalized tree.
    Exact,
    /// Token vectors over the threshold.
    Near,
    /// Summary vectors over the threshold.
    SameIdea,
}

impl DuplicateKind {
    pub const ALL: [DuplicateKind; 3] = [
        DuplicateKind::Exact,
        DuplicateKind::Near,
        DuplicateKind::SameIdea,
    ];

    /// The name of the kind, as the tools show it.
    pub fn as_str(self) -> &'static str {
        match self {
            DuplicateKind::Exact => "exact",
            DuplicateKind::Near => "near",
            DuplicateKind::SameIdea => "same-idea",
        }
    }
}

/// Which repeated code to find.
#[derive(Debug, Clone)]
pub struct DuplicateOptions {
    /// The kinds to find. Default: all 3.
    pub kinds: Vec<DuplicateKind>,
    /// The smallest symbol, in lines. Default 5.
    pub min_lines: u32,
    /// Also compare test code. Default false.
    pub include_tests: bool,
    /// Only the symbols of the files under this path.
    pub under: Option<String>,
    /// The lowest score of a `near` pair.
    pub near_threshold: f64,
    /// The lowest score of a `same-idea` pair.
    pub same_idea_threshold: f64,
}

impl Default for DuplicateOptions {
    fn default() -> Self {
        DuplicateOptions {
            kinds: DuplicateKind::ALL.to_vec(),
            min_lines: DEFAULT_MIN_LINES,
            include_tests: false,
            under: None,
            near_threshold: NEAR_THRESHOLD,
            same_idea_threshold: SAME_IDEA_THRESHOLD,
        }
    }
}

/// One group of repeated code.
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateGroup {
    pub kind: DuplicateKind,
    /// From 0 to 1: the lowest score of the pairs that joined the group.
    pub score: f64,
    /// By file and place.
    pub symbols: Vec<SymbolInfo>,
}

/// What a search for repeated code found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Duplicates {
    /// By kind (`exact`, `near`, `same-idea`), then the best score first,
    /// then the largest symbol first.
    pub groups: Vec<DuplicateGroup>,
    /// The symbols that were compared.
    pub compared: usize,
    /// Whether the compared symbols have summary vectors. Without them,
    /// no `same-idea` group is found.
    pub summary_vectors: bool,
}

/// One symbol that a search compares.
struct Candidate {
    info: SymbolInfo,
    tree_hash: String,
    token_vector: Option<Vec<u32>>,
    token_count: u32,
    start_byte: u32,
    end_byte: u32,
    summary_key: Option<String>,
    summary_vector: Option<Vec<f32>>,
}

impl Candidate {
    /// Rust, Python, TypeScript (with TSX) or Go: a `near` pair has one.
    fn family(&self) -> &str {
        match self.info.language.as_str() {
            "tsx" => "typescript",
            other => other,
        }
    }
}

impl Index {
    /// Find repeated code: the groups of the kinds in `options`.
    pub fn duplicates(&self, options: &DuplicateOptions) -> Result<Duplicates, IndexError> {
        let candidates = self.candidates(options)?;
        let mut found = Duplicates {
            compared: candidates.len(),
            summary_vectors: candidates.iter().any(|c| c.summary_vector.is_some()),
            ..Duplicates::default()
        };
        let wants = |k| options.kinds.contains(&k);

        // The symbols of each tree hash. An exact group is a tree hash with
        // more than one symbol.
        let mut by_hash: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, c) in candidates.iter().enumerate() {
            by_hash.entry(c.tree_hash.as_str()).or_default().push(i);
        }
        let hashes: Vec<Vec<usize>> = by_hash.into_values().collect();
        // Joined symbols: exact and near groups. A same-idea pair inside one
        // of them is left out.
        let mut joined = UnionFind::new(candidates.len());
        for members in &hashes {
            for &m in &members[1..] {
                joined.union(members[0], m);
            }
        }
        if wants(DuplicateKind::Exact) {
            for members in hashes.iter().filter(|m| m.len() > 1) {
                found.groups.push(group(
                    DuplicateKind::Exact,
                    1.0,
                    members.clone(),
                    &candidates,
                ));
            }
        }

        // Near: one node for each tree hash with a token vector.
        let nodes: Vec<&Vec<usize>> = hashes
            .iter()
            .filter(|m| {
                let c = &candidates[m[0]];
                c.token_vector.is_some() && c.token_count >= MIN_NEAR_TOKENS
            })
            .collect();
        let pairs = near_pairs(&nodes, &candidates, options.near_threshold);
        for (members, score) in components(nodes.len(), &pairs) {
            let symbols: Vec<usize> = members.iter().flat_map(|&n| nodes[n].clone()).collect();
            for &s in &symbols[1..] {
                joined.union(symbols[0], s);
            }
            if wants(DuplicateKind::Near) {
                found
                    .groups
                    .push(group(DuplicateKind::Near, score, symbols, &candidates));
            }
        }

        if wants(DuplicateKind::SameIdea) {
            // One node for each summary key with a vector: the same key is
            // the same tree, so an exact copy.
            let mut by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for (i, c) in candidates.iter().enumerate() {
                if let (Some(key), Some(_)) = (&c.summary_key, &c.summary_vector) {
                    by_key.entry(key.as_str()).or_default().push(i);
                }
            }
            let nodes: Vec<Vec<usize>> = by_key.into_values().collect();
            let mut pairs = Vec::new();
            for a in 0..nodes.len() {
                for b in a + 1..nodes.len() {
                    let (x, y) = (nodes[a][0], nodes[b][0]);
                    if joined.find(x) == joined.find(y) {
                        continue;
                    }
                    let (Some(va), Some(vb)) =
                        (&candidates[x].summary_vector, &candidates[y].summary_vector)
                    else {
                        continue;
                    };
                    if let Some(cos) = kairos_embed::cosine(va, vb).map(f64::from)
                        && cos >= options.same_idea_threshold
                    {
                        pairs.push((a, b, cos.min(1.0)));
                    }
                }
            }
            for (members, score) in components(nodes.len(), &pairs) {
                let symbols = members.iter().flat_map(|&n| nodes[n].clone()).collect();
                found
                    .groups
                    .push(group(DuplicateKind::SameIdea, score, symbols, &candidates));
            }
        }

        found.groups.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then(b.score.total_cmp(&a.score))
                .then(largest(b).cmp(&largest(a)))
                .then_with(|| a.symbols[0].place().cmp(&b.symbols[0].place()))
        });
        Ok(found)
    }

    /// The functions and methods that `options` keeps.
    fn candidates(&self, options: &DuplicateOptions) -> Result<Vec<Candidate>, IndexError> {
        let kinds = VECTOR_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SYMBOL_COLUMNS}, s.tree_hash, s.token_vector, m.vector, f.decision,
                    s.summary_key, coalesce(s.token_count, 0), s.start_byte, s.end_byte
             FROM {SYMBOL_FROM}
             WHERE s.kind IN ({kinds}) AND s.end_line - s.start_line + 1 >= ?1
             ORDER BY f.path, s.start_byte"
        ))?;
        let rows = stmt.query_map([options.min_lines.max(1)], |r| {
            let token_vector: Option<Vec<u8>> = r.get(12)?;
            let summary_vector: Option<Vec<u8>> = r.get(13)?;
            let decision: String = r.get(14)?;
            Ok((
                Candidate {
                    info: symbol_info(r, 0)?,
                    tree_hash: r.get(11)?,
                    token_vector: token_vector.map(|b| tokens::from_blob(&b)),
                    token_count: r.get(16)?,
                    start_byte: r.get(17)?,
                    end_byte: r.get(18)?,
                    summary_key: r.get(15)?,
                    summary_vector: summary_vector.map(|b| {
                        b.chunks_exact(4)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect()
                    }),
                },
                decision,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (c, decision) = row?;
            if !options.include_tests && (c.info.is_test || decision == "test") {
                continue;
            }
            if let Some(under) = &options.under
                && !c.info.file.starts_with(under.as_str())
            {
                continue;
            }
            out.push(c);
        }
        Ok(out)
    }
}

/// The most lines of a symbol of the group.
fn largest(g: &DuplicateGroup) -> u32 {
    g.symbols
        .iter()
        .map(|s| s.end_line - s.start_line + 1)
        .max()
        .unwrap_or(0)
}

fn group(
    kind: DuplicateKind,
    score: f64,
    mut members: Vec<usize>,
    candidates: &[Candidate],
) -> DuplicateGroup {
    members.sort_unstable();
    members.dedup();
    DuplicateGroup {
        kind,
        score,
        symbols: members
            .into_iter()
            .map(|i| candidates[i].info.clone())
            .collect(),
    }
}

/// The pairs of nodes (tree hashes) whose token vectors are at least
/// `threshold` alike, with their score. LSH bands give the candidates, so
/// that not each pair is compared.
fn near_pairs(
    nodes: &[&Vec<usize>],
    candidates: &[Candidate],
    threshold: f64,
) -> Vec<(usize, usize, f64)> {
    let vector = |n: usize| {
        candidates[nodes[n][0]]
            .token_vector
            .as_deref()
            .expect("a node has a token vector")
    };
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut pairs = Vec::new();
    for band in 0..tokens::SIGNATURE_LEN / BAND_ROWS {
        let rows = band * BAND_ROWS..(band + 1) * BAND_ROWS;
        let mut buckets: HashMap<(&str, &[u32]), Vec<usize>> = HashMap::new();
        for n in 0..nodes.len() {
            let family = candidates[nodes[n][0]].family();
            buckets
                .entry((family, &vector(n)[rows.clone()]))
                .or_default()
                .push(n);
        }
        for bucket in buckets.values().filter(|b| b.len() > 1) {
            for (i, &a) in bucket.iter().enumerate() {
                for &b in &bucket[i + 1..] {
                    if !seen.insert((a, b))
                        || nested(&candidates[nodes[a][0]], &candidates[nodes[b][0]])
                    {
                        continue;
                    }
                    let score = tokens::similarity(vector(a), vector(b));
                    if score >= threshold {
                        pairs.push((a, b, score));
                    }
                }
            }
        }
    }
    pairs
}

/// Whether one symbol holds the other: a function and a function defined in
/// it share their code, so they are not a copy.
fn nested(a: &Candidate, b: &Candidate) -> bool {
    a.info.file == b.info.file
        && ((a.start_byte <= b.start_byte && b.end_byte <= a.end_byte)
            || (b.start_byte <= a.start_byte && a.end_byte <= b.end_byte))
}

/// The connected components of `pairs` over `n` nodes that have more than
/// one node, each with the lowest score of its pairs.
fn components(n: usize, pairs: &[(usize, usize, f64)]) -> Vec<(Vec<usize>, f64)> {
    let mut sets = UnionFind::new(n);
    for &(a, b, _) in pairs {
        sets.union(a, b);
    }
    let mut lowest: HashMap<usize, f64> = HashMap::new();
    for &(a, _, score) in pairs {
        let root = sets.find(a);
        let entry = lowest.entry(root).or_insert(score);
        *entry = entry.min(score);
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for node in 0..n {
        let root = sets.find(node);
        if lowest.contains_key(&root) {
            members.entry(root).or_default().push(node);
        }
    }
    members
        .into_iter()
        .map(|(root, m)| (m, lowest[&root]))
        .collect()
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[a.max(b)] = a.min(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_keep_the_lowest_score() {
        let found = components(5, &[(0, 1, 0.9), (1, 2, 0.85), (3, 4, 0.95)]);
        assert_eq!(found, vec![(vec![0, 1, 2], 0.85), (vec![3, 4], 0.95)]);
        assert!(components(3, &[]).is_empty());
    }
}
