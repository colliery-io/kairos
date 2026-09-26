//! `kairos-server` library surface (KAIROS-T-0017): everything the binary
//! does apart from CLI parsing, exposed so integration tests can build the
//! exact production router in-process.
//!
//! - [`config`] — env-var configuration (KAIROS-A-0013, fail-fast)
//! - [`error`] — the S-0005 error envelope
//! - [`metrics`] — Prometheus `/metrics` + `/readyz` (KAIROS-A-0013, T-0049)
//! - [`middleware`] — OIDC auth (A-0010) + tenant resolution (A-0005 §2)
//! - [`api`] — the S-0005 entity endpoint families (KAIROS-T-0018)
//! - [`blocking`] — the sync pool bridging handlers to kairos-db services
//! - [`app`] — state, router construction, and the serve loop
//! - [`ws`] — the `/ws/events` channel (A-0005 §5, KAIROS-T-0022)
//! - [`mcp`] — the `/mcp` MCP endpoint (A-0011 / S-0006, KAIROS-T-0026)
//! - [`web`] — GUI serving + SPA auth support (A-0015, KAIROS-T-0039)

pub mod api;
pub mod app;
pub mod blocking;
pub mod config;
pub mod embedding;
pub mod error;
pub mod forge;
pub mod local_auth;
pub mod login;
pub mod metrics;
pub mod middleware;
pub mod rate_limit;
pub mod ws;

pub mod mcp;

pub mod scim;

pub mod service_accounts;

pub mod web;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{AppConfig, LogFormat};

/// Held by `main` for the process's life; flushes pending spans on drop.
///
/// Without this, a process that exits promptly after an interesting request
/// loses exactly the spans someone was trying to look at — the exporter batches,
/// and an un-flushed batch dies with the process. The `Drop` impl is the whole
/// reason this type exists rather than the function returning nothing.
pub struct TracingGuard {
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take()
            && let Err(error) = provider.shutdown()
        {
            // Nothing useful to do about it, and panicking while shutting down
            // would replace a lost trace with a lost exit code.
            eprintln!("otel: shutting down the tracer provider failed: {error}");
        }
    }
}

/// Initialize `tracing-subscriber` per KAIROS-A-0013: `KAIROS_LOG_LEVEL` filter
/// directive, `KAIROS_LOG_FORMAT` json (default) or pretty.
///
/// KAIROS-T-0196: when `KAIROS_OTEL_ENDPOINT` is set, an OTLP layer is added
/// alongside the log layer — the two are independent, so turning tracing on never
/// changes what gets logged. When it is unset, no exporter is constructed and the
/// OpenTelemetry code is inert.
///
/// Keep the returned guard alive for as long as you want spans exported.
#[must_use = "dropping the guard immediately flushes and shuts down tracing"]
pub fn init_tracing(config: &AppConfig) -> TracingGuard {
    let filter = EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    // Boxed so both formats are one type and the registry below can be built
    // once instead of duplicated per arm.
    let log_layer: Box<dyn tracing_subscriber::Layer<_> + Send + Sync> = match config.log_format {
        LogFormat::Json => Box::new(tracing_subscriber::fmt::layer().json()),
        LogFormat::Pretty => Box::new(tracing_subscriber::fmt::layer().pretty()),
    };

    let provider = config.otel_endpoint.as_deref().and_then(|endpoint| {
        match tracer_provider(endpoint, config.otel_sample_ratio) {
            Ok(provider) => Some(provider),
            Err(error) => {
                // Deliberately NOT fatal. A collector that is unreachable, or an
                // endpoint with a typo, must not stop Kairos serving: telemetry
                // is how you observe the product, not part of it. Reported loudly
                // instead, because the failure mode of silence here is an
                // operator staring at an empty collector.
                eprintln!(
                    "otel: tracing is DISABLED — could not build an exporter for \
                     {endpoint:?}: {error}"
                );
                None
            }
        }
    });

    let otel_layer = provider.as_ref().map(|provider| {
        use opentelemetry::trace::TracerProvider as _;
        tracing_opentelemetry::layer().with_tracer(provider.tracer("kairos-server"))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(log_layer)
        .with(otel_layer)
        .init();

    if let Some(endpoint) = config.otel_endpoint.as_deref()
        && provider.is_some()
    {
        tracing::info!(
            endpoint,
            sample_ratio = config.otel_sample_ratio,
            "otel: exporting traces over OTLP/HTTP"
        );
    }

    TracingGuard { provider }
}

/// Build the OTLP/HTTP exporter and a batching provider around it.
fn tracer_provider(
    endpoint: &str,
    sample_ratio: f64,
) -> Result<opentelemetry_sdk::trace::SdkTracerProvider, opentelemetry_otlp::ExporterBuildError> {
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()?;

    Ok(opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        // TraceIdRatioBased, wrapped in ParentBased so a sampling decision made
        // upstream is respected: if a caller already decided to record a trace,
        // dropping our half of it would produce a trace with a hole in it, which
        // is worse than either keeping or dropping the whole thing.
        .with_sampler(opentelemetry_sdk::trace::Sampler::ParentBased(Box::new(
            opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(sample_ratio),
        )))
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("kairos")
                .build(),
        )
        .build())
}
