//! Task-aware OpenTelemetry integration.
//!
//! This module owns the per-`mise run` telemetry lifecycle so that `cli::run`
//! and `task_executor` don't have to reach into OTEL internals.
//!
//! Spans are real SDK spans: started when the task starts, ended when it
//! finishes. The SDK handles IDs, parenting, timing, batching, and export, so
//! there is no ID reservation or duration bookkeeping here. Because a task's
//! span is alive while the task runs, its `SpanContext` is available for both
//! W3C context propagation and log correlation.

use crate::otel::traces_enabled;
use crate::task::Task;
use crate::task::task_executor::TaskRunOutcome;
use eyre::Result;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{
    Span as _, SpanContext, Status, TraceContextExt, Tracer, TracerProvider as _,
};
use opentelemetry::{Array, Context, KeyValue, StringValue, Value};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A live span for a running task. Ended by [`TaskRunTelemetry::end_task`];
/// if the task future is cancelled instead, the SDK ends it on drop.
pub type TaskSpan = opentelemetry_sdk::trace::Span;

/// All OpenTelemetry state attached to a single `mise run` invocation.
///
/// Cheap to clone (`Arc` inner) so per-task code can hold one. The root span
/// is alive for the whole invocation and is finalized by [`Self::finish`] on
/// the normal path, or by `Drop` when the run future is cancelled (e.g. by
/// `--timeout`). The root span defaults to an error status; `set_succeeded`
/// marks the happy path.
#[derive(Clone)]
pub struct TaskRunTelemetry {
    inner: Arc<Inner>,
}

struct Inner {
    provider: SdkTracerProvider,
    tracer: SdkTracer,
    /// Context holding the live root span; parents task and group spans.
    root_cx: Context,
    /// Live monorepo group spans keyed by config root.
    groups: Mutex<HashMap<PathBuf, Context>>,
    has_failures: AtomicBool,
    finished: AtomicBool,
}

impl TaskRunTelemetry {
    /// Initialize a `TaskRunTelemetry` if trace export is enabled, i.e.
    /// `otel.enabled = true` and a traces endpoint is configured.
    ///
    /// When mise is invoked from another mise run (or any OTEL-aware
    /// parent), the `TRACEPARENT` env var carries W3C Traceparent so
    /// the nested run joins the same distributed trace.
    pub fn init_if_enabled(requested_task_names: &[String]) -> Option<Self> {
        if !traces_enabled() {
            return None;
        }
        let suffix = if requested_task_names.is_empty() {
            String::new()
        } else {
            format!(" {}", requested_task_names.join(" "))
        };
        let root_span_name = format!("mise run{suffix}");

        let provider = crate::otel::build_tracer_provider(crate::otel::build_resource())?;
        Some(Self::new(&root_span_name, provider, parent_cx_from_env()))
    }

    fn new(root_span_name: &str, provider: SdkTracerProvider, parent_cx: Context) -> Self {
        let tracer = provider.tracer("mise.tasks");
        let mut root_span = tracer.start_with_context(root_span_name.to_string(), &parent_cx);
        root_span.set_attribute(KeyValue::new("mise.span_type", "run"));
        Self {
            inner: Arc::new(Inner {
                provider,
                tracer,
                root_cx: Context::new().with_span(root_span),
                groups: Mutex::new(HashMap::new()),
                has_failures: AtomicBool::new(true),
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
                    monorepo_group_display_name(config_root, project_root),
                    &self.inner.root_cx,
                );
                span.set_attribute(KeyValue::new("mise.span_type", "monorepo_group"));
                span.set_attribute(KeyValue::new(
                    "mise.config_root",
                    config_root.display().to_string(),
                ));
                // Status is deliberately left unset: a group is a grouping,
                // not a unit of work, so one failing member must not paint
                // the whole package red.
                self.inner.root_cx.with_span(span)
            })
            .clone()
    }

    /// End a task's span with attributes and status derived from the result
    /// (did work / skipped / failed).
    ///
    /// `cancelled` marks a task that was torn down because a *sibling* task
    /// failed. Its span is still recorded — it did run, and its duration is
    /// real — but left `Unset` rather than `Error`: only the task that
    /// actually failed should show up as a failure in the trace.
    ///
    /// `end_time` is passed explicitly so the span covers the task itself,
    /// not the error reporting and sibling teardown that follow it.
    pub fn end_task(
        &self,
        mut span: TaskSpan,
        task: &Task,
        end_time: SystemTime,
        result: &Result<TaskRunOutcome>,
        cancelled: bool,
    ) {
        for attr in task_attributes(task) {
            span.set_attribute(attr);
        }
        match result {
            Ok(outcome) if outcome.did_work => {
                span.set_attribute(KeyValue::new("process.exit.code", 0i64));
                span.set_status(Status::Ok);
            }
            Ok(_) => {
                span.set_attribute(KeyValue::new("mise.task.skipped", true));
                // Skipped tasks didn't run; per CLI semconv, exit code is 0.
                span.set_attribute(KeyValue::new("process.exit.code", 0i64));
            }
            Err(err) if cancelled => {
                span.set_attribute(KeyValue::new("mise.task.cancelled", true));
                // A task killed by the sibling-shutdown signal usually has no
                // exit code at all — only record one when the OS gave us one,
                // rather than inventing a failure code.
                if let Some(code) = crate::errors::Error::get_exit_status(err) {
                    span.set_attribute(KeyValue::new("process.exit.code", code as i64));
                }
            }
            Err(err) => {
                let code = crate::errors::Error::get_exit_status(err).unwrap_or(1);
                span.set_attribute(KeyValue::new("process.exit.code", code as i64));
                span.set_status(Status::error(err.to_string()));
            }
        }
        span.end_with_timestamp(end_time);
    }

    /// Mark the run as succeeded. Must be called explicitly on the happy
    /// path — the default is a failed run so that cancelled futures (e.g.
    /// timeout) produce an errored root span.
    pub fn set_succeeded(&self) {
        self.inner.has_failures.store(false, Ordering::Relaxed);
    }

    /// End the group and root spans and shut down the tracer provider,
    /// flushing pending spans. Idempotent; also runs on drop so traces
    /// survive cancellation (e.g. `--timeout`).
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
        if self.has_failures.load(Ordering::Relaxed) {
            root.set_status(Status::error(""));
        } else {
            root.set_status(Status::Ok);
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

/// Display name for a monorepo group span: the config root relative to the
/// project root when possible, otherwise its last path component.
fn monorepo_group_display_name(config_root: &Path, project_root: Option<&PathBuf>) -> String {
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

/// Standard OpenTelemetry attributes attached to every task span.
fn task_attributes(task: &Task) -> Vec<KeyValue> {
    let display_name = task_span_name(task);
    let mut attrs = vec![
        KeyValue::new("mise.task.name", task.name.clone()),
        KeyValue::new("mise.task.display_name", display_name),
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

/// Inject a task span's context into its env vars using the standard
/// W3C Trace Context propagator and env-carrier variable names.
pub fn inject_otel_context(env: &mut BTreeMap<String, String>, span_cx: &SpanContext) {
    let cx = Context::new().with_remote_span_context(span_cx.clone());
    let mut carrier = HashMap::new();
    TraceContextPropagator::new().inject_context(&cx, &mut carrier);
    if let Some(traceparent) = carrier.remove("traceparent") {
        env.insert("TRACEPARENT".into(), traceparent);
    }
    if let Some(tracestate) = carrier.remove("tracestate") {
        env.insert("TRACESTATE".into(), tracestate);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{SpanId, TraceFlags, TraceId, TraceState};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{SimpleSpanProcessor, SpanData, SpanExporter};
    use std::future;

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

    fn test_telemetry_with_parent(
        root_span_name: &str,
        parent_cx: Context,
    ) -> (TaskRunTelemetry, RetainingSpanExporter) {
        let exporter = RetainingSpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let t = TaskRunTelemetry::new(root_span_name, provider, parent_cx);
        (t, exporter)
    }

    fn test_telemetry(root_span_name: &str) -> (TaskRunTelemetry, RetainingSpanExporter) {
        test_telemetry_with_parent(root_span_name, Context::new())
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

    fn task_in(name: &str, config_root: &str) -> Task {
        let mut task = task_for(name, "", &[]);
        task.config_root = Some(PathBuf::from(config_root));
        task
    }

    fn span_by_name<'a>(spans: &'a [SpanData], name: &str) -> &'a SpanData {
        spans
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing span '{name}'"))
    }

    fn attr<'a>(span: &'a SpanData, key: &str) -> Option<&'a Value> {
        span.attributes
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| &kv.value)
    }

    fn is_error(status: &Status) -> bool {
        matches!(status, Status::Error { .. })
    }

    /// Run one task to completion with the given result and return its span.
    fn ran() -> Result<TaskRunOutcome> {
        Ok(TaskRunOutcome {
            did_work: true,
            ..Default::default()
        })
    }

    fn skipped() -> Result<TaskRunOutcome> {
        Ok(TaskRunOutcome::default())
    }

    fn end_one_task(result: Result<TaskRunOutcome>, cancelled: bool) -> SpanData {
        let (t, exporter) = test_telemetry("mise run build");
        let task = task_for("build", "", &[]);
        let span = t.start_task(&task, None);
        t.end_task(span, &task, SystemTime::now(), &result, cancelled);
        span_by_name(&exporter.finished_spans(), "build").clone()
    }

    #[test]
    fn finish_builds_root_and_monorepo_hierarchy() {
        let (t, exporter) = test_telemetry("mise run");
        let project_root = PathBuf::from("/workspace");

        // Direct task (config_root == project_root → child of root).
        let direct = task_in("lint", "/workspace");
        let span = t.start_task(&direct, Some(&project_root));
        t.end_task(span, &direct, SystemTime::now(), &ran(), false);

        // Monorepo task (config_root != project_root → child of group span).
        let nested = task_in("build", "/workspace/packages/frontend");
        let span = t.start_task(&nested, Some(&project_root));
        t.end_task(span, &nested, SystemTime::now(), &ran(), false);

        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        assert_eq!(spans.len(), 4, "expected root + group + 2 task spans");

        let root = span_by_name(&spans, "mise run");
        let group = span_by_name(&spans, "packages/frontend");
        let lint = span_by_name(&spans, "lint");
        let build = span_by_name(&spans, "build");

        // Root has no parent; no failures so status is Ok.
        assert_eq!(root.parent_span_id, SpanId::INVALID);
        assert_eq!(root.status, Status::Ok);

        // Group is child of root; group status is always Unset (per OTel spec).
        assert_eq!(group.parent_span_id, root.span_context.span_id());
        assert_eq!(group.status, Status::Unset);

        // Direct task is child of root, monorepo task is child of the group.
        assert_eq!(lint.parent_span_id, root.span_context.span_id());
        assert_eq!(build.parent_span_id, group.span_context.span_id());

        // Everything shares one trace.
        for s in &spans {
            assert_eq!(s.span_context.trace_id(), root.span_context.trace_id());
        }
    }

    #[test]
    fn group_span_spans_its_children() {
        let (t, exporter) = test_telemetry("mise run");
        let project_root = PathBuf::from("/workspace");
        let task = task_in("build", "/workspace/packages/frontend");

        let span = t.start_task(&task, Some(&project_root));
        t.end_task(span, &task, SystemTime::now(), &ran(), false);
        t.set_succeeded();
        t.finish();

        // Live spans mean group/root timing needs no aggregation: each is
        // simply alive for as long as its children are.
        let spans = exporter.finished_spans();
        let root = span_by_name(&spans, "mise run");
        let group = span_by_name(&spans, "packages/frontend");
        let build = span_by_name(&spans, "build");
        assert!(root.start_time <= group.start_time);
        assert!(group.start_time <= build.start_time);
        assert!(build.end_time <= group.end_time);
        assert!(group.end_time <= root.end_time);
    }

    #[test]
    fn finish_has_failures_does_not_taint_ok_group() {
        let (t, exporter) = test_telemetry("mise run");
        let project_root = PathBuf::from("/workspace");
        let task = task_in("build", "/workspace/packages/frontend");

        let span = t.start_task(&task, Some(&project_root));
        t.end_task(span, &task, SystemTime::now(), &ran(), false);

        // Scheduler-level failure (e.g. ctrl-c) should mark root as
        // errored, but a group whose own tasks all succeeded stays OK.
        t.finish();

        let spans = exporter.finished_spans();
        assert_eq!(span_by_name(&spans, "build").status, Status::Ok);
        assert_eq!(
            span_by_name(&spans, "packages/frontend").status,
            Status::Unset
        );
        assert!(is_error(&span_by_name(&spans, "mise run").status));
    }

    #[test]
    fn finish_group_stays_unset_even_when_child_fails() {
        let (t, exporter) = test_telemetry("mise run");
        let project_root = PathBuf::from("/workspace");
        let task = task_in("build", "/workspace/packages/frontend");

        let span = t.start_task(&task, Some(&project_root));
        t.end_task(
            span,
            &task,
            SystemTime::now(),
            &Err(eyre::eyre!("boom")),
            false,
        );
        t.finish();

        let spans = exporter.finished_spans();
        assert!(is_error(&span_by_name(&spans, "build").status));
        assert_eq!(
            span_by_name(&spans, "packages/frontend").status,
            Status::Unset
        );
        assert!(is_error(&span_by_name(&spans, "mise run").status));
    }

    #[test]
    fn task_without_config_root_is_direct_child_of_root() {
        let (t, exporter) = test_telemetry("mise run");
        let task = task_for("lint", "", &[]);
        let span = t.start_task(&task, None);
        t.end_task(span, &task, SystemTime::now(), &ran(), false);
        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        let root = span_by_name(&spans, "mise run");
        assert_eq!(
            span_by_name(&spans, "lint").parent_span_id,
            root.span_context.span_id()
        );
        // No monorepo groups were created.
        assert!(
            !spans.iter().any(|s| attr(s, "mise.span_type")
                == Some(&Value::String("monorepo_group".into()))),
            "unexpected monorepo group span in non-monorepo run"
        );
    }

    #[test]
    fn tasks_sharing_a_config_root_share_one_group_span() {
        let (t, exporter) = test_telemetry("mise run");
        let project_root = PathBuf::from("/workspace");
        for name in ["build", "test"] {
            let task = task_in(name, "/workspace/packages/frontend");
            let span = t.start_task(&task, Some(&project_root));
            t.end_task(span, &task, SystemTime::now(), &ran(), false);
        }
        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        assert_eq!(
            spans
                .iter()
                .filter(|s| s.name == "packages/frontend")
                .count(),
            1,
            "group span must be created once per config root"
        );
        let group = span_by_name(&spans, "packages/frontend");
        for name in ["build", "test"] {
            assert_eq!(
                span_by_name(&spans, name).parent_span_id,
                group.span_context.span_id()
            );
        }
    }

    #[test]
    fn finish_keeps_parent_span_for_nested_run() {
        let parent_trace_id = TraceId::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ]);
        let parent_span_id = SpanId::from_bytes([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
        let parent_cx = Context::new().with_remote_span_context(SpanContext::new(
            parent_trace_id,
            parent_span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        ));
        let (t, exporter) = test_telemetry_with_parent("mise run nested", parent_cx);
        t.set_succeeded();
        t.finish();

        let spans = exporter.finished_spans();
        let root = span_by_name(&spans, "mise run nested");
        assert_eq!(root.span_context.trace_id(), parent_trace_id);
        assert_eq!(root.parent_span_id, parent_span_id);
    }

    #[test]
    fn finish_is_idempotent() {
        let (t, exporter) = test_telemetry("mise run");
        t.finish();
        t.finish();
        drop(t);
        assert_eq!(
            exporter
                .finished_spans()
                .iter()
                .filter(|s| s.name == "mise run")
                .count(),
            1,
            "root span must be emitted exactly once"
        );
    }

    #[test]
    fn drop_without_finish_emits_errored_root_span() {
        let (t, exporter) = test_telemetry("mise run //ci");
        // Simulate timeout/cancellation: drop without finishing.
        drop(t);

        let spans = exporter.finished_spans();
        let root = span_by_name(&spans, "mise run //ci");
        assert!(
            is_error(&root.status),
            "expected errored root span on cancelled drop, got {:?}",
            root.status
        );
    }

    #[test]
    fn drop_after_set_succeeded_emits_ok_root_span() {
        let (t, exporter) = test_telemetry("mise run //ci");
        t.set_succeeded();
        drop(t);

        assert_eq!(
            span_by_name(&exporter.finished_spans(), "mise run //ci").status,
            Status::Ok
        );
    }

    #[test]
    fn cancelled_task_span_is_still_exported() {
        let (t, exporter) = test_telemetry("mise run");
        let task = task_for("build", "", &[]);
        // Task future cancelled mid-flight: the span is dropped, not ended.
        drop(t.start_task(&task, None));
        t.finish();

        // The SDK ends it on drop, so the trace still shows the task started.
        let spans = exporter.finished_spans();
        assert_eq!(
            span_by_name(&spans, "build").parent_span_id,
            span_by_name(&spans, "mise run").span_context.span_id()
        );
    }

    #[test]
    fn failed_task_span_is_marked_error() {
        let span = end_one_task(Err(eyre::eyre!("boom")), false);
        assert!(
            is_error(&span.status),
            "expected errored span, got {:?}",
            span.status
        );
        assert_eq!(attr(&span, "process.exit.code"), Some(&Value::I64(1)));
        assert!(attr(&span, "mise.task.cancelled").is_none());
    }

    #[test]
    fn cancelled_task_span_is_not_marked_error() {
        // A sibling failed and SIGTERMed this task — it should not be
        // reported as a failure of its own.
        let span = end_one_task(Err(eyre::eyre!("boom")), true);
        assert_eq!(span.status, Status::Unset);
        assert_eq!(attr(&span, "mise.task.cancelled"), Some(&Value::Bool(true)));
        // No exit code was reported by the OS, so none is invented.
        assert!(attr(&span, "process.exit.code").is_none());
    }

    #[test]
    fn succeeded_task_span_ignores_cancelled_flag() {
        let span = end_one_task(ran(), true);
        assert_eq!(span.status, Status::Ok);
        assert_eq!(attr(&span, "process.exit.code"), Some(&Value::I64(0)));
        assert!(attr(&span, "mise.task.cancelled").is_none());
    }

    #[test]
    fn skipped_task_span_is_marked_skipped() {
        let span = end_one_task(skipped(), false);
        assert_eq!(span.status, Status::Unset);
        assert_eq!(attr(&span, "mise.task.skipped"), Some(&Value::Bool(true)));
        assert_eq!(attr(&span, "process.exit.code"), Some(&Value::I64(0)));
    }

    #[test]
    fn end_task_honours_the_supplied_end_time() {
        let (t, exporter) = test_telemetry("mise run");
        let task = task_for("build", "", &[]);
        let span = t.start_task(&task, None);
        let end_time = SystemTime::now();
        // Error reporting and sibling teardown happen between the task
        // finishing and the span being ended; they must not inflate it.
        std::thread::sleep(std::time::Duration::from_millis(20));
        t.end_task(span, &task, end_time, &ran(), false);

        let build = span_by_name(&exporter.finished_spans(), "build").clone();
        assert_eq!(build.end_time, end_time);
    }

    #[test]
    fn span_name_uses_display_name_with_args() {
        assert_eq!(
            task_span_name(&task_for("build", "Build", &["--release"])),
            "Build --release"
        );
    }

    #[test]
    fn span_name_falls_back_to_task_name() {
        assert_eq!(task_span_name(&task_for("build", "", &[])), "build");
    }

    #[test]
    fn attributes_include_args_and_config_root() {
        let mut task = task_for("build", "Build", &["x", "y"]);
        task.config_root = Some(PathBuf::from("/workspace/packages/a"));
        let attrs = task_attributes(&task);
        let find_str = |k: &str| {
            attrs.iter().find(|kv| kv.key.as_str() == k).map(|kv| {
                if let Value::String(s) = &kv.value {
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
        if let Value::Array(Array::String(items)) = &argv.value {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            assert_eq!(strs, vec!["mise", "build", "x", "y"]);
        } else {
            panic!("process.command_args should be a string array");
        }
    }

    #[test]
    fn attributes_omit_args_when_empty() {
        let attrs = task_attributes(&task_for("build", "", &[]));
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
        if let Value::Array(Array::String(items)) = &argv.value {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            assert_eq!(strs, vec!["mise", "build"]);
        } else {
            panic!("process.command_args should be a string array");
        }
    }

    #[test]
    fn inject_otel_context_uses_propagator_output() {
        let span_cx = SpanContext::new(
            TraceId::from_bytes([
                0x0a, 0xf7, 0x65, 0x19, 0x16, 0xcd, 0x43, 0xdd, 0x84, 0x48, 0xeb, 0x21, 0x1c, 0x80,
                0x31, 0x9c,
            ]),
            SpanId::from_bytes([0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31]),
            TraceFlags::SAMPLED,
            false,
            TraceState::default(),
        );
        let mut env = BTreeMap::new();
        inject_otel_context(&mut env, &span_cx);
        assert_eq!(
            env.get("TRACEPARENT").map(String::as_str),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
        );
    }

    #[test]
    fn inject_otel_context_round_trips_through_a_live_span() {
        let (t, _exporter) = test_telemetry("mise run");
        let span = t.start_task(&task_for("build", "", &[]), None);
        let span_cx = span.span_context().clone();

        let mut env = BTreeMap::new();
        inject_otel_context(&mut env, &span_cx);

        // What a nested `mise run` parses back out of its environment.
        let parsed = extract_parent_cx(env.get("TRACEPARENT").unwrap(), None);
        let parsed = parsed.span().span_context().clone();
        assert_eq!(parsed.trace_id(), span_cx.trace_id());
        assert_eq!(parsed.span_id(), span_cx.span_id());
    }

    #[test]
    fn parse_otel_context_extracts_ids_from_traceparent_env() {
        let cx = extract_parent_cx(
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            None,
        );
        let sc = cx.span().span_context().clone();
        assert_eq!(
            sc.trace_id(),
            TraceId::from_bytes([
                0x0a, 0xf7, 0x65, 0x19, 0x16, 0xcd, 0x43, 0xdd, 0x84, 0x48, 0xeb, 0x21, 0x1c, 0x80,
                0x31, 0x9c,
            ])
        );
        assert_eq!(
            sc.span_id(),
            SpanId::from_bytes([0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31])
        );
    }

    #[test]
    fn parse_otel_context_rejects_invalid_traceparent_env() {
        let cx = extract_parent_cx("00-short-also_short-01", None);
        assert!(!cx.span().span_context().is_valid());
    }

    #[test]
    fn parse_otel_context_preserves_upstream_tracestate() {
        let cx = extract_parent_cx(
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            Some("vendor=value"),
        );
        assert_eq!(
            cx.span().span_context().trace_state().header(),
            "vendor=value"
        );
    }

    #[test]
    fn monorepo_group_display_name_uses_relative_path() {
        assert_eq!(
            monorepo_group_display_name(
                Path::new("/workspace/packages/frontend"),
                Some(&PathBuf::from("/workspace")),
            ),
            "packages/frontend"
        );
    }

    #[test]
    fn monorepo_group_display_name_falls_back_to_leaf() {
        assert_eq!(
            monorepo_group_display_name(
                Path::new("/other/frontend"),
                Some(&PathBuf::from("/workspace")),
            ),
            "frontend"
        );
    }

    #[test]
    fn monorepo_group_display_name_no_project_root() {
        assert_eq!(
            monorepo_group_display_name(Path::new("/workspace/packages/frontend"), None),
            "frontend"
        );
    }
}
