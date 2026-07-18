//! Pure short-code semantics (KAIROS-S-0004 "Short Code Sequences",
//! KAIROS-T-0012): the entity-type vocabulary, the
//! `{PREFIX}-{TYPE_LETTER}-{NNNN}` format, and the per-tenant default
//! prefix.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no I/O. `kairos-db::items::next_short_code` supplies the number
//! (a PostgreSQL `seq_*_code` sequence, concurrency-safe by construction)
//! and calls [`format_short_code`] to render it.
//!
//! # Prefix (interpretation recorded in KAIROS-T-0012)
//!
//! S-0004 says the PREFIX "is application config per tenant" but the DDL has
//! no per-org prefix column. The DEFAULT is therefore derived from the
//! tenant's organization slug: upper-cased and sanitized to `[A-Z0-9]`
//! ([`default_prefix`], e.g. slug `acme-co` → `ACMECO`). A per-organization
//! override is a future API-layer setting; when it lands it replaces the
//! default at the call site, not the format.

/// The five content-bearing entity types that carry short codes, versions,
/// and history (KAIROS-A-0001/A-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemType {
    /// Flight Level 3 (`strategies`, letter `S`).
    Strategy,
    /// Flight Level 2 (`initiatives`, letter `I`).
    Initiative,
    /// Flight Level 1 (`tasks`, letter `T`).
    Task,
    /// Supporting documents (`documents`, letter `D`).
    Document,
    /// Architecture Decision Records (`adrs`, letter `A`).
    Adr,
}

impl ItemType {
    /// Every variant, in declaration order.
    pub const ALL: &'static [ItemType] = &[
        ItemType::Strategy,
        ItemType::Initiative,
        ItemType::Task,
        ItemType::Document,
        ItemType::Adr,
    ];

    /// The S-0004 type letter used in short codes (`S`/`I`/`T`/`D`/`A`).
    pub fn letter(self) -> char {
        match self {
            ItemType::Strategy => 'S',
            ItemType::Initiative => 'I',
            ItemType::Task => 'T',
            ItemType::Document => 'D',
            ItemType::Adr => 'A',
        }
    }

    /// The `entity_type` string used in `activity_log` and the
    /// `searchable_items`/`entity_directory` views.
    pub fn entity_type(self) -> &'static str {
        match self {
            ItemType::Strategy => "strategy",
            ItemType::Initiative => "initiative",
            ItemType::Task => "task",
            ItemType::Document => "document",
            ItemType::Adr => "adr",
        }
    }
}

impl std::fmt::Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.entity_type())
    }
}

/// Render a short code as `{PREFIX}-{TYPE_LETTER}-{NNNN}` (S-0004): the
/// number is zero-padded to 4 digits and grows past 9999 without truncation
/// (`10000` → `…-T-10000`).
pub fn format_short_code(prefix: &str, item_type: ItemType, number: i64) -> String {
    format!("{prefix}-{}-{number:04}", item_type.letter())
}

/// The DEFAULT per-tenant short-code prefix: the organization slug
/// upper-cased and sanitized to `[A-Z0-9]` (`-`/`_` and anything else
/// dropped). Slugs match `^[a-z][a-z0-9_-]{1,62}$` (S-0004), so the result
/// is never empty for a valid slug.
///
/// See the module docs: a per-organization override is a future API-layer
/// setting; this default is the KAIROS-T-0012 interpretation of S-0004's
/// "application config per tenant".
pub fn default_prefix(slug: &str) -> String {
    slug.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_letters_match_s0004() {
        let letters: Vec<char> = ItemType::ALL.iter().map(|t| t.letter()).collect();
        assert_eq!(letters, ['S', 'I', 'T', 'D', 'A']);
        let names: Vec<&str> = ItemType::ALL.iter().map(|t| t.entity_type()).collect();
        assert_eq!(names, ["strategy", "initiative", "task", "document", "adr"]);
    }

    #[test]
    fn short_code_zero_pads_to_four() {
        assert_eq!(
            format_short_code("ACME", ItemType::Strategy, 1),
            "ACME-S-0001"
        );
        assert_eq!(format_short_code("ACME", ItemType::Task, 42), "ACME-T-0042");
        assert_eq!(format_short_code("K", ItemType::Adr, 9999), "K-A-9999");
    }

    #[test]
    fn short_code_grows_past_9999_without_truncation() {
        assert_eq!(
            format_short_code("ACME", ItemType::Document, 10000),
            "ACME-D-10000"
        );
        assert_eq!(
            format_short_code("ACME", ItemType::Initiative, 123456),
            "ACME-I-123456"
        );
    }

    #[test]
    fn default_prefix_uppercases_and_sanitizes() {
        assert_eq!(default_prefix("acme"), "ACME");
        assert_eq!(default_prefix("acme-co"), "ACMECO");
        assert_eq!(default_prefix("acme_co2"), "ACMECO2");
        // Valid slugs always start with a letter, so never empty; junk input
        // degrades to the alphanumerics present.
        assert_eq!(default_prefix("a-_-b"), "AB");
    }
}
