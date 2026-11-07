//! `kairos index upload` (KAIROS-T-0592): send the index of a checkout to
//! Kairos as the index of its HEAD commit
//! (`PUT /api/repositories/{slug}/code-indexes/{commit}`).
//!
//! The CLI does not build an index (KAIROS-T-0746): an upload sends an
//! index that `kairos index update` made from a base of the server. The
//! index describes the tree of the checkout. So the CLI refuses a tree
//! with changes that are not committed: Kairos would keep the index for a
//! commit that it does not describe.
//!
//! The deployment and the repository come as for `kairos index update`
//! ([`super::base`]).

use std::path::Path;
use std::time::Duration;

use kairos_client::types_code_index::UploadedCodeIndex;

use super::base::{self, RemoteArgs};
use super::{IndexCommandError, db_path, git_output, lock_index};
use crate::context::print_json;
use crate::error::CliError;

/// How long the upload of an index can take.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Send the index of the checkout at `root` for its HEAD commit. `ref` is
/// the branch, the pull request or the tag; the default is the branch of
/// the checkout.
pub async fn upload(
    root: &Path,
    remote: &RemoteArgs,
    r#ref: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let db = db_path(root);
    if !db.exists() {
        return Err(IndexCommandError::NoIndex(db).into());
    }
    let commit =
        git_output(root, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]).ok_or_else(|| {
            CliError::Failure(format!(
                "The checkout at {} has no git commit. The upload needs a commit.",
                root.display()
            ))
        })?;
    let changes = git_output(root, &["status", "--porcelain"]).unwrap_or_default();
    if let Some(refusal) = dirty_refusal(&changes) {
        return Err(CliError::Failure(refusal));
    }
    let r#ref = r#ref.or_else(|| git_output(root, &["symbolic-ref", "-q", "--short", "HEAD"]));
    let (client, repository) = base::repository_of(root, remote)
        .await
        .map_err(|why| CliError::Failure(format!("{why}.")))?;
    let bytes = {
        let _lock = lock_index(&db)?;
        std::fs::read(&db)
            .map_err(|e| CliError::Failure(format!("The CLI cannot read {}: {e}.", db.display())))?
    };
    let uploaded = client
        .with_timeout(UPLOAD_TIMEOUT)
        .upload_code_index(&repository, &commit, r#ref.as_deref(), bytes)
        .await?;
    if json {
        return print_json(&uploaded);
    }
    println!("{}", uploaded_line(&repository, &uploaded));
    Ok(())
}

/// The refusal of a checkout with changes that are not committed
/// (`git status --porcelain`), or `None` for a clean tree. Pure.
fn dirty_refusal(porcelain: &str) -> Option<String> {
    let changed: Vec<&str> = porcelain
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if changed.is_empty() {
        return None;
    }
    // `XY path`; the first line can come trimmed, so the path is after
    // the first space.
    let first = changed[0]
        .trim_start()
        .split_once(' ')
        .map_or(changed[0], |(_, path)| path.trim());
    Some(format!(
        "The checkout has {} changed file{} that {} not committed, for example {first}. The \
         index describes the tree, so it is not the index of the commit. Commit or stash the \
         changes, run `kairos index update`, then upload again.",
        changed.len(),
        if changed.len() == 1 { "" } else { "s" },
        if changed.len() == 1 { "is" } else { "are" },
    ))
}

/// The line after an upload. Pure.
fn uploaded_line(repository: &str, uploaded: &UploadedCodeIndex) -> String {
    let index = &uploaded.index;
    format!(
        "Kairos keeps the index of {}{} for the repository {repository}. It uses {} \
         summaries, and {} of them are new. The pool has {} summaries.",
        base::short(&index.commit),
        index
            .r#ref
            .as_deref()
            .map_or_else(String::new, |r| format!(" ({r})")),
        index.summary_keys,
        uploaded.new_summaries,
        uploaded.pool_size
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_with_changes_is_refused_with_a_file_that_changed() {
        assert_eq!(dirty_refusal(""), None);
        assert_eq!(dirty_refusal("\n"), None);
        let text = dirty_refusal("M pkg/stats.py\n?? notes.txt\n").expect("a refusal");
        assert!(
            text.starts_with(
                "The checkout has 2 changed files that are not committed, for example \
                 pkg/stats.py."
            ),
            "{text}"
        );
        assert!(text.contains("kairos index update"), "{text}");
        let one = dirty_refusal("?? notes.txt").expect("a refusal");
        assert!(one.contains("1 changed file that is not"), "{one}");
    }

    #[test]
    fn the_upload_line_names_the_commit_the_ref_and_the_summaries() {
        let uploaded = UploadedCodeIndex {
            index: kairos_client::types_code_index::CodeIndex {
                commit: "0123456789abcdef0123456789abcdef01234567".into(),
                r#ref: Some("main".into()),
                refs: vec![kairos_client::types_code_index::CodeIndexRef {
                    name: "main".into(),
                    kind: "branch".into(),
                }],
                source: "upload".into(),
                structure_bytes: 10,
                summary_keys: 7,
                schema_version: Some(3),
                vector_model: None,
                created_at: "2026-10-18T00:00:00Z".into(),
                updated_at: "2026-10-18T00:00:00Z".into(),
            },
            new_summaries: 2,
            pool_size: 40,
        };
        assert_eq!(
            uploaded_line("payments-api", &uploaded),
            "Kairos keeps the index of 0123456789ab (main) for the repository payments-api. It \
             uses 7 summaries, and 2 of them are new. The pool has 40 summaries."
        );
    }
}
