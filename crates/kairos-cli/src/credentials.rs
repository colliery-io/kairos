//! The credential cache (KAIROS-A-0010/A-0015): OAuth tokens per
//! deployment in `~/.config/kairos/credentials.json`, mode 0600, keyed by
//! the normalized deployment URL.
//!
//! Resolution order for the config directory: `KAIROS_CONFIG_DIR`
//! (tests/dev override) → `$XDG_CONFIG_HOME/kairos` → `$HOME/.config/kairos`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::CliError;

/// Refresh this many seconds BEFORE the recorded expiry, so a token never
/// dies mid-request.
pub const EXPIRY_SKEW_SECONDS: u64 = 30;

/// One deployment's cached credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeploymentCredentials {
    pub access_token: String,
    /// Absent when the issuer declined `offline_access`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Access-token expiry as unix seconds.
    pub expires_at: u64,
    /// The OIDC issuer the tokens came from (refresh goes back here).
    pub issuer: String,
    /// The OAuth client id used for the device grant and for refresh.
    pub client_id: String,
    /// `X-Tenant` slug for dev/test host-less tenant resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Which token `access_token` above actually holds — `access_token`
    /// (default) or `id_token` (opaque-access-token issuers, T-0054). Refresh
    /// re-selects the same kind. `#[serde(default)]`: pre-T-0054 cache files
    /// (no field) resolve to `access_token`, unchanged.
    #[serde(default)]
    pub api_bearer: crate::oidc::ApiBearer,
}

impl DeploymentCredentials {
    /// True when the access token is expired (or within the skew window)
    /// and should be refreshed before use.
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at <= now + EXPIRY_SKEW_SECONDS
    }
}

/// The on-disk credential store: `{ "version": 1, "deployments": { url: … } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStore {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Keyed by normalized deployment URL. BTreeMap keeps the file diffable.
    #[serde(default)]
    pub deployments: BTreeMap<String, DeploymentCredentials>,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self {
            version: default_version(),
            deployments: BTreeMap::new(),
        }
    }
}

fn default_version() -> u32 {
    1
}

/// Unix seconds now.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Normalize a deployment URL into a cache key: trim whitespace and
/// trailing slashes so `https://x/` and `https://x` share one entry.
pub fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// The kairos config directory (see module docs for resolution order).
pub fn config_dir() -> Result<PathBuf, CliError> {
    if let Ok(dir) = std::env::var("KAIROS_CONFIG_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Ok(Path::new(&xdg).join("kairos"));
    }
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => Ok(Path::new(&home).join(".config").join("kairos")),
        _ => Err(CliError::Failure(
            "cannot locate the config directory: none of KAIROS_CONFIG_DIR, XDG_CONFIG_HOME, \
             or HOME is set"
                .to_string(),
        )),
    }
}

/// `credentials.json` inside the config directory.
pub fn credentials_path() -> Result<PathBuf, CliError> {
    Ok(config_dir()?.join("credentials.json"))
}

/// Load the store from `path`. A missing file is an empty store; a
/// corrupted file is an actionable auth error (the fix is to re-login,
/// which rewrites the file).
pub fn load(path: &Path) -> Result<CredentialStore, CliError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CredentialStore::default());
        }
        Err(err) => {
            return Err(CliError::Failure(format!(
                "cannot read {}: {err}",
                path.display()
            )));
        }
    };
    serde_json::from_str(&contents).map_err(|err| {
        CliError::Auth(format!(
            "the credential cache at {} is corrupted ({err}).\n\
             Run `kairos login --url <deployment>` to re-authenticate (this rewrites the \
             cache), or delete the file.",
            path.display()
        ))
    })
}

/// Write the store to `path` atomically (temp file + rename) with mode
/// 0600, creating the parent directory (0700) when needed.
pub fn save(path: &Path, store: &CredentialStore) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Failure(format!("{} has no parent directory", path.display())))?;
    create_private_dir(parent)?;

    let json = serde_json::to_string_pretty(store)
        .map_err(|err| CliError::Failure(format!("cannot serialize credentials: {err}")))?;

    let tmp = path.with_extension("json.tmp");
    write_private_file(&tmp, &json)
        .map_err(|err| CliError::Failure(format!("cannot write {}: {err}", tmp.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|err| CliError::Failure(format!("cannot write {}: {err}", path.display())))?;
    Ok(())
}

/// Create `dir` (and parents) with owner-only permissions.
fn create_private_dir(dir: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|err| CliError::Failure(format!("cannot create {}: {err}", dir.display())))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)
        .map_err(|err| CliError::Failure(format!("cannot create {}: {err}", dir.display())))?;
    Ok(())
}

/// Write `contents` to `path` with mode 0600 (KAIROS-A-0010).
fn write_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // The 0600 mode only applies at creation; enforce it on rewrites too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents.as_bytes())?;
    file.flush()
}

/// The cached entry for `url` (normalized), or the actionable "not logged
/// in" auth error.
pub fn entry_for(store: &CredentialStore, url: &str) -> Result<DeploymentCredentials, CliError> {
    let key = normalize_url(url);
    store.deployments.get(&key).cloned().ok_or_else(|| {
        CliError::Auth(format!(
            "not logged in to {key}.\nRun `kairos login --url {key}` first."
        ))
    })
}

/// Resolve which deployment a command targets: the explicit `--url`, or
/// the single cached deployment, or an actionable error.
pub fn resolve_deployment(store: &CredentialStore, url: Option<&str>) -> Result<String, CliError> {
    if let Some(url) = url {
        return Ok(normalize_url(url));
    }
    let mut keys = store.deployments.keys();
    match (keys.next(), keys.next()) {
        (Some(only), None) => Ok(only.clone()),
        (None, _) => Err(CliError::Auth(
            "no cached credentials.\nRun `kairos login --url <deployment>` first.".to_string(),
        )),
        (Some(_), Some(_)) => {
            let known: Vec<&str> = store.deployments.keys().map(String::as_str).collect();
            Err(CliError::Failure(format!(
                "multiple deployments are cached ({}); pass --url to pick one",
                known.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{EXIT_AUTH, EXIT_FAILURE};

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kairos-cli-credentials-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn entry(tenant: Option<&str>) -> DeploymentCredentials {
        DeploymentCredentials {
            access_token: "access-1".into(),
            refresh_token: Some("refresh-1".into()),
            expires_at: 4_000_000_000,
            issuer: "http://localhost:5558/dex".into(),
            client_id: "kairos-cli".into(),
            tenant: tenant.map(str::to_string),
            api_bearer: crate::oidc::ApiBearer::AccessToken,
        }
    }

    /// Round trip: save writes 0600 under a 0700 dir; load returns the
    /// same entries keyed by normalized URL.
    #[test]
    fn save_load_round_trip_with_0600() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join("credentials.json");
        let mut store = CredentialStore::default();
        store.deployments.insert(
            normalize_url("http://one.kairos.test/"),
            entry(Some("acme")),
        );
        store
            .deployments
            .insert(normalize_url("http://two.kairos.test"), entry(None));
        save(&path, &store).expect("save");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600, "credentials.json must be 0600");
            let dir_mode = std::fs::metadata(&dir).expect("meta").permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "config dir must be 0700");
        }

        let loaded = load(&path).expect("load");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.deployments.len(), 2);
        // Normalization: trailing slash stripped at insert; lookups with a
        // slash find the same entry.
        let found = entry_for(&loaded, "http://one.kairos.test/").expect("entry");
        assert_eq!(found.tenant.as_deref(), Some("acme"));
        assert_eq!(found, store.deployments["http://one.kairos.test"]);

        // Overwrite (the refresh path) keeps 0600.
        save(&path, &loaded).expect("re-save");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Missing file = empty store; corrupted file = auth error (exit 2)
    /// that names the file and the fix.
    #[test]
    fn missing_and_corrupted_cache() {
        let dir = scratch_dir("corrupt");
        let path = dir.join("credentials.json");
        assert!(load(&path).expect("missing = empty").deployments.is_empty());

        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&path, "{not json").expect("write garbage");
        let err = load(&path).expect_err("corrupted must fail");
        assert_eq!(err.exit_code(), EXIT_AUTH);
        let message = err.to_string();
        assert!(message.contains("credentials.json"), "{message}");
        assert!(message.contains("kairos login"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The skew window: fresh tokens pass, tokens expiring within the
    /// skew (or already expired) need a refresh.
    #[test]
    fn needs_refresh_skew() {
        let now = 1_000_000;
        let mut credentials = entry(None);
        credentials.expires_at = now + EXPIRY_SKEW_SECONDS + 1;
        assert!(!credentials.needs_refresh(now));
        credentials.expires_at = now + EXPIRY_SKEW_SECONDS;
        assert!(credentials.needs_refresh(now));
        credentials.expires_at = now - 1;
        assert!(credentials.needs_refresh(now));
    }

    /// --url picking: explicit wins, single entry is implied, none is an
    /// auth error, several without --url is a usage failure.
    #[test]
    fn deployment_resolution() {
        let mut store = CredentialStore::default();
        assert_eq!(
            resolve_deployment(&store, None)
                .expect_err("empty")
                .exit_code(),
            EXIT_AUTH
        );

        store
            .deployments
            .insert("http://one.kairos.test".into(), entry(None));
        assert_eq!(
            resolve_deployment(&store, None).expect("single"),
            "http://one.kairos.test"
        );
        assert_eq!(
            resolve_deployment(&store, Some("http://two.kairos.test/")).expect("explicit"),
            "http://two.kairos.test"
        );

        store
            .deployments
            .insert("http://two.kairos.test".into(), entry(None));
        let err = resolve_deployment(&store, None).expect_err("ambiguous");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("--url"), "{err}");

        // entry_for on a missing deployment: actionable auth error.
        let err = entry_for(&store, "http://three.kairos.test").expect_err("missing");
        assert_eq!(err.exit_code(), EXIT_AUTH);
        assert!(err.to_string().contains("kairos login --url"), "{err}");
    }
}
