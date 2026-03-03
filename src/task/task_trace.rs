use crate::task::Task;
use crate::ui::style;
use crate::{errors::Error, file::display_path};
use eyre::Report;
use regex::Regex;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone)]
struct TaskTraceEvent {
    _at_ms: u128,
    _label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTraceFrame {
    pub task_name: String,
    pub source: Option<PathBuf>,
    pub line: Option<usize>,
    pub why: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTraceStage {
    SpawnReceived,
    SchedulerAdmissionPrepared {
        class: String,
        seq: u64,
        owner: Option<u64>,
        stopping: bool,
        runnable_post_dep: bool,
    },
    SchedulerAdmissionWait,
    SchedulerAdmissionError,
    SchedulerAdmissionDrop,
    SchedulerAdmissionStart,
    SchedulerAdmissionPostStopDrop,
    SchedulerExecutionStart {
        owner: Option<u64>,
    },
    SchedulerExecutionOk,
    SchedulerExecutionError,
    SchedulerCompletionFinalize,
    SchedulerCompletionDone,
    SchedulerAdmissionFailed,
    SchedulerTaskExecutionFailed,
    ExecutorEntry,
    ExecutorSkipTaskSkip,
    ExecutorSourcesCheck,
    ExecutorSkipSourcesFresh,
    ExecutorToolsCollect,
    ExecutorToolsParse,
    ExecutorToolsetBuildStart,
    ExecutorToolsetBuild,
    ExecutorToolsetBuildOk,
    ExecutorEnvRenderStart,
    ExecutorEnvRender,
    ExecutorEnvRenderOk,
    ExecutorTaskFile,
    ExecutorFilePath,
    ExecutorExecFileStart,
    ExecutorExecFile,
    ExecutorExecFileOk,
    ExecutorRunEntriesPrepare,
    ExecutorRunEntriesRender,
    ExecutorUsageParse,
    ExecutorUsage,
    ExecutorConfirm,
    ExecutorRunEntriesExec,
    ExecutorRunEntriesOk,
    ExecutorChecksumSave,
    ExecutorChecksum,
    ExecutorDone,
    ExecutorRunEntriesStart,
    ExecutorRunEntryScript {
        index: usize,
    },
    ExecutorRunEntryScriptExec,
    ExecutorRunEntrySingle {
        index: usize,
    },
    ExecutorRunEntrySingleExec,
    ExecutorRunEntryGroup {
        index: usize,
    },
    ExecutorRunEntryGroupExec,
    ExecutorRunEntriesDone,
    ExecutorInjectStart {
        specs: String,
    },
    ExecutorInjectLoad,
    ExecutorInjectMatch,
    ExecutorInjectDeps,
    ExecutorInjectStopRequested,
    ExecutorInjectDoneSignal,
    ExecutorInjectDoneChannel,
    ExecutorInjectFinalStop,
    ExecutorInjectOk,
    ExecutorExecScriptStart,
    ExecutorExecScriptConfig,
    ExecutorExecScriptShebang,
    ExecutorExecScriptInline,
    ExecutorExecFilePrepare,
    ExecutorExecFileUsage,
    ExecutorExecFileConfirm,
    ExecutorExecRetryEtxtbusy {
        attempt: usize,
    },
    ExecutorExecProgramStart {
        program: String,
    },
    ExecutorExecProgramConfig,
    ExecutorExecProgramCwd,
    ExecutorExecProgramDryRun,
    ExecutorExecProgramExecute,
    ExecutorExecProgramOk,
}

impl Display for TaskTraceStage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use TaskTraceStage as S;
        match self {
            S::SpawnReceived => write!(f, "spawn.received"),
            S::SchedulerAdmissionPrepared {
                class,
                seq,
                owner,
                stopping,
                runnable_post_dep,
            } => write!(
                f,
                "admission.prepared class={class} seq={seq} owner={owner:?} stopping={stopping} runnable_post_dep={runnable_post_dep}"
            ),
            S::SchedulerAdmissionWait => write!(f, "admission.wait"),
            S::SchedulerAdmissionError => write!(f, "admission.error"),
            S::SchedulerAdmissionDrop => write!(f, "admission.drop"),
            S::SchedulerAdmissionStart => write!(f, "admission.start"),
            S::SchedulerAdmissionPostStopDrop => write!(f, "admission.post_stop_drop"),
            S::SchedulerExecutionStart { owner } => {
                write!(f, "execution.start owner={owner:?}")
            }
            S::SchedulerExecutionOk => write!(f, "execution.ok"),
            S::SchedulerExecutionError => write!(f, "execution.error"),
            S::SchedulerCompletionFinalize => write!(f, "completion.finalize"),
            S::SchedulerCompletionDone => write!(f, "completion.done"),
            S::SchedulerAdmissionFailed => write!(f, "scheduler.admission_failed"),
            S::SchedulerTaskExecutionFailed => write!(f, "scheduler.task_execution_failed"),
            S::ExecutorEntry => write!(f, "executor.entry"),
            S::ExecutorSkipTaskSkip => write!(f, "executor.skip.task_skip"),
            S::ExecutorSourcesCheck => write!(f, "executor.sources.check"),
            S::ExecutorSkipSourcesFresh => write!(f, "executor.skip.sources_fresh"),
            S::ExecutorToolsCollect => write!(f, "executor.tools.collect"),
            S::ExecutorToolsParse => write!(f, "executor.tools.parse"),
            S::ExecutorToolsetBuildStart => write!(f, "executor.toolset.build.start"),
            S::ExecutorToolsetBuild => write!(f, "executor.toolset.build"),
            S::ExecutorToolsetBuildOk => write!(f, "executor.toolset.build.ok"),
            S::ExecutorEnvRenderStart => write!(f, "executor.env.render.start"),
            S::ExecutorEnvRender => write!(f, "executor.env.render"),
            S::ExecutorEnvRenderOk => write!(f, "executor.env.render.ok"),
            S::ExecutorTaskFile => write!(f, "executor.task_file"),
            S::ExecutorFilePath => write!(f, "executor.file_path"),
            S::ExecutorExecFileStart => write!(f, "executor.exec_file.start"),
            S::ExecutorExecFile => write!(f, "executor.exec_file"),
            S::ExecutorExecFileOk => write!(f, "executor.exec_file.ok"),
            S::ExecutorRunEntriesPrepare => write!(f, "executor.run_entries.prepare"),
            S::ExecutorRunEntriesRender => write!(f, "executor.run_entries.render"),
            S::ExecutorUsageParse => write!(f, "executor.usage.parse"),
            S::ExecutorUsage => write!(f, "executor.usage"),
            S::ExecutorConfirm => write!(f, "executor.confirm"),
            S::ExecutorRunEntriesExec => write!(f, "executor.run_entries.exec"),
            S::ExecutorRunEntriesOk => write!(f, "executor.run_entries.ok"),
            S::ExecutorChecksumSave => write!(f, "executor.checksum.save"),
            S::ExecutorChecksum => write!(f, "executor.checksum"),
            S::ExecutorDone => write!(f, "executor.done"),
            S::ExecutorRunEntriesStart => write!(f, "executor.run_entries.start"),
            S::ExecutorRunEntryScript { index } => {
                write!(f, "executor.run_entry[{index}].script")
            }
            S::ExecutorRunEntryScriptExec => write!(f, "executor.run_entry.script"),
            S::ExecutorRunEntrySingle { index } => {
                write!(f, "executor.run_entry[{index}].single")
            }
            S::ExecutorRunEntrySingleExec => write!(f, "executor.run_entry.single"),
            S::ExecutorRunEntryGroup { index } => {
                write!(f, "executor.run_entry[{index}].group")
            }
            S::ExecutorRunEntryGroupExec => write!(f, "executor.run_entry.group"),
            S::ExecutorRunEntriesDone => write!(f, "executor.run_entries.done"),
            S::ExecutorInjectStart { specs } => {
                write!(f, "executor.inject.start specs={specs}")
            }
            S::ExecutorInjectLoad => write!(f, "executor.inject.load"),
            S::ExecutorInjectMatch => write!(f, "executor.inject.match"),
            S::ExecutorInjectDeps => write!(f, "executor.inject.deps"),
            S::ExecutorInjectStopRequested => write!(f, "executor.inject.stop_requested"),
            S::ExecutorInjectDoneSignal => write!(f, "executor.inject.done_signal"),
            S::ExecutorInjectDoneChannel => write!(f, "executor.inject.done_channel"),
            S::ExecutorInjectFinalStop => write!(f, "executor.inject.final_stop"),
            S::ExecutorInjectOk => write!(f, "executor.inject.ok"),
            S::ExecutorExecScriptStart => write!(f, "executor.exec_script.start"),
            S::ExecutorExecScriptConfig => write!(f, "executor.exec_script.config"),
            S::ExecutorExecScriptShebang => write!(f, "executor.exec_script.shebang"),
            S::ExecutorExecScriptInline => write!(f, "executor.exec_script.inline"),
            S::ExecutorExecFilePrepare => write!(f, "executor.exec_file.prepare"),
            S::ExecutorExecFileUsage => write!(f, "executor.exec_file.usage"),
            S::ExecutorExecFileConfirm => write!(f, "executor.exec_file.confirm"),
            S::ExecutorExecRetryEtxtbusy { attempt } => {
                write!(f, "executor.exec.retry_etxtbusy attempt={attempt}")
            }
            S::ExecutorExecProgramStart { program } => {
                write!(f, "executor.exec_program.start program={program}")
            }
            S::ExecutorExecProgramConfig => write!(f, "executor.exec_program.config"),
            S::ExecutorExecProgramCwd => write!(f, "executor.exec_program.cwd"),
            S::ExecutorExecProgramDryRun => write!(f, "executor.exec_program.dry_run"),
            S::ExecutorExecProgramExecute => write!(f, "executor.exec_program.execute"),
            S::ExecutorExecProgramOk => write!(f, "executor.exec_program.ok"),
        }
    }
}

/// Lightweight execution timeline used to enrich task failures with scheduler context.
#[derive(Debug, Clone)]
pub struct TaskTraceReport {
    task_name: String,
    task_args: Vec<String>,
    frames: Vec<TaskTraceFrame>,
    command: Option<String>,
    exit_code: Option<i32>,
    started_at: Instant,
    events: Vec<TaskTraceEvent>,
}

impl TaskTraceReport {
    pub fn new(task: &Task) -> Self {
        let mut report = Self {
            task_name: task.name.clone(),
            task_args: task.args.clone(),
            frames: vec![],
            command: None,
            exit_code: None,
            started_at: Instant::now(),
            events: Vec::new(),
        };
        report.add_task_frame_with_reason(
            task,
            Some("scheduled for execution in this run".to_string()),
        );
        report.mark(TaskTraceStage::SpawnReceived);
        report
    }

    pub fn mark(&mut self, stage: TaskTraceStage) {
        self.events.push(TaskTraceEvent {
            _at_ms: self.started_at.elapsed().as_millis(),
            _label: stage.to_string(),
        });
    }

    pub fn set_command(&mut self, command: impl Into<String>) {
        let command = command.into();
        if command.is_empty() {
            return;
        }
        if self.command.is_none() {
            self.command = Some(command);
        }
    }

    pub fn set_exit_code(&mut self, exit_code: Option<i32>) {
        if exit_code.is_some() {
            self.exit_code = exit_code;
        }
    }

    pub fn add_task_frame_with_reason(&mut self, task: &Task, why: impl Into<Option<String>>) {
        let line = resolve_task_line(&task.config_source, &task.name);
        self.push_frame(TaskTraceFrame {
            task_name: task.name.clone(),
            source: (!task.config_source.as_os_str().is_empty())
                .then_some(task.config_source.clone()),
            line,
            why: why.into(),
        });
    }

    pub fn add_task_name_frame_with_reason(
        &mut self,
        task_name: impl Into<String>,
        source: Option<&Path>,
        why: impl Into<Option<String>>,
    ) {
        let task_name = task_name.into();
        let (source_buf, line) = if let Some(source) = source {
            let source_buf = source.to_path_buf();
            let line = resolve_task_line(&source_buf, &task_name);
            (Some(source_buf), line)
        } else {
            (None, None)
        };
        self.push_frame(TaskTraceFrame {
            task_name,
            source: source_buf,
            line,
            why: why.into(),
        });
    }

    fn push_frame(&mut self, frame: TaskTraceFrame) {
        if self.frames.last() == Some(&frame) {
            return;
        }
        self.frames.push(frame);
    }

    pub fn wrap_error(&mut self, err: Report, stage: TaskTraceStage) -> Report {
        if self.exit_code.is_none() {
            self.set_exit_code(Error::get_exit_status(&err));
        }
        if error_chain_has_report(&err) {
            return err;
        }
        let detail_lines = extract_error_details(&err);
        err.wrap_err(self.render(stage, &detail_lines))
    }

    fn render(&self, stage: TaskTraceStage, detail_lines: &[String]) -> String {
        let mut lines = Vec::with_capacity(self.frames.len() + 8);
        let failed_frame = self.frames.first();
        lines.push(format!(
            "{}:",
            style::ered("Task Failure Report").bold().bright()
        ));
        lines.push(format!(
            "  {}:",
            style::eyellow("Failed Task").bold().bright()
        ));
        lines.push(format!(
            "    {}: {}",
            style::edim("Name"),
            style::ered(&self.task_name).bold().bright()
        ));
        if !self.task_args.is_empty() {
            lines.push(format!(
                "    {}: {}",
                style::edim("Args"),
                style::ebold(self.task_args.join(" "))
            ));
        }
        if let Some(frame) = failed_frame {
            lines.push(format!(
                "    {}: {}",
                style::edim("Defined At"),
                format_frame_location(frame, false)
            ));
        }
        lines.push(String::new());
        lines.push(format!("  {}:", style::eyellow("Failure").bold().bright()));
        lines.push(format!(
            "    {}: {}",
            style::edim("Reason"),
            style::ebold(user_facing_failure_reason(&stage, &self.task_name))
        ));
        if !detail_lines.is_empty() {
            lines.push(format!("    {}:", style::eyellow("Details").bold()));
            for detail in detail_lines {
                lines.push(format!("      - {detail}"));
            }
        }
        if let Some(command) = &self.command {
            lines.push(format!(
                "    {}: {}",
                style::edim("Command"),
                style::ebold(command)
            ));
        }
        if let Some(code) = self.exit_code {
            lines.push(format!(
                "    {}: {}",
                style::edim("Exit Code"),
                style::ered(code).bold().bright()
            ));
        }
        if !self.frames.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "  {}:",
                style::eyellow("Task Path").bold().bright()
            ));
            for frame in &self.frames {
                let styled_task_name = if frame.task_name == self.task_name {
                    style::ered(&frame.task_name).bold().bright().to_string()
                } else {
                    style::ecyan(&frame.task_name).bold().to_string()
                };
                lines.push(format!(
                    "    - {}: {} ({})",
                    style::edim("Task"),
                    styled_task_name,
                    format_frame_location(frame, true)
                ));
                if let Some(why) = &frame.why
                    && !why.trim().is_empty()
                {
                    lines.push(format!(
                        "      {}: {}",
                        style::edim("Why"),
                        style::ebold(why)
                    ));
                }
            }
        }
        lines.join("\n")
    }
}

type TaskLineCache = LazyLock<Mutex<HashMap<(PathBuf, String), Option<usize>>>>;

static TASK_LINE_CACHE: TaskLineCache = LazyLock::new(|| Mutex::new(HashMap::new()));
static ANSI_ESCAPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Strip common CSI/OSC/control escape sequences from rendered reports.
    Regex::new(r"\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x1B\x07]*(?:\x07|\x1B\\)|[@-Z\\-_])")
        .expect("invalid ANSI regex")
});

fn resolve_task_line(path: &Path, task_name: &str) -> Option<usize> {
    if path.as_os_str().is_empty() || task_name.is_empty() || !path.exists() {
        return None;
    }
    let key = (path.to_path_buf(), task_name.to_string());
    if let Some(cached) = TASK_LINE_CACHE.lock().unwrap().get(&key).cloned() {
        return cached;
    }
    let found = std::fs::read_to_string(path)
        .ok()
        .and_then(|body| find_task_line_in_toml(&body, task_name));
    TASK_LINE_CACHE.lock().unwrap().insert(key, found);
    found
}

fn format_frame_location(frame: &TaskTraceFrame, include_unknown: bool) -> String {
    let source = frame
        .source
        .as_ref()
        .map(display_path)
        .unwrap_or_else(|| "<unknown>".to_string());
    match (source.as_str(), frame.line, include_unknown) {
        ("<unknown>", _, false) => "<unknown>".to_string(),
        (_, Some(line), _) => format!("{source}:{line}"),
        _ => source,
    }
}

fn error_chain_has_report(err: &Report) -> bool {
    err.chain().any(|cause| {
        let text = normalize_report_text(&cause.to_string());
        text.starts_with("task failure report:")
            || text.starts_with("task trace report:")
            || text.contains("\ntask failure report:")
            || text.contains("\ntask trace report:")
    })
}

fn extract_error_details(err: &Report) -> Vec<String> {
    let mut details = Vec::new();
    for cause in err.chain() {
        let text = cause.to_string();
        let normalized = normalize_report_text(&text);
        let text = text.trim();
        if text.is_empty()
            || normalized.starts_with("task failure report:")
            || normalized.starts_with("task trace report:")
        {
            continue;
        }
        if details.last().is_some_and(|prev| prev == text) {
            continue;
        }
        details.push(text.to_string());
        if details.len() >= 3 {
            break;
        }
    }
    details
}

fn normalize_report_text(text: &str) -> String {
    strip_ansi(text).to_ascii_lowercase()
}

fn strip_ansi(input: &str) -> String {
    ANSI_ESCAPE_RE.replace_all(input, "").into_owned()
}

fn user_facing_failure_reason(stage: &TaskTraceStage, task_name: &str) -> String {
    use TaskTraceStage as S;
    match stage {
        S::SchedulerAdmissionFailed => {
            format!("could not schedule `{task_name}` before execution started")
        }
        S::SchedulerTaskExecutionFailed => format!("`{task_name}` failed during execution"),
        S::ExecutorSourcesCheck => format!("failed while checking sources for `{task_name}`"),
        S::ExecutorToolsParse => {
            format!("failed while parsing tool requirements for `{task_name}`")
        }
        S::ExecutorToolsetBuild => format!("failed while preparing tools for `{task_name}`"),
        S::ExecutorEnvRender => format!("failed while building environment for `{task_name}`"),
        S::ExecutorTaskFile | S::ExecutorFilePath => {
            format!("failed while resolving file command for `{task_name}`")
        }
        S::ExecutorExecFile | S::ExecutorExecFileUsage => {
            format!("failed while executing file task `{task_name}`")
        }
        S::ExecutorRunEntriesRender => {
            format!("failed while rendering command templates for `{task_name}`")
        }
        S::ExecutorUsage => format!("failed while parsing arguments for `{task_name}`"),
        S::ExecutorConfirm | S::ExecutorExecFileConfirm => {
            format!("confirmation step failed for `{task_name}`")
        }
        S::ExecutorRunEntryScriptExec => {
            format!("a script entry failed while executing `{task_name}`")
        }
        S::ExecutorRunEntrySingleExec | S::ExecutorRunEntryGroupExec => {
            format!("a nested task reference failed while executing `{task_name}`")
        }
        S::ExecutorRunEntriesExec => {
            format!("a nested run entry failed while executing `{task_name}`")
        }
        S::ExecutorChecksum => format!("failed while updating outputs metadata for `{task_name}`"),
        S::ExecutorInjectLoad | S::ExecutorInjectDeps => {
            format!("failed while preparing nested task graph for `{task_name}`")
        }
        S::ExecutorInjectMatch => {
            format!("`{task_name}` references a task that could not be resolved")
        }
        S::ExecutorInjectStopRequested
        | S::ExecutorInjectDoneChannel
        | S::ExecutorInjectFinalStop => {
            format!("nested tasks for `{task_name}` were interrupted after a failure")
        }
        S::ExecutorExecScriptConfig => {
            format!("failed while setting up script runtime for `{task_name}`")
        }
        S::ExecutorExecProgramConfig | S::ExecutorExecProgramCwd => {
            format!("failed while preparing command execution for `{task_name}`")
        }
        S::ExecutorExecProgramExecute => format!("command execution failed for `{task_name}`"),
        _ => format!("`{task_name}` failed"),
    }
}

fn find_task_line_in_toml(body: &str, task_name: &str) -> Option<usize> {
    let mut in_multiline_basic = false;
    let mut in_multiline_literal = false;

    for (idx, line) in body.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        let trimmed_start = line.trim_start();

        // Skip content inside TOML multiline strings to avoid false positives.
        if in_multiline_basic {
            if triple_quote_count(line, "\"\"\"") % 2 == 1 {
                in_multiline_basic = false;
            }
            continue;
        }
        if in_multiline_literal {
            if triple_quote_count(line, "'''") % 2 == 1 {
                in_multiline_literal = false;
            }
            continue;
        }

        if trimmed_start.starts_with('#') {
            continue;
        }

        // [tasks.name] and [tasks."name.with.dots"] table forms
        if let Some(rest) = trimmed.strip_prefix("[tasks.")
            && let Some(inner) = rest.strip_suffix(']')
            && let Some(key) = parse_toml_key(inner)
            && key == task_name
        {
            return Some(line_no);
        }

        // tasks.name = ... and tasks."name.with.dots".run = ... inline forms
        if let Some((lhs, _rhs)) = trimmed.split_once('=') {
            let lhs = lhs.trim();
            if let Some(rest) = lhs.strip_prefix("tasks.")
                && let Some(key) = parse_toml_key_prefix(rest)
                && key == task_name
            {
                return Some(line_no);
            }
        }

        if triple_quote_count(line, "\"\"\"") % 2 == 1 {
            in_multiline_basic = true;
        }
        if triple_quote_count(line, "'''") % 2 == 1 {
            in_multiline_literal = true;
        }
    }
    None
}

fn triple_quote_count(line: &str, quote: &str) -> usize {
    line.match_indices(quote).count()
}

fn parse_toml_key(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(stripped) = input.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(stripped.to_string())
    } else {
        Some(input.to_string())
    }
}

fn parse_toml_key_prefix(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        let end = input.find('.').unwrap_or(input.len());
        Some(input[..end].trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TaskTraceReport, TaskTraceStage, find_task_line_in_toml, parse_toml_key,
        parse_toml_key_prefix,
    };
    use crate::task::Task;
    use eyre::eyre;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn report_render_contains_user_facing_fields() {
        let task = Task {
            name: "build".to_string(),
            args: vec!["--release".to_string()],
            ..Default::default()
        };
        let mut report = TaskTraceReport::new(&task);
        report.mark(TaskTraceStage::SchedulerAdmissionStart);
        report.mark(TaskTraceStage::SchedulerExecutionStart { owner: Some(7) });
        report.set_command("bash -c echo");
        report.set_exit_code(Some(42));

        let rendered = report
            .wrap_error(eyre!("boom"), TaskTraceStage::SchedulerTaskExecutionFailed)
            .to_string();
        assert!(rendered.contains("Task Failure Report:"));
        assert!(rendered.contains("Failed Task:"));
        assert!(rendered.contains("Name: build"));
        assert!(rendered.contains("Args: --release"));
        assert!(rendered.contains("Reason: `build` failed during execution"));
        assert!(rendered.contains("Command: bash -c echo"));
        assert!(rendered.contains("Exit Code: 42"));
        assert!(rendered.contains("Task Path:"));
        assert!(rendered.contains("- Task: build"));
        assert!(rendered.contains("Why: scheduled for execution in this run"));
    }

    #[test]
    fn wrapped_error_keeps_original_message() {
        let task = Task {
            name: "test".to_string(),
            ..Default::default()
        };
        let mut report = TaskTraceReport::new(&task);
        report.mark(TaskTraceStage::SchedulerAdmissionError);

        let wrapped = report.wrap_error(
            eyre!("inner failure"),
            TaskTraceStage::SchedulerAdmissionFailed,
        );
        let rendered = format!("{wrapped:#}");
        assert!(rendered.contains("Task Failure Report"));
        assert!(rendered.contains("inner failure"));
    }

    #[test]
    fn wrap_error_is_idempotent_when_error_already_contains_report() {
        let task = Task {
            name: "demo".to_string(),
            ..Default::default()
        };
        let mut first = TaskTraceReport::new(&task);
        let err = first.wrap_error(eyre!("root cause"), TaskTraceStage::ExecutorUsage);

        let mut second = TaskTraceReport::new(&task);
        let wrapped_again = second.wrap_error(err, TaskTraceStage::SchedulerTaskExecutionFailed);
        let rendered = format!("{wrapped_again:#}");

        assert_eq!(rendered.matches("Task Failure Report:").count(), 1);
        assert!(rendered.contains("root cause"));
    }

    #[test]
    fn command_prefers_first_user_facing_value() {
        let task = Task {
            name: "deploy".to_string(),
            ..Default::default()
        };
        let mut report = TaskTraceReport::new(&task);
        report.set_command("npm run deploy");
        report.set_command("/bin/sh -lc npm run deploy");

        let rendered = report
            .wrap_error(eyre!("boom"), TaskTraceStage::ExecutorExecProgramExecute)
            .to_string();
        assert!(rendered.contains("Command: npm run deploy"));
        assert!(!rendered.contains("Command: /bin/sh -lc npm run deploy"));
    }

    #[test]
    fn task_path_uses_task_name_and_mise_toml_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        fs::write(
            &path,
            r#"
[tasks.build]
run = "echo build"
"#,
        )
        .unwrap();
        let task = Task {
            name: "build".to_string(),
            config_source: path,
            ..Default::default()
        };
        let mut report = TaskTraceReport::new(&task);

        let rendered = report
            .wrap_error(
                eyre!("failure"),
                TaskTraceStage::SchedulerTaskExecutionFailed,
            )
            .to_string();
        assert!(rendered.contains("Task Path:"));
        assert!(rendered.contains("build ("));
        assert!(rendered.contains("mise.toml:2"));
    }

    #[test]
    fn parse_toml_keys_support_plain_and_quoted() {
        assert_eq!(parse_toml_key("build"), Some("build".to_string()));
        assert_eq!(
            parse_toml_key("\"build.all\""),
            Some("build.all".to_string())
        );
        assert_eq!(
            parse_toml_key_prefix("build.run"),
            Some("build".to_string())
        );
        assert_eq!(
            parse_toml_key_prefix("\"build.all\".run"),
            Some("build.all".to_string())
        );
    }

    #[test]
    fn find_task_line_supports_table_and_inline_forms() {
        let body = r#"
[tasks.build]
run = "echo build"
tasks.test = "echo test"
tasks."lint.all".run = "echo lint"
"#;
        assert_eq!(find_task_line_in_toml(body, "build"), Some(2));
        assert_eq!(find_task_line_in_toml(body, "test"), Some(4));
        assert_eq!(find_task_line_in_toml(body, "lint.all"), Some(5));
        assert_eq!(find_task_line_in_toml(body, "missing"), None);
    }

    #[test]
    fn find_task_line_ignores_task_like_text_in_comments_and_strings() {
        let body = r#"
# [tasks.fake]
[tasks.real]
run = """
tasks.fake = "not a task declaration"
"""
"#;
        assert_eq!(find_task_line_in_toml(body, "real"), Some(3));
        assert_eq!(find_task_line_in_toml(body, "fake"), None);
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc_sequences() {
        let input = "\u{1b}[31merror\u{1b}[0m and \u{1b}]0;title\u{7}text";
        assert_eq!(super::strip_ansi(input), "error and text");
    }
}
