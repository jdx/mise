use crate::otel::trace_context::StartedSpan;
use opentelemetry::logs::{LogRecord as _, Logger, Severity};
use opentelemetry::trace::{SpanId, TraceFlags, TraceId};
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::cmd::CmdLineRunner;

/// Collects task output lines and emits them as OTLP log records via the SDK.
///
/// Uses the SDK's `SdkLogger` which delegates to `BatchLogProcessor` for
/// batching and export. No background task needed — the SDK manages it.
#[derive(Clone)]
pub struct OtelLogCollector {
    logger: Arc<SdkLogger>,
    logger_provider: Arc<Mutex<Option<SdkLoggerProvider>>>,
    is_shutdown: Arc<AtomicBool>,
}

impl OtelLogCollector {
    /// Create a new log collector backed by the given logger provider.
    pub fn new(provider: SdkLoggerProvider) -> Self {
        use opentelemetry::logs::LoggerProvider;
        let logger = provider.logger("mise.tasks");
        Self {
            logger: Arc::new(logger),
            logger_provider: Arc::new(Mutex::new(Some(provider))),
            is_shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shut down the collector by shutting down the logger provider,
    /// which flushes all pending log batches.
    pub fn shutdown(&self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
        if let Some(provider) = self.logger_provider.lock().unwrap().take() {
            let _ = provider.shutdown();
        }
    }

    /// Create a hook closure bound to a specific task's trace/span context.
    fn hook(
        &self,
        task_name: Arc<str>,
        task_args: Arc<str>,
        trace_id: TraceId,
        span_id: SpanId,
        is_stderr: bool,
    ) -> impl Fn(String) + Send + 'static {
        let logger = Arc::clone(&self.logger);
        let is_shutdown = Arc::clone(&self.is_shutdown);
        move |line: String| {
            if is_shutdown.load(Ordering::Relaxed) {
                return;
            }
            // Progress bars use \r to overwrite themselves; when piped,
            // all frames arrive as one concatenated line. Keep only the
            // last \r-delimited segment (the final state).
            let line = match line.rfind('\r') {
                Some(pos) => &line[pos + 1..],
                None => line.as_str(),
            };
            if line.is_empty() {
                return;
            }
            let now = SystemTime::now();
            let mut record = logger.create_log_record();
            record.set_timestamp(now);
            record.set_observed_timestamp(now);
            record.set_severity_number(if is_stderr {
                Severity::Warn
            } else {
                Severity::Info
            });
            record.set_severity_text(if is_stderr { "WARN" } else { "INFO" });
            record.set_body(opentelemetry::logs::AnyValue::String(
                line.to_string().into(),
            ));
            record.set_trace_context(trace_id, span_id, Some(TraceFlags::SAMPLED));
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
    /// the collector or the trace context is absent.
    pub fn attach_hooks<'a>(
        collector: Option<&Self>,
        task_name: &str,
        task_args: &[String],
        started: Option<&StartedSpan>,
        mut cmd: CmdLineRunner<'a>,
    ) -> CmdLineRunner<'a> {
        let (Some(collector), Some(started)) = (collector, started) else {
            return cmd;
        };
        let task_name: Arc<str> = Arc::from(task_name);
        let task_args: Arc<str> = Arc::from(task_args.join(" "));
        cmd = cmd.with_stdout_hook(collector.hook(
            Arc::clone(&task_name),
            Arc::clone(&task_args),
            started.trace_id,
            started.span_id,
            false,
        ));
        cmd = cmd.with_stderr_hook(collector.hook(
            task_name,
            task_args,
            started.trace_id,
            started.span_id,
            true,
        ));
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
    fn attach_hooks_is_noop_without_collector_or_context() {
        let ctx = StartedSpan::for_test(TraceId::from_bytes([1; 16]), SpanId::from_bytes([1; 8]));
        // No collector — cmd passes through unchanged
        let cmd = CmdLineRunner::new("true");
        let cmd = OtelLogCollector::attach_hooks(None, "build", &[], Some(&ctx), cmd);
        assert!(!cmd.has_stdout_hooks());

        // No context — cmd passes through unchanged
        let collector = OtelLogCollector::new(noop_provider());
        let cmd = CmdLineRunner::new("true");
        let cmd = OtelLogCollector::attach_hooks(Some(&collector), "build", &[], None, cmd);
        assert!(!cmd.has_stdout_hooks());
        collector.shutdown();
    }

    #[test]
    fn attach_hooks_registers_both_streams() {
        let collector = OtelLogCollector::new(noop_provider());
        let ctx = StartedSpan::for_test(TraceId::from_bytes([1; 16]), SpanId::from_bytes([1; 8]));
        let cmd = CmdLineRunner::new("true");
        let cmd = OtelLogCollector::attach_hooks(Some(&collector), "build", &[], Some(&ctx), cmd);
        assert!(cmd.has_stdout_hooks());
        collector.shutdown();
    }

    #[test]
    fn emitted_logs_include_expected_metadata_and_trace_context() {
        let (provider, exporter) = test_provider();
        let collector = OtelLogCollector::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let stdout_hook = collector.hook(
            Arc::from("build"),
            Arc::from("--release"),
            trace_id,
            span_id,
            false,
        );
        let stderr_hook = collector.hook(
            Arc::from("build"),
            Arc::from("--release"),
            trace_id,
            span_id,
            true,
        );
        stdout_hook("hello".to_string());
        stderr_hook("boom".to_string());
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
        let collector = OtelLogCollector::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let hook = collector.hook(Arc::from("build"), Arc::from(""), trace_id, span_id, false);
        // Simulate a progress bar line with \r-separated frames
        hook("10% done\r50% done\r100% done".to_string());
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
        let collector = OtelLogCollector::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let hook = collector.hook(Arc::from("build"), Arc::from(""), trace_id, span_id, false);
        // Line ending with \r produces empty string after split
        hook("progress\r".to_string());
        collector.shutdown();

        assert!(exporter.emitted().is_empty());
    }

    #[test]
    fn hooks_become_noop_after_shutdown() {
        let (provider, exporter) = test_provider();
        let collector = OtelLogCollector::new(provider);
        let trace_id = TraceId::from_bytes([0x11; 16]);
        let span_id = SpanId::from_bytes([0x22; 8]);

        let stdout_hook =
            collector.hook(Arc::from("build"), Arc::from(""), trace_id, span_id, false);
        collector.shutdown();
        stdout_hook("late".to_string());

        assert!(exporter.emitted().is_empty());
    }
}
