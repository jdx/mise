mod log_claim;
mod task_output_forwarder;
pub(crate) mod task_run_telemetry;

pub(crate) use log_claim::{LOG_CLAIM_ENV, LogClaim, LogClaimWatcher};
pub(crate) use task_output_forwarder::{ExportStreams, TaskOutputForwarder};
pub(crate) use task_run_telemetry::TaskRunTelemetry;

use crate::config::Settings;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::time::Duration;

/// How long one export request may take when `OTEL_EXPORTER_OTLP_TIMEOUT` is
/// unset. The final flush runs after the tasks finish, so the SDK's 10s
/// default would hold up `mise run` that long whenever the collector stalls.
const DEFAULT_EXPORT_TIMEOUT: Duration = Duration::from_secs(3);

/// Check if OpenTelemetry trace export is enabled.
///
/// Requires `otel.enabled = true` (or `MISE_OTEL_ENABLED=1`) AND a traces
/// endpoint configured via `OTEL_EXPORTER_OTLP_ENDPOINT` or the
/// signal-specific `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`. This prevents
/// mise from emitting spans in environments that set those vars for
/// other tools. Offline mode turns export off.
pub(crate) fn traces_enabled() -> bool {
    let settings = Settings::get();
    settings.otel.enabled
        && !settings.offline()
        && (env_is_set("OTEL_EXPORTER_OTLP_ENDPOINT")
            || env_is_set("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"))
}

/// An empty endpoint counts as unset: the exporter would otherwise fall back
/// to `http://localhost:4318`, which the user never asked for.
fn env_is_set(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.trim().is_empty())
}

/// The OTLP/HTTP encoding: JSON when `OTEL_EXPORTER_OTLP_<SIGNAL>_PROTOCOL` or
/// `OTEL_EXPORTER_OTLP_PROTOCOL` asks for `http/json`, protobuf otherwise.
/// Set explicitly because the exporter's own fallback, with the `http-json`
/// feature enabled, is JSON, and the spec's default is protobuf.
fn http_protocol(signal_var: &str) -> opentelemetry_otlp::Protocol {
    let requested = std::env::var(signal_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok());
    match requested.as_deref().map(str::trim) {
        Some("http/json") => opentelemetry_otlp::Protocol::HttpJson,
        _ => opentelemetry_otlp::Protocol::HttpBinary,
    }
}

/// The per-request export timeout, unless the user configured one for this
/// signal (`OTEL_EXPORTER_OTLP_<SIGNAL>_TIMEOUT`) or for all of them.
fn export_timeout(signal_var: &str) -> Option<Duration> {
    (!env_is_set(signal_var) && !env_is_set("OTEL_EXPORTER_OTLP_TIMEOUT"))
        .then_some(DEFAULT_EXPORT_TIMEOUT)
}

/// Check if OpenTelemetry log export is enabled.
///
/// Log export is a separate opt-in from trace export because task
/// stdout/stderr is shipped to the collector — a different trust boundary
/// than spans. Requires `otel.logs = true` (or `MISE_OTEL_LOGS=1`) AND a
/// logs endpoint configured via `OTEL_EXPORTER_OTLP_ENDPOINT` or the
/// signal-specific `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`.
pub(crate) fn logs_enabled() -> bool {
    let settings = Settings::get();
    settings.otel.logs
        && !settings.offline()
        && (env_is_set("OTEL_EXPORTER_OTLP_ENDPOINT")
            || env_is_set("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT"))
}

// ── Resource ────────────────────────────────────────────────────────

/// Build an `opentelemetry_sdk::Resource` using the SDK's built-in detectors.
///
/// `Resource::builder()` automatically reads `OTEL_SERVICE_NAME` and
/// `OTEL_RESOURCE_ATTRIBUTES` via `EnvResourceDetector`.
pub(crate) fn build_resource() -> Resource {
    // Default the service name to `mise` only when neither env var set one;
    // `with_service_name` would override both.
    let resource = Resource::builder().build();
    let service_name = opentelemetry::Key::from_static_str("service.name");
    let has_service_name = resource
        .get(&service_name)
        .is_some_and(|name| !name.as_str().starts_with("unknown_service"));
    if has_service_name {
        resource
    } else {
        Resource::builder().with_service_name("mise").build()
    }
}

// ── Provider builders ───────────────────────────────────────────────

/// Build a `SdkTracerProvider` with the OTLP/HTTP protobuf exporter.
///
/// The OTLP crate natively reads `OTEL_EXPORTER_OTLP_ENDPOINT`,
/// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`,
/// `OTEL_EXPORTER_OTLP_TRACES_HEADERS`, etc.
pub(crate) fn build_tracer_provider(resource: Resource) -> Option<SdkTracerProvider> {
    let mut builder = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(http_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"));
    if let Some(timeout) = export_timeout("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT") {
        builder = builder.with_timeout(timeout);
    }
    let exporter = match builder.build() {
        Ok(e) => e,
        Err(err) => {
            debug!("otel: failed to build span exporter: {err}");
            return None;
        }
    };

    Some(
        SdkTracerProvider::builder()
            .with_span_processor(
                opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(exporter, runtime::Tokio)
                    .build(),
            )
            .with_resource(resource)
            .build(),
    )
}

/// Build a `SdkLoggerProvider` with the OTLP/HTTP protobuf exporter.
pub(crate) fn build_logger_provider(resource: Resource) -> Option<SdkLoggerProvider> {
    let mut builder = opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .with_protocol(http_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"));
    if let Some(timeout) = export_timeout("OTEL_EXPORTER_OTLP_LOGS_TIMEOUT") {
        builder = builder.with_timeout(timeout);
    }
    let exporter = match builder.build() {
        Ok(e) => e,
        Err(err) => {
            debug!("otel: failed to build log exporter: {err}");
            return None;
        }
    };

    Some(
        SdkLoggerProvider::builder()
            .with_log_processor(
                opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor::builder(exporter, runtime::Tokio)
                    .build(),
            )
            .with_resource(resource)
            .build(),
    )
}
