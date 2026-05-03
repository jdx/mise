//! Task-aware OpenTelemetry integration.
//!
//! This module is the bridge between the generic OTLP primitives
//! (`TraceContext`, `OtelLogCollector`) and the mise task model. It owns
//! the per-`mise run` telemetry lifecycle so that `cli::run` and
//! `task_executor` don't have to reach into OTEL internals.

use crate::otel::trace_context::StartedSpan;
use crate::otel::{OtelLogCollector, TraceContext, is_enabled};
use crate::task::Task;
use eyre::Result;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{SpanContext, Status, TraceContextExt};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// All OpenTelemetry state attached to a single `mise run` invocation.
///
/// - `trace` is cheaply clonable and can be passed to per-task code.
/// - `log_collector` is cheaply clonable and is installed on the
///   task executor so it can forward stdout/stderr lines.
///
/// Implements `Drop` to flush logs and emit spans, so traces are
/// captured even when the future is cancelled (e.g. by `--timeout`).
/// Defaults to `has_failures = true`; call `set_succeeded()` on
/// the happy path to mark the root span as OK.
pub struct RunTrace {
    pub trace: TraceContext,
    pub log_collector: OtelLogCollector,
    has_failures: std::sync::atomic::AtomicBool,
}

impl RunTrace {
    /// Initialize a `RunTrace` if otel is configured.
    ///
    /// When mise is invoked from another mise run (or any OTEL-aware
    /// parent), the `TRACEPARENT` env var carries W3C Traceparent so
    /// the nested run joins the same distributed trace.
    pub fn init_if_enabled(requested_task_names: &[String]) -> Option<Self> {
        if !is_enabled() {
            return None;
        }
        let suffix = if requested_task_names.is_empty() {
            String::new()
        } else {
            format!(" {}", requested_task_names.join(" "))
        };
        let root_span_name = format!("mise run{suffix}");

        let resource = crate::otel::build_resource();
        let tracer_provider = crate::otel::build_tracer_provider(resource.clone())?;
        let logger_provider = crate::otel::build_logger_provider(resource);

        let trace = match parse_otel_context() {
            Some(parent_span_context) => TraceContext::from_parent_context(
                &root_span_name,
                parent_span_context,
                tracer_provider,
            ),
            _ => TraceContext::new(&root_span_name, tracer_provider),
        };
        let log_collector = match logger_provider {
            Some(lp) => OtelLogCollector::new(lp),
            None => {
                // If no logs URL is configured, create a no-op provider
                OtelLogCollector::new(opentelemetry_sdk::logs::SdkLoggerProvider::builder().build())
            }
        };
        Some(Self {
            trace,
            log_collector,
            has_failures: std::sync::atomic::AtomicBool::new(true),
        })
    }

    /// Mark the run as succeeded. Must be called explicitly on the
    /// happy path — the default is `has_failures = true` so that
    /// cancelled futures (e.g. timeout) produce an errored root span.
    pub fn set_succeeded(&self) {
        self.has_failures
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for RunTrace {
    fn drop(&mut self) {
        let has_failures = *self.has_failures.get_mut();
        // Shutdown the log collector first to flush all pending log batches.
        self.log_collector.shutdown();
        // Then finish the trace which emits root/group spans and shuts down
        // the tracer provider.
        self.trace.finish(has_failures);
    }
}

impl TraceContext {
    /// Reserve span IDs for a task. The full span (attributes, timing,
    /// status) is emitted once from `end_task`.
    pub fn start_task(&self, task: &Task, project_root: Option<&PathBuf>) -> StartedSpan {
        let parent_span_id = self.parent_span_for_task(task.config_root.as_ref(), project_root);
        self.reserve_task_span(parent_span_id)
    }

    /// Emit the completed span for a task, derived from the run result.
    pub fn end_task(
        &self,
        started: StartedSpan,
        task: &Task,
        end_time: SystemTime,
        result: &Result<bool>,
    ) {
        let display_name = task_span_name(task);
        let status = match result {
            Ok(true) => Status::Ok,
            Ok(false) => Status::Unset, // skipped
            Err(err) => Status::Error {
                description: Cow::Owned(err.to_string()),
            },
        };
        let mut attrs = task_attributes(task, &display_name);
        if let Ok(false) = result {
            attrs.push(("mise.task.skipped".to_string(), "true".to_string()));
        }
        self.end_task_span(started, &display_name, end_time, status, attrs);
    }
}

/// Human-readable span name for a task (display name + args).
pub fn task_span_name(task: &Task) -> String {
    let base = if task.display_name.is_empty() {
        task.name.clone()
    } else {
        task.display_name.clone()
    };
    if task.args.is_empty() {
        base
    } else {
        format!("{base} {}", task.args.join(" "))
    }
}

/// Standard OpenTelemetry attributes attached to every task span.
fn task_attributes(task: &Task, display_name: &str) -> Vec<(String, String)> {
    let mut attrs = vec![
        ("mise.task.name".to_string(), task.name.clone()),
        (
            "mise.task.display_name".to_string(),
            display_name.to_string(),
        ),
        (
            "mise.task.source".to_string(),
            task.config_source.display().to_string(),
        ),
    ];
    // Args are exported verbatim, no different from what's already visible in terminal output and process listings.
    if !task.args.is_empty() {
        attrs.push(("mise.task.args".to_string(), task.args.join(" ")));
    }
    if let Some(ref cr) = task.config_root {
        attrs.push((
            "mise.task.config_root".to_string(),
            cr.display().to_string(),
        ));
    }
    attrs
}

/// Inject a span context into task env vars using the standard
/// W3C Trace Context propagator and env-carrier variable names.
pub fn inject_otel_context(env: &mut BTreeMap<String, String>, started: &StartedSpan) {
    let propagator = TraceContextPropagator::new();
    let cx = opentelemetry::Context::new().with_remote_span_context(started.span_context());
    let mut carrier = HashMap::new();
    propagator.inject_context(&cx, &mut carrier);
    if let Some(traceparent) = carrier.remove("traceparent") {
        env.insert("TRACEPARENT".into(), traceparent);
    }
    if let Some(tracestate) = carrier.remove("tracestate") {
        env.insert("TRACESTATE".into(), tracestate);
    }
}

/// Extract parent trace context from the `TRACEPARENT` env var.
/// Uses the SDK's `TraceContextPropagator` for parsing.
fn parse_otel_context() -> Option<SpanContext> {
    let mut carrier = HashMap::new();
    carrier.insert(
        "traceparent".to_string(),
        std::env::var("TRACEPARENT").ok()?,
    );
    if let Ok(tracestate) = std::env::var("TRACESTATE") {
        carrier.insert("tracestate".to_string(), tracestate);
    }
    extract_span_context(&carrier)
}

#[cfg(test)]
fn parse_otel_context_from_str(traceparent: &str) -> Option<SpanContext> {
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());
    extract_span_context(&carrier)
}

#[cfg(test)]
fn parse_otel_context_from_parts(traceparent: &str, tracestate: &str) -> Option<SpanContext> {
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());
    carrier.insert("tracestate".to_string(), tracestate.to_string());
    extract_span_context(&carrier)
}

fn extract_span_context(carrier: &HashMap<String, String>) -> Option<SpanContext> {
    let propagator = TraceContextPropagator::new();
    let cx = propagator.extract(carrier);
    let sc = cx.span().span_context().clone();
    if sc.is_valid() { Some(sc) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otel::OtelLogCollector;
    use crate::task::Task;
    use opentelemetry::trace::{SpanId, Status, TraceFlags, TraceId, TraceState};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{SimpleSpanProcessor, SpanData, SpanExporter};
    use std::future;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RetainingSpanExporter {
        spans: Arc<Mutex<Vec<SpanData>>>,
    }

    impl std::fmt::Debug for RetainingSpanExporter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RetainingSpanExporter").finish()
        }
    }

    impl SpanExporter for RetainingSpanExporter {
        fn export(
            &self,
            batch: Vec<SpanData>,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            self.spans.lock().unwrap().extend(batch);
            future::ready(Ok(()))
        }
    }

    impl RetainingSpanExporter {
        fn finished_spans(&self) -> Vec<SpanData> {
            self.spans.lock().unwrap().clone()
        }
    }

    fn test_run_trace() -> (RunTrace, RetainingSpanExporter) {
        let exporter = RetainingSpanExporter::default();
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let trace = TraceContext::new("mise run //ci", provider);
        let log_collector =
            OtelLogCollector::new(opentelemetry_sdk::logs::SdkLoggerProvider::builder().build());
        let rt = RunTrace {
            trace,
            log_collector,
            has_failures: std::sync::atomic::AtomicBool::new(true),
        };
        (rt, exporter)
    }

    fn task_for(name: &str, display: &str, args: &[&str]) -> Task {
        Task {
            name: name.to_string(),
            display_name: display.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            config_source: PathBuf::from("/tmp/mise.toml"),
            ..Default::default()
        }
    }

    #[test]
    fn span_name_uses_display_name_with_args() {
        let task = task_for("build", "Build", &["--release"]);
        assert_eq!(task_span_name(&task), "Build --release");
    }

    #[test]
    fn span_name_falls_back_to_task_name() {
        let task = task_for("build", "", &[]);
        assert_eq!(task_span_name(&task), "build");
    }

    #[test]
    fn attributes_include_args_and_config_root() {
        let mut task = task_for("build", "Build", &["x", "y"]);
        task.config_root = Some(PathBuf::from("/workspace/packages/a"));
        let attrs = task_attributes(&task, "Build x y");
        let find = |k: &str| {
            attrs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(find("mise.task.name"), Some("build"));
        assert_eq!(find("mise.task.display_name"), Some("Build x y"));
        assert_eq!(find("mise.task.args"), Some("x y"));
        assert_eq!(find("mise.task.config_root"), Some("/workspace/packages/a"),);
    }

    #[test]
    fn attributes_omit_args_when_empty() {
        let task = task_for("build", "", &[]);
        let attrs = task_attributes(&task, "build");
        assert!(attrs.iter().all(|(k, _)| k != "mise.task.args"));
        assert!(attrs.iter().all(|(k, _)| k != "mise.task.config_root"));
    }

    #[test]
    fn inject_otel_context_uses_propagator_output() {
        let trace_id = TraceId::from_bytes([
            0x0a, 0xf7, 0x65, 0x19, 0x16, 0xcd, 0x43, 0xdd, 0x84, 0x48, 0xeb, 0x21, 0x1c, 0x80,
            0x31, 0x9c,
        ]);
        let span_id = SpanId::from_bytes([0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31]);
        let started = StartedSpan::for_test_with_context(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            TraceState::default(),
        );

        let mut env = BTreeMap::new();
        inject_otel_context(&mut env, &started);
        assert_eq!(
            env.get("TRACEPARENT").map(String::as_str),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
        );
    }

    #[test]
    fn parse_otel_context_extracts_ids_from_traceparent_env() {
        let trace_id = TraceId::from_bytes([
            0x0a, 0xf7, 0x65, 0x19, 0x16, 0xcd, 0x43, 0xdd, 0x84, 0x48, 0xeb, 0x21, 0x1c, 0x80,
            0x31, 0x9c,
        ]);
        let span_id = SpanId::from_bytes([0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31]);
        let parsed =
            parse_otel_context_from_str("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
                .unwrap();
        assert_eq!(parsed.trace_id(), trace_id);
        assert_eq!(parsed.span_id(), span_id);
    }

    #[test]
    fn parse_otel_context_rejects_invalid_traceparent_env() {
        let parsed = parse_otel_context_from_str("00-short-also_short-01");
        assert_eq!(parsed, None);
    }

    #[test]
    fn parse_otel_context_preserves_upstream_tracestate() {
        let parsed = parse_otel_context_from_parts(
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            "vendor=value",
        )
        .unwrap();
        assert_eq!(parsed.trace_state().header(), "vendor=value");
    }

    #[test]
    fn drop_without_set_succeeded_emits_errored_root_span() {
        let (rt, exporter) = test_run_trace();
        // Simulate timeout/cancellation: drop without calling set_succeeded().
        drop(rt);

        let spans = exporter.finished_spans();
        let root = spans.iter().find(|s| s.name == "mise run //ci").unwrap();
        assert!(
            matches!(root.status, Status::Error { .. }),
            "expected errored root span on cancelled drop, got {:?}",
            root.status
        );
    }

    #[test]
    fn drop_after_set_succeeded_emits_ok_root_span() {
        let (rt, exporter) = test_run_trace();
        rt.set_succeeded();
        drop(rt);

        let spans = exporter.finished_spans();
        let root = spans.iter().find(|s| s.name == "mise run //ci").unwrap();
        assert_eq!(root.status, Status::Ok);
    }
}
