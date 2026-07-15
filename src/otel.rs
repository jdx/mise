//! OpenTelemetry trace export for `mise run`.
//!
//! When `otel.enabled` is set and an OTLP endpoint is configured, each
//! `mise run` exports a root span covering the whole invocation, one span
//! per executed task, and (in monorepos) a group span per config root.
//!
//! Spans are real SDK spans started when the task starts and ended when it
//! finishes — the SDK's `BatchSpanProcessor` handles IDs, batching, and
//! export. Because task spans are alive while the task runs, their span
//! context is available for future log correlation without any ID
//! reservation machinery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use eyre::Result;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{Span as _, Status, TraceContextExt, Tracer, TracerProvider as _};
use opentelemetry::{Array, Context, KeyValue, StringValue, Value};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::SdkTracerProvider;

use crate::config::Settings;
use crate::task::Task;

/// A live span for a running task. Ended via [`RunTelemetry::end_task`];
/// if the task future is cancelled instead, the SDK ends it on drop.
pub type TaskSpan = opentelemetry_sdk::trace::Span;

/// Trace export is enabled only when the mise setting is on AND an OTLP
/// traces endpoint is configured. The env var requirement prevents mise
/// from emitting spans in environments that set `otel.enabled` globally
/// but point no collector at this machine — and vice versa, the setting
/// prevents surprise emission in environments that set
/// `OTEL_EXPORTER_OTLP_*` for other tools.
fn enabled() -> bool {
    Settings::get().otel.enabled
        && (std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
            || std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_ok())
}

/// All OpenTelemetry state for a single `mise run` invocation.
///
/// Cheap to clone (`Arc` inner). The root span is finalized and pending
/// spans are flushed by [`RunTelemetry::finish`] on the normal path, or by
/// `Drop` when the run future is cancelled (e.g. `--timeout`). The root
/// span defaults to an error status; `set_succeeded` marks the happy path.
#[derive(Clone)]
pub struct RunTelemetry {
    inner: Arc<Inner>,
}

struct Inner {
    provider: SdkTracerProvider,
    tracer: opentelemetry_sdk::trace::SdkTracer,
    /// Context holding the live root span; parents task and group spans.
    root_cx: Context,
    /// Live monorepo group spans keyed by config root.
    groups: Mutex<HashMap<PathBuf, Context>>,
    succeeded: AtomicBool,
    finished: AtomicBool,
}

impl RunTelemetry {
    /// Build the OTLP exporter pipeline and start the root span, or return
    /// `None` when trace export is not enabled.
    ///
    /// The OTLP crate natively reads the standard env vars
    /// (`OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`, ...);
    /// `Resource::builder` reads `OTEL_SERVICE_NAME` and
    /// `OTEL_RESOURCE_ATTRIBUTES`.
    pub fn init_if_enabled(requested_task_names: &[String]) -> Option<Self> {
        if !enabled() {
            return None;
        }
        let exporter = match opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
        {
            Ok(exporter) => exporter,
            Err(err) => {
                debug!("otel: failed to build span exporter: {err}");
                return None;
            }
        };
        let mut resource = Resource::builder();
        // Only default the service name when the user hasn't set one, since
        // with_service_name would override the env var.
        if std::env::var("OTEL_SERVICE_NAME").is_err() {
            resource = resource.with_service_name("mise");
        }
        let provider = SdkTracerProvider::builder()
            .with_span_processor(
                opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(
                    exporter,
                    runtime::Tokio,
                )
                .build(),
            )
            .with_resource(resource.build())
            .build();
        Some(Self::new(requested_task_names, provider))
    }

    fn new(requested_task_names: &[String], provider: SdkTracerProvider) -> Self {
        Self::new_with_parent(requested_task_names, provider, parent_cx_from_env())
    }

    fn new_with_parent(
        requested_task_names: &[String],
        provider: SdkTracerProvider,
        parent_cx: Context,
    ) -> Self {
        let root_name = if requested_task_names.is_empty() {
            "mise run".to_string()
        } else {
            format!("mise run {}", requested_task_names.join(" "))
        };
        let tracer = provider.tracer("mise.tasks");
        let mut root_span = tracer.start_with_context(root_name, &parent_cx);
        root_span.set_attribute(KeyValue::new("mise.span_type", "run"));
        let root_cx = Context::new().with_span(root_span);
        Self {
            inner: Arc::new(Inner {
                provider,
                tracer,
                root_cx,
                groups: Mutex::new(HashMap::new()),
                succeeded: AtomicBool::new(false),
                finished: AtomicBool::new(false),
            }),
        }
    }

    /// Start a span for a task, parented under its monorepo group span
    /// (created lazily) or directly under the root span.
    pub fn start_task(&self, task: &Task, project_root: Option<&PathBuf>) -> TaskSpan {
        let parent_cx = match &task.config_root {
            // A task belongs to a monorepo group when its config root
            // differs from the project root.
            Some(cr) if project_root.is_none_or(|pr| cr != pr) => self.group_cx(cr, project_root),
            _ => self.inner.root_cx.clone(),
        };
        self.inner
            .tracer
            .start_with_context(task_span_name(task), &parent_cx)
    }

    fn group_cx(&self, config_root: &Path, project_root: Option<&PathBuf>) -> Context {
        let mut groups = self.inner.groups.lock().unwrap();
        groups
            .entry(config_root.to_path_buf())
            .or_insert_with(|| {
                let mut span = self.inner.tracer.start_with_context(
                    group_display_name(config_root, project_root),
                    &self.inner.root_cx,
                );
                span.set_attribute(KeyValue::new("mise.span_type", "monorepo_group"));
                span.set_attribute(KeyValue::new(
                    "mise.config_root",
                    config_root.display().to_string(),
                ));
                self.inner.root_cx.with_span(span)
            })
            .clone()
    }

    /// End a task span with attributes and status derived from the result
    /// (`Ok(true)` ran, `Ok(false)` skipped, `Err` failed).
    pub fn end_task(&self, mut span: TaskSpan, task: &Task, result: &Result<bool>) {
        for attr in task_attributes(task) {
            span.set_attribute(attr);
        }
        match result {
            Ok(true) => {
                span.set_attribute(KeyValue::new("process.exit.code", 0i64));
                span.set_status(Status::Ok);
            }
            Ok(false) => {
                span.set_attribute(KeyValue::new("mise.task.skipped", true));
                // Skipped tasks didn't run; per CLI semconv, exit code is 0.
                span.set_attribute(KeyValue::new("process.exit.code", 0i64));
            }
            Err(err) => {
                let code = crate::errors::Error::get_exit_status(err).unwrap_or(1);
                span.set_attribute(KeyValue::new("process.exit.code", code as i64));
                span.set_status(Status::error(err.to_string()));
            }
        }
        span.end();
    }

    /// Mark the run as succeeded so the root span ends with an OK status.
    /// Anything short of an explicit success — including cancellation —
    /// produces an errored root span.
    pub fn set_succeeded(&self) {
        self.inner.succeeded.store(true, Ordering::Relaxed);
    }

    /// End group and root spans, then shut down the provider to flush all
    /// pending spans to the collector. Idempotent; also runs on drop so
    /// traces survive cancellation (e.g. `--timeout`).
    pub fn finish(&self) {
        self.inner.finish();
    }
}

impl Inner {
    fn finish(&self) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        for (_, cx) in self.groups.lock().unwrap().drain() {
            cx.span().end();
        }
        let root = self.root_cx.span();
        if self.succeeded.load(Ordering::Relaxed) {
            root.set_status(Status::Ok);
        } else {
            root.set_status(Status::error(""));
        }
        root.end();
        if let Err(err) = self.provider.shutdown() {
            debug!("otel: failed to flush spans: {err}");
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.finish();
    }
}

/// W3C Trace Context env vars for a task's environment, so nested
/// `mise run` and OTEL-aware tools the task invokes join this trace.
/// Plain strings — the task executor never sees OTEL types.
/// https://opentelemetry.io/docs/specs/otel/context/env-carriers/
pub struct TraceEnv {
    pub traceparent: String,
    pub tracestate: Option<String>,
}

/// Render a live task span's context as `TRACEPARENT`/`TRACESTATE` values
/// using the standard W3C propagator.
pub fn trace_env(span: &TaskSpan) -> Option<TraceEnv> {
    let cx = Context::new().with_remote_span_context(span.span_context().clone());
    let mut carrier: HashMap<String, String> = HashMap::new();
    TraceContextPropagator::new().inject_context(&cx, &mut carrier);
    Some(TraceEnv {
        traceparent: carrier.remove("traceparent")?,
        tracestate: carrier.remove("tracestate").filter(|ts| !ts.is_empty()),
    })
}

/// Parent context extracted from the `TRACEPARENT`/`TRACESTATE` env vars,
/// set by an OTEL-aware parent (CI, or an outer `mise run`). An invalid or
/// absent traceparent yields an empty context, i.e. a new root trace.
fn parent_cx_from_env() -> Context {
    match std::env::var("TRACEPARENT") {
        Ok(tp) => extract_parent_cx(&tp, std::env::var("TRACESTATE").ok().as_deref()),
        Err(_) => Context::new(),
    }
}

fn extract_parent_cx(traceparent: &str, tracestate: Option<&str>) -> Context {
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());
    if let Some(ts) = tracestate {
        carrier.insert("tracestate".to_string(), ts.to_string());
    }
    TraceContextPropagator::new().extract(&carrier)
}

/// Human-readable span name for a task (display name + args).
fn task_span_name(task: &Task) -> String {
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

/// Display name for a monorepo group span: the config root relative to the
/// project root when possible, otherwise its last path component.
fn group_display_name(config_root: &Path, project_root: Option<&PathBuf>) -> String {
    if let Some(pr) = project_root
        && let Ok(rel) = config_root.strip_prefix(pr)
    {
        let rel = rel.to_string_lossy();
        if !rel.is_empty() {
            return rel.into_owned();
        }
    }
    config_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| config_root.display().to_string())
}

/// Attributes attached to every task span, following the CLI semantic
/// conventions (https://opentelemetry.io/docs/specs/semconv/cli/cli-spans).
fn task_attributes(task: &Task) -> Vec<KeyValue> {
    let mut attrs = vec![
        KeyValue::new("mise.task.name", task.name.clone()),
        KeyValue::new("mise.task.source", task.config_source.display().to_string()),
    ];
    // Args are exported verbatim — no different from what's already visible
    // in terminal output and process listings.
    if !task.args.is_empty() {
        attrs.push(KeyValue::new("mise.task.args", task.args.join(" ")));
    }
    if let Some(cr) = &task.config_root {
        attrs.push(KeyValue::new(
            "mise.task.config_root",
            cr.display().to_string(),
        ));
    }
    let argv: Vec<StringValue> = ["mise", task.name.as_str()]
        .into_iter()
        .map(String::from)
        .chain(task.args.iter().cloned())
        .map(StringValue::from)
        .collect();
    attrs.push(KeyValue::new(
        "process.command_args",
        Value::Array(Array::String(argv)),
    ));
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{SpanData, SpanExporter};

    /// Like the SDK's `InMemorySpanExporter`, but keeps its records across
    /// `shutdown()` (which `finish()` always triggers).
    #[derive(Clone, Debug, Default)]
    struct RetainingSpanExporter {
        spans: Arc<Mutex<Vec<SpanData>>>,
    }

    impl SpanExporter for RetainingSpanExporter {
        fn export(
            &self,
            batch: Vec<SpanData>,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            self.spans.lock().unwrap().extend(batch);
            std::future::ready(Ok(()))
        }
    }

    impl RetainingSpanExporter {
        fn finished_spans(&self) -> Vec<SpanData> {
            self.spans.lock().unwrap().clone()
        }
    }

    fn test_telemetry(task_names: &[&str]) -> (RunTelemetry, RetainingSpanExporter) {
        let exporter = RetainingSpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let names: Vec<String> = task_names.iter().map(|s| s.to_string()).collect();
        // new_with_parent so tests are hermetic to TRACEPARENT in the env
        let t = RunTelemetry::new_with_parent(&names, provider, Context::new());
        (t, exporter)
    }

    fn task_for(name: &str, args: &[&str], config_root: Option<&str>) -> Task {
        Task {
            name: name.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            config_source: PathBuf::from("/w/mise.toml"),
            config_root: config_root.map(PathBuf::from),
            ..Default::default()
        }
    }

    fn span<'a>(spans: &'a [SpanData], name: &str) -> &'a SpanData {
        spans.iter().find(|s| s.name == name).unwrap_or_else(|| {
            let names: Vec<_> = spans.iter().map(|s| s.name.as_ref()).collect();
            panic!("missing span {name}; got {names:?}")
        })
    }

    #[test]
    fn task_spans_are_children_of_root() {
        let (t, exporter) = test_telemetry(&["build"]);
        let task = task_for("build", &["--fast"], None);
        let s = t.start_task(&task, None);
        t.end_task(s, &task, &Ok(true));
        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        let root = span(&spans, "mise run build");
        let task_span = span(&spans, "build --fast");
        assert_eq!(task_span.parent_span_id, root.span_context.span_id());
        assert_eq!(
            task_span.span_context.trace_id(),
            root.span_context.trace_id()
        );
        assert_eq!(root.status, Status::Ok);
        assert_eq!(task_span.status, Status::Ok);
    }

    #[test]
    fn monorepo_tasks_share_a_group_span() {
        let (t, exporter) = test_telemetry(&[]);
        let project_root = PathBuf::from("/w");
        let a = task_for("a", &[], Some("/w/pkg/x"));
        let b = task_for("b", &[], Some("/w/pkg/x"));
        let sa = t.start_task(&a, Some(&project_root));
        let sb = t.start_task(&b, Some(&project_root));
        t.end_task(sa, &a, &Ok(true));
        t.end_task(sb, &b, &Ok(true));
        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        let root = span(&spans, "mise run");
        let group = span(&spans, "pkg/x");
        assert_eq!(group.parent_span_id, root.span_context.span_id());
        assert_eq!(
            span(&spans, "a").parent_span_id,
            group.span_context.span_id()
        );
        assert_eq!(
            span(&spans, "b").parent_span_id,
            group.span_context.span_id()
        );
        assert_eq!(spans.len(), 4); // one group span, not two
    }

    #[test]
    fn task_at_project_root_is_not_grouped() {
        let (t, exporter) = test_telemetry(&[]);
        let project_root = PathBuf::from("/w");
        let task = task_for("build", &[], Some("/w"));
        let s = t.start_task(&task, Some(&project_root));
        t.end_task(s, &task, &Ok(true));
        t.finish();

        let spans = exporter.finished_spans();
        let root = span(&spans, "mise run");
        assert_eq!(
            span(&spans, "build").parent_span_id,
            root.span_context.span_id()
        );
    }

    #[test]
    fn failed_task_records_error_status_and_exit_code() {
        let (t, exporter) = test_telemetry(&[]);
        let task = task_for("build", &[], None);
        let s = t.start_task(&task, None);
        t.end_task(s, &task, &Err(eyre::eyre!("boom")));
        t.finish();

        let spans = exporter.finished_spans();
        let task_span = span(&spans, "build");
        assert!(
            matches!(&task_span.status, Status::Error { description } if description == "boom")
        );
        let exit = task_span
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "process.exit.code")
            .unwrap();
        assert_eq!(exit.value, Value::I64(1));
        // root defaults to error unless set_succeeded was called
        assert!(matches!(
            span(&spans, "mise run").status,
            Status::Error { .. }
        ));
    }

    #[test]
    fn skipped_task_is_marked() {
        let (t, exporter) = test_telemetry(&[]);
        let task = task_for("build", &[], None);
        let s = t.start_task(&task, None);
        t.end_task(s, &task, &Ok(false));
        t.finish();

        let spans = exporter.finished_spans();
        let task_span = span(&spans, "build");
        assert_eq!(task_span.status, Status::Unset);
        assert!(
            task_span
                .attributes
                .iter()
                .any(|kv| kv.key.as_str() == "mise.task.skipped" && kv.value == Value::Bool(true))
        );
    }

    #[test]
    fn task_attributes_follow_cli_semconv() {
        let task = task_for("build", &["x", "y"], Some("/w/pkg/x"));
        let attrs = task_attributes(&task);
        let get = |k: &str| {
            attrs
                .iter()
                .find(|kv| kv.key.as_str() == k)
                .map(|kv| kv.value.clone())
        };
        assert_eq!(get("mise.task.name"), Some(Value::from("build")));
        assert_eq!(get("mise.task.args"), Some(Value::from("x y")));
        assert_eq!(get("mise.task.config_root"), Some(Value::from("/w/pkg/x")));
        let argv: Vec<StringValue> = ["mise", "build", "x", "y"]
            .map(String::from)
            .map(StringValue::from)
            .to_vec();
        assert_eq!(
            get("process.command_args"),
            Some(Value::Array(Array::String(argv)))
        );
    }

    const UPSTREAM_TRACEPARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

    #[test]
    fn root_span_joins_upstream_trace_from_traceparent() {
        let exporter = RetainingSpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let parent_cx = extract_parent_cx(UPSTREAM_TRACEPARENT, Some("vendor=value"));
        let t = RunTelemetry::new_with_parent(&[], provider, parent_cx);
        t.finish();

        let root = &exporter.finished_spans()[0];
        assert_eq!(
            root.span_context.trace_id().to_string(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(root.parent_span_id.to_string(), "b7ad6b7169203331");
        assert_eq!(root.span_context.trace_state().header(), "vendor=value");
    }

    #[test]
    fn invalid_traceparent_starts_a_new_trace() {
        let exporter = RetainingSpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let parent_cx = extract_parent_cx("00-not-valid-01", None);
        let t = RunTelemetry::new_with_parent(&[], provider, parent_cx);
        t.finish();

        let root = &exporter.finished_spans()[0];
        assert_eq!(root.parent_span_id, opentelemetry::trace::SpanId::INVALID);
        assert!(root.span_context.is_valid());
    }

    #[test]
    fn trace_env_carries_the_live_task_span_context() {
        let (t, _exporter) = test_telemetry(&[]);
        let task = task_for("build", &[], None);
        let span = t.start_task(&task, None);
        let te = trace_env(&span).unwrap();
        assert_eq!(
            te.traceparent,
            format!(
                "00-{}-{}-01",
                span.span_context().trace_id(),
                span.span_context().span_id()
            )
        );
        // Round-trip: a nested run extracting this env joins the same trace.
        let cx = extract_parent_cx(&te.traceparent, te.tracestate.as_deref());
        assert_eq!(
            cx.span().span_context().trace_id(),
            span.span_context().trace_id()
        );
        assert_eq!(
            cx.span().span_context().span_id(),
            span.span_context().span_id()
        );
        t.end_task(span, &task, &Ok(true));
        t.finish();
    }

    #[test]
    fn drop_flushes_and_errors_root_span_on_cancellation() {
        let (t, exporter) = test_telemetry(&[]);
        // Simulate cancellation: telemetry dropped without finish/set_succeeded.
        drop(t);
        let spans = exporter.finished_spans();
        assert!(matches!(
            span(&spans, "mise run").status,
            Status::Error { .. }
        ));
    }
}
