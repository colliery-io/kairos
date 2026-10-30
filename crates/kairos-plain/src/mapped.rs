//! A string that remembers where each of its bytes came from.
//!
//! The checker cleans a text before it applies a rule: it drops code fences,
//! puts `CODE` in place of inline code, joins the lines of a paragraph. A
//! violation must still point at the text the caller sent, so each byte of
//! the cleaned text keeps the byte range of the input it stands for. A copied
//! byte stands for itself; an inserted byte (`CODE`, a joining space) stands
//! for the whole range it replaced, and is marked synthetic.

use std::ops::Range;

use regex::Regex;

#[derive(Clone, Debug, Default)]
pub(crate) struct Mapped {
    pub(crate) text: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
    synthetic: Vec<bool>,
}

/// What a match of [`Mapped::replace_all`] becomes.
#[derive(Clone, Copy)]
pub(crate) enum Rep<'a> {
    /// Nothing: the match goes.
    Delete,
    /// A fixed text, synthetic, that stands for the whole match.
    Literal(&'a str),
    /// The bytes of one capture group, copied with their own origin.
    Group(usize),
}

impl Mapped {
    pub(crate) fn new(text: &str) -> Self {
        let n = text.len();
        Self {
            text: text.to_owned(),
            starts: (0..n).collect(),
            ends: (1..=n).collect(),
            synthetic: vec![false; n],
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.text.len()
    }

    fn push_copy(&mut self, from: &Mapped, range: Range<usize>) {
        self.text.push_str(&from.text[range.clone()]);
        self.starts.extend_from_slice(&from.starts[range.clone()]);
        self.ends.extend_from_slice(&from.ends[range.clone()]);
        self.synthetic.extend_from_slice(&from.synthetic[range]);
    }

    fn push_synthetic(&mut self, text: &str, origin: Range<usize>) {
        self.text.push_str(text);
        for _ in 0..text.len() {
            self.starts.push(origin.start);
            self.ends.push(origin.end);
            self.synthetic.push(true);
        }
    }

    /// The input range of `range` (a range of this text).
    fn origin(&self, range: Range<usize>) -> Range<usize> {
        if range.start >= range.end {
            let at = if range.start < self.len() {
                self.starts[range.start]
            } else if let Some(last) = self.ends.last() {
                *last
            } else {
                0
            };
            return at..at;
        }
        let start = self.starts[range.start];
        let end = self.ends[range.end - 1].max(start);
        start..end
    }

    /// The input byte range of `range`, a byte range of this text.
    pub(crate) fn span(&self, range: Range<usize>) -> Range<usize> {
        self.origin(range)
    }

    /// The input byte range of the whole text.
    pub(crate) fn whole_span(&self) -> Range<usize> {
        self.origin(0..self.len())
    }

    /// True when each byte of `range` was inserted by the cleaning.
    pub(crate) fn is_synthetic(&self, range: Range<usize>) -> bool {
        !range.is_empty() && self.synthetic[range].iter().all(|s| *s)
    }

    pub(crate) fn slice(&self, range: Range<usize>) -> Mapped {
        let mut out = Mapped::default();
        out.push_copy(self, range);
        out
    }

    /// The text without leading and trailing whitespace.
    pub(crate) fn trim(&self) -> Mapped {
        let start = self.text.len() - self.text.trim_start().len();
        let end = self.text.trim_end().len().max(start);
        self.slice(start..end)
    }

    /// Each match of `re` replaced by `rep`, as `re.sub` does.
    pub(crate) fn replace_all(&self, re: &Regex, rep: Rep<'_>) -> Mapped {
        let mut out = Mapped::default();
        let mut last = 0;
        for caps in re.captures_iter(&self.text) {
            let whole = caps.get(0).expect("group 0 is always set");
            out.push_copy(self, last..whole.start());
            match rep {
                Rep::Delete => {}
                Rep::Literal(text) => {
                    out.push_synthetic(text, self.origin(whole.range()));
                }
                Rep::Group(group) => {
                    if let Some(g) = caps.get(group) {
                        out.push_copy(self, g.range());
                    }
                }
            }
            last = whole.end();
        }
        out.push_copy(self, last..self.len());
        out
    }

    /// Each `from` replaced by `to`, left to right, as `str.replace` does.
    pub(crate) fn replace_str(&self, from: &str, to: &str) -> Mapped {
        let mut out = Mapped::default();
        let mut last = 0;
        for (at, _) in self.text.match_indices(from) {
            out.push_copy(self, last..at);
            out.push_synthetic(to, self.origin(at..at + from.len()));
            last = at + from.len();
        }
        out.push_copy(self, last..self.len());
        out
    }

    /// The parts joined by `sep`, a synthetic text that stands for the input
    /// between the two parts it joins.
    pub(crate) fn join(parts: &[Mapped], sep: &str) -> Mapped {
        let mut out = Mapped::default();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                let from = out.ends.last().copied().unwrap_or(0);
                let to = part.starts.first().copied().unwrap_or(from).max(from);
                out.push_synthetic(sep, from..to);
            }
            out.push_copy(part, 0..part.len());
        }
        out
    }

    /// The text split at each `sep`, as `str.split` does.
    pub(crate) fn split(&self, sep: &str) -> Vec<Mapped> {
        let mut out = Vec::new();
        let mut last = 0;
        for (at, _) in self.text.match_indices(sep) {
            out.push(self.slice(last..at));
            last = at + sep.len();
        }
        out.push(self.slice(last..self.len()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_points_at_the_text_it_replaced() {
        let input = "Run `kairos login` now.";
        let m =
            Mapped::new(input).replace_all(&Regex::new(r"`[^`]*`").unwrap(), Rep::Literal("CODE"));
        assert_eq!(m.text, "Run CODE now.");
        let at = m.text.find("CODE").unwrap();
        assert_eq!(&input[m.span(at..at + 4)], "`kairos login`");
        assert!(m.is_synthetic(at..at + 4));
        let now = m.text.find("now").unwrap();
        assert_eq!(&input[m.span(now..now + 3)], "now");
    }

    #[test]
    fn a_group_keeps_its_own_origin() {
        let input = "See [the guide](a/b.md) first.";
        let m = Mapped::new(input).replace_all(
            &Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap(),
            Rep::Group(1),
        );
        assert_eq!(m.text, "See the guide first.");
        let at = m.text.find("the guide").unwrap();
        assert_eq!(&input[m.span(at..at + 9)], "the guide");
    }
}
