//! Pure repository rules (KAIROS-T-0103, decision KAIROS-A-0019): the
//! slug vocabulary and how a slug is derived from a forge full name.
//! No I/O (A-0009); [`kairos_db::repositories`] enforces these at the
//! write path and the API surfaces them as 422s.
//!
//! COLLIERY-T-0267: the form of the full name, of the URL and of the
//! default branch. One function has the rule of each field
//! ([`check_full_name`], [`check_url`], [`check_default_branch`]). No
//! function trims a value: a value with a space at its start or at its end
//! is refused, and the refusal says to remove the space.

/// The forge of a repository, for the rule of the full name
/// (COLLIERY-T-0267). `kairos_db` has the enum of the column and converts
/// it to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    Github,
    Gitlab,
    Other,
}

impl ForgeKind {
    /// The name of the forge, as a request gives it.
    pub fn as_str(self) -> &'static str {
        match self {
            ForgeKind::Github => "github",
            ForgeKind::Gitlab => "gitlab",
            ForgeKind::Other => "other",
        }
    }
}

/// What is wrong with a value (COLLIERY-T-0267). The tests read it, and
/// the user reads [`FieldFault::message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// No characters, or only spaces.
    Empty,
    /// A space at the start or at the end.
    OuterSpace,
    /// A space, a tab or a line break in the value.
    Space,
    /// A control character.
    Control,
    /// More characters than the maximum.
    TooLong,
    /// The full name ends with `.git`.
    GitSuffix,
    /// The full name has a `/` at its start or its end, or `//`.
    EmptyPart,
    /// The full name does not have the number of parts of its forge.
    PartCount,
    /// The URL does not start with `http://` or `https://`.
    NotHttp,
    /// The URL has no host, or it is not a URL.
    NoHost,
    /// The URL has a user name or a password.
    Credentials,
    /// The branch name breaks a rule of `git check-ref-format`.
    NotABranch,
}

/// A value that does not have the form of its field (COLLIERY-T-0267).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldFault {
    /// The name of the field in the request: `repo_full_name`, `repo_url`
    /// or `default_branch`.
    pub field: &'static str,
    pub kind: FaultKind,
    /// The text for the user: what is wrong, and what to send.
    pub message: String,
}

impl std::fmt::Display for FieldFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FieldFault {}

/// The maximum length of a full name, in characters. GitHub permits 39
/// for the owner and 100 for the repository. GitLab permits 255 for one
/// path, and a full path has some of them.
pub const FULL_NAME_MAX: usize = 255;
/// The maximum length of a URL, in characters.
pub const URL_MAX: usize = 2048;
/// The maximum length of a branch name, in characters.
pub const BRANCH_MAX: usize = 255;

/// The rule of the full name for `forge`, as one sentence.
pub fn full_name_rule(forge: ForgeKind) -> &'static str {
    match forge {
        ForgeKind::Github => {
            "For the forge github, the name has 2 parts, for example acme/payments-api."
        }
        ForgeKind::Gitlab => {
            "For the forge gitlab, the name has 2 or more parts, for example \
             acme/portal/web."
        }
        ForgeKind::Other => {
            "For the forge other, the name has 1 or more parts, for example acme/site."
        }
    }
}

/// The checks that the three fields have together, in this sequence:
/// empty, a space at an end, a space, a control character, the length.
/// The text does not show the value of a URL, which can have a password
/// in it.
fn check_text(
    field: &'static str,
    value: &str,
    max: usize,
    example: &str,
) -> Result<(), FieldFault> {
    let fault = |kind, message: String| {
        Err(FieldFault {
            field,
            kind,
            message,
        })
    };
    let shown = match field {
        "repo_url" => String::new(),
        _ => format!(" {value:?}"),
    };
    if value.trim().is_empty() {
        return fault(
            FaultKind::Empty,
            format!("The {field} is empty. Send a value, for example {example}."),
        );
    }
    if value.trim() != value {
        return fault(
            FaultKind::OuterSpace,
            format!(
                "The {field}{shown} has a space at its start or at its end. Remove the \
                 spaces."
            ),
        );
    }
    if value.chars().any(char::is_whitespace) {
        return fault(
            FaultKind::Space,
            format!("The {field}{shown} has a space in it. Remove the space."),
        );
    }
    if value.chars().any(char::is_control) {
        return fault(
            FaultKind::Control,
            format!(
                "The {field}{shown} has a control character in it. Remove the control \
                 character."
            ),
        );
    }
    let length = value.chars().count();
    if length > max {
        return fault(
            FaultKind::TooLong,
            format!(
                "The {field} has {length} characters, and the maximum is {max}. Send a \
                 shorter value."
            ),
        );
    }
    Ok(())
}

/// The form of `repo_full_name` (COLLIERY-T-0267): the name of the
/// repository on the forge, as the forge sends it in a webhook. The parts
/// have `/` between them, and each part has text. GitHub has 2 parts
/// (`owner/repo`), GitLab has 2 or more (`group/subgroup/project`), and
/// the forge `other` has 1 or more. The name does not end with `.git`.
pub fn check_full_name(forge: ForgeKind, value: &str) -> Result<(), FieldFault> {
    let field = "repo_full_name";
    let rule = full_name_rule(forge);
    let fault = |kind, message: String| {
        Err(FieldFault {
            field,
            kind,
            message,
        })
    };
    if value.trim().is_empty() {
        return fault(FaultKind::Empty, format!("The {field} is empty. {rule}"));
    }
    check_text(field, value, FULL_NAME_MAX, "acme/payments-api")?;
    if value.to_ascii_lowercase().ends_with(".git") {
        return fault(
            FaultKind::GitSuffix,
            format!("The {field} {value:?} ends with .git. Remove .git from the end. {rule}"),
        );
    }
    let parts: Vec<&str> = value.split('/').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return fault(
            FaultKind::EmptyPart,
            format!(
                "The {field} {value:?} has a part with no text. Remove each / that has no \
                 text before it or after it. {rule}"
            ),
        );
    }
    let correct = match forge {
        ForgeKind::Github => parts.len() == 2,
        ForgeKind::Gitlab => parts.len() >= 2,
        ForgeKind::Other => true,
    };
    if !correct {
        return fault(
            FaultKind::PartCount,
            format!(
                "The {field} {value:?} has {} {}. {rule}",
                parts.len(),
                if parts.len() == 1 { "part" } else { "parts" }
            ),
        );
    }
    Ok(())
}

/// The form of `repo_url` (COLLIERY-T-0267): an absolute `http` or `https`
/// URL with a host, and with no user name and no password. Each member of
/// the organization can read the URL, so a token in it is not a secret.
/// For the same reason no refusal shows the value.
pub fn check_url(value: &str) -> Result<(), FieldFault> {
    let field = "repo_url";
    let example = "https://github.com/acme/payments-api";
    let fault = |kind, message: String| {
        Err(FieldFault {
            field,
            kind,
            message,
        })
    };
    check_text(field, value, URL_MAX, example)?;
    // `Url::parse` accepts `https:host` and `https:///host`. The text of the
    // value must have the scheme and the two `/`.
    let lower = value.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return fault(
            FaultKind::NotHttp,
            format!(
                "The {field} does not start with http:// or https://. Send the URL for a \
                 browser, for example {example}."
            ),
        );
    }
    let after_scheme = &value[value.find("://").map_or(0, |at| at + 3)..];
    let parsed = match url::Url::parse(value) {
        Ok(parsed) if !after_scheme.starts_with('/') => parsed,
        _ => {
            return fault(
                FaultKind::NoHost,
                format!("The {field} has no host. Send a full URL, for example {example}."),
            );
        }
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return fault(
            FaultKind::Credentials,
            format!(
                "The {field} has a user name or a password in it. Each member of the \
                 organization can read the URL. Remove the user name and the password."
            ),
        );
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return fault(
            FaultKind::NoHost,
            format!("The {field} has no host. Send a full URL, for example {example}."),
        );
    }
    Ok(())
}

/// The form of `default_branch` (COLLIERY-T-0267): the rules of
/// `git check-ref-format --branch` that a name can break. The name does not
/// start with `-` or `/`, and it does not end with `/`, `.` or `.lock`. It
/// does not have `..`, `//` or `@{`, and it is not `@`. No part starts with
/// a dot. It has none of the characters `~ ^ : ? * [ \`.
pub fn check_default_branch(value: &str) -> Result<(), FieldFault> {
    let field = "default_branch";
    check_text(field, value, BRANCH_MAX, "main")?;
    let broken = if value.starts_with('-') {
        Some("starts with a hyphen".to_string())
    } else if value.starts_with('/') {
        Some("starts with /".to_string())
    } else if value.ends_with('/') {
        Some("ends with /".to_string())
    } else if value.ends_with(".lock") {
        Some("ends with .lock".to_string())
    } else if value.ends_with('.') {
        Some("ends with a dot".to_string())
    } else if value.contains("..") {
        Some("has 2 dots together".to_string())
    } else if value.contains("//") {
        Some("has 2 / characters together".to_string())
    } else if value.contains("@{") {
        Some("has the characters @{ together".to_string())
    } else if value == "@" {
        Some("is only the character @".to_string())
    } else if value.split('/').any(|part| part.starts_with('.')) {
        Some("has a part that starts with a dot".to_string())
    } else {
        value
            .chars()
            .find(|c| ['~', '^', ':', '?', '*', '[', '\\'].contains(c))
            .map(|c| format!("has the character {c}"))
    };
    match broken {
        None => Ok(()),
        Some(broken) => Err(FieldFault {
            field,
            kind: FaultKind::NotABranch,
            message: format!(
                "The {field} {value:?} {broken}. Git does not permit that in the name of \
                 a branch. Send the name of a branch, for example main."
            ),
        }),
    }
}

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
        // A UUID-shaped slug would be unreachable by slug: references are
        // parsed as ids first (KAIROS-T-0116).
        && !looks_like_uuid(slug)
}

/// `8-4-4-4-12` lowercase hex — the canonical UUID text form.
pub(crate) fn looks_like_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(len, part)| part.len() == *len && part.bytes().all(|b| b.is_ascii_hexdigit()))
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

    const FORGES: [ForgeKind; 3] = [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Other];

    /// COLLIERY-T-0267: the full names that each forge accepts.
    #[test]
    fn full_name_accepted() {
        let table: &[(ForgeKind, &[&str])] = &[
            (
                ForgeKind::Github,
                &[
                    "acme/payments-api",
                    "3scale/apicast",
                    "Acme/Portal.Web",
                    "a/b",
                ],
            ),
            (
                ForgeKind::Gitlab,
                &["acme/portal", "acme/portal/web", "a/b/c/d/e"],
            ),
            (
                ForgeKind::Other,
                &["site", "acme/site", "acme/portal/web", "my.gitignore"],
            ),
        ];
        for (forge, names) in table {
            for name in *names {
                assert_eq!(check_full_name(*forge, name), Ok(()), "{forge:?} {name:?}");
            }
        }
        let longest = format!("{}/{}", "a".repeat(127), "b".repeat(127));
        assert_eq!(longest.chars().count(), FULL_NAME_MAX);
        for forge in FORGES {
            assert_eq!(check_full_name(forge, &longest), Ok(()), "{forge:?}");
        }
    }

    /// COLLIERY-T-0267: the full names that each forge refuses.
    #[test]
    fn full_name_refused() {
        let too_long = format!("{}/{}", "a".repeat(128), "b".repeat(127));
        let each_forge: &[(&str, FaultKind)] = &[
            ("", FaultKind::Empty),
            ("   ", FaultKind::Empty),
            ("\t", FaultKind::Empty),
            (" acme/payments-api", FaultKind::OuterSpace),
            ("acme/payments-api ", FaultKind::OuterSpace),
            ("acme/payments-api\n", FaultKind::OuterSpace),
            ("acme/payments api", FaultKind::Space),
            ("acme/payments\tapi", FaultKind::Space),
            ("acme/payments\u{7}api", FaultKind::Control),
            (&too_long, FaultKind::TooLong),
            ("acme/payments-api.git", FaultKind::GitSuffix),
            ("acme/payments-api.GIT", FaultKind::GitSuffix),
            ("/acme/payments-api", FaultKind::EmptyPart),
            ("acme/payments-api/", FaultKind::EmptyPart),
            ("acme//payments-api", FaultKind::EmptyPart),
            ("/", FaultKind::EmptyPart),
        ];
        for forge in FORGES {
            for (name, kind) in each_forge {
                let fault = check_full_name(forge, name).expect_err(name);
                assert_eq!(fault.kind, *kind, "{forge:?} {name:?}: {fault}");
                assert_eq!(fault.field, "repo_full_name");
                // The refusal gives the rule of the forge, but for a fault
                // that does not change with the forge.
                let common = [
                    FaultKind::OuterSpace,
                    FaultKind::Space,
                    FaultKind::Control,
                    FaultKind::TooLong,
                ];
                assert_eq!(
                    fault.message.contains(full_name_rule(forge)),
                    !common.contains(kind),
                    "{fault}"
                );
            }
        }
        let by_forge: &[(ForgeKind, &str)] = &[
            (ForgeKind::Github, "payments-api"),
            (ForgeKind::Github, "acme/portal/web"),
            (ForgeKind::Gitlab, "portal"),
        ];
        for (forge, name) in by_forge {
            let fault = check_full_name(*forge, name).expect_err(name);
            assert_eq!(fault.kind, FaultKind::PartCount, "{forge:?} {name:?}");
        }
        assert_eq!(
            check_full_name(ForgeKind::Github, "acme/portal/web")
                .expect_err("3 parts")
                .message,
            "The repo_full_name \"acme/portal/web\" has 3 parts. For the forge github, the \
             name has 2 parts, for example acme/payments-api."
        );
        assert_eq!(
            check_full_name(ForgeKind::Gitlab, "portal")
                .expect_err("1 part")
                .message,
            "The repo_full_name \"portal\" has 1 part. For the forge gitlab, the name has 2 \
             or more parts, for example acme/portal/web."
        );
        assert_eq!(
            check_full_name(ForgeKind::Other, "")
                .expect_err("empty")
                .message,
            "The repo_full_name is empty. For the forge other, the name has 1 or more parts, \
             for example acme/site."
        );
        assert_eq!(
            check_full_name(ForgeKind::Github, "acme/fidius.git")
                .expect_err(".git")
                .message,
            "The repo_full_name \"acme/fidius.git\" ends with .git. Remove .git from the end. \
             For the forge github, the name has 2 parts, for example acme/payments-api."
        );
        assert_eq!(
            check_full_name(ForgeKind::Github, " acme/fidius")
                .expect_err("space")
                .message,
            "The repo_full_name \" acme/fidius\" has a space at its start or at its end. \
             Remove the spaces."
        );
    }

    /// COLLIERY-T-0267: the form of the URL.
    #[test]
    fn url_form() {
        for url in [
            "https://github.com/acme/payments-api",
            "http://git.acme.example/acme/site",
            "HTTPS://GitHub.com/acme/payments-api",
            "https://git.acme.example:8443/acme/site",
            "https://192.0.2.7/acme/site",
            "https://[2001:db8::7]/acme/site",
            "https://wiki.acme.test",
            "https://github.com/acme/payments-api?tab=readme#top",
        ] {
            assert_eq!(check_url(url), Ok(()), "{url:?}");
        }
        let too_long = format!("https://github.com/{}", "a".repeat(URL_MAX));
        let table: &[(&str, FaultKind)] = &[
            ("", FaultKind::Empty),
            ("  ", FaultKind::Empty),
            (" https://github.com/acme/site", FaultKind::OuterSpace),
            ("https://github.com/acme/site ", FaultKind::OuterSpace),
            ("https://github.com/acme/my site", FaultKind::Space),
            ("https://github.com/acme/\tsite", FaultKind::Space),
            ("https://github.com/acme/\u{1}site", FaultKind::Control),
            (&too_long, FaultKind::TooLong),
            ("github.com/acme/site", FaultKind::NotHttp),
            ("//github.com/acme/site", FaultKind::NotHttp),
            ("/acme/site", FaultKind::NotHttp),
            ("git@github.com:acme/site.git", FaultKind::NotHttp),
            ("ssh://git@github.com/acme/site", FaultKind::NotHttp),
            ("ftp://github.com/acme/site", FaultKind::NotHttp),
            ("file:///srv/git/site", FaultKind::NotHttp),
            ("javascript:alert(1)", FaultKind::NotHttp),
            ("https:github.com/acme/site", FaultKind::NotHttp),
            ("https://", FaultKind::NoHost),
            ("https:///github.com/acme/site", FaultKind::NoHost),
            ("https:///", FaultKind::NoHost),
            ("https://:8443/acme/site", FaultKind::NoHost),
            (
                "https://alice:token@github.com/acme/site",
                FaultKind::Credentials,
            ),
            ("https://alice@github.com/acme/site", FaultKind::Credentials),
            (
                "https://:token@github.com/acme/site",
                FaultKind::Credentials,
            ),
        ];
        for (url, kind) in table {
            let fault = check_url(url).expect_err(url);
            assert_eq!(fault.kind, *kind, "{url:?}: {fault}");
            assert_eq!(fault.field, "repo_url");
        }
        let fault = check_url("https://alice:s3cret@github.com/acme/site").expect_err("token");
        assert_eq!(
            fault.message,
            "The repo_url has a user name or a password in it. Each member of the \
             organization can read the URL. Remove the user name and the password."
        );
        // No refusal shows the value: it can have a password in it.
        for url in [
            "https://alice:s3cret@github.com/acme/site",
            " https://alice:s3cret@github.com/acme/site",
            "https://alice:s3cret@github.com/acme/my site",
            "ssh://alice:s3cret@github.com/acme/site",
        ] {
            let fault = check_url(url).expect_err(url);
            assert!(!fault.message.contains("s3cret"), "{fault}");
        }
        assert_eq!(
            check_url("git@github.com:acme/site.git")
                .expect_err("scp form")
                .message,
            "The repo_url does not start with http:// or https://. Send the URL for a \
             browser, for example https://github.com/acme/payments-api."
        );
    }

    /// COLLIERY-T-0267: the form of the default branch.
    #[test]
    fn default_branch_form() {
        let longest = "b".repeat(BRANCH_MAX);
        for branch in [
            "main",
            "trunk",
            "release/2026.09",
            "feature/COLLIERY-T-0267",
            "v1.2",
            "a",
            "user@host",
            "überzweig",
            longest.as_str(),
        ] {
            assert_eq!(check_default_branch(branch), Ok(()), "{branch:?}");
        }
        let too_long = "b".repeat(BRANCH_MAX + 1);
        let table: &[(&str, FaultKind)] = &[
            ("", FaultKind::Empty),
            (" ", FaultKind::Empty),
            (" main", FaultKind::OuterSpace),
            ("main ", FaultKind::OuterSpace),
            ("my branch", FaultKind::Space),
            ("my\u{7f}branch", FaultKind::Control),
            (&too_long, FaultKind::TooLong),
            ("-main", FaultKind::NotABranch),
            ("--upload-pack=x", FaultKind::NotABranch),
            ("/main", FaultKind::NotABranch),
            ("main/", FaultKind::NotABranch),
            ("main.lock", FaultKind::NotABranch),
            ("release/1.lock", FaultKind::NotABranch),
            ("main.", FaultKind::NotABranch),
            ("release..1", FaultKind::NotABranch),
            ("release//1", FaultKind::NotABranch),
            ("main@{1}", FaultKind::NotABranch),
            ("@", FaultKind::NotABranch),
            (".hidden", FaultKind::NotABranch),
            ("release/.hidden", FaultKind::NotABranch),
            ("main~1", FaultKind::NotABranch),
            ("main^", FaultKind::NotABranch),
            ("a:b", FaultKind::NotABranch),
            ("what?", FaultKind::NotABranch),
            ("release/*", FaultKind::NotABranch),
            ("a[1]", FaultKind::NotABranch),
            ("a\\b", FaultKind::NotABranch),
        ];
        for (branch, kind) in table {
            let fault = check_default_branch(branch).expect_err(branch);
            assert_eq!(fault.kind, *kind, "{branch:?}: {fault}");
            assert_eq!(fault.field, "default_branch");
        }
        assert_eq!(
            check_default_branch("").expect_err("empty").message,
            "The default_branch is empty. Send a value, for example main."
        );
        assert_eq!(
            check_default_branch("-main").expect_err("hyphen").message,
            "The default_branch \"-main\" starts with a hyphen. Git does not permit that in \
             the name of a branch. Send the name of a branch, for example main."
        );
        assert_eq!(
            check_default_branch("main.lock").expect_err("lock").message,
            "The default_branch \"main.lock\" ends with .lock. Git does not permit that in \
             the name of a branch. Send the name of a branch, for example main."
        );
    }

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
        assert!(!is_valid_slug("0193a1c2-0000-7000-8000-000000000003"));
        assert!(is_valid_slug("0193a1c2-0000-7000-8000-00000000000x"));
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
        // Long names truncate to 63 and never end in a hyphen.
        let long = format!("{}/{}", "a".repeat(40), "b".repeat(40));
        let slug = slug_from_full_name(&long);
        assert_eq!(slug.len(), 63);
        assert!(!slug.ends_with('-'));
        assert!(is_valid_slug(&slug));
        // A name that is 62 chars then a separator: the truncation lands on
        // the hyphen and it must be trimmed (the SQL mirrors this with
        // rtrim(left(..)), KAIROS-T-0113).
        let edge = format!("{}/x", "a".repeat(62));
        assert_eq!(slug_from_full_name(&edge), "a".repeat(62));
    }
}
