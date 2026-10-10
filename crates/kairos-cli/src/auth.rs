//! Where the bearer of a command comes from: a key in the environment, or
//! the credential cache of `kairos login`. One resolver for the commands
//! that talk to the API ([`crate::context::client`]) and for the base index
//! of `kairos index update`.
//!
//! - **The key.** `KAIROS_KEY`, else `KAIROS_MCP_KEY`: an agent key of a
//!   person (it acts as that person) or an API key of a service account.
//!   With a key, the CLI does not read the credential cache, and the key
//!   wins over a login. This is the route on a deployment whose issuer
//!   gives the CLI no device grant, for example Google.
//! - **The deployment.** `--url`, else `KAIROS_URL`, else the only
//!   deployment in the credential cache.

use std::fmt;
use std::sync::OnceLock;

use kairos_client::KairosClient;
use kairos_client::types_auth::REDACTED;

use crate::credentials::{self, CredentialStore};
use crate::error::CliError;

/// The variables that can hold the key, in order.
pub const KEY_VARS: [&str; 2] = ["KAIROS_KEY", "KAIROS_MCP_KEY"];
/// The variable that can hold the deployment URL.
pub const URL_VAR: &str = "KAIROS_URL";

/// The variable of the key that this process sends, when it sends one. The
/// message of a 401 names it in place of `kairos login`.
static KEY_IN_USE: OnceLock<&'static str> = OnceLock::new();

/// The variable of the key that this process sends, if any.
pub fn key_in_use() -> Option<&'static str> {
    KEY_IN_USE.get().copied()
}

/// The values of the environment that choose the deployment and the bearer.
#[derive(Clone, Default)]
pub struct Env {
    pub url: Option<String>,
    /// The name of the variable, and the key.
    pub key: Option<(&'static str, String)>,
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|(var, _)| (var, REDACTED)))
            .finish()
    }
}

impl Env {
    /// The values of the environment of this process.
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The values from `lookup`. An empty or blank value counts as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let get = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
        Self {
            url: get(URL_VAR),
            key: KEY_VARS
                .iter()
                .find_map(|var| get(var).map(|key| (*var, key.trim().to_string()))),
        }
    }
}

/// Where the bearer of a command comes from.
pub enum Source {
    /// A key from the environment.
    Key {
        deployment: String,
        var: &'static str,
        key: String,
    },
    /// The credential cache of `kairos login`.
    Cache {
        deployment: String,
        store: CredentialStore,
    },
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Key {
                deployment, var, ..
            } => f
                .debug_struct("Key")
                .field("deployment", deployment)
                .field("var", var)
                .field("key", &REDACTED)
                .finish(),
            Source::Cache { deployment, .. } => f
                .debug_struct("Cache")
                .field("deployment", deployment)
                .finish_non_exhaustive(),
        }
    }
}

/// Choose the deployment and the bearer. `url` is the `--url` of the
/// command; it wins over `KAIROS_URL`. `load` reads the credential cache.
/// With a key and a URL, the CLI does not read the cache.
pub fn resolve(
    url: Option<&str>,
    env: &Env,
    load: impl FnOnce() -> Result<CredentialStore, CliError>,
) -> Result<Source, CliError> {
    let url = url.map(str::to_string).or_else(|| env.url.clone());
    match &env.key {
        Some((var, key)) => {
            let deployment = match url {
                Some(url) => credentials::normalize_url(&url),
                None => load()
                    .and_then(|store| credentials::resolve_deployment(&store, None))
                    .map_err(|_| {
                        CliError::Auth(format!(
                            "{var} is set, but the CLI does not know the deployment.\n\
                             Set {URL_VAR} or use --url."
                        ))
                    })?,
            };
            Ok(Source::Key {
                deployment,
                var,
                key: key.clone(),
            })
        }
        None => {
            let store = load()?;
            let deployment = credentials::resolve_deployment(&store, url.as_deref())?;
            Ok(Source::Cache { deployment, store })
        }
    }
}

/// A client that sends the key from `var` as its bearer.
pub fn key_client(deployment: &str, var: &'static str, key: String) -> KairosClient {
    let _ = KEY_IN_USE.set(var);
    KairosClient::with_static_token(deployment, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::DeploymentCredentials;
    use crate::error::EXIT_AUTH;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> Env {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Env::from_lookup(|name| map.get(name).cloned())
    }

    fn store(urls: &[&str]) -> CredentialStore {
        let mut store = CredentialStore::default();
        for url in urls {
            store.deployments.insert(
                url.to_string(),
                DeploymentCredentials {
                    access_token: "cached".into(),
                    refresh_token: None,
                    expires_at: u64::MAX,
                    issuer: String::new(),
                    client_id: String::new(),
                    tenant: None,
                    api_bearer: Default::default(),
                    kind: Default::default(),
                    email: None,
                },
            );
        }
        store
    }

    fn no_cache() -> Result<CredentialStore, CliError> {
        panic!("the CLI must not read the cache here")
    }

    #[test]
    fn kairos_key_wins_over_kairos_mcp_key_and_blank_is_unset() {
        let both = env(&[("KAIROS_KEY", "a"), ("KAIROS_MCP_KEY", "b")]);
        assert_eq!(
            both.key.as_ref().map(|(v, k)| (*v, k.as_str())),
            Some(("KAIROS_KEY", "a"))
        );
        let blank = env(&[("KAIROS_KEY", "  "), ("KAIROS_MCP_KEY", "b")]);
        assert_eq!(blank.key.as_ref().map(|(v, _)| *v), Some("KAIROS_MCP_KEY"));
        assert!(env(&[]).key.is_none());
    }

    #[test]
    fn a_key_with_a_url_does_not_read_the_cache() {
        let env = env(&[
            ("KAIROS_KEY", "kairos_sk_x"),
            ("KAIROS_URL", "https://env/"),
        ]);
        match resolve(None, &env, no_cache).unwrap() {
            Source::Key {
                deployment,
                var,
                key,
            } => {
                assert_eq!(deployment, "https://env");
                assert_eq!(var, "KAIROS_KEY");
                assert_eq!(key, "kairos_sk_x");
            }
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn the_url_flag_wins_over_kairos_url() {
        let env = env(&[("KAIROS_MCP_KEY", "k"), ("KAIROS_URL", "https://env")]);
        let Source::Key { deployment, .. } =
            resolve(Some("https://flag/"), &env, no_cache).unwrap()
        else {
            panic!("expected a key");
        };
        assert_eq!(deployment, "https://flag");
    }

    #[test]
    fn a_key_with_no_url_uses_the_only_cached_deployment() {
        let env = env(&[("KAIROS_KEY", "k")]);
        let Source::Key { deployment, .. } =
            resolve(None, &env, || Ok(store(&["https://cached"]))).unwrap()
        else {
            panic!("expected a key");
        };
        assert_eq!(deployment, "https://cached");
    }

    #[test]
    fn a_key_with_no_url_and_no_single_deployment_is_an_auth_error() {
        let env = env(&[("KAIROS_KEY", "k")]);
        for cached in [vec![], vec!["https://a", "https://b"]] {
            let err = resolve(None, &env, || Ok(store(&cached))).unwrap_err();
            assert_eq!(err.exit_code(), EXIT_AUTH);
            let message = err.to_string();
            assert!(message.contains("KAIROS_KEY is set"), "{message}");
            assert!(message.contains("KAIROS_URL"), "{message}");
        }
    }

    #[test]
    fn no_key_uses_the_cache_and_kairos_url_selects_the_deployment() {
        let env = env(&[("KAIROS_URL", "https://b/")]);
        match resolve(None, &env, || Ok(store(&["https://a", "https://b"]))).unwrap() {
            Source::Cache { deployment, .. } => assert_eq!(deployment, "https://b"),
            other => panic!("expected the cache, got {other:?}"),
        }
    }

    #[test]
    fn no_key_and_no_login_says_to_log_in() {
        let err = resolve(None, &Env::default(), || Ok(store(&[]))).unwrap_err();
        assert_eq!(err.exit_code(), EXIT_AUTH);
        assert!(err.to_string().contains("kairos login"));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let env = env(&[("KAIROS_KEY", "kairos_sk_secret")]);
        assert!(!format!("{env:?}").contains("kairos_sk_secret"));
        let source = resolve(Some("https://x"), &env, no_cache).unwrap();
        assert!(!format!("{source:?}").contains("kairos_sk_secret"));
    }
}
