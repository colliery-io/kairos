//! The rules of the notifications of a person (KAIROS-T-0381,
//! COLLIERY-I-0614): the kinds, the mentions in Markdown, and the P0 scale.
//!
//! No I/O. `kairos-db` writes the notifications.

use std::collections::BTreeMap;

/// The kind of a notification. The wire name is [`NotificationKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotificationKind {
    /// A person handed off the claim of a task to you.
    HandOff,
    /// A blocker of an item that you have the claim on (or own) is done,
    /// archived or purged, or its `blocks` link is removed.
    BlockerCleared,
    /// A blocker of an item that you have the claim on (or own, or made)
    /// left a done column, so it blocks again (KAIROS-T-0568).
    BlockedAgain,
    /// A person cancelled a task that you created.
    Cancelled,
    /// A person cancelled a task that blocks an item that you have the
    /// claim on, own or made. The cancelled task still blocks
    /// (KAIROS-T-0567).
    BlockerCancelled,
    /// A person wrote `@your_user_name` in a note or in the content of an item.
    Mention,
    /// A bug with the priority P0 (critical) is on the board of your team.
    P0Bug,
    /// A webhook target that you own was turned off after failed deliveries
    /// (KAIROS-T-0384).
    DeliveryFailing,
    /// An organization admin revoked an agent key of yours (KAIROS-T-0388).
    AgentKeyRevoked,
    /// A person left the organization, so their claim on a task of the
    /// board of your team ended (KAIROS-T-0531).
    ClaimDropped,
    /// A claim on a task had no write and no state change for some days:
    /// your claim, or a claim on a task of an initiative that you own
    /// (KAIROS-T-0531).
    ClaimStale,
    /// An agent key of yours, or a key of a service account of your
    /// organization (you are an admin), expires within
    /// [`KEY_EXPIRING_DAYS`] days (KAIROS-T-0540).
    KeyExpiring,
    /// A person confirmed or rejected an edge proposal that you made
    /// (KAIROS-T-0558).
    ProposalDecided,
    /// A person, or an agent key of a person, sent you a notice: to you,
    /// or to a team of yours (KAIROS-T-0737).
    Notice,
}

/// A key that expires within this many days gets one `key_expiring`
/// notification (KAIROS-T-0540).
pub const KEY_EXPIRING_DAYS: i64 = 7;

impl NotificationKind {
    /// Each kind, in the order of the settings.
    pub const ALL: [NotificationKind; 14] = [
        NotificationKind::HandOff,
        NotificationKind::BlockerCleared,
        NotificationKind::BlockedAgain,
        NotificationKind::Cancelled,
        NotificationKind::BlockerCancelled,
        NotificationKind::Mention,
        NotificationKind::P0Bug,
        NotificationKind::DeliveryFailing,
        NotificationKind::AgentKeyRevoked,
        NotificationKind::ClaimDropped,
        NotificationKind::ClaimStale,
        NotificationKind::KeyExpiring,
        NotificationKind::ProposalDecided,
        NotificationKind::Notice,
    ];

    /// The wire name (`kind` column and API field).
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationKind::HandOff => "hand_off",
            NotificationKind::BlockerCleared => "blocker_cleared",
            NotificationKind::BlockedAgain => "blocked_again",
            NotificationKind::Cancelled => "cancelled",
            NotificationKind::BlockerCancelled => "blocker_cancelled",
            NotificationKind::Mention => "mention",
            NotificationKind::P0Bug => "p0_bug",
            NotificationKind::DeliveryFailing => "delivery_failing",
            NotificationKind::AgentKeyRevoked => "agent_key_revoked",
            NotificationKind::ClaimDropped => "claim_dropped",
            NotificationKind::ClaimStale => "claim_stale",
            NotificationKind::KeyExpiring => "key_expiring",
            NotificationKind::ProposalDecided => "proposal_decided",
            NotificationKind::Notice => "notice",
        }
    }

    /// The kind of a wire name. `None` for an unknown name.
    pub fn parse(name: &str) -> Option<NotificationKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// The most characters in the text of a notice (KAIROS-T-0737).
pub const NOTICE_TEXT_MAX: usize = 2000;

/// The notices that one agent key (or one person with no key) can send in
/// one hour, when the operator sets no other limit (KAIROS-T-0737). A
/// notice to a team counts as one.
pub const NOTICES_PER_HOUR: u32 = 20;

/// Whether a value of the metadata `priority` is P0: `critical` (the top of
/// the default scale) or `p0`. Case is not significant.
pub fn is_p0_priority(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("critical") || value.eq_ignore_ascii_case("p0")
}

/// A character of a user name after `@`.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+' | '@')
}

/// The text of `markdown` outside code: fenced code blocks (``` or ~~~)
/// and code spans (a run of backticks to the next run of the same length)
/// become spaces. The line breaks stay.
fn outside_code(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = marker.map_or(0, |m| trimmed.chars().take_while(|c| *c == m).count());
        match fence {
            Some((m, n)) => {
                if marker == Some(m) && run >= n && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
                out.push_str(if line.ends_with('\n') { "\n" } else { "" });
                continue;
            }
            None if indent < 4 && run >= 3 => {
                if let Some(m) = marker {
                    fence = Some((m, run));
                    out.push_str(if line.ends_with('\n') { "\n" } else { "" });
                    continue;
                }
            }
            None => {}
        }
        out.push_str(&blank_code_spans(line));
    }
    out
}

/// One line with its code spans as spaces. A run of backticks with no
/// closing run of the same length is text.
fn blank_code_spans(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] == '`' {
            i += 1;
        }
        let run = i - start;
        // Find the closing run of the same length.
        let mut j = i;
        let mut close = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let s = j;
                while j < chars.len() && chars[j] == '`' {
                    j += 1;
                }
                if j - s == run {
                    close = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                out.extend(std::iter::repeat_n(' ', end - start));
                i = end;
            }
            None => out.extend(std::iter::repeat_n('`', run)),
        }
    }
    out
}

/// The mentions in `markdown`, by user name (lower case), with the count of
/// each. A mention is `@` and a user name, at the start of the text or
/// after a character that is not a letter, a digit, `_`, `.`, `@` or `` ` ``
/// (so an email address is not a mention). The user name ends at a
/// character that is not a letter, a digit, `_`, `.`, `-`, `+` or `@`; a
/// `.`, `-` or `@` at its end is not part of it. Code (fenced blocks and
/// code spans) has no mentions.
pub fn mentions(markdown: &str) -> BTreeMap<String, usize> {
    let text = outside_code(markdown);
    let chars: Vec<char> = text.chars().collect();
    let mut found = BTreeMap::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let after_word = i > 0
            && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '.' | '@' | '`'));
        let mut j = i + 1;
        while j < chars.len() && is_name_char(chars[j]) {
            j += 1;
        }
        let mut name: String = chars[i + 1..j].iter().collect();
        while name.ends_with(['.', '-', '@']) {
            name.pop();
        }
        if !after_word && !name.is_empty() {
            *found.entry(name.to_lowercase()).or_insert(0) += 1;
        }
        i = j.max(i + 1);
    }
    found
}

/// The new mentions of an update: each user name with more mentions in
/// `after` than in `before`. A mention that only moved is not new.
pub fn new_mentions(before: &str, after: &str) -> Vec<String> {
    let old = mentions(before);
    mentions(after)
        .into_iter()
        .filter(|(name, count)| old.get(name).copied().unwrap_or(0) < *count)
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(markdown: &str) -> Vec<String> {
        mentions(markdown).into_keys().collect()
    }

    #[test]
    fn kinds_round_trip() {
        for kind in NotificationKind::ALL {
            assert_eq!(NotificationKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(NotificationKind::parse("email"), None);
    }

    #[test]
    fn p0_is_critical_or_p0() {
        assert!(is_p0_priority("critical"));
        assert!(is_p0_priority(" Critical "));
        assert!(is_p0_priority("P0"));
        assert!(!is_p0_priority("high"));
        assert!(!is_p0_priority("p1"));
    }

    #[test]
    fn a_mention_is_at_and_a_user_name() {
        assert_eq!(names("Ask @alice."), vec!["alice"]);
        assert_eq!(names("@Bob, see this"), vec!["bob"]);
        assert_eq!(names("(@robin_s) and @sam-j"), vec!["robin_s", "sam-j"]);
        assert_eq!(names("cc @jamie@example.com."), vec!["jamie@example.com"]);
        assert_eq!(mentions("@alice and @alice").get("alice"), Some(&2));
    }

    #[test]
    fn an_email_address_and_a_lone_at_are_not_mentions() {
        assert!(names("mail robin@example.com").is_empty());
        assert!(names("an @ sign, and @.").is_empty());
    }

    #[test]
    fn code_has_no_mentions() {
        assert!(names("Run `ping @alice` now").is_empty());
        assert!(names("Run ``a ` @alice`` now").is_empty());
        assert!(names("```\n@alice\n```\n").is_empty());
        assert!(names("~~~rust\nlet x = \"@alice\";\n~~~\n").is_empty());
        assert_eq!(names("```\n@alice\n```\nthen @bob"), vec!["bob"]);
        // An unclosed code span is text.
        assert_eq!(names("a ` and @alice"), vec!["alice"]);
    }

    #[test]
    fn only_new_mentions_count_in_an_update() {
        assert_eq!(
            new_mentions("Ask @alice.", "@alice: ask"),
            Vec::<String>::new()
        );
        assert_eq!(
            new_mentions("Ask @alice.", "Ask @alice and @bob."),
            vec!["bob"]
        );
        assert_eq!(
            new_mentions("Ask @alice.", "Ask @alice.\n\nNote: @alice"),
            vec!["alice"]
        );
        assert_eq!(new_mentions("", "`@alice`"), Vec::<String>::new());
    }
}
