use crate::otel::LogClaimWatcher;
use opentelemetry::logs::{LogRecord as _, Logger, Severity};
use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId};
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::cmd::CmdLineRunner;

/// Everything a stdout/stderr hook needs about the task it belongs to.
///
/// `claim`, when present, lets a nested `mise run` take over reporting for
/// this stream — see [`crate::otel::log_claim`].
#[derive(Clone)]
struct HookContext {
    task_name: Arc<str>,
    task_args: Arc<str>,
    trace_id: TraceId,
    span_id: SpanId,
    trace_flags: TraceFlags,
    claim: Option<LogClaimWatcher>,
}

/// Forwards a task's stdout/stderr lines to the OTEL log pipeline.
///
/// This bridges mise's `CmdLineRunner` line callbacks to the SDK's
/// `SdkLogger`, which delegates to `BatchLogProcessor` for batching and
/// export. No background task needed — the SDK manages it.
#[derive(Clone)]
pub struct TaskOutputForwarder {
    logger: Arc<SdkLogger>,
    logger_provider: Arc<Mutex<Option<SdkLoggerProvider>>>,
    is_shutdown: Arc<AtomicBool>,
}

impl TaskOutputForwarder {
    /// Create a new output forwarder backed by the given logger provider.
    pub fn new(provider: SdkLoggerProvider) -> Self {
        use opentelemetry::logs::LoggerProvider;
        let logger = provider.logger("mise.tasks");
        Self {
            logger: Arc::new(logger),
            logger_provider: Arc::new(Mutex::new(Some(provider))),
            is_shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shut down the forwarder by shutting down the logger provider,
    /// which flushes all pending log batches.
    pub fn shutdown(&self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
        if let Some(provider) = self.logger_provider.lock().unwrap().take() {
            let _ = provider.shutdown();
        }
    }

    /// Create a hook closure bound to a specific task's trace/span context.
    ///
    /// Lines are still printed to the terminal regardless; only the OTLP
    /// export is skipped when a nested run holds the claim.
    fn hook(&self, cx: HookContext, is_stderr: bool) -> impl Fn(&str) + Send + 'static {
        let HookContext {
            task_name,
            task_args,
            trace_id,
            span_id,
            trace_flags,
            claim,
        } = cx;
        let logger = Arc::clone(&self.logger);
        let is_shutdown = Arc::clone(&self.is_shutdown);
        move |line: &str| {
            if is_shutdown.load(Ordering::Relaxed) {
                return;
            }
            // A nested `mise run` is reporting this stream against its own
            // task spans; re-exporting here would duplicate every line.
            if claim.as_ref().is_some_and(|c| c.claimed()) {
                return;
            }
            // Progress bars use \r to overwrite themselves; when piped,
            // all frames arrive as one concatenated line. Keep only the
            // last \r-delimited segment (the final state).
            let line = match line.rfind('\r') {
                Some(pos) => &line[pos + 1..],
                None => line,
            };
            if line.is_empty() {
                return;
            }
            let now = SystemTime::now();
            let mut record = logger.create_log_record();
            record.set_timestamp(now);
            record.set_observed_timestamp(now);
            // stderr is mapped to WARN rather than ERROR: many tools write
            // diagnostics, progress, and compiler warnings to stderr that
            // are not actual errors. Real failure is conveyed by the task
            // span status / `process.exit.code` attribute.
            record.set_severity_number(if is_stderr {
                Severity::Warn
            } else {
                Severity::Info
            });
            record.set_severity_text(if is_stderr { "WARN" } else { "INFO" });
            record.set_body(opentelemetry::logs::AnyValue::String(
                line.to_string().into(),
            ));
            record.set_trace_context(trace_id, span_id, Some(trace_flags));
            record.add_attribute("mise.task.name", task_name.to_string());
            if !task_args.is_empty() {
                record.add_attribute("mise.task.args", task_args.to_string());
            }
            record.add_attribute("output.stream", if is_stderr { "stderr" } else { "stdout" });
            logger.emit(record);
        }
    }

    /// Attach stdout/stderr hooks to a `CmdLineRunner` that forward each
    /// line to the OTLP log exporter. Returns the cmd unchanged when either
    /// the forwarder or the trace context is absent.
    ///
    /// Only call this when the output is actually captured — under `--raw` or
    /// a fully silenced task the hooks never fire, and the claim path handed
    /// to the child would then tell a nested `mise run` to take over a stream
    /// nobody is reading.
    pub fn attach_hooks<'a>(
        forwarder: Option<&Self>,
        task_name: &str,
        task_args: &[String],
        span_cx: Option<&SpanContext>,
        claim: Option<LogClaimWatcher>,
        mut cmd: CmdLineRunner<'a>,
    ) -> CmdLineRunner<'a> {
        let (Some(forwarder), Some(span_cx)) = (forwarder, span_cx) else {
            return cmd;
        };
        // Offer the stream to any nested `mise run` in this task.
        if let Some(claim) = &claim {
            cmd = cmd.env(crate::otel::LOG_CLAIM_ENV, claim.path());
        }
        let cx = HookContext {
            task_name: Arc::from(task_name),
            task_args: Arc::from(task_args.join(" ")),
            trace_id: span_cx.trace_id(),
            span_id: span_cx.span_id(),
            trace_flags: span_cx.trace_flags(),
            claim,
        };
        cmd = cmd.with_stdout_observer(forwarder.hook(cx.clone(), false));
        cmd = cmd.with_stderr_observer(forwarder.hook(cx, true));
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::InstrumentationScope;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLogRecord};
    use std::fmt;
    use std::future;

    fn test_span_context() -> SpanContext {
        SpanContext::new(
            TraceId::from_bytes([1; 16]),
            SpanId::from_bytes([1; 8]),
            TraceFlags::SAMPLED,
            false,
            opentelemetry::trace::TraceState::default(),
        )
    }

    fn noop_provider() -> SdkLoggerProvider {
        SdkLoggerProvider::builder().build()
    }

    #[derive(Clone, Default)]
    struct RetainingLogExporter {
        logs: Arc<Mutex<Vec<(SdkLogRecord, InstrumentationScope)>>>,
    }

    impl fmt::Debug for RetainingLogExporter {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("RetainingLogExporter").finish()
        }
    }

    impl LogExporter for RetainingLogExporter {
        fn export(
            &self,
            batch: LogBatch<'_>,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            let mut logs = self.logs.lock().unwrap();
            for (record, scope) in batch.iter() {
                logs.push((record.clone(), scope.clone()));
            }
            future::ready(Ok(()))
        }
    }

    impl RetainingLogExporter {
        fn emitted(&self) -> Vec<(SdkLogRecord, InstrumentationScope)> {
            self.logs.lock().unwrap().clone()
        }
    }

    fn test_provider() -> (SdkLoggerProvider, RetainingLogExporter) {
        let exporter = RetainingLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        (provider, exporter)
    }

    #[test]
    fn attach_hooks_is_noop_without_forwarder_or_context() {
        let ctx = test_span_context();
        // No forwarder — cmd passes through unchanged
        let cmd = CmdLineRunner::new("true");
        let cmd = TaskOutputForwarder::attach_hooks(None, "build", &[], Some(&ctx), None, cmd);
        assert!(!cmd.has_stdout_observer());

        // No context — cmd passes through unchanged
        let forwarder = TaskOutputForwarder::new(noop_provider());
        let cmd = CmdLineRunner::new("true");
        let cmd =
            TaskOutputForwarder::attach_hooks(Some(&forwarder), "build", &[], None, None, cmd);
        assert!(!cmd.has_stdout_observer());
        forwarder.shutdown();
    }

    #[test]
    fn attach_hooks_registers_both_streams() {
        let collector = TaskOutputForwarder::new(noop_provider());
        let ctx = test_span_context();
        let cmd = CmdLineRunner::new("true");
        let cmd = TaskOutputForwarder::attach_hooks(
            Some(&collector),
            "build",
            &[],
            Some(&ctx),
            None,
            cmd,
        );
        assert!(cmd.has_stdout_observer());
        collector.shutdown();
    }

    #[test]
    fn attach_hooks_offers_the_claim_path_to_the_child() {
        let collector = TaskOutputForwarder::new(noop_provider());
        let ctx = test_span_context();
        let dir = tempfile::tempdir().unwrap();
        let claim = LogClaimWatcher::new(dir.path().join("claim"));
        let cmd = TaskOutputForwarder::attach_hooks(
            Some(&collector),
            "build",
            &[],
            Some(&ctx),
            Some(claim.clone()),
            CmdLineRunner::new("true"),
        );
        assert_eq!(
            cmd.get_env(crate::otel::LOG_CLAIM_ENV),
            Some(claim.path().as_os_str()),
            "nested mise needs the claim path to take over reporting"
        );
        collector.shutdown();
    }

    #[test]
    fn hook_skips_export_while_a_nested_run_holds_the_claim() {
        let (provider, exporter) = test_provider();
        let collector = TaskOutputForwarder::new(provider);
        let dir = tempfile::tempdir().unwrap();
        let claim = LogClaimWatcher::new(dir.path().join("claim"));
        let hook = collector.hook(
            HookContext {
                task_name: Arc::from("outer"),
                task_args: Arc::from(""),
                trace_id: TraceId::from_bytes([0x11; 16]),
                span_id: SpanId::from_bytes([0x22; 8]),
                trace_flags: TraceFlags::SAMPLED,
                claim: Some(claim.clone()),
            },
            false,
        );

        // The outer task's own output is ours to report.
        hook("building");
        // A nested `mise run` takes over...
        std::fs::write(claim.path(), std::process::id().to_string()).unwrap();
        hook("relayed from inner");
        // ...and hands the stream back when it exits.
        std::fs::remove_file(claim.path()).unwrap();
        hook("done");
        collector.shutdown();

        let bodies: Vec<String> = exporter
            .emitted()
            .iter()
            .filter_map(|(record, _)| match record.body() {
                Some(AnyValue::String(s)) => Some(s.as_str().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(bodies, vec!["building", "done"]);
    }

    #[test]
    fn emitted_logs_include_expected_metadata_and_trace_context() {
        let (provider, exporter) = test_provider();
        let collector = TaskOutputForwarder::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let stdout_hook = collector.hook(
            HookContext {
                task_name: Arc::from("build"),
                task_args: Arc::from("--release"),
                trace_id,
                span_id,
                trace_flags: TraceFlags::SAMPLED,
                claim: None,
            },
            false,
        );
        let stderr_hook = collector.hook(
            HookContext {
                task_name: Arc::from("build"),
                task_args: Arc::from("--release"),
                trace_id,
                span_id,
                trace_flags: TraceFlags::SAMPLED,
                claim: None,
            },
            true,
        );
        stdout_hook("hello");
        stderr_hook("boom");
        collector.shutdown();

        let emitted = exporter.emitted();
        assert_eq!(emitted.len(), 2);

        let stdout = emitted
            .iter()
            .find(|(record, _)| matches!(record.body(), Some(AnyValue::String(s)) if s.as_str() == "hello"))
            .expect("missing stdout log");
        let stderr = emitted
            .iter()
            .find(|(record, _)| matches!(record.body(), Some(AnyValue::String(s)) if s.as_str() == "boom"))
            .expect("missing stderr log");

        assert_eq!(stdout.0.severity_text(), Some("INFO"));
        assert_eq!(stderr.0.severity_text(), Some("WARN"));
        assert_eq!(
            stdout.0.trace_context().map(|cx| cx.trace_id),
            Some(trace_id)
        );
        assert_eq!(stdout.0.trace_context().map(|cx| cx.span_id), Some(span_id));
        assert_eq!(
            stderr.0.trace_context().map(|cx| cx.trace_id),
            Some(trace_id)
        );
        assert_eq!(stderr.0.trace_context().map(|cx| cx.span_id), Some(span_id));

        let stdout_attrs: Vec<_> = stdout.0.attributes_iter().collect();
        let stderr_attrs: Vec<_> = stderr.0.attributes_iter().collect();
        assert!(
            stdout_attrs
                .iter()
                .any(|(k, v)| k.as_str() == "mise.task.name"
                    && matches!(v, AnyValue::String(s) if s.as_str() == "build"))
        );
        assert!(
            stdout_attrs
                .iter()
                .any(|(k, v)| k.as_str() == "output.stream"
                    && matches!(v, AnyValue::String(s) if s.as_str() == "stdout"))
        );
        assert!(
            stderr_attrs
                .iter()
                .any(|(k, v)| k.as_str() == "output.stream"
                    && matches!(v, AnyValue::String(s) if s.as_str() == "stderr"))
        );
        assert!(
            stdout_attrs
                .iter()
                .any(|(k, v)| k.as_str() == "mise.task.args"
                    && matches!(v, AnyValue::String(s) if s.as_str() == "--release"))
        );
    }

    #[test]
    fn hook_strips_cr_progress_bar_frames() {
        let (provider, exporter) = test_provider();
        let collector = TaskOutputForwarder::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let hook = collector.hook(
            HookContext {
                task_name: Arc::from("build"),
                task_args: Arc::from(""),
                trace_id,
                span_id,
                trace_flags: TraceFlags::SAMPLED,
                claim: None,
            },
            false,
        );
        // Simulate a progress bar line with \r-separated frames
        hook("10% done\r50% done\r100% done");
        collector.shutdown();

        let emitted = exporter.emitted();
        assert_eq!(emitted.len(), 1);
        let body = emitted[0].0.body().unwrap();
        assert!(
            matches!(body, AnyValue::String(s) if s.as_str() == "100% done"),
            "expected only last \\r segment, got: {body:?}"
        );
    }

    #[test]
    fn hook_skips_empty_line_after_cr_strip() {
        let (provider, exporter) = test_provider();
        let collector = TaskOutputForwarder::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let hook = collector.hook(
            HookContext {
                task_name: Arc::from("build"),
                task_args: Arc::from(""),
                trace_id,
                span_id,
                trace_flags: TraceFlags::SAMPLED,
                claim: None,
            },
            false,
        );
        // Line ending with \r produces empty string after split
        hook("progress\r");
        collector.shutdown();

        assert!(exporter.emitted().is_empty());
    }

    #[test]
    fn hooks_become_noop_after_shutdown() {
        let (provider, exporter) = test_provider();
        let collector = TaskOutputForwarder::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let stdout_hook = collector.hook(
            HookContext {
                task_name: Arc::from("build"),
                task_args: Arc::from(""),
                trace_id,
                span_id,
                trace_flags: TraceFlags::SAMPLED,
                claim: None,
            },
            false,
        );
        collector.shutdown();
        stdout_hook("late");

        assert!(exporter.emitted().is_empty());
    }
}
