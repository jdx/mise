use opentelemetry::KeyValue;
use opentelemetry::trace::{
    Span as _, SpanContext, SpanId, Status, TraceContextExt, TraceFlags, TraceId, TraceState,
    Tracer, TracerProvider,
};
use opentelemetry_sdk::trace::{IdGenerator, RandomIdGenerator, SdkTracerProvider};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Shared ID generator — uses the SDK's `RandomIdGenerator` so we don't
/// depend on `rand` directly for trace/span IDs.
fn id_gen() -> &'static RandomIdGenerator {
    static GEN: std::sync::LazyLock<RandomIdGenerator> =
        std::sync::LazyLock::new(RandomIdGenerator::default);
    &GEN
}

/// Shared state for collecting spans across concurrent task execution.
///
/// Nesting model:
/// - Tasks with a monorepo `config_root` are grouped under a parent
///   span for that config_root.
/// - All other tasks are direct children of the trace's root span.
///
/// Emission model: every span is created using the SDK tracer with
/// explicit IDs and timing, then ended immediately. Task spans are
/// emitted from `end_task_span`. The root span and monorepo-group
/// spans are emitted from `finish()` with their aggregated duration.
#[derive(Clone)]
pub struct TraceContext {
    inner: Arc<Mutex<TraceContextInner>>,
    /// Provider lives outside the Mutex — it is Arc-based and
    /// thread-safe on its own. This lets `end_task_span` release the
    /// lock before calling `emit_span`, avoiding any contention with
    /// `finish()`/`shutdown()`.
    tracer_provider: SdkTracerProvider,
}

struct TraceContextInner {
    trace_id: TraceId,
    root_span: RootSpan,
    /// Monorepo parent spans keyed by config_root path
    monorepo_parents: HashMap<PathBuf, MonorepoParent>,
    /// Reverse index from monorepo parent `span_id` to its config_root
    /// key in `monorepo_parents`, so fold-by-parent-span-id is O(1)
    /// instead of O(N) across all groups.
    monorepo_parent_by_span_id: HashMap<SpanId, PathBuf>,
}

struct RootSpan {
    parent_span_id: Option<SpanId>,
    span_id: SpanId,
    name: String,
    init_time: SystemTime,
    min_start: Option<SystemTime>,
    max_end: Option<SystemTime>,
}

fn status_from_error(has_error: bool) -> Status {
    if has_error {
        Status::Error {
            description: Cow::Borrowed(""),
        }
    } else {
        Status::Ok
    }
}

#[cfg(test)]
fn is_error(status: &Status) -> bool {
    matches!(status, Status::Error { .. })
}

struct MonorepoParent {
    span_id: SpanId,
    display_name: String,
    init_time: SystemTime,
    min_start: Option<SystemTime>,
    max_end: Option<SystemTime>,
}

/// Derive a display name for a monorepo group span.
fn monorepo_group_display_name(
    config_root: &std::path::Path,
    project_root: Option<&PathBuf>,
) -> String {
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

/// Info returned when a task span is reserved, needed for log correlation
/// and later span emission.
#[derive(Clone)]
pub struct StartedSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    reserved_time: SystemTime,
    start_time: Arc<Mutex<Option<SystemTime>>>,
}

impl StartedSpan {
    pub fn mark_started(&self, start_time: SystemTime) {
        let mut guard = self.start_time.lock().unwrap();
        if guard.is_none() {
            *guard = Some(start_time);
        }
    }

    fn effective_start_time(&self) -> SystemTime {
        self.start_time
            .lock()
            .unwrap()
            .unwrap_or(self.reserved_time)
    }

    #[cfg(test)]
    pub fn for_test(trace_id: TraceId, span_id: SpanId) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            reserved_time: SystemTime::UNIX_EPOCH,
            start_time: Arc::new(Mutex::new(None)),
        }
    }
}

impl TraceContext {
    pub fn new(root_span_name: &str, provider: SdkTracerProvider) -> Self {
        Self::new_with_parent(root_span_name, id_gen().new_trace_id(), None, provider)
    }

    pub fn from_parent(
        root_span_name: &str,
        trace_id: TraceId,
        parent_span_id: SpanId,
        provider: SdkTracerProvider,
    ) -> Self {
        Self::new_with_parent(root_span_name, trace_id, Some(parent_span_id), provider)
    }

    fn new_with_parent(
        root_span_name: &str,
        trace_id: TraceId,
        parent_span_id: Option<SpanId>,
        tracer_provider: SdkTracerProvider,
    ) -> Self {
        let root_span_id = id_gen().new_span_id();
        let root_init_time = SystemTime::now();

        Self {
            inner: Arc::new(Mutex::new(TraceContextInner {
                trace_id,
                root_span: RootSpan {
                    parent_span_id,
                    span_id: root_span_id,
                    name: root_span_name.to_string(),
                    init_time: root_init_time,
                    min_start: None,
                    max_end: None,
                },
                monorepo_parents: HashMap::new(),
                monorepo_parent_by_span_id: HashMap::new(),
            })),
            tracer_provider,
        }
    }

    /// Get or create a monorepo parent span for a given config_root.
    fn get_or_create_monorepo_parent(
        &self,
        config_root: &PathBuf,
        project_root: Option<&PathBuf>,
    ) -> SpanId {
        let mut inner = self.inner.lock().unwrap();
        if let Some(parent) = inner.monorepo_parents.get(config_root) {
            return parent.span_id;
        }
        let span_id = id_gen().new_span_id();
        let display_name = monorepo_group_display_name(config_root, project_root);
        inner
            .monorepo_parent_by_span_id
            .insert(span_id, config_root.clone());
        inner.monorepo_parents.insert(
            config_root.clone(),
            MonorepoParent {
                span_id,
                display_name,
                init_time: SystemTime::now(),
                min_start: None,
                max_end: None,
            },
        );
        span_id
    }

    /// Fold a completed task into the root span's aggregate timing.
    fn fold_into_root(inner: &mut TraceContextInner, task_start: SystemTime, task_end: SystemTime) {
        inner.root_span.min_start = Some(match inner.root_span.min_start {
            Some(existing) if existing <= task_start => existing,
            _ => task_start,
        });
        inner.root_span.max_end = Some(match inner.root_span.max_end {
            Some(existing) if existing >= task_end => existing,
            _ => task_end,
        });
    }

    /// Fold a completed task into the matching monorepo group's aggregate timing.
    fn fold_into_group(
        inner: &mut TraceContextInner,
        parent_span_id: SpanId,
        task_start: SystemTime,
        task_end: SystemTime,
    ) {
        let Some(config_root) = inner
            .monorepo_parent_by_span_id
            .get(&parent_span_id)
            .cloned()
        else {
            return;
        };
        let Some(parent) = inner.monorepo_parents.get_mut(&config_root) else {
            return;
        };
        parent.min_start = Some(match parent.min_start {
            Some(existing) if existing <= task_start => existing,
            _ => task_start,
        });
        parent.max_end = Some(match parent.max_end {
            Some(existing) if existing >= task_end => existing,
            _ => task_end,
        });
    }

    /// Determine the parent span ID for a task.
    pub fn parent_span_for_task(
        &self,
        config_root: Option<&PathBuf>,
        project_root: Option<&PathBuf>,
    ) -> Option<SpanId> {
        let root_span_id = self.inner.lock().unwrap().root_span.span_id;
        if let Some(cr) = config_root {
            let is_monorepo = project_root.is_none_or(|pr| cr != pr);
            if is_monorepo {
                return Some(self.get_or_create_monorepo_parent(cr, project_root));
            }
        }
        Some(root_span_id)
    }

    /// Reserve IDs for a task span with an immediate start time.
    #[cfg(test)]
    pub fn start_task_span(&self, parent_span_id: Option<SpanId>) -> StartedSpan {
        self.start_task_span_at(parent_span_id, Some(SystemTime::now()))
    }

    /// Reserve IDs for a task span without committing to an execution start time yet.
    pub fn reserve_task_span(&self, parent_span_id: Option<SpanId>) -> StartedSpan {
        self.start_task_span_at(parent_span_id, None)
    }

    fn start_task_span_at(
        &self,
        parent_span_id: Option<SpanId>,
        start_time: Option<SystemTime>,
    ) -> StartedSpan {
        let trace_id = self.inner.lock().unwrap().trace_id;
        StartedSpan {
            trace_id,
            span_id: id_gen().new_span_id(),
            parent_span_id,
            reserved_time: SystemTime::now(),
            start_time: Arc::new(Mutex::new(start_time)),
        }
    }

    /// Emit a completed task span through the SDK pipeline.
    pub fn end_task_span(
        &self,
        started: StartedSpan,
        task_name: &str,
        end_time: SystemTime,
        status: Status,
        attributes: Vec<(String, String)>,
    ) {
        // Lock held through emit_span so finish()/shutdown() cannot
        // race between the aggregate update and the span export.
        // No deadlock risk: emit_span uses self.tracer_provider which
        // lives outside the Mutex.
        let mut inner = self.inner.lock().unwrap();
        if let Some(parent_span_id) = started.parent_span_id {
            Self::fold_into_group(
                &mut inner,
                parent_span_id,
                started.effective_start_time(),
                end_time,
            );
        }
        Self::fold_into_root(&mut inner, started.effective_start_time(), end_time);

        emit_span(
            &self.tracer_provider,
            EmitSpanParams {
                name: task_name.to_string(),
                trace_id: started.trace_id,
                span_id: started.span_id,
                parent_span_id: started.parent_span_id,
                start_time: started.effective_start_time(),
                end_time,
                status,
                attributes,
            },
        );
    }

    /// Emit the monorepo group spans and the root span, then shut down
    /// the tracer provider to flush pending spans.
    pub fn finish(&self, has_failures: bool) {
        // Hold the lock across both final span emission and provider
        // shutdown, so no concurrent end_task_span() can sneak an
        // emit_span() call in between.
        let mut inner = self.inner.lock().unwrap();
        self.emit_final_spans_locked(&mut inner, has_failures);
        let _ = self.tracer_provider.shutdown();
    }

    /// Emit the monorepo group spans and the root span with their
    /// aggregated duration / error state. Does **not** shut down the
    /// tracer provider — call `finish()` when you also want flushing.
    /// Test-only: emit final spans without shutting down the provider,
    /// so the in-memory exporter can still be inspected.
    #[cfg(test)]
    fn emit_final_spans(&self, has_failures: bool) {
        let mut inner = self.inner.lock().unwrap();
        self.emit_final_spans_locked(&mut inner, has_failures);
    }

    fn emit_final_spans_locked(&self, inner: &mut TraceContextInner, has_failures: bool) {
        let now = SystemTime::now();
        let trace_id = inner.trace_id;
        let root_span_id = inner.root_span.span_id;

        // Emit monorepo group spans.
        let groups: Vec<(PathBuf, MonorepoParent)> = inner.monorepo_parents.drain().collect();
        for (config_root, parent) in groups {
            emit_span(
                &self.tracer_provider,
                EmitSpanParams {
                    name: parent.display_name,
                    trace_id,
                    span_id: parent.span_id,
                    parent_span_id: Some(root_span_id),
                    start_time: parent.min_start.unwrap_or(parent.init_time),
                    end_time: parent.max_end.unwrap_or(now),
                    status: Status::Unset,
                    attributes: vec![
                        ("mise.span_type".to_string(), "monorepo_group".to_string()),
                        (
                            "mise.config_root".to_string(),
                            config_root.display().to_string(),
                        ),
                    ],
                },
            );
        }

        // Emit the root span — only mark error if the run itself failed.
        emit_span(
            &self.tracer_provider,
            EmitSpanParams {
                name: inner.root_span.name.clone(),
                trace_id,
                span_id: root_span_id,
                parent_span_id: inner.root_span.parent_span_id,
                start_time: inner
                    .root_span
                    .min_start
                    .unwrap_or(inner.root_span.init_time),
                end_time: inner.root_span.max_end.unwrap_or(now),
                status: status_from_error(has_failures),
                attributes: vec![("mise.span_type".to_string(), "run".to_string())],
            },
        );
    }
}

/// Parameters for `emit_span`.
struct EmitSpanParams {
    name: String,
    trace_id: TraceId,
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    start_time: SystemTime,
    end_time: SystemTime,
    status: Status,
    attributes: Vec<(String, String)>,
}

/// Create an SDK span with explicit IDs and timing, then end it immediately.
/// This causes the span to flow through the configured span processor.
fn emit_span(provider: &SdkTracerProvider, params: EmitSpanParams) {
    let tracer = provider.tracer("mise.tasks");

    // Build parent context if we have a parent span ID.
    let parent_cx = if let Some(psid) = params.parent_span_id {
        let parent_sc = SpanContext::new(
            params.trace_id,
            psid,
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        );
        opentelemetry::Context::new().with_remote_span_context(parent_sc)
    } else {
        opentelemetry::Context::new()
    };

    let mut builder = tracer.span_builder(Cow::Owned(params.name));
    builder.trace_id = Some(params.trace_id);
    builder.span_id = Some(params.span_id);
    builder.start_time = Some(params.start_time);
    builder.attributes = Some(
        params
            .attributes
            .into_iter()
            .map(|(k, v)| KeyValue::new(k, v))
            .collect(),
    );

    let mut span = tracer.build_with_context(builder, &parent_cx);
    span.set_status(params.status);
    span.end_with_timestamp(params.end_time);
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{SpanId, Status};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{
        InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor, SpanData, SpanExporter,
    };
    use std::fmt;
    use std::future;

    /// Build a test provider backed by an in-memory exporter so we can
    /// inspect emitted spans synchronously (no batching delay).
    fn test_provider() -> (SdkTracerProvider, InMemorySpanExporter) {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        (provider, exporter)
    }

    #[derive(Clone, Default)]
    struct RetainingSpanExporter {
        spans: Arc<Mutex<Vec<SpanData>>>,
    }

    impl fmt::Debug for RetainingSpanExporter {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

    fn span_by_name<'a>(spans: &'a [SpanData], name: &str) -> &'a SpanData {
        spans
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing span '{name}'"))
    }

    fn has_attr(span: &SpanData, key: &str, value: &str) -> bool {
        span.attributes
            .iter()
            .any(|kv| kv.key.as_str() == key && kv.value.as_str() == value)
    }

    #[test]
    fn finish_builds_root_and_monorepo_hierarchy() {
        let (provider, exporter) = test_provider();
        let trace = TraceContext::new("mise run", provider);
        let project_root = PathBuf::from("/workspace");
        let monorepo_root = PathBuf::from("/workspace/packages/frontend");

        // Direct task (config_root == project_root → child of root).
        let direct_parent = trace.parent_span_for_task(Some(&project_root), Some(&project_root));
        let direct_task = trace.start_task_span(direct_parent);
        trace.end_task_span(direct_task, "lint", SystemTime::now(), Status::Ok, vec![]);

        // Monorepo task (config_root != project_root → child of group span).
        let monorepo_parent = trace.parent_span_for_task(Some(&monorepo_root), Some(&project_root));
        let monorepo_task = trace.start_task_span(monorepo_parent);
        trace.end_task_span(
            monorepo_task,
            "build",
            SystemTime::now(),
            Status::Ok,
            vec![],
        );

        trace.emit_final_spans(false);

        let spans = exporter.get_finished_spans().unwrap();
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

        // Direct task is child of root.
        assert_eq!(lint.parent_span_id, root.span_context.span_id());

        // Monorepo task is child of group.
        assert_eq!(build.parent_span_id, group.span_context.span_id());
    }

    #[test]
    fn finish_has_failures_does_not_taint_ok_group() {
        let (provider, exporter) = test_provider();
        let trace = TraceContext::new("mise run", provider);
        let project_root = PathBuf::from("/workspace");
        let monorepo_root = PathBuf::from("/workspace/packages/frontend");

        let parent_span = trace.parent_span_for_task(Some(&monorepo_root), Some(&project_root));
        let task = trace.start_task_span(parent_span);
        trace.end_task_span(task, "build", SystemTime::now(), Status::Ok, vec![]);

        // Scheduler-level failure (e.g. ctrl-c) should mark root as
        // errored, but a group whose own tasks all succeeded stays OK.
        trace.emit_final_spans(true);

        let spans = exporter.get_finished_spans().unwrap();
        let root = span_by_name(&spans, "mise run");
        let group = span_by_name(&spans, "packages/frontend");
        let build = span_by_name(&spans, "build");

        assert_eq!(build.status, Status::Ok);
        assert_eq!(group.status, Status::Unset);
        assert!(is_error(&root.status));
    }

    #[test]
    fn finish_group_stays_unset_even_when_child_fails() {
        let (provider, exporter) = test_provider();
        let trace = TraceContext::new("mise run", provider);
        let project_root = PathBuf::from("/workspace");
        let monorepo_root = PathBuf::from("/workspace/packages/frontend");

        let parent_span = trace.parent_span_for_task(Some(&monorepo_root), Some(&project_root));
        let task = trace.start_task_span(parent_span);
        trace.end_task_span(
            task,
            "build",
            SystemTime::now(),
            Status::Error {
                description: Cow::Borrowed("boom"),
            },
            vec![],
        );

        trace.emit_final_spans(true);

        let spans = exporter.get_finished_spans().unwrap();
        let root = span_by_name(&spans, "mise run");
        let group = span_by_name(&spans, "packages/frontend");
        let build = span_by_name(&spans, "build");

        assert!(is_error(&build.status));
        assert_eq!(group.status, Status::Unset);
        assert!(is_error(&root.status));
    }

    #[test]
    fn task_without_config_root_is_direct_child_of_root() {
        let (provider, exporter) = test_provider();
        let trace = TraceContext::new("mise run", provider);

        let parent = trace.parent_span_for_task(None, None);
        let started = trace.start_task_span(parent);
        trace.end_task_span(started, "lint", SystemTime::now(), Status::Ok, vec![]);

        trace.emit_final_spans(false);

        let spans = exporter.get_finished_spans().unwrap();
        let root = span_by_name(&spans, "mise run");
        let lint = span_by_name(&spans, "lint");

        assert_eq!(lint.parent_span_id, root.span_context.span_id());
        // No monorepo groups were created.
        assert!(
            !spans
                .iter()
                .any(|s| has_attr(s, "mise.span_type", "monorepo_group")),
            "unexpected monorepo group span in non-monorepo run"
        );
    }

    #[test]
    fn finish_keeps_parent_span_for_nested_run() {
        let (provider, exporter) = test_provider();
        let parent_trace_id = TraceId::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ]);
        let parent_span_id = SpanId::from_bytes([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);

        let trace =
            TraceContext::from_parent("mise run nested", parent_trace_id, parent_span_id, provider);

        trace.emit_final_spans(false);

        let spans = exporter.get_finished_spans().unwrap();
        let root = span_by_name(&spans, "mise run nested");

        assert_eq!(root.span_context.trace_id(), parent_trace_id);
        assert_eq!(root.parent_span_id, parent_span_id);
    }

    #[test]
    fn finish_flushes_final_spans_via_provider_shutdown() {
        let exporter = RetainingSpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let trace = TraceContext::new("mise run", provider);
        let project_root = PathBuf::from("/workspace");
        let monorepo_root = PathBuf::from("/workspace/packages/frontend");

        let direct_parent = trace.parent_span_for_task(Some(&project_root), Some(&project_root));
        let direct_task = trace.start_task_span(direct_parent);
        trace.end_task_span(direct_task, "lint", SystemTime::now(), Status::Ok, vec![]);

        let monorepo_parent = trace.parent_span_for_task(Some(&monorepo_root), Some(&project_root));
        let monorepo_task = trace.start_task_span(monorepo_parent);
        trace.end_task_span(
            monorepo_task,
            "build",
            SystemTime::now(),
            Status::Ok,
            vec![],
        );

        trace.finish(false);

        let spans = exporter.finished_spans();
        assert_eq!(spans.len(), 4, "expected root + group + 2 task spans");
        assert!(spans.iter().any(|s| s.name == "mise run"));
        assert!(spans.iter().any(|s| s.name == "packages/frontend"));
        assert!(spans.iter().any(|s| s.name == "lint"));
        assert!(spans.iter().any(|s| s.name == "build"));
    }

    #[test]
    fn monorepo_group_display_name_uses_relative_path() {
        let name = monorepo_group_display_name(
            &PathBuf::from("/workspace/packages/frontend"),
            Some(&PathBuf::from("/workspace")),
        );
        assert_eq!(name, "packages/frontend");
    }

    #[test]
    fn monorepo_group_display_name_falls_back_to_leaf() {
        let name = monorepo_group_display_name(
            &PathBuf::from("/other/frontend"),
            Some(&PathBuf::from("/workspace")),
        );
        assert_eq!(name, "frontend");
    }

    #[test]
    fn monorepo_group_display_name_no_project_root() {
        let name =
            monorepo_group_display_name(&PathBuf::from("/workspace/packages/frontend"), None);
        assert_eq!(name, "frontend");
    }
}
