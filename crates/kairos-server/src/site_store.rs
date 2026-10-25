//! The store of the uploaded repository sites (KAIROS-T-0405, KAIROS-D-0012
//! "Storage").
//!
//! One interface with two backends, both from the `object_store` crate: a
//! folder on disk, or an S3 bucket (any S3-compatible store with an
//! endpoint). The key of a file is
//!
//! ```text
//! <org_id>/<repository_id>/<publish_id>/<path>
//! ```
//!
//! IDs, never slugs: a rename or a move to another team touches no stored
//! file. A publish writes its files under a new publish id, and the switch to
//! it is one database write, so S3 (which has no rename) switches as
//! atomically as a disk. The files of a replaced, failed or unpublished
//! publish are deleted by a [`CLEANUP_KIND`] job.
//!
//! With more than one server, a `disk` store must be a shared volume.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{StreamExt as _, TryStreamExt as _};
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as StorePath;
use object_store::{ObjectStore, PutPayload};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::ConfigError;
use crate::jobs::{JobFuture, JobHandle, JobHandler, Worker};

/// The kind of the job that deletes the files of one publish.
pub const CLEANUP_KIND: &str = "site_cleanup";

/// The priority of a cleanup: below a periodic pass, as nothing waits on it.
pub const CLEANUP_PRIORITY: i32 = 5;

/// The wait before the files of a replaced publish are deleted, so a reader
/// in the middle of loading a page does not see holes.
pub const REPLACED_GRACE: Duration = Duration::from_secs(5 * 60);

/// The default limit of the files of one site: 1 GiB, as GitHub Pages
/// ("a published site may be no larger than 1 GB").
pub const DEFAULT_MAX_SIZE: u64 = 1 << 30;

/// The default limit of the entries (files and folders) of one site, as
/// GitLab Pages (200,000 file entries).
pub const DEFAULT_MAX_FILES: u64 = 200_000;

/// The settings of the docs sites (`KAIROS_PAGES_*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagesSettings {
    /// Where the uploaded sites are kept; `None`: no store, and a publish is
    /// refused.
    pub store: Option<StoreConfig>,
    /// `KAIROS_PAGES_MAX_SIZE`: the bytes of the files of one site, and of
    /// its archive (KAIROS-T-0406).
    pub max_size: u64,
    /// `KAIROS_PAGES_MAX_FILES`: the entries of one archive.
    pub max_files: u64,
}

impl Default for PagesSettings {
    fn default() -> Self {
        PagesSettings {
            store: None,
            max_size: DEFAULT_MAX_SIZE,
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

/// `KAIROS_PAGES_STORE` and the variables of its backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreConfig {
    /// `disk`: `KAIROS_PAGES_DIR`.
    Disk { dir: PathBuf },
    /// `s3`: `KAIROS_PAGES_S3_*`; credentials from the standard AWS
    /// variables or the instance role.
    S3 {
        bucket: String,
        prefix: Option<String>,
        endpoint: Option<String>,
        region: Option<String>,
    },
}

/// Read `KAIROS_PAGES_STORE` and the variables of its backend. Unset: no
/// store. An unknown value or a missing variable stops the server and names
/// the variable.
pub fn parse_settings(get: &impl Fn(&str) -> Option<String>) -> Result<PagesSettings, ConfigError> {
    let store = match get("KAIROS_PAGES_STORE").map(|v| v.trim().to_ascii_lowercase()) {
        None => None,
        Some(kind) if kind.is_empty() => None,
        Some(kind) if kind == "disk" => Some(StoreConfig::Disk {
            dir: get("KAIROS_PAGES_DIR")
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .ok_or(ConfigError::Missing {
                    var: "KAIROS_PAGES_DIR",
                    hint: "set it to the folder of the uploaded sites when KAIROS_PAGES_STORE=disk",
                })?,
        }),
        Some(kind) if kind == "s3" => Some(StoreConfig::S3 {
            bucket: get("KAIROS_PAGES_S3_BUCKET")
                .filter(|v| !v.trim().is_empty())
                .ok_or(ConfigError::Missing {
                    var: "KAIROS_PAGES_S3_BUCKET",
                    hint: "set it to the bucket of the uploaded sites when KAIROS_PAGES_STORE=s3",
                })?,
            prefix: get("KAIROS_PAGES_S3_PREFIX")
                .map(|v| v.trim().trim_matches('/').to_string())
                .filter(|v| !v.is_empty()),
            endpoint: get("KAIROS_PAGES_S3_ENDPOINT").filter(|v| !v.trim().is_empty()),
            region: get("KAIROS_PAGES_S3_REGION").filter(|v| !v.trim().is_empty()),
        }),
        Some(other) => {
            return Err(ConfigError::Invalid {
                var: "KAIROS_PAGES_STORE",
                message: format!("{other:?} is not a store. Set disk or s3, or leave it unset."),
            });
        }
    };
    let limit = |var: &'static str, default: u64| -> Result<u64, ConfigError> {
        match get(var) {
            None => Ok(default),
            Some(value) => match value.trim().parse::<u64>() {
                Ok(n) if n > 0 => Ok(n),
                _ => Err(ConfigError::Invalid {
                    var,
                    message: format!("{value:?} is not a number larger than 0"),
                }),
            },
        }
    };
    Ok(PagesSettings {
        store,
        max_size: limit("KAIROS_PAGES_MAX_SIZE", DEFAULT_MAX_SIZE)?,
        max_files: limit("KAIROS_PAGES_MAX_FILES", DEFAULT_MAX_FILES)?,
    })
}

/// A failure of the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The store could not be built from its settings.
    #[error("the site store cannot start: {0}")]
    Build(String),
    /// A read, a write or a delete failed.
    #[error("the site store failed: {0}")]
    Io(#[from] object_store::Error),
    /// A local file of an upload could not be read.
    #[error("the site store cannot read {path}: {}", crate::error::io_words(.source))]
    Local {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The store of the uploaded sites.
#[derive(Clone)]
pub struct SiteStore {
    inner: Arc<dyn ObjectStore>,
    /// The key prefix inside the bucket (S3 only).
    prefix: Option<String>,
}

impl std::fmt::Debug for SiteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiteStore")
            .field("store", &self.inner.to_string())
            .field("prefix", &self.prefix)
            .finish()
    }
}

/// The publish that a set of files belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishKey {
    pub org_id: Uuid,
    pub repository_id: Uuid,
    pub publish_id: Uuid,
}

impl SiteStore {
    /// Build the store of `config`. A disk folder is made when it is missing.
    pub fn new(config: &StoreConfig) -> Result<Self, StoreError> {
        match config {
            StoreConfig::Disk { dir } => {
                std::fs::create_dir_all(dir).map_err(|e| {
                    StoreError::Build(format!("cannot make {}: {e}", dir.display()))
                })?;
                let store = LocalFileSystem::new_with_prefix(dir)
                    .map_err(|e| StoreError::Build(e.to_string()))?
                    .with_automatic_cleanup(true);
                Ok(SiteStore {
                    inner: Arc::new(store),
                    prefix: None,
                })
            }
            StoreConfig::S3 {
                bucket,
                prefix,
                endpoint,
                region,
            } => {
                let mut builder = AmazonS3Builder::from_env().with_bucket_name(bucket);
                if let Some(endpoint) = endpoint {
                    builder = builder
                        .with_endpoint(endpoint)
                        .with_allow_http(endpoint.starts_with("http://"))
                        // MinIO and most S3-compatible stores take path-style URLs.
                        .with_virtual_hosted_style_request(false);
                }
                if let Some(region) = region {
                    builder = builder.with_region(region);
                }
                let store = builder
                    .build()
                    .map_err(|e| StoreError::Build(e.to_string()))?;
                Ok(SiteStore {
                    inner: Arc::new(store),
                    prefix: prefix.clone(),
                })
            }
        }
    }

    fn publish_prefix(&self, key: &PublishKey) -> StorePath {
        let base = format!("{}/{}/{}", key.org_id, key.repository_id, key.publish_id);
        match &self.prefix {
            Some(prefix) => StorePath::from(format!("{prefix}/{base}")),
            None => StorePath::from(base),
        }
    }

    /// The key of the file `path` (relative, `/`-separated) of a publish.
    pub fn file_key(&self, key: &PublishKey, path: &str) -> StorePath {
        let mut store_path = self.publish_prefix(key);
        for part in path.split('/').filter(|part| !part.is_empty()) {
            store_path = store_path.child(part);
        }
        store_path
    }

    /// Write `bytes` as the file `path` of a publish.
    pub async fn put(&self, key: &PublishKey, path: &str, bytes: Bytes) -> Result<(), StoreError> {
        self.inner
            .put(&self.file_key(key, path), PutPayload::from(bytes))
            .await?;
        Ok(())
    }

    /// Write the local file `local` as the file `path` of a publish.
    pub async fn put_file(
        &self,
        key: &PublishKey,
        path: &str,
        local: &Path,
    ) -> Result<(), StoreError> {
        let bytes = tokio::fs::read(local)
            .await
            .map_err(|source| StoreError::Local {
                path: local.to_path_buf(),
                source,
            })?;
        self.put(key, path, Bytes::from(bytes)).await
    }

    /// Read the file `path` of a publish; `None` when the store does not
    /// have it.
    pub async fn get(&self, key: &PublishKey, path: &str) -> Result<Option<Bytes>, StoreError> {
        match self.inner.get(&self.file_key(key, path)).await {
            Ok(result) => Ok(Some(result.bytes().await?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete each file of a publish. Returns the number of files deleted.
    pub async fn delete_publish(&self, key: &PublishKey) -> Result<usize, StoreError> {
        let prefix = self.publish_prefix(key);
        let locations = self
            .inner
            .list(Some(&prefix))
            .map_ok(|meta| meta.location)
            .boxed();
        let deleted: Vec<StorePath> = self.inner.delete_stream(locations).try_collect().await?;
        Ok(deleted.len())
    }
}

/// The payload of a [`CLEANUP_KIND`] job.
pub fn cleanup_job(key: &PublishKey, delay: Duration) -> kairos_db::jobs::NewJob {
    let mut job = kairos_db::jobs::NewJob::new(CLEANUP_KIND);
    job.payload = serde_json::to_value(key).expect("a publish key is JSON");
    job.priority = CLEANUP_PRIORITY;
    job.dedupe_key = Some(format!("{CLEANUP_KIND}:{}", key.publish_id));
    job.run_after =
        Some(chrono::Utc::now() + chrono::Duration::from_std(delay).expect("a short delay"));
    job
}

/// The handler of the [`CLEANUP_KIND`] jobs.
pub struct CleanupHandler {
    store: SiteStore,
}

impl CleanupHandler {
    /// Run the cleanups of `store` on `worker`.
    pub fn register(store: SiteStore, worker: &Worker) {
        worker.register(CLEANUP_KIND, Arc::new(CleanupHandler { store }));
    }
}

impl JobHandler for CleanupHandler {
    fn run(&self, handle: JobHandle) -> JobFuture {
        let store = self.store.clone();
        Box::pin(async move {
            let key: PublishKey = serde_json::from_str(&handle.job().payload)
                .map_err(|e| crate::jobs::JobError::fail(format!("bad payload: {e}")))?;
            let deleted = store
                .delete_publish(&key)
                .await
                .map_err(|e| crate::jobs::JobError::retry(e.to_string()))?;
            handle.progress();
            tracing::info!(
                publish_id = %key.publish_id,
                repository_id = %key.repository_id,
                deleted,
                "deleted the files of a site publish"
            );
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn no_store_when_unset() {
        assert_eq!(parse_settings(&env(&[])).unwrap(), PagesSettings::default());
    }

    #[test]
    fn disk_needs_its_folder() {
        let err = parse_settings(&env(&[("KAIROS_PAGES_STORE", "disk")])).unwrap_err();
        assert!(err.to_string().contains("KAIROS_PAGES_DIR"), "{err}");
        let settings = parse_settings(&env(&[
            ("KAIROS_PAGES_STORE", "Disk"),
            ("KAIROS_PAGES_DIR", "/var/lib/kairos/pages"),
        ]))
        .unwrap();
        assert_eq!(
            settings.store,
            Some(StoreConfig::Disk {
                dir: PathBuf::from("/var/lib/kairos/pages")
            })
        );
    }

    #[test]
    fn s3_needs_its_bucket() {
        let err = parse_settings(&env(&[("KAIROS_PAGES_STORE", "s3")])).unwrap_err();
        assert!(err.to_string().contains("KAIROS_PAGES_S3_BUCKET"), "{err}");
        let settings = parse_settings(&env(&[
            ("KAIROS_PAGES_STORE", "s3"),
            ("KAIROS_PAGES_S3_BUCKET", "sites"),
            ("KAIROS_PAGES_S3_PREFIX", "/kairos/"),
            ("KAIROS_PAGES_S3_ENDPOINT", "http://minio.test:9000"),
            ("KAIROS_PAGES_S3_REGION", "us-east-1"),
        ]))
        .unwrap();
        assert_eq!(
            settings.store,
            Some(StoreConfig::S3 {
                bucket: "sites".into(),
                prefix: Some("kairos".into()),
                endpoint: Some("http://minio.test:9000".into()),
                region: Some("us-east-1".into()),
            })
        );
    }

    #[test]
    fn the_limits() {
        let settings = parse_settings(&env(&[])).unwrap();
        assert_eq!((settings.max_size, settings.max_files), (1 << 30, 200_000));
        let settings = parse_settings(&env(&[
            ("KAIROS_PAGES_MAX_SIZE", "1048576"),
            ("KAIROS_PAGES_MAX_FILES", "10"),
        ]))
        .unwrap();
        assert_eq!((settings.max_size, settings.max_files), (1 << 20, 10));
        let err = parse_settings(&env(&[("KAIROS_PAGES_MAX_FILES", "0")])).unwrap_err();
        assert!(err.to_string().contains("KAIROS_PAGES_MAX_FILES"), "{err}");
    }

    #[test]
    fn an_unknown_store_is_refused() {
        let err = parse_settings(&env(&[("KAIROS_PAGES_STORE", "ftp")])).unwrap_err();
        assert!(err.to_string().contains("KAIROS_PAGES_STORE"), "{err}");
    }

    fn key() -> PublishKey {
        PublishKey {
            org_id: Uuid::new_v4(),
            repository_id: Uuid::new_v4(),
            publish_id: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn disk_write_read_missing_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = SiteStore::new(&StoreConfig::Disk {
            dir: dir.path().join("pages"),
        })
        .unwrap();
        let (one, two) = (key(), key());
        store
            .put(&one, "index.html", Bytes::from_static(b"<h1>one</h1>"))
            .await
            .unwrap();
        store
            .put(&one, "guide/install.html", Bytes::from_static(b"install"))
            .await
            .unwrap();
        store
            .put(&two, "index.html", Bytes::from_static(b"two"))
            .await
            .unwrap();
        assert_eq!(
            store.get(&one, "index.html").await.unwrap().as_deref(),
            Some(&b"<h1>one</h1>"[..])
        );
        assert_eq!(store.get(&one, "missing.html").await.unwrap(), None);
        // The layout on disk: <org>/<repo>/<publish>/<path>.
        assert!(
            dir.path()
                .join("pages")
                .join(one.org_id.to_string())
                .join(one.repository_id.to_string())
                .join(one.publish_id.to_string())
                .join("guide/install.html")
                .exists()
        );
        assert_eq!(store.delete_publish(&one).await.unwrap(), 2);
        assert_eq!(store.get(&one, "index.html").await.unwrap(), None);
        // The other publish stays.
        assert_eq!(
            store.get(&two, "index.html").await.unwrap().as_deref(),
            Some(&b"two"[..])
        );
    }
}
