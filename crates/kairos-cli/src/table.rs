//! A tiny aligned-column table renderer for the human-readable output mode
//! (KAIROS-A-0015: "human-readable tables by default" — plain text, no TUI
//! dependency; `--json` is the scripting surface).

/// A simple text table: headers plus rows, rendered with columns padded to
/// their widest cell and separated by two spaces.
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    /// A table with the given column headers.
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    /// Append one row. Rows shorter than the header count render as blank
    /// trailing cells; longer rows keep their extra cells unaligned.
    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// Render with each column padded to its widest cell (headers
    /// included), two spaces between columns, no trailing whitespace.
    pub fn render(&self) -> String {
        let columns = self
            .rows
            .iter()
            .map(Vec::len)
            .chain([self.headers.len()])
            .max()
            .unwrap_or(0);
        let mut widths = vec![0usize; columns];
        for line in std::iter::once(&self.headers).chain(&self.rows) {
            for (i, cell) in line.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }

        let render_line = |line: &[String]| -> String {
            let mut out = String::new();
            for (i, cell) in line.iter().enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                out.push_str(cell);
                // Pad every column but the last so lines never carry
                // trailing spaces.
                if i + 1 < line.len() {
                    let pad = widths[i].saturating_sub(cell.chars().count());
                    out.extend(std::iter::repeat_n(' ', pad));
                }
            }
            out
        };

        let mut out = String::new();
        out.push_str(&render_line(&self.headers));
        out.push('\n');
        for row in &self.rows {
            out.push_str(&render_line(row));
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Columns align to the widest cell, headers included; no trailing
    /// whitespace on any line.
    #[test]
    fn renders_aligned_columns() {
        let mut table = Table::new(&["CODE", "TITLE", "VER"]);
        table.row(vec!["ACME-T-0001".into(), "Fix login".into(), "3".into()]);
        table.row(vec!["ACME-T-0002".into(), "A".into(), "12".into()]);
        let rendered = table.render();
        assert_eq!(
            rendered,
            "CODE         TITLE      VER\n\
             ACME-T-0001  Fix login  3\n\
             ACME-T-0002  A          12\n"
        );
        assert!(rendered.lines().all(|l| l.trim_end() == l), "{rendered}");
    }

    /// A header-only table renders just the header line.
    #[test]
    fn renders_empty_table() {
        let table = Table::new(&["A", "B"]);
        assert_eq!(table.render(), "A  B\n");
    }
}
