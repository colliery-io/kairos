//! Choosing a provider (KAIROS-T-0189).
//!
//! Configuration is read from the environment, like the rest of the server's
//! configuration (KAIROS-A-0013), and the default is **local with no
//! configuration at all** — rule 1's "local by default" only means something if
//! it is also local by silence.
//!
//! | variable | effect |
//! |---|---|
//! | *(none)* | local provider, model read from the image's cache directory |
//! | `KAIROS_EMBED_PROVIDER` | `local` \| `remote` \| `deterministic` \| `none` |
//! | `KAIROS_EMBED_CACHE` | where the local model lives |
//! | `KAIROS_EMBED_ALLOW_DOWNLOAD` | let a cache miss fetch the model |
//! | `KAIROS_EMBED_URL` | OpenAI-compatible base URL; implies `remote` |
//! | `KAIROS_EMBED_MODEL` | model name for `remote` |
//! | `KAIROS_EMBED_API_KEY` | bearer token for `remote` |
//! | `KAIROS_EMBED_TIMEOUT_SECS` | per-request timeout for `remote` |
//!
//! `KAIROS_EMBED_URL` implying `remote` is deliberate: an operator who has
//! configured an endpoint has said what they want, and making them also set
//! `KAIROS_EMBED_PROVIDER=remote` would be a second chance to get it wrong.
//!
//! `none` exists because retrieval degrades to lexical by design (A-0021 rule 7).
//! A deployment that wants no embeddings at all — no model in the image, no
//! outbound calls — says so, and gets search that still works.

use std::sync::Arc;
use std::time::Duration;

use crate::{EmbedError, EmbeddingProvider};

/// Which provider a deployment wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    /// In-process ONNX (the default).
    Local,
    /// OpenAI-compatible HTTP.
    Remote,
    /// Hash-derived, for tests.
    Deterministic,
    /// No embeddings; retrieval stays lexical.
    None,
}

/// Everything needed to build a provider.
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    /// Which provider.
    pub kind: ProviderKind,
    /// Local provider settings, used when `kind` is [`ProviderKind::Local`].
    #[cfg(feature = "local")]
    pub local: crate::local::LocalConfig,
    /// Remote provider settings, used when `kind` is [`ProviderKind::Remote`].
    #[cfg(feature = "remote")]
    pub remote: Option<crate::remote::RemoteConfig>,
}

/// A configuration that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `KAIROS_EMBED_PROVIDER` named something unknown.
    #[error("KAIROS_EMBED_PROVIDER must be one of local, remote, deterministic, none (got {0:?})")]
    UnknownProvider(String),
    /// `remote` was asked for without a URL.
    #[error("KAIROS_EMBED_PROVIDER=remote needs KAIROS_EMBED_URL")]
    RemoteWithoutUrl,
    /// `remote` was asked for without a model name. There is no sensible
    /// default: every endpoint names its models differently.
    #[error("KAIROS_EMBED_PROVIDER=remote needs KAIROS_EMBED_MODEL")]
    RemoteWithoutModel,
    /// A numeric variable would not parse.
    #[error("{name} must be a positive whole number of seconds (got {value:?})")]
    BadNumber {
        /// The variable.
        name: &'static str,
        /// What it was set to.
        value: String,
    },
    /// The build asked for a provider whose feature is not compiled in.
    #[error("this binary was built without the {0:?} embedding provider")]
    ProviderNotCompiledIn(&'static str),
}

impl EmbedConfig {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Read from an arbitrary lookup, so this is testable without touching the
    /// process environment — which is global, and so makes tests order-dependent
    /// when they run in one process.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let url = get("KAIROS_EMBED_URL").filter(|s| !s.trim().is_empty());
        let kind = match get("KAIROS_EMBED_PROVIDER")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("local") => ProviderKind::Local,
            Some("remote") => ProviderKind::Remote,
            Some("deterministic") => ProviderKind::Deterministic,
            Some("none" | "off" | "disabled") => ProviderKind::None,
            Some(other) => return Err(ConfigError::UnknownProvider(other.to_string())),
            // A configured URL says what the operator wants without their having
            // to say it twice.
            None if url.is_some() => ProviderKind::Remote,
            None => ProviderKind::Local,
        };

        #[cfg(feature = "remote")]
        let remote = if kind == ProviderKind::Remote {
            let base_url = url.ok_or(ConfigError::RemoteWithoutUrl)?;
            let model = get("KAIROS_EMBED_MODEL")
                .filter(|s| !s.trim().is_empty())
                .ok_or(ConfigError::RemoteWithoutModel)?;
            let timeout = match get("KAIROS_EMBED_TIMEOUT_SECS") {
                Some(v) => {
                    Duration::from_secs(v.trim().parse().map_err(|_| ConfigError::BadNumber {
                        name: "KAIROS_EMBED_TIMEOUT_SECS",
                        value: v.clone(),
                    })?)
                }
                None => Duration::from_secs(30),
            };
            Some(crate::remote::RemoteConfig {
                base_url,
                model,
                api_key: get("KAIROS_EMBED_API_KEY").filter(|s| !s.trim().is_empty()),
                timeout,
            })
        } else {
            None
        };

        #[cfg(feature = "local")]
        let local = {
            let mut local = crate::local::LocalConfig::default();
            if let Some(dir) = get("KAIROS_EMBED_CACHE").filter(|s| !s.trim().is_empty()) {
                local.cache_dir = dir.into();
            }
            local.allow_download = matches!(
                get("KAIROS_EMBED_ALLOW_DOWNLOAD")
                    .map(|s| s.trim().to_lowercase())
                    .as_deref(),
                Some("1" | "true" | "yes")
            );
            local
        };

        Ok(Self {
            kind,
            #[cfg(feature = "local")]
            local,
            #[cfg(feature = "remote")]
            remote,
        })
    }

    /// Build the provider, or `None` for [`ProviderKind::None`].
    ///
    /// Returns `Ok(None)` rather than an error when embeddings are switched off:
    /// a caller that gets `None` degrades to lexical, which is rule 7's promise,
    /// and an error would make a deliberate choice look like a failure.
    pub fn build(&self) -> Result<Option<Arc<dyn EmbeddingProvider>>, EmbedError> {
        match self.kind {
            ProviderKind::None => Ok(None),
            ProviderKind::Deterministic => {
                Ok(Some(Arc::new(crate::DeterministicProvider::default())))
            }
            #[cfg(feature = "local")]
            ProviderKind::Local => Ok(Some(Arc::new(crate::local::LocalProvider::new(
                &self.local,
            )?))),
            #[cfg(not(feature = "local"))]
            ProviderKind::Local => Err(EmbedError::Unavailable(
                ConfigError::ProviderNotCompiledIn("local").to_string(),
            )),
            #[cfg(feature = "remote")]
            ProviderKind::Remote => {
                let config = self.remote.clone().ok_or_else(|| {
                    EmbedError::Unavailable(ConfigError::RemoteWithoutUrl.to_string())
                })?;
                Ok(Some(Arc::new(crate::remote::RemoteProvider::new(config)?)))
            }
            #[cfg(not(feature = "remote"))]
            ProviderKind::Remote => Err(EmbedError::Unavailable(
                ConfigError::ProviderNotCompiledIn("remote").to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    /// Rule 1 is only satisfied if the default needs no configuration.
    #[test]
    fn the_default_is_local_with_nothing_set() {
        let c = EmbedConfig::from_vars(vars(&[])).unwrap();
        assert_eq!(c.kind, ProviderKind::Local);
        #[cfg(feature = "local")]
        assert!(
            !c.local.allow_download,
            "and it does not reach the network on its own"
        );
    }

    /// Setting a URL is saying what you want; it should not also require saying
    /// which provider.
    #[test]
    fn a_url_alone_selects_remote() {
        let c = EmbedConfig::from_vars(vars(&[
            ("KAIROS_EMBED_URL", "http://ollama:11434/v1"),
            ("KAIROS_EMBED_MODEL", "nomic-embed-text"),
        ]))
        .unwrap();
        assert_eq!(c.kind, ProviderKind::Remote);
        #[cfg(feature = "remote")]
        {
            let r = c.remote.unwrap();
            assert_eq!(r.base_url, "http://ollama:11434/v1");
            assert_eq!(r.model, "nomic-embed-text");
            assert_eq!(r.api_key, None, "a local Ollama needs no key");
        }
    }

    #[test]
    fn remote_without_a_url_or_model_is_refused() {
        let err = EmbedConfig::from_vars(vars(&[("KAIROS_EMBED_PROVIDER", "remote")])).unwrap_err();
        assert!(matches!(err, ConfigError::RemoteWithoutUrl), "{err}");
        let err = EmbedConfig::from_vars(vars(&[
            ("KAIROS_EMBED_PROVIDER", "remote"),
            ("KAIROS_EMBED_URL", "http://x/v1"),
        ]))
        .unwrap_err();
        assert!(matches!(err, ConfigError::RemoteWithoutModel), "{err}");
    }

    #[test]
    fn an_unknown_provider_is_refused_by_name() {
        let err = EmbedConfig::from_vars(vars(&[("KAIROS_EMBED_PROVIDER", "magic")])).unwrap_err();
        assert!(err.to_string().contains("magic"), "{err}");
        assert!(
            err.to_string().contains("deterministic"),
            "lists the options: {err}"
        );
    }

    /// Switching embeddings off is a choice, not a failure.
    #[test]
    fn none_builds_no_provider_and_is_not_an_error() {
        for spelling in ["none", "off", "disabled", "NONE"] {
            let c = EmbedConfig::from_vars(vars(&[("KAIROS_EMBED_PROVIDER", spelling)])).unwrap();
            assert_eq!(c.kind, ProviderKind::None, "{spelling}");
            assert!(c.build().unwrap().is_none(), "{spelling}");
        }
    }

    #[test]
    fn deterministic_builds_without_any_other_configuration() {
        let c =
            EmbedConfig::from_vars(vars(&[("KAIROS_EMBED_PROVIDER", "deterministic")])).unwrap();
        let p = c.build().unwrap().expect("a provider");
        assert_eq!(
            p.model_id().dimension,
            384,
            "matches the local default width"
        );
    }

    #[test]
    fn a_bad_timeout_names_the_variable() {
        let err = EmbedConfig::from_vars(vars(&[
            ("KAIROS_EMBED_URL", "http://x/v1"),
            ("KAIROS_EMBED_MODEL", "m"),
            ("KAIROS_EMBED_TIMEOUT_SECS", "soon"),
        ]))
        .unwrap_err();
        assert!(
            err.to_string().contains("KAIROS_EMBED_TIMEOUT_SECS"),
            "{err}"
        );
    }

    #[test]
    fn blank_values_are_treated_as_unset() {
        // Empty environment variables are a common artefact of templated
        // deployment manifests; treating "" as "configured" would select remote
        // and then fail on a missing model.
        let c = EmbedConfig::from_vars(vars(&[("KAIROS_EMBED_URL", "   ")])).unwrap();
        assert_eq!(c.kind, ProviderKind::Local);
    }

    #[cfg(feature = "local")]
    #[test]
    fn the_cache_directory_and_download_flag_are_read() {
        let c = EmbedConfig::from_vars(vars(&[
            ("KAIROS_EMBED_CACHE", "/models"),
            ("KAIROS_EMBED_ALLOW_DOWNLOAD", "true"),
        ]))
        .unwrap();
        assert_eq!(c.local.cache_dir, std::path::PathBuf::from("/models"));
        assert!(c.local.allow_download);
    }
}
