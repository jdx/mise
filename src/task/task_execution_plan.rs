use crate::file::display_path;
use crate::hash;
use crate::task::Task;
use crate::task::task_helpers::task_is_runtime;
use crate::task::task_helpers::task_logical_name;
use crate::task::task_identity::TaskIdentity;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type DeclarationLineCache = HashMap<(PathBuf, String), Option<usize>>;
static DECLARATION_LINE_CACHE: Lazy<Mutex<DeclarationLineCache>> = Lazy::new(Default::default);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TaskDeclarationRef {
    pub source: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedTask {
    pub identity: TaskIdentity,
    pub runtime: bool,
    pub interactive: bool,
    pub declaration: TaskDeclarationRef,
}

impl PlannedTask {
    pub fn from_task(task: &Task) -> Self {
        Self {
            identity: TaskIdentity::from_task(task),
            runtime: task_is_runtime(task),
            interactive: task.is_interactive(),
            declaration: task_declaration_ref(task),
        }
    }

    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

pub fn task_declaration_ref(task: &Task) -> TaskDeclarationRef {
    if task.config_source.as_os_str().is_empty() {
        return TaskDeclarationRef {
            source: "<generated>".to_string(),
            line: None,
        };
    }

    let source = task.config_source.to_string_lossy().to_string();
    let line = declaration_line(task);
    TaskDeclarationRef { source, line }
}

pub fn declaration_source_is_generated(source: &str) -> bool {
    source.starts_with('<')
}

pub fn declaration_source_is_toml(source: &str) -> bool {
    if declaration_source_is_generated(source) {
        return false;
    }
    Path::new(source)
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("toml"))
}

pub fn format_declaration_location(declaration: &TaskDeclarationRef) -> String {
    let source = display_path(declaration.source.as_str()).to_string();
    match declaration.line {
        Some(line) => format!("{source}:{line}"),
        None => source,
    }
}

pub fn collect_toml_declaration_sources<'a>(
    sources: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut files = BTreeSet::new();
    for source in sources {
        if declaration_source_is_toml(source) {
            files.insert(display_path(source).to_string());
        }
    }
    files.into_iter().collect()
}

fn declaration_line(task: &Task) -> Option<usize> {
    let path = &task.config_source;
    if !path.exists() {
        return None;
    }
    if path.extension().is_some_and(|ext| ext == "toml") {
        let names = declaration_name_candidates(task);
        for name in names {
            if let Some(line) = cached_toml_task_declaration_line(path, &name) {
                return Some(line);
            }
        }
        return None;
    }
    // File tasks declare themselves in their script file.
    Some(1)
}

fn declaration_name_candidates(task: &Task) -> Vec<String> {
    let mut names = Vec::new();
    let logical = task_logical_name(task).to_string();
    names.push(logical.clone());

    // Monorepo tasks are prefixed at load time ("//path:task"), while the
    // declaration in each sub-config remains local ("task").
    if let Some(local) = logical.strip_prefix("//").and_then(|stripped| {
        stripped
            .find(':')
            .map(|idx| stripped[idx + 1..].to_string())
    }) && !local.is_empty()
        && !names.contains(&local)
    {
        names.push(local);
    }

    if !task.display_name.is_empty() && !names.contains(&task.display_name) {
        names.push(task.display_name.clone());
    }
    names
}

fn cached_toml_task_declaration_line(path: &Path, task_name: &str) -> Option<usize> {
    let key = (path.to_path_buf(), task_name.to_string());
    if let Some(cached) = DECLARATION_LINE_CACHE.lock().unwrap().get(&key).cloned() {
        return cached;
    }

    let line = find_toml_task_declaration_line(path, task_name);
    DECLARATION_LINE_CACHE.lock().unwrap().insert(key, line);
    line
}

fn find_toml_task_declaration_line(path: &Path, task_name: &str) -> Option<usize> {
    let content = std::fs::read_to_string(path).ok()?;
    let key_forms = toml_key_forms(task_name);
    let mut in_tasks_table = false;
    let mut scan_state = TomlDeclarationScanState::default();

    for (idx, raw_line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let line = sanitize_toml_line_for_decl_scan(raw_line, &mut scan_state);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();

        if compact.starts_with('[') && compact.ends_with(']') {
            in_tasks_table =
                compact == "[tasks]" || compact == "[\"tasks\"]" || compact == "['tasks']";

            if key_forms
                .iter()
                .any(|key| compact == format!("[tasks.{key}]"))
            {
                return Some(line_no);
            }
            continue;
        }

        if key_forms
            .iter()
            .any(|key| compact.starts_with(&format!("tasks.{key}=")))
        {
            return Some(line_no);
        }

        if in_tasks_table
            && key_forms
                .iter()
                .any(|key| compact.starts_with(&format!("{key}=")))
        {
            return Some(line_no);
        }
    }

    None
}

fn toml_key_forms(task_name: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if task_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        keys.push(task_name.to_string());
    }
    keys.push(format!(
        "\"{}\"",
        task_name.replace('\\', "\\\\").replace('"', "\\\"")
    ));
    keys.push(format!("'{}'", task_name.replace('\'', "''")));
    keys
}

#[derive(Debug, Default)]
struct TomlDeclarationScanState {
    in_multiline_literal: bool,
    in_multiline_basic: bool,
}

fn sanitize_toml_line_for_decl_scan(line: &str, state: &mut TomlDeclarationScanState) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    let mut i = 0usize;
    while i < bytes.len() {
        if state.in_multiline_literal {
            if bytes[i..].starts_with(b"'''") {
                state.in_multiline_literal = false;
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }
        if state.in_multiline_basic {
            if bytes[i..].starts_with(b"\"\"\"") && !is_escaped_in_basic_multiline(bytes, i) {
                state.in_multiline_basic = false;
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }

        if !in_double && bytes[i..].starts_with(b"'''") {
            state.in_multiline_literal = true;
            i += 3;
            continue;
        }
        if !in_single && bytes[i..].starts_with(b"\"\"\"") {
            state.in_multiline_basic = true;
            i += 3;
            continue;
        }

        let ch = bytes[i] as char;

        if escaped {
            escaped = false;
            out.push(ch);
            i += 1;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if !in_double && ch == '\'' {
            in_single = !in_single;
            out.push(ch);
            i += 1;
            continue;
        }
        if !in_single && ch == '"' {
            in_double = !in_double;
            out.push(ch);
            i += 1;
            continue;
        }
        if !in_single && !in_double && ch == '#' {
            break;
        }
        out.push(ch);
        i += 1;
    }

    out
}

fn is_escaped_in_basic_multiline(bytes: &[u8], quote_idx: usize) -> bool {
    if quote_idx == 0 {
        return false;
    }
    let mut i = quote_idx;
    let mut backslashes = 0usize;
    while i > 0 && bytes[i - 1] == b'\\' {
        backslashes += 1;
        i -= 1;
    }
    backslashes % 2 == 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionStageKind {
    Parallel,
    InteractiveExclusive,
}

pub fn execution_stage_kind_label(kind: ExecutionStageKind) -> &'static str {
    match kind {
        ExecutionStageKind::Parallel => "parallel",
        ExecutionStageKind::InteractiveExclusive => "interactive-exclusive",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionStage {
    pub kind: ExecutionStageKind,
    pub tasks: Vec<PlannedTask>,
}

impl ExecutionStage {
    pub fn parallel(tasks: Vec<PlannedTask>) -> Self {
        Self {
            kind: ExecutionStageKind::Parallel,
            tasks,
        }
    }

    pub fn interactive(task: PlannedTask) -> Self {
        Self {
            kind: ExecutionStageKind::InteractiveExclusive,
            tasks: vec![task],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ExecutionPlan {
    pub stages: Vec<ExecutionStage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ExecutionPlanStats {
    pub stage_count: usize,
    pub task_count: usize,
    pub runtime_count: usize,
    pub interactive_count: usize,
    pub orchestrator_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedTaskExecutionContext {
    pub stage_index: usize,
    pub stage_kind: ExecutionStageKind,
    pub declaration: TaskDeclarationRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanContextIndex {
    stage_count: usize,
    plan_hash: Option<String>,
    by_identity: HashMap<TaskIdentity, PlannedTaskExecutionContext>,
}

impl PlanContextIndex {
    pub fn from_plan(plan: &ExecutionPlan, plan_hash: Option<String>) -> Self {
        Self {
            stage_count: plan.stages.len(),
            plan_hash,
            by_identity: execution_plan_task_context(plan),
        }
    }

    pub fn stage_count(&self) -> usize {
        self.stage_count
    }

    #[cfg(test)]
    pub fn plan_hash(&self) -> Option<&str> {
        self.plan_hash.as_deref()
    }

    pub fn contexts(&self) -> &HashMap<TaskIdentity, PlannedTaskExecutionContext> {
        &self.by_identity
    }

    pub fn context_for_task(&self, task: &Task) -> Option<&PlannedTaskExecutionContext> {
        let identity = TaskIdentity::from_task(task);
        self.by_identity.get(&identity)
    }

    pub fn declaration_for_task(&self, task: &Task) -> String {
        if let Some(context) = self.context_for_task(task) {
            return format_declaration_location(&context.declaration);
        }
        format_declaration_location(&task_declaration_ref(task))
    }

    pub fn stage_suffix_for_task(&self, task: &Task) -> String {
        if self.stage_count == 0 {
            return String::new();
        }
        let Some(context) = self.context_for_task(task) else {
            return String::new();
        };
        format!(
            " [stage {}/{}, kind={}]",
            context.stage_index,
            self.stage_count,
            execution_stage_kind_label(context.stage_kind)
        )
    }

    pub fn inject_env_for_task(&self, task: &Task, env: &mut BTreeMap<String, String>) {
        let Some(context) = self.context_for_task(task) else {
            return;
        };
        if self.stage_count > 0 {
            env.insert(
                "MISE_TASK_STAGE_INDEX".to_string(),
                context.stage_index.to_string(),
            );
            env.insert(
                "MISE_TASK_STAGE_COUNT".to_string(),
                self.stage_count.to_string(),
            );
            env.insert(
                "MISE_TASK_STAGE_KIND".to_string(),
                execution_stage_kind_label(context.stage_kind).to_string(),
            );
        }
        if let Some(plan_hash) = &self.plan_hash {
            env.insert("MISE_TASK_PLAN_HASH".to_string(), plan_hash.clone());
        }
    }
}

impl ExecutionPlan {
    pub fn interactive_task_names(&self) -> Vec<String> {
        self.stages
            .iter()
            .flat_map(|stage| stage.tasks.iter())
            .filter(|task| task.interactive)
            .map(|task| task.name().to_string())
            .collect()
    }
}

pub fn execution_plan_stats(plan: &ExecutionPlan) -> ExecutionPlanStats {
    let mut stats = ExecutionPlanStats {
        stage_count: plan.stages.len(),
        ..ExecutionPlanStats::default()
    };
    for stage in &plan.stages {
        for task in &stage.tasks {
            stats.task_count += 1;
            if task.runtime {
                stats.runtime_count += 1;
            } else {
                stats.orchestrator_count += 1;
            }
            if task.interactive {
                stats.interactive_count += 1;
            }
        }
    }
    stats
}

pub fn execution_plan_hash(plan: &ExecutionPlan) -> std::result::Result<String, serde_json::Error> {
    let json = serde_json::to_string(plan)?;
    Ok(format!("sha256:{}", hash::hash_sha256_to_str(&json)))
}

pub fn execution_stage_hash(
    stage: &ExecutionStage,
) -> std::result::Result<String, serde_json::Error> {
    let json = serde_json::to_string(stage)?;
    Ok(format!("sha256:{}", hash::hash_sha256_to_str(&json)))
}

pub fn execution_plan_config_files(plan: &ExecutionPlan) -> Vec<String> {
    collect_toml_declaration_sources(
        plan.stages
            .iter()
            .flat_map(|stage| stage.tasks.iter())
            .map(|task| task.declaration.source.as_str()),
    )
}

pub fn execution_plan_task_context(
    plan: &ExecutionPlan,
) -> HashMap<TaskIdentity, PlannedTaskExecutionContext> {
    let mut contexts = HashMap::new();
    for (idx, stage) in plan.stages.iter().enumerate() {
        for task in &stage.tasks {
            contexts.insert(
                task.identity.clone(),
                PlannedTaskExecutionContext {
                    stage_index: idx + 1,
                    stage_kind: stage.kind,
                    declaration: task.declaration.clone(),
                },
            );
        }
    }
    contexts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn interactive_task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        }
    }

    #[test]
    fn test_execution_plan_interactive_task_names_follow_stage_order() {
        // MatrixRef: B06 / C10
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![PlannedTask::from_task(&Task {
                    name: "build".to_string(),
                    run: vec![RunEntry::Script("echo hi".to_string())],
                    ..Default::default()
                })]),
                ExecutionStage::interactive(PlannedTask::from_task(&interactive_task("ask-a"))),
                ExecutionStage::interactive(PlannedTask::from_task(&interactive_task("ask-b"))),
            ],
        };

        assert_eq!(
            plan.interactive_task_names(),
            vec!["ask-a".to_string(), "ask-b".to_string()]
        );
    }

    #[test]
    fn test_planned_task_declaration_ref_for_task_table() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(
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
            run: vec![RunEntry::Script("echo build".to_string())],
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(2));
        assert!(planned.declaration.source.ends_with("mise.toml"));
    }

    #[test]
    fn test_planned_task_declaration_ref_for_dotted_assignment() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(
            &path,
            r#"
tasks.build = "echo build"
"#,
        )
        .unwrap();

        let task = Task {
            name: "build".to_string(),
            config_source: path,
            run: vec![RunEntry::Script("echo build".to_string())],
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(2));
    }

    #[test]
    fn test_planned_task_declaration_ref_for_file_task() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("task.sh");
        std::fs::write(&path, "#!/usr/bin/env bash\necho hi\n").unwrap();

        let task = Task {
            name: "task".to_string(),
            config_source: path.clone(),
            file: Some(path),
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(1));
    }

    #[test]
    fn test_planned_task_declaration_ref_for_monorepo_prefixed_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(
            &path,
            r#"
[tasks.build]
run = "echo build"
"#,
        )
        .unwrap();

        let task = Task {
            name: "//packages/web:build".to_string(),
            config_source: path,
            run: vec![RunEntry::Script("echo build".to_string())],
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(2));
    }

    #[test]
    fn test_planned_task_declaration_ref_ignores_multiline_string_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(
            &path,
            r#"[tasks.a]
run = '''
[tasks.b]
echo fake
'''

[tasks.b]
run = "echo real"
"#,
        )
        .unwrap();

        let task = Task {
            name: "b".to_string(),
            config_source: path,
            run: vec![RunEntry::Script("echo real".to_string())],
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(7));
    }

    #[test]
    fn test_planned_task_declaration_ref_ignores_escaped_multiline_basic_delimiter_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(
            &path,
            r#"[tasks.a]
run = """
line with escaped triple quote: \"""
[tasks.b]
still inside a
"""

[tasks.b]
run = "echo real"
"#,
        )
        .unwrap();

        let task = Task {
            name: "b".to_string(),
            config_source: path,
            run: vec![RunEntry::Script("echo real".to_string())],
            ..Default::default()
        };
        let planned = PlannedTask::from_task(&task);

        assert_eq!(planned.declaration.line, Some(8));
    }

    #[test]
    fn test_collect_toml_declaration_sources_filters_and_deduplicates() {
        let files = collect_toml_declaration_sources([
            "<generated>",
            "/tmp/mise.toml",
            "/tmp/mise.toml",
            "/tmp/.tool-versions",
            "/tmp/mise.local.toml",
        ]);
        assert_eq!(
            files,
            vec![
                "/tmp/mise.local.toml".to_string(),
                "/tmp/mise.toml".to_string()
            ]
        );
    }

    #[test]
    fn test_format_declaration_location_appends_line_when_present() {
        let declaration = TaskDeclarationRef {
            source: "/tmp/mise.toml".to_string(),
            line: Some(34),
        };
        assert_eq!(
            format_declaration_location(&declaration),
            "/tmp/mise.toml:34"
        );
    }

    #[test]
    fn test_execution_plan_hash_is_stable_for_same_plan() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::parallel(vec![PlannedTask {
                identity: TaskIdentity {
                    name: "build".to_string(),
                    args: vec![],
                    env: vec![],
                },
                runtime: true,
                interactive: false,
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(2),
                },
            }])],
        };

        let h1 = execution_plan_hash(&plan).unwrap();
        let h2 = execution_plan_hash(&plan).unwrap();
        assert_eq!(h1, h2);
        assert!(h1.starts_with("sha256:"));
    }

    #[test]
    fn test_execution_plan_task_context_includes_stage_kind_and_declaration() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: TaskDeclarationRef {
                        source: "/tmp/mise.toml".to_string(),
                        line: Some(2),
                    },
                }]),
                ExecutionStage::interactive(PlannedTask {
                    identity: TaskIdentity {
                        name: "ask".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: true,
                    declaration: TaskDeclarationRef {
                        source: "/tmp/mise.toml".to_string(),
                        line: Some(8),
                    },
                }),
            ],
        };

        let contexts = execution_plan_task_context(&plan);
        assert_eq!(contexts.len(), 2);
        let build = contexts
            .get(&TaskIdentity {
                name: "build".to_string(),
                args: vec![],
                env: vec![],
            })
            .unwrap();
        assert_eq!(build.stage_index, 1);
        assert_eq!(build.stage_kind, ExecutionStageKind::Parallel);
        assert_eq!(build.declaration.line, Some(2));

        let ask = contexts
            .get(&TaskIdentity {
                name: "ask".to_string(),
                args: vec![],
                env: vec![],
            })
            .unwrap();
        assert_eq!(ask.stage_index, 2);
        assert_eq!(ask.stage_kind, ExecutionStageKind::InteractiveExclusive);
        assert_eq!(ask.declaration.line, Some(8));
    }

    #[test]
    fn test_execution_stage_kind_label_uses_kebab_case() {
        assert_eq!(
            execution_stage_kind_label(ExecutionStageKind::Parallel),
            "parallel"
        );
        assert_eq!(
            execution_stage_kind_label(ExecutionStageKind::InteractiveExclusive),
            "interactive-exclusive"
        );
    }

    #[test]
    fn test_execution_plan_stats_counts_runtime_interactive_and_orchestrator() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![
                    PlannedTask {
                        identity: TaskIdentity {
                            name: "build".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: Default::default(),
                    },
                    PlannedTask {
                        identity: TaskIdentity {
                            name: "group".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: Default::default(),
                    },
                ]),
                ExecutionStage::interactive(PlannedTask {
                    identity: TaskIdentity {
                        name: "ask".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: true,
                    declaration: Default::default(),
                }),
            ],
        };
        let stats = execution_plan_stats(&plan);
        assert_eq!(stats.stage_count, 2);
        assert_eq!(stats.task_count, 3);
        assert_eq!(stats.runtime_count, 2);
        assert_eq!(stats.interactive_count, 1);
        assert_eq!(stats.orchestrator_count, 1);
    }

    #[test]
    fn test_plan_context_index_prefers_planned_declaration_and_stage_suffix() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage::parallel(vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: TaskDeclarationRef {
                        source: "/tmp/mise.toml".to_string(),
                        line: Some(7),
                    },
                }]),
                ExecutionStage::interactive(PlannedTask {
                    identity: TaskIdentity {
                        name: "ask".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: true,
                    declaration: TaskDeclarationRef {
                        source: "/tmp/mise.toml".to_string(),
                        line: Some(13),
                    },
                }),
            ],
        };

        let index = PlanContextIndex::from_plan(&plan, Some("sha256:test".to_string()));
        let task = Task {
            name: "ask".to_string(),
            run: vec![RunEntry::Script("read x".to_string())],
            config_source: "/tmp/fallback.toml".into(),
            interactive: Some(true),
            ..Default::default()
        };

        assert_eq!(index.stage_count(), 2);
        assert_eq!(index.plan_hash(), Some("sha256:test"));
        assert_eq!(index.declaration_for_task(&task), "/tmp/mise.toml:13");
        assert_eq!(
            index.stage_suffix_for_task(&task),
            " [stage 2/2, kind=interactive-exclusive]"
        );
    }

    #[test]
    fn test_plan_context_index_falls_back_without_context_and_injects_env() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::parallel(vec![PlannedTask {
                identity: TaskIdentity {
                    name: "build".to_string(),
                    args: vec![],
                    env: vec![],
                },
                runtime: true,
                interactive: false,
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(2),
                },
            }])],
        };

        let index = PlanContextIndex::from_plan(&plan, Some("sha256:abc".to_string()));
        let task = Task {
            name: "test".to_string(),
            config_source: "/tmp/test.toml".into(),
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        index.inject_env_for_task(&task, &mut env);

        assert_eq!(index.declaration_for_task(&task), "/tmp/test.toml");
        assert_eq!(index.stage_suffix_for_task(&task), "");
        assert!(env.is_empty());
    }

    #[test]
    fn test_plan_context_index_injects_stage_and_plan_hash_env() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::interactive(PlannedTask {
                identity: TaskIdentity {
                    name: "prompt".to_string(),
                    args: vec![],
                    env: vec![],
                },
                runtime: true,
                interactive: true,
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(5),
                },
            })],
        };

        let index = PlanContextIndex::from_plan(&plan, Some("sha256:xyz".to_string()));
        let task = Task {
            name: "prompt".to_string(),
            run: vec![RunEntry::Script("read x".to_string())],
            interactive: Some(true),
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        index.inject_env_for_task(&task, &mut env);

        assert_eq!(env.get("MISE_TASK_STAGE_INDEX"), Some(&"1".to_string()));
        assert_eq!(env.get("MISE_TASK_STAGE_COUNT"), Some(&"1".to_string()));
        assert_eq!(
            env.get("MISE_TASK_STAGE_KIND"),
            Some(&"interactive-exclusive".to_string())
        );
        assert_eq!(
            env.get("MISE_TASK_PLAN_HASH"),
            Some(&"sha256:xyz".to_string())
        );
    }
}
