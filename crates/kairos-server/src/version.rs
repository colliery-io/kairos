//! `GET /version` (KAIROS-T-0432): the version of the server and where the
//! CLI of that version is, for `kairos self-update`. Open, like the probes.
//!
//! `GET /cli/<file>` (KAIROS-T-0745, KAIROS-A-0080): the release workflow
//! copies the CLI tarballs of this version into the image, and the server
//! serves them, so an upgrade needs no access to github.com. `/version`
//! names them when the folder has them.

use std::path::Path;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::app::AppState;
use crate::error::ApiError;

/// The GitHub releases of Kairos, when `KAIROS_CLI_DOWNLOAD_URL` is not set.
pub const DEFAULT_DOWNLOAD_URL: &str = "https://github.com/colliery-io/kairos/releases";

/// The folder of the CLI tarballs in the image, when `KAIROS_CLI_DIR` is
/// not set. The Dockerfile copies them there.
pub const DEFAULT_CLI_DIR: &str = "/usr/share/kairos/cli";

/// The targets that the release workflow builds the CLI for.
pub const CLI_TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

/// The body of `GET /version`.
#[derive(Debug, Serialize)]
pub struct VersionResponse {
    pub version: &'static str,
    /// One tarball of the CLI of this version for each target.
    pub cli: Vec<CliDownload>,
}

/// The tarball of the CLI for one target, and its checksum.
#[derive(Debug, Serialize)]
pub struct CliDownload {
    pub target: &'static str,
    pub url: String,
    pub sha256_url: String,
}

/// Where `GET /version` sends the CLI of each target.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliSource<'a> {
    /// `KAIROS_CLI_DOWNLOAD_URL`: a mirror of the GitHub releases. It wins.
    pub mirror: Option<&'a str>,
    /// `KAIROS_CLI_DIR`: the tarballs that this server serves.
    pub dir: Option<&'a Path>,
    /// `KAIROS_PUBLIC_URL`: makes the link of a served tarball absolute.
    /// With none, the link is a path, and the CLI resolves it against the
    /// deployment.
    pub public_url: Option<&'a str>,
}

/// The name of the tarball of the CLI of this version for a target.
fn tarball(target: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!("kairos-{version}-{target}.tar.gz")
}

/// The response for a source (testable).
pub fn response(source: CliSource<'_>) -> VersionResponse {
    let version = env!("CARGO_PKG_VERSION");
    let cli = CLI_TARGETS
        .iter()
        .map(|target| {
            let file = tarball(target);
            let url = match (source.mirror, source.dir) {
                (Some(base), _) => format!("{base}/download/v{version}/{file}"),
                (None, Some(dir)) if dir.join(&file).is_file() => {
                    format!("{}/cli/{file}", source.public_url.unwrap_or(""))
                }
                (None, _) => format!("{DEFAULT_DOWNLOAD_URL}/download/v{version}/{file}"),
            };
            CliDownload {
                target,
                sha256_url: format!("{url}.sha256"),
                url,
            }
        })
        .collect();
    VersionResponse { version, cli }
}

pub(crate) async fn version(State(state): State<AppState>) -> Json<VersionResponse> {
    let config = &state.config;
    Json(response(CliSource {
        mirror: config.cli_download_url.as_deref(),
        dir: config.cli_dir.as_deref(),
        public_url: config.public_url.as_deref(),
    }))
}

/// The file of `GET /cli/<file>` in a folder: only a tarball of this
/// version or its checksum, so no other name reads the disk.
fn served_file(dir: &Path, file: &str) -> Option<(std::path::PathBuf, &'static str)> {
    CLI_TARGETS.iter().find_map(|target| {
        let tarball = tarball(target);
        if file == tarball {
            Some((dir.join(file), "application/gzip"))
        } else if file == format!("{tarball}.sha256") {
            Some((dir.join(file), "text/plain; charset=utf-8"))
        } else {
            None
        }
    })
}

pub(crate) async fn cli_file(
    State(state): State<AppState>,
    UrlPath(file): UrlPath<String>,
) -> Result<Response, ApiError> {
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!(
                "This server has no CLI file `{file}`. It serves the CLI {} only: \
                 `GET /version` names the files.",
                env!("CARGO_PKG_VERSION")
            ),
        )
        .with_details(serde_json::json!({"field": "file"}))
    };
    let Some(dir) = state.config.cli_dir.as_deref() else {
        return Err(not_found());
    };
    let Some((path, content_type)) = served_file(dir, &file) else {
        return Err(not_found());
    };
    let bytes = tokio::fs::read(&path).await.map_err(|_| not_found())?;
    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_has_a_tarball_and_a_checksum() {
        let body = response(CliSource::default());
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(body.cli.len(), 4);
        let mac = body
            .cli
            .iter()
            .find(|download| download.target == "aarch64-apple-darwin")
            .unwrap();
        assert_eq!(
            mac.url,
            format!(
                "https://github.com/colliery-io/kairos/releases/download/v{version}/kairos-{version}-aarch64-apple-darwin.tar.gz"
            )
        );
        assert_eq!(mac.sha256_url, format!("{}.sha256", mac.url));
    }

    #[test]
    fn a_mirror_replaces_the_base() {
        let body = response(CliSource {
            mirror: Some("https://mirror.example/kairos"),
            ..CliSource::default()
        });
        assert!(body.cli.iter().all(|download| {
            download
                .url
                .starts_with("https://mirror.example/kairos/download/v")
        }));
    }

    /// KAIROS-T-0745: a tarball in the folder is served by this server;
    /// a target with no tarball goes to the published release.
    #[test]
    fn a_tarball_in_the_folder_is_served_here() {
        let dir = tempfile::tempdir().expect("a folder");
        std::fs::write(dir.path().join(tarball("x86_64-unknown-linux-gnu")), b"t").unwrap();
        let link = |body: &VersionResponse, target: &str| {
            body.cli
                .iter()
                .find(|download| download.target == target)
                .map(|download| (download.url.clone(), download.sha256_url.clone()))
                .unwrap()
        };

        let body = response(CliSource {
            dir: Some(dir.path()),
            ..CliSource::default()
        });
        let file = tarball("x86_64-unknown-linux-gnu");
        assert_eq!(
            link(&body, "x86_64-unknown-linux-gnu"),
            (format!("/cli/{file}"), format!("/cli/{file}.sha256"))
        );
        assert!(
            link(&body, "aarch64-apple-darwin")
                .0
                .starts_with(DEFAULT_DOWNLOAD_URL)
        );

        let body = response(CliSource {
            dir: Some(dir.path()),
            public_url: Some("https://kairos.example.com"),
            ..CliSource::default()
        });
        assert_eq!(
            link(&body, "x86_64-unknown-linux-gnu").0,
            format!("https://kairos.example.com/cli/{file}")
        );

        let body = response(CliSource {
            mirror: Some("https://mirror.example"),
            dir: Some(dir.path()),
            ..CliSource::default()
        });
        assert!(
            link(&body, "x86_64-unknown-linux-gnu")
                .0
                .starts_with("https://mirror.example/")
        );
    }

    /// Only the tarballs of this version and their checksums are files.
    #[test]
    fn only_the_files_of_this_version_are_served() {
        let dir = Path::new("/srv/cli");
        let file = tarball("aarch64-apple-darwin");
        assert_eq!(
            served_file(dir, &file),
            Some((dir.join(&file), "application/gzip"))
        );
        assert!(served_file(dir, &format!("{file}.sha256")).is_some());
        for name in [
            "kairos-0.0.1-aarch64-apple-darwin.tar.gz",
            "../etc/passwd",
            "..",
            "kairos",
            "",
        ] {
            assert_eq!(served_file(dir, name), None, "{name}");
        }
    }
}
