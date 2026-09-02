//! Git-forge payload normalization (KAIROS-T-0098, design in
//! KAIROS-I-0009): two forge vocabularies in, one [`ForgeEvent`] out,
//! plus short-code extraction.
//!
//! Pure by design (A-0009): no I/O, no database, no HTTP. Parsers take
//! the RAW body string because the webhook endpoint must verify the HMAC
//! over exactly the bytes that arrived before it parses anything — the
//! signature path must never depend on a round-tripped value.
//!
//! # What "one event shape" hides
//!
//! - GitHub has **no `merged` action**: a merged pull request arrives as
//!   `closed` with `pull_request.merged == true`. Reading the action
//!   alone silently records every merge as a close.
//! - GitLab calls them merge requests, numbers them with the
//!   project-scoped `iid` (the number humans see, not `id`), and reports
//!   drafts through `work_in_progress`.
//! - Both send event types nobody asked for (pings, tag pushes). Those
//!   normalize to `None`, never an error — an error would make the forge
//!   retry a delivery that will never succeed.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::short_code::ItemType;

/// What a link points at. A GitHub pull request and a GitLab merge
/// request are the same thing under different names — one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Branch,
    PullRequest,
}

impl LinkKind {
    /// The wire/storage string (matches `item_links.kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Branch => "branch",
            LinkKind::PullRequest => "pull_request",
        }
    }
}

/// The forge-side state of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Open,
    Merged,
    Closed,
    Draft,
}

impl LinkState {
    /// The wire/storage string (matches `item_links.state`).
    pub fn as_str(self) -> &'static str {
        match self {
            LinkState::Open => "open",
            LinkState::Merged => "merged",
            LinkState::Closed => "closed",
            LinkState::Draft => "draft",
        }
    }
}

/// One normalized forge event, ready to become an `item_links` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeEvent {
    pub kind: LinkKind,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub state: LinkState,
    pub author: String,
    /// The forge's own last-update time — the ordering guard the upsert
    /// compares against.
    pub forge_updated_at: DateTime<Utc>,
    /// Everything short codes may appear in (branch name, title, body),
    /// concatenated so extraction has one input.
    pub match_text: String,
    /// The repository the event came from (`owner/repo`), used to resolve
    /// the connection.
    pub repo_full_name: String,
}

/// Parse a GitHub delivery. `event_type` is the `X-GitHub-Event` header.
/// Returns `None` for event types this integration does not consume.
pub fn parse_github(event_type: &str, body: &str) -> Option<ForgeEvent> {
    let v: Value = serde_json::from_str(body).ok()?;
    let repo_full_name = v["repository"]["full_name"].as_str()?.to_string();
    match event_type {
        "pull_request" => {
            let pr = &v["pull_request"];
            let merged = pr["merged"].as_bool().unwrap_or(false);
            let draft = pr["draft"].as_bool().unwrap_or(false);
            // GitHub has no `merged` action: a merge is `closed` + merged.
            let state = match (pr["state"].as_str().unwrap_or("open"), merged, draft) {
                (_, true, _) => LinkState::Merged,
                ("closed", false, _) => LinkState::Closed,
                (_, false, true) => LinkState::Draft,
                _ => LinkState::Open,
            };
            let number = pr["number"].as_i64()?;
            let title = pr["title"].as_str().unwrap_or_default().to_string();
            let body_text = pr["body"].as_str().unwrap_or_default();
            let branch = pr["head"]["ref"].as_str().unwrap_or_default();
            Some(ForgeEvent {
                kind: LinkKind::PullRequest,
                external_id: number.to_string(),
                match_text: format!("{branch} {title} {body_text}"),
                title,
                url: pr["html_url"].as_str().unwrap_or_default().to_string(),
                state,
                author: pr["user"]["login"].as_str().unwrap_or_default().to_string(),
                forge_updated_at: parse_time(pr["updated_at"].as_str())?,
                repo_full_name,
            })
        }
        // Branch creation. `create` also fires for tags — only branches.
        "create" if v["ref_type"].as_str() == Some("branch") => {
            let branch = v["ref"].as_str()?.to_string();
            let repo_url = v["repository"]["html_url"].as_str().unwrap_or_default();
            Some(ForgeEvent {
                kind: LinkKind::Branch,
                url: format!("{repo_url}/tree/{branch}"),
                title: branch.clone(),
                match_text: branch.clone(),
                external_id: branch,
                state: LinkState::Open,
                author: v["sender"]["login"].as_str().unwrap_or_default().to_string(),
                // `create` carries no timestamp; the delivery IS the event.
                forge_updated_at: Utc::now(),
                repo_full_name,
            })
        }
        _ => None,
    }
}

/// Parse a GitLab delivery. `event_type` is the `X-Gitlab-Event` header.
pub fn parse_gitlab(event_type: &str, body: &str) -> Option<ForgeEvent> {
    let v: Value = serde_json::from_str(body).ok()?;
    let repo_full_name = v["project"]["path_with_namespace"].as_str()?.to_string();
    match event_type {
        "Merge Request Hook" => {
            let attrs = &v["object_attributes"];
            let draft = attrs["work_in_progress"].as_bool().unwrap_or(false);
            let state = match (attrs["state"].as_str().unwrap_or("opened"), draft) {
                ("merged", _) => LinkState::Merged,
                ("closed", _) => LinkState::Closed,
                (_, true) => LinkState::Draft,
                _ => LinkState::Open,
            };
            // `iid` is the project-scoped number humans see, not `id`.
            let iid = attrs["iid"].as_i64()?;
            let title = attrs["title"].as_str().unwrap_or_default().to_string();
            let description = attrs["description"].as_str().unwrap_or_default();
            let branch = attrs["source_branch"].as_str().unwrap_or_default();
            Some(ForgeEvent {
                kind: LinkKind::PullRequest,
                external_id: iid.to_string(),
                match_text: format!("{branch} {title} {description}"),
                title,
                url: attrs["url"].as_str().unwrap_or_default().to_string(),
                state,
                author: v["user"]["username"].as_str().unwrap_or_default().to_string(),
                forge_updated_at: parse_time(attrs["updated_at"].as_str())?,
                repo_full_name,
            })
        }
        "Push Hook" => {
            // refs/heads/<branch>; tag pushes use refs/tags and are ignored.
            let branch = v["ref"].as_str()?.strip_prefix("refs/heads/")?.to_string();
            let repo_url = v["project"]["web_url"].as_str().unwrap_or_default();
            Some(ForgeEvent {
                kind: LinkKind::Branch,
                url: format!("{repo_url}/-/tree/{branch}"),
                title: branch.clone(),
                match_text: branch.clone(),
                external_id: branch,
                state: LinkState::Open,
                author: v["user_username"].as_str().unwrap_or_default().to_string(),
                forge_updated_at: Utc::now(),
                repo_full_name,
            })
        }
        _ => None,
    }
}

/// GitLab timestamps are not always RFC 3339 (`2026-09-01 10:00:00 UTC`),
/// so try both shapes.
fn parse_time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.with_timezone(&Utc));
    }
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S UTC")
        .ok()
        .map(|naive| naive.and_utc())
}

/// Every Kairos short code in `text`, deduplicated, in order of first
/// appearance.
///
/// The shape is `PREFIX-<letter>-NNNN` where the letter set is derived
/// from [`ItemType`] — a sixth entity type cannot silently fail to match.
/// Case-sensitive: short codes are uppercase, and matching lowercase
/// would turn ordinary prose into false links.
pub fn extract_short_codes(text: &str) -> Vec<String> {
    let letters: Vec<u8> = ItemType::ALL.iter().map(|t| t.letter() as u8).collect();
    let bytes = text.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0usize;
    // A sliding scan, NOT a token match: the primary carrier is a branch
    // name like `dylan/DEMO-T-0002-fix-auth`, where the code is embedded
    // with a prefix before it and a slug after. Anchor on the `-L-NNNN`
    // tail, then walk backwards over the prefix run.
    //
    // Short codes are pure ASCII, so byte scanning is safe on UTF-8 input:
    // non-ASCII bytes match none of these classes, and every slice
    // boundary below lands on a verified ASCII byte.
    while i < bytes.len() {
        let tail_matches = bytes[i] == b'-'
            && i + 6 < bytes.len()
            && letters.contains(&bytes[i + 1])
            && bytes[i + 2] == b'-'
            && bytes[i + 3..i + 7].iter().all(u8::is_ascii_digit)
            // Exactly four digits: a fifth means it is not a short code.
            && !matches!(bytes.get(i + 7), Some(b) if b.is_ascii_digit());
        if tail_matches {
            // The prefix is the maximal [A-Z0-9] run ending at `i`, and it
            // must START with a letter (`0EMO-T-0001` is not a code).
            let mut start = i;
            while start > 0
                && (bytes[start - 1].is_ascii_uppercase() || bytes[start - 1].is_ascii_digit())
            {
                start -= 1;
            }
            if start < i && bytes[start].is_ascii_uppercase() {
                let code = &text[start..i + 7];
                if !found.iter().any(|f| f == code) {
                    found.push(code.to_string());
                }
                i += 7;
                continue;
            }
        }
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/forge/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|e| panic!("reading fixture {name}: {e}"))
    }

    // ---- short-code extraction --------------------------------------------

    #[test]
    fn extracts_codes_from_branches_titles_and_bodies() {
        assert_eq!(
            extract_short_codes("dylan/DEMO-T-0002-fix-auth"),
            vec!["DEMO-T-0002"]
        );
        assert_eq!(
            extract_short_codes("Fix login (DEMO-T-0002) and docs DEMO-D-0001."),
            vec!["DEMO-T-0002", "DEMO-D-0001"]
        );
        // Every entity letter participates.
        assert_eq!(
            extract_short_codes("A-S-0001 A-I-0002 A-T-0003 A-D-0004 A-A-0005").len(),
            5
        );
    }

    #[test]
    fn deduplicates_preserving_first_appearance() {
        assert_eq!(
            extract_short_codes("DEMO-T-0002 then DEMO-T-0001 then DEMO-T-0002"),
            vec!["DEMO-T-0002", "DEMO-T-0001"]
        );
    }

    #[test]
    fn rejects_lookalikes() {
        // Lowercase, wrong digit count, unknown type letter, empty or
        // digit-leading prefix.
        for text in [
            "demo-t-0002",
            "DEMO-T-002",
            "DEMO-T-00021",
            "DEMO-X-0002",
            "DEMO--0002",
            "0EMO-T-0002",
            "-T-0002",
        ] {
            assert!(
                extract_short_codes(text).is_empty(),
                "{text:?} must not match"
            );
        }
    }

    /// A trailing slug is NORMAL, not a lookalike: branch names carry one
    /// (`dylan/DEMO-T-0002-fix-auth`), which is the whole point of
    /// scanning rather than matching whole tokens.
    #[test]
    fn matches_codes_embedded_in_branch_names() {
        for (text, expected) in [
            ("dylan/DEMO-T-0002-fix-auth", "DEMO-T-0002"),
            ("DEMO-T-0002-EXTRA", "DEMO-T-0002"),
            ("feature/DEMO-I-0003", "DEMO-I-0003"),
            ("refs/heads/carol/DEMO-T-0004-portal", "DEMO-T-0004"),
        ] {
            assert_eq!(extract_short_codes(text), vec![expected], "{text:?}");
        }
    }

    #[test]
    fn trims_surrounding_punctuation() {
        assert_eq!(extract_short_codes("[DEMO-T-0002]"), vec!["DEMO-T-0002"]);
        assert_eq!(extract_short_codes("(DEMO-T-0002),"), vec!["DEMO-T-0002"]);
        assert_eq!(extract_short_codes("#DEMO-T-0002!"), vec!["DEMO-T-0002"]);
    }

    // ---- GitHub -----------------------------------------------------------

    #[test]
    fn github_pull_request_opened() {
        let event = parse_github("pull_request", &fixture("github_pr_opened.json"))
            .expect("PR events normalize");
        assert_eq!(event.kind, LinkKind::PullRequest);
        assert_eq!(event.state, LinkState::Open);
        assert_eq!(event.external_id, "42");
        assert_eq!(event.repo_full_name, "acme/payments-api");
        assert_eq!(event.author, "dylan");
        // Branch, title, and body all feed matching.
        assert_eq!(
            extract_short_codes(&event.match_text),
            vec!["DEMO-T-0002", "DEMO-D-0001"]
        );
    }

    /// The trap: GitHub reports a merge as `closed` + `merged: true`.
    #[test]
    fn github_merged_is_closed_plus_merged_flag() {
        let event = parse_github("pull_request", &fixture("github_pr_merged.json"))
            .expect("merged PR normalizes");
        assert_eq!(
            event.state,
            LinkState::Merged,
            "a merged PR must not be recorded as merely closed"
        );
    }

    #[test]
    fn github_closed_without_merge_is_closed() {
        let event = parse_github("pull_request", &fixture("github_pr_closed.json"))
            .expect("closed PR normalizes");
        assert_eq!(event.state, LinkState::Closed);
    }

    #[test]
    fn github_draft_is_draft() {
        let event = parse_github("pull_request", &fixture("github_pr_draft.json"))
            .expect("draft PR normalizes");
        assert_eq!(event.state, LinkState::Draft);
    }

    #[test]
    fn github_branch_creation() {
        let event =
            parse_github("create", &fixture("github_create_branch.json")).expect("branch event");
        assert_eq!(event.kind, LinkKind::Branch);
        assert_eq!(event.external_id, "dylan/DEMO-T-0002-fix-auth");
        assert_eq!(extract_short_codes(&event.match_text), vec!["DEMO-T-0002"]);
    }

    #[test]
    fn github_tag_creation_and_pings_are_ignored() {
        assert!(parse_github("create", &fixture("github_create_tag.json")).is_none());
        assert!(parse_github("ping", &fixture("github_ping.json")).is_none());
        // Unknown event types too, and malformed bodies never panic.
        assert!(parse_github("issues", &fixture("github_ping.json")).is_none());
        assert!(parse_github("pull_request", "not json").is_none());
    }

    // ---- GitLab -----------------------------------------------------------

    #[test]
    fn gitlab_merge_request_opened() {
        let event = parse_gitlab("Merge Request Hook", &fixture("gitlab_mr_open.json"))
            .expect("MR events normalize");
        assert_eq!(event.kind, LinkKind::PullRequest);
        assert_eq!(event.state, LinkState::Open);
        // `iid` (project-scoped), not the global `id`.
        assert_eq!(event.external_id, "7");
        assert_eq!(event.repo_full_name, "acme/portal-web");
        assert_eq!(extract_short_codes(&event.match_text), vec!["DEMO-T-0004"]);
    }

    #[test]
    fn gitlab_merged_and_draft_states() {
        let merged = parse_gitlab("Merge Request Hook", &fixture("gitlab_mr_merged.json"))
            .expect("merged MR");
        assert_eq!(merged.state, LinkState::Merged);
        let draft =
            parse_gitlab("Merge Request Hook", &fixture("gitlab_mr_draft.json")).expect("draft MR");
        assert_eq!(draft.state, LinkState::Draft);
    }

    #[test]
    fn gitlab_push_hook_branch_only() {
        let event =
            parse_gitlab("Push Hook", &fixture("gitlab_push.json")).expect("push event");
        assert_eq!(event.kind, LinkKind::Branch);
        assert_eq!(event.external_id, "dylan/DEMO-T-0004-portal");
        // Tag pushes carry refs/tags and are ignored.
        assert!(parse_gitlab("Push Hook", &fixture("gitlab_push_tag.json")).is_none());
        assert!(parse_gitlab("Pipeline Hook", &fixture("gitlab_push.json")).is_none());
    }

    #[test]
    fn gitlab_accepts_its_non_rfc3339_timestamps() {
        // GitLab sends `2026-09-01 10:00:00 UTC` in places.
        let event = parse_gitlab("Merge Request Hook", &fixture("gitlab_mr_open.json"))
            .expect("MR with GitLab-style timestamp");
        assert_eq!(event.forge_updated_at.to_rfc3339(), "2026-09-01T10:00:00+00:00");
    }
}
