mod task_output_forwarder;
pub(crate) mod task_run_telemetry;
pub(crate) mod task_span_tracker;

pub use task_output_forwarder::TaskOutputForwarder;
pub use task_run_telemetry::TaskRunTelemetry;
pub use task_span_tracker::TaskSpanTracker;

use crate::config::Settings;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Check if OpenTelemetry trace export is enabled.
///
/// Requires `otel.enabled = true` (or `MISE_OTEL_ENABLED=1`) AND a traces
/// endpoint configured via `OTEL_EXPORTER_OTLP_ENDPOINT` or the
/// signal-specific `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`. This prevents
/// mise from emitting spans in environments that set those vars for
/// other tools.
pub fn traces_enabled() -> bool {
    if !Settings::get().otel.enabled {
        return false;
    }
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
        || std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_ok()
}

/// Check if OpenTelemetry log export is enabled.
///
/// Log export is a separate opt-in from trace export because task
/// stdout/stderr is shipped to the collector — a different trust boundary
/// than spans. Requires `otel.logs = true` (or `MISE_OTEL_LOGS=1`) AND a
/// logs endpoint configured via `OTEL_EXPORTER_OTLP_ENDPOINT` or the
/// signal-specific `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`.
pub fn logs_enabled() -> bool {
    if !Settings::get().otel.logs {
        return false;
    }
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
        || std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").is_ok()
}

// ── Resource ────────────────────────────────────────────────────────

/// Build an `opentelemetry_sdk::Resource` using the SDK's built-in detectors.
///
/// `Resource::builder()` automatically reads `OTEL_SERVICE_NAME` and
/// `OTEL_RESOURCE_ATTRIBUTES` via `EnvResourceDetector`.
pub fn build_resource() -> Resource {
    let mut builder = Resource::builder();
    // Only set a default service name if the user hasn't provided one,
    // since with_service_name would override the env var.
    if std::env::var("OTEL_SERVICE_NAME").is_err() {
        builder = builder.with_service_name("mise");
    }
    builder.build()
}

// ── Provider builders ───────────────────────────────────────────────

/// Build a `SdkTracerProvider` with the OTLP/HTTP JSON exporter.
///
/// The OTLP crate natively reads `OTEL_EXPORTER_OTLP_ENDPOINT`,
/// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`,
/// `OTEL_EXPORTER_OTLP_TRACES_HEADERS`, etc.
pub fn build_tracer_provider(resource: Resource) -> Option<SdkTracerProvider> {
    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
    {
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

/// Build a `SdkLoggerProvider` with the OTLP/HTTP JSON exporter.
pub fn build_logger_provider(resource: Resource) -> Option<SdkLoggerProvider> {
    let exporter = match opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .build()
    {
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
