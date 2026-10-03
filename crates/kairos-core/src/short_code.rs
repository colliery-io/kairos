//! Pure short-code semantics (KAIROS-S-0004 "Short Code Sequences",
//! KAIROS-T-0012, COLLIERY-T-3099): the entity-type vocabulary, the
//! `{PREFIX}-{TYPE_LETTER}-{NNNN}` format, and the rule for a prefix.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no I/O. `kairos-db::items::next_short_code` supplies the number
//! (a row of the table `short_code_sequences` for each prefix and type) and
//! calls [`format_short_code`] to render it.
//!
//! # Prefix (COLLIERY-T-3099, COLLIERY-I-0407)
//!
//! Each board has a prefix (`boards.code_prefix`). An admin sets it when
//! the board is created, and it does not change. It matches [`PREFIX_RULE`]
//! ([`is_valid_prefix`]). An item takes the prefix of its board: the board
//! of a strategy, an initiative, a task or an ADR, and the owner board of a
//! document.
//!
//! An item with no board (an ADR off the boards, a document with no owner
//! board) takes the prefix of the tenant ([`tenant_prefix`]). Before
//! COLLIERY-T-3099 each item took that prefix (KAIROS-T-0012).
//!
//! [`prefix_from_slug`] makes a prefix from a slug. The migration, the
//! provisioning of a tenant and a SCIM group use it, because they have no
//! admin to choose a prefix.

/// The rule of a board prefix (COLLIERY-T-3099): a capital letter, then 1
/// to 9 capital letters or digits.
pub const PREFIX_RULE: &str = "^[A-Z][A-Z0-9]{1,9}$";

/// True when `prefix` matches [`PREFIX_RULE`].
pub fn is_valid_prefix(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    (2..=10).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

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

/// The parts of a short code: `SKADI-T-0577` gives `("SKADI", Task, 577)`.
/// `None` when `code` does not have the form `{PREFIX}-{TYPE_LETTER}-{NNNN}`
/// (COLLIERY-T-3100). The prefix is a capital letter, then capital letters
/// or digits. It can be longer than [`PREFIX_RULE`] allows, because a code
/// from before COLLIERY-T-3099 has the [`default_prefix`] of the tenant. The
/// number has 4 digits at least and 18 at most.
pub fn parse_short_code(code: &str) -> Option<(&str, ItemType, i64)> {
    let mut parts = code.splitn(3, '-');
    let prefix = parts.next()?;
    let letter = parts.next()?;
    let number = parts.next()?;
    let prefix_ok = prefix
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_uppercase)
        && prefix
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let item_type = match letter {
        "S" => ItemType::Strategy,
        "I" => ItemType::Initiative,
        "T" => ItemType::Task,
        "D" => ItemType::Document,
        "A" => ItemType::Adr,
        _ => return None,
    };
    let number_ok = (4..=18).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit());
    if !prefix_ok || !number_ok {
        return None;
    }
    Some((prefix, item_type, number.parse().ok()?))
}

/// The letters and digits of a slug, in capitals (`acme-co` → `ACMECO`).
/// Each other character is dropped. Before COLLIERY-T-3099 this was the
/// prefix of each item of the tenant (KAIROS-T-0012). Old codes have it,
/// and it can be longer than 10 characters.
pub fn default_prefix(slug: &str) -> String {
    slug.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// A prefix that matches [`PREFIX_RULE`], made from a slug: the
/// [`default_prefix`] of the slug, with the characters before the first
/// letter dropped, cut to 10 characters, and with `0` added at the end
/// while it has fewer than 2 characters. A slug with no letter gives `X0`.
///
/// It does not make the prefix unique. The caller does that
/// (`kairos_db::boards::free_code_prefix`).
pub fn prefix_from_slug(slug: &str) -> String {
    let all = default_prefix(slug);
    let mut prefix: String = all
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .chars()
        .take(10)
        .collect();
    if prefix.is_empty() {
        prefix.push('X');
    }
    while prefix.len() < 2 {
        prefix.push('0');
    }
    prefix
}

/// The prefix of the tenant (COLLIERY-T-3099): the [`prefix_from_slug`] of
/// the slug of the organization (`colliery` → `COLLIERY`). The boards that
/// a new tenant gets have it, and an item with no board takes it.
pub fn tenant_prefix(slug: &str) -> String {
    prefix_from_slug(slug)
}

/// The prefix with a number at the end, for a prefix that a board has
/// already: `n` = 2 gives `ABC2`. The prefix is cut so that the result has
/// 10 characters at most.
pub fn numbered_prefix(prefix: &str, n: u32) -> String {
    let number = n.to_string();
    let keep = 10usize.saturating_sub(number.len());
    let mut out: String = prefix.chars().take(keep).collect();
    out.push_str(&number);
    out
}

/// The start of the provenance footer that the Metis importer writes
/// (`scripts/metis_import/remap.py`, `FOOTER_RULE + FOOTER_LEAD`). The footer
/// names the Metis code of the item, and a rename does not change it.
const IMPORT_FOOTER: &str = "\n\n---\n\nThis item came from the Metis record of the repository ";

/// The characters that end a token around a code: white space and the
/// brackets, quotes and punctuation that markdown puts around a reference.
fn ends_token(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '(' | ')' | '[' | ']' | '<' | '>' | '{' | '}' | '"' | '\'' | '`' | '|' | ',' | ';'
        )
}

/// A character that can be a part of a word or a code. A reference is a
/// whole code: `X-T-0001` is not a reference in `X-T-00012` or `AX-T-0001`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Replace each reference to the short code `old` in `text` with `new`
/// (COLLIERY-T-3101: a move that renames an item). `None` when `text` has
/// no reference to change.
///
/// A reference is the whole code. The rules for the text around it:
///
/// - A code in a URL (a token with `://`) or in a path (a token with `/` or
///   `\`) does not change: the URL or the file has its own name. The token
///   is the text between white space, brackets and quotes.
/// - The Kairos item link `/items/{code}` changes: it names the item.
/// - A file name (`{code}.md`: a `.` and then a letter or a digit) does not
///   change.
/// - The import footer does not change: it records the Metis code of the
///   item, the same as `rewrite_content` of the Metis importer.
pub fn rewrite_code_references(text: &str, old: &str, new: &str) -> Option<String> {
    if old.is_empty() || !text.contains(old) {
        return None;
    }
    let (body, footer) = match text.rfind(IMPORT_FOOTER) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut changed = false;
    for (start, _) in body.match_indices(old) {
        let end = start + old.len();
        let before = body[..start].chars().next_back();
        let after = body[end..].chars().next();
        if before.is_some_and(is_word) || after.is_some_and(is_word) {
            continue;
        }
        let rest = &body[end..];
        if rest.starts_with('.')
            && rest[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        let token_start = body[..start]
            .char_indices()
            .rev()
            .find(|(_, c)| ends_token(*c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let token_end = body[end..].find(ends_token).map_or(body.len(), |i| end + i);
        let token = &body[token_start..token_end];
        let lead = &body[token_start..start];
        let item_link = lead == "/items/";
        if token.contains("://") {
            continue;
        }
        if (token.contains('/') || token.contains('\\')) && !item_link {
            continue;
        }
        out.push_str(&body[copied..start]);
        out.push_str(new);
        copied = end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&body[copied..]);
    out.push_str(footer);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_changes_to_the_new_code() {
        let rw = |t: &str| rewrite_code_references(t, "COLLIERY-T-0100", "SKADI-T-0001");
        assert_eq!(
            rw("See COLLIERY-T-0100. Then (COLLIERY-T-0100), `COLLIERY-T-0100`, [[COLLIERY-T-0100]], **COLLIERY-T-0100**: done").as_deref(),
            Some("See SKADI-T-0001. Then (SKADI-T-0001), `SKADI-T-0001`, [[SKADI-T-0001]], **SKADI-T-0001**: done")
        );
        assert_eq!(
            rw("COLLIERY-T-0100").as_deref(),
            Some("SKADI-T-0001"),
            "a code with no text around it"
        );
        assert_eq!(
            rw("[COLLIERY-T-0100](/items/COLLIERY-T-0100)").as_deref(),
            Some("[SKADI-T-0001](/items/SKADI-T-0001)"),
            "the Kairos item link names the item"
        );
    }

    #[test]
    fn a_code_in_a_url_a_path_or_a_file_name_does_not_change() {
        let rw = |t: &str| rewrite_code_references(t, "COLLIERY-T-0100", "SKADI-T-0001");
        for kept in [
            "https://example.com/COLLIERY-T-0100.md",
            "<https://example.com/x/COLLIERY-T-0100>",
            "[the file](https://example.com/COLLIERY-T-0100)",
            "https://kairos.example/items/COLLIERY-T-0100",
            "docs/COLLIERY-T-0100/notes",
            "`.metis/tasks/COLLIERY-T-0100`",
            "C:\\work\\COLLIERY-T-0100",
            "COLLIERY-T-0100.md",
            "COLLIERY-T-01000 and XCOLLIERY-T-0100 and COLLIERY-T-0100_x",
            "no reference here",
        ] {
            assert_eq!(rw(kept), None, "{kept}");
        }
        assert_eq!(
            rw("COLLIERY-T-0100 is at https://example.com/COLLIERY-T-0100.md").as_deref(),
            Some("SKADI-T-0001 is at https://example.com/COLLIERY-T-0100.md")
        );
    }

    #[test]
    fn the_import_footer_does_not_change() {
        let text = "Body COLLIERY-T-0100.\n\n---\n\nThis item came from the Metis record of the \
                    repository kairos. Its Metis code was COLLIERY-T-0100.\n";
        assert_eq!(
            rewrite_code_references(text, "COLLIERY-T-0100", "SKADI-T-0001").as_deref(),
            Some(
                "Body SKADI-T-0001.\n\n---\n\nThis item came from the Metis record of the \
                 repository kairos. Its Metis code was COLLIERY-T-0100.\n"
            )
        );
        let only_footer = "Body.\n\n---\n\nThis item came from the Metis record of the \
                           repository kairos. Its Metis code was COLLIERY-T-0100.\n";
        assert_eq!(
            rewrite_code_references(only_footer, "COLLIERY-T-0100", "SKADI-T-0001"),
            None
        );
    }

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

    #[test]
    fn the_prefix_rule() {
        for good in [
            "AB",
            "COLLIERY",
            "SKADI",
            "GQLITE",
            "A1",
            "ABCDEFGHIJ",
            "CRT",
        ] {
            assert!(is_valid_prefix(good), "{good}");
        }
        for bad in [
            "",
            "A",
            "sk-adi",
            "SKADI-",
            "1AB",
            "ABCDEFGHIJK",
            "Skadi",
            "SK_ADI",
            "ÄB",
        ] {
            assert!(!is_valid_prefix(bad), "{bad}");
        }
    }

    #[test]
    fn a_prefix_from_a_slug_matches_the_rule() {
        assert_eq!(prefix_from_slug("skadi"), "SKADI");
        assert_eq!(prefix_from_slug("colliery-io-delivery"), "COLLIERYIO");
        assert_eq!(prefix_from_slug("graphqlite"), "GRAPHQLITE");
        assert_eq!(prefix_from_slug("a_"), "A0");
        assert_eq!(prefix_from_slug("a-2"), "A2");
        for slug in [
            "acme",
            "a_",
            "web",
            "x-1-2-3-4-5-6-7-8-9",
            "colliery-io-delivery",
        ] {
            assert!(is_valid_prefix(&prefix_from_slug(slug)), "{slug}");
        }
        assert_eq!(tenant_prefix("colliery"), "COLLIERY");
    }

    #[test]
    fn a_short_code_parses_into_its_parts() {
        assert_eq!(
            parse_short_code("SKADI-T-0577"),
            Some(("SKADI", ItemType::Task, 577))
        );
        assert_eq!(
            parse_short_code("COLLIERY-I-0407"),
            Some(("COLLIERY", ItemType::Initiative, 407))
        );
        assert_eq!(
            parse_short_code("ACMECORPORATION-D-10000"),
            Some(("ACMECORPORATION", ItemType::Document, 10000))
        );
        for bad in [
            "",
            "SKADI",
            "SKADI-T",
            "SKADI-T-577",
            "SKADI-X-0577",
            "skadi-T-0577",
            "1SK-T-0577",
            "SKADI-T-0577-1",
            "SKADI-T-05a7",
            "SK ADI-T-0577",
            "-T-0577",
            "SKADI-T-1234567890123456789",
        ] {
            assert_eq!(parse_short_code(bad), None, "{bad}");
        }
        for t in ItemType::ALL {
            let code = format_short_code("ACME", *t, 42);
            assert_eq!(parse_short_code(&code), Some(("ACME", *t, 42)));
        }
    }

    #[test]
    fn a_numbered_prefix_has_10_characters_at_most() {
        assert_eq!(numbered_prefix("ABC", 2), "ABC2");
        assert_eq!(numbered_prefix("ABCDEFGHIJ", 2), "ABCDEFGHI2");
        assert_eq!(numbered_prefix("ABCDEFGHIJ", 12), "ABCDEFGH12");
    }
}
