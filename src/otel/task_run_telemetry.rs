//! Task-aware OpenTelemetry integration.
//!
//! This module is the bridge between the generic OTLP primitives
//! (`TaskSpanTracker`, `TaskOutputForwarder`) and the mise task model. It owns
//! the per-`mise run` telemetry lifecycle so that `cli::run` and
//! `task_executor` don't have to reach into OTEL internals.

use crate::otel::task_span_tracker::StartedSpan;
use crate::otel::{TaskOutputForwarder, TaskSpanTracker, is_enabled};
use crate::task::Task;
use eyre::Result;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{SpanContext, Status, TraceContextExt};
use opentelemetry::{Array, KeyValue, StringValue, Value};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// All OpenTelemetry state attached to a single `mise run` invocation.
///
/// - `span_tracker` is cheaply clonable and can be passed to per-task code.
/// - `output_forwarder` is cheaply clonable and is installed on the
///   task executor so it can forward stdout/stderr lines.
///
/// Implements `Drop` to flush logs and emit spans, so traces are
/// captured even when the future is cancelled (e.g. by `--timeout`).
/// Defaults to `has_failures = true`; call `set_succeeded()` on
/// the happy path to mark the root span as OK.
pub struct TaskRunTelemetry {
    pub span_tracker: TaskSpanTracker,
    pub output_forwarder: TaskOutputForwarder,
    has_failures: std::sync::atomic::AtomicBool,
}

impl TaskRunTelemetry {
    /// Initialize a `TaskRunTelemetry` if otel is configured.
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

        let span_tracker = match parse_otel_context() {
            Some(parent_span_context) => TaskSpanTracker::from_parent_context(
                &root_span_name,
                parent_span_context,
                tracer_provider,
            ),
            _ => TaskSpanTracker::new(&root_span_name, tracer_provider),
        };
        let output_forwarder = match logger_provider {
            Some(lp) => TaskOutputForwarder::new(lp),
            None => {
                // If no logs URL is configured, create a no-op provider
                TaskOutputForwarder::new(
                    opentelemetry_sdk::logs::SdkLoggerProvider::builder().build(),
                )
            }
        };
        Some(Self {
            span_tracker,
            output_forwarder,
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

impl Drop for TaskRunTelemetry {
    fn drop(&mut self) {
        let has_failures = *self.has_failures.get_mut();
        // Shutdown the output forwarder first to flush all pending log batches.
        self.output_forwarder.shutdown();
        // Then finish the trace which emits root/group spans and shuts down
        // the tracer provider.
        self.span_tracker.finish(has_failures);
    }
}

impl TaskSpanTracker {
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
        match result {
            Ok(false) => {
                attrs.push(KeyValue::new("mise.task.skipped", true));
                // Skipped tasks didn't run; per CLI semconv, exit code is 0.
                attrs.push(KeyValue::new("process.exit.code", 0i64));
            }
            Ok(true) => {
                attrs.push(KeyValue::new("process.exit.code", 0i64));
            }
            Err(err) => {
                let code = crate::errors::Error::get_exit_status(err).unwrap_or(1);
                attrs.push(KeyValue::new("process.exit.code", code as i64));
            }
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
fn task_attributes(task: &Task, display_name: &str) -> Vec<KeyValue> {
    let mut attrs = vec![
        KeyValue::new("mise.task.name", task.name.clone()),
        KeyValue::new("mise.task.display_name", display_name.to_string()),
        KeyValue::new("mise.task.source", task.config_source.display().to_string()),
    ];
    // Args are exported verbatim, no different from what's already visible in terminal output and process listings.
    if !task.args.is_empty() {
        attrs.push(KeyValue::new("mise.task.args", task.args.join(" ")));
    }
    if let Some(ref cr) = task.config_root {
        attrs.push(KeyValue::new(
            "mise.task.config_root",
            cr.display().to_string(),
        ));
    }
    // CLI semantic conventions: full argv (executable + args) per
    // https://opentelemetry.io/docs/specs/semconv/cli/cli-spans
    let mut argv: Vec<StringValue> = Vec::with_capacity(2 + task.args.len());
    argv.push(StringValue::from("mise"));
    argv.push(StringValue::from(task.name.clone()));
    for a in &task.args {
        argv.push(StringValue::from(a.clone()));
    }
    attrs.push(KeyValue::new(
        "process.command_args",
        Value::Array(Array::String(argv)),
    ));
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
    use crate::otel::TaskOutputForwarder;
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

    fn test_telemetry() -> (TaskRunTelemetry, RetainingSpanExporter) {
        let exporter = RetainingSpanExporter::default();
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let span_tracker = TaskSpanTracker::new("mise run //ci", provider);
        let output_forwarder =
            TaskOutputForwarder::new(opentelemetry_sdk::logs::SdkLoggerProvider::builder().build());
        let t = TaskRunTelemetry {
            span_tracker,
            output_forwarder,
            has_failures: std::sync::atomic::AtomicBool::new(true),
        };
        (t, exporter)
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
        let find_str = |k: &str| {
            attrs.iter().find(|kv| kv.key.as_str() == k).map(|kv| {
                if let opentelemetry::Value::String(s) = &kv.value {
                    s.as_str().to_string()
                } else {
                    panic!("expected string value for {k}");
                }
            })
        };
        assert_eq!(find_str("mise.task.name").as_deref(), Some("build"));
        assert_eq!(
            find_str("mise.task.display_name").as_deref(),
            Some("Build x y")
        );
        assert_eq!(find_str("mise.task.args").as_deref(), Some("x y"));
        assert_eq!(
            find_str("mise.task.config_root").as_deref(),
            Some("/workspace/packages/a")
        );
        // CLI semconv: process.command_args is an array of [exe, task name, ...args]
        let argv = attrs
            .iter()
            .find(|kv| kv.key.as_str() == "process.command_args")
            .expect("missing process.command_args");
        if let opentelemetry::Value::Array(opentelemetry::Array::String(items)) = &argv.value {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            assert_eq!(strs, vec!["mise", "build", "x", "y"]);
        } else {
            panic!("process.command_args should be a string array");
        }
    }

    #[test]
    fn attributes_omit_args_when_empty() {
        let task = task_for("build", "", &[]);
        let attrs = task_attributes(&task, "build");
        assert!(attrs.iter().all(|kv| kv.key.as_str() != "mise.task.args"));
        assert!(
            attrs
                .iter()
                .all(|kv| kv.key.as_str() != "mise.task.config_root")
        );
        // process.command_args is still emitted (just exe + task name).
        let argv = attrs
            .iter()
            .find(|kv| kv.key.as_str() == "process.command_args")
            .expect("missing process.command_args");
        if let opentelemetry::Value::Array(opentelemetry::Array::String(items)) = &argv.value {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            assert_eq!(strs, vec!["mise", "build"]);
        } else {
            panic!("process.command_args should be a string array");
        }
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
        let (rt, exporter) = test_telemetry();
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
        let (rt, exporter) = test_telemetry();
        rt.set_succeeded();
        drop(rt);

        let spans = exporter.finished_spans();
        let root = spans.iter().find(|s| s.name == "mise run //ci").unwrap();
        assert_eq!(root.status, Status::Ok);
    }
}
