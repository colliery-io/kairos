//! Pure repository rules (KAIROS-T-0103, decision KAIROS-A-0019): the
//! slug vocabulary and how a slug is derived from a forge full name.
//! No I/O (A-0009); [`kairos_db::repositories`] enforces these at the
//! write path and the API surfaces them as 422s.

/// Whether `slug` is a valid repository slug:
/// `^[a-z0-9][a-z0-9-]{1,62}$` — lowercase, digits, hyphens; 2..=63
/// bytes; may start with a digit (unlike org slugs) because forge names
/// like `3scale/apicast` derive to `3scale-apicast`.
pub fn is_valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
}

/// Derive a slug from a forge full name (`acme/payments-api` ->
/// `acme-payments-api`): lowercase, every run of non-alphanumerics
/// collapsed to one hyphen, leading/trailing hyphens trimmed, cut to 63
/// bytes. Mirrors the SQL backfill expression in the `repositories`
/// migration so migrated and API-created rows agree.
pub fn slug_from_full_name(full_name: &str) -> String {
    let mut out = String::with_capacity(full_name.len());
    let mut pending_hyphen = false;
    for ch in full_name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    out.truncate(63);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_vocabulary() {
        assert!(is_valid_slug("kairos"));
        assert!(is_valid_slug("acme-payments-api"));
        assert!(is_valid_slug("3scale-apicast"));
        assert!(!is_valid_slug("k"));
        assert!(!is_valid_slug("-leading"));
        assert!(!is_valid_slug("Upper"));
        assert!(!is_valid_slug("under_score"));
        assert!(!is_valid_slug("dot.name"));
        assert!(!is_valid_slug(&"a".repeat(64)));
    }

    #[test]
    fn derives_slug_from_full_name() {
        assert_eq!(
            slug_from_full_name("acme/payments-api"),
            "acme-payments-api"
        );
        assert_eq!(slug_from_full_name("Acme/Portal.Web"), "acme-portal-web");
        assert_eq!(
            slug_from_full_name("group/sub/project"),
            "group-sub-project"
        );
        assert_eq!(slug_from_full_name("/weird//name/"), "weird-name");
        assert_eq!(slug_from_full_name("3scale/apicast"), "3scale-apicast");
        assert!(is_valid_slug(&slug_from_full_name("acme/payments-api")));
    }
}
