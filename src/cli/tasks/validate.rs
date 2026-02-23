use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::duration;
use crate::file;
use crate::task::Task;
use crate::task::task_fetcher::TaskFetcher;
use crate::ui::style;
use console::style as console_style;
use eyre::{Result, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use serde::Serialize;

/// Validate tasks for common errors and issues
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksValidate {
    /// Tasks to validate
    /// If not specified, validates all tasks
    #[clap(verbatim_doc_comment)]
    pub tasks: Option<Vec<String>>,

    /// Only show errors (skip warnings)
    #[clap(long, verbatim_doc_comment)]
    pub errors_only: bool,

    /// Output validation results in JSON format
    #[clap(long, verbatim_doc_comment)]
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
struct ValidationIssue {
    task: String,
    severity: Severity,
    category: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

#[derive(Debug, Serialize)]
struct ValidationResults {
    tasks_validated: usize,
    errors: usize,
    warnings: usize,
    issues: Vec<ValidationIssue>,
}

impl TasksValidate {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;

        // Resolve all remote task files before validation
        // so we can properly validate remote tasks and circular dependencies
        let mut resolved_tasks: Vec<Task> = config.tasks().await?.values().cloned().collect();
        // always no_cache=false as the command doesn't take no-cache argument
        // MISE_TASK_REMOTE_NO_CACHE env var is still respected if set
        TaskFetcher::new(false)
            .fetch_tasks(&mut resolved_tasks)
            .await?;
        let all_tasks: BTreeMap<String, Task> = resolved_tasks
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        // Filter tasks to validate
        let tasks = if let Some(ref task_names) = self.tasks {
            self.get_specific_tasks(&all_tasks, task_names).await?
        } else {
            self.get_all_tasks(&all_tasks)
        };

        // Run validation
        let mut issues = Vec::new();
        for task in &tasks {
            issues.extend(self.validate_task(task, &all_tasks, &config).await);
        }

        // Filter by severity if needed
        if self.errors_only {
            issues.retain(|i| i.severity == Severity::Error);
        }

        let results = ValidationResults {
            tasks_validated: tasks.len(),
            errors: issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count(),
            warnings: issues
                .iter()
                .filter(|i| i.severity == Severity::Warning)
                .count(),
            issues,
        };

        // Output results
        if self.json {
            self.output_json(&results)?;
        } else {
            self.output_human(&results)?;
        }

        // Exit with error if there are errors
        if results.errors > 0 {
            return Err(eyre!("Validation failed with {} error(s)", results.errors));
        }

        Ok(())
    }

    fn get_all_tasks(&self, all_tasks: &BTreeMap<String, Task>) -> Vec<Task> {
        all_tasks.values().cloned().collect()
    }

    async fn get_specific_tasks(
        &self,
        all_tasks: &BTreeMap<String, Task>,
        task_names: &[String],
    ) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        for name in task_names {
            // Check if task exists by name, display_name, or alias
            match all_tasks
                .get(name)
                .or_else(|| all_tasks.values().find(|t| &t.display_name == name))
                .or_else(|| {
                    all_tasks
                        .values()
                        .find(|t| t.aliases.contains(&name.to_string()))
                })
                .cloned()
            {
                Some(task) => tasks.push(task),
                None => {
                    return Err(eyre!(
                        "Task '{}' not found. Available tasks: {}",
                        name,
                        all_tasks.keys().map(style::ecyan).join(", ")
                    ));
                }
            }
        }
        Ok(tasks)
    }

    async fn validate_task(
        &self,
        task: &Task,
        all_tasks: &BTreeMap<String, Task>,
        config: &Arc<Config>,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // 1. Validate circular dependencies
        issues.extend(self.validate_circular_dependencies(task, all_tasks));

        // 2. Validate missing task references
        issues.extend(self.validate_missing_references(task, all_tasks));

        // 3. Validate usage spec parsing
        issues.extend(self.validate_usage_spec(task, config).await);

        // 4. Validate timeout format
        issues.extend(self.validate_timeout(task));

        // 5. Validate alias conflicts
        issues.extend(self.validate_aliases(task, all_tasks));

        // 6. Validate file existence
        issues.extend(self.validate_file_existence(task));

        // 7. Validate directory template
        issues.extend(self.validate_directory(task, config).await);

        // 8. Validate shell command
        issues.extend(self.validate_shell(task));

        // 9. Validate source globs
        issues.extend(self.validate_source_patterns(task));

        // 10. Validate output patterns
        issues.extend(self.validate_output_patterns(task));

        // 11. Validate run entries
        issues.extend(self.validate_run_entries(task, all_tasks));

        issues
    }

    fn validate_circular_dependencies(
        &self,
        task: &Task,
        all_tasks: &BTreeMap<String, Task>,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        match task.all_depends(all_tasks) {
            Ok(_) => {}
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("circular dependency") {
                    issues.push(ValidationIssue {
                        task: task.name.clone(),
                        severity: Severity::Error,
                        category: "circular-dependency".to_string(),
                        message: "Circular dependency detected".to_string(),
                        details: Some(err_msg),
                    });
                }
            }
        }

        issues
    }

    /// Check if a task exists by name, display_name, or alias
    fn task_exists(all_tasks: &BTreeMap<String, Task>, task_name: &str) -> bool {
        all_tasks.contains_key(task_name)
            || all_tasks.values().any(|t| t.display_name == task_name)
            || all_tasks
                .values()
                .any(|t| t.aliases.contains(&task_name.to_string()))
    }

    fn validate_missing_references(
        &self,
        task: &Task,
        all_tasks: &BTreeMap<String, Task>,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Check all dependency types
        let all_deps = task
            .depends
            .iter()
            .map(|d| ("depends", &d.task))
            .chain(task.depends_post.iter().map(|d| ("depends_post", &d.task)))
            .chain(task.wait_for.iter().map(|d| ("wait_for", &d.task)));

        for (dep_type, dep_name) in all_deps {
            // Skip pattern wildcards for now (they're resolved at runtime)
            if dep_name.contains('*') || dep_name.contains('?') {
                continue;
            }

            // Check if task exists
            if !Self::task_exists(all_tasks, dep_name) {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "missing-dependency".to_string(),
                    message: format!("Dependency '{}' not found", dep_name),
                    details: Some(format!(
                        "Referenced in '{}' but no matching task exists",
                        dep_type
                    )),
                });
            }
        }

        issues
    }

    async fn validate_usage_spec(&self, task: &Task, config: &Arc<Config>) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Try to parse the usage spec
        match task.parse_usage_spec_for_display(config).await {
            Ok(_spec) => {
                // Successfully parsed
            }
            Err(e) => {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Warning,
                    category: "usage-parse-error".to_string(),
                    message: "Failed to parse usage specification".to_string(),
                    details: Some(format!("{:#}", e)),
                });
            }
        }

        // If task has explicit usage field, validate it's not empty
        if !task.usage.is_empty() {
            // Check if usage contains common USAGE directive errors
            if task.usage.contains("#USAGE") || task.usage.contains("# USAGE") {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Warning,
                    category: "usage-directive".to_string(),
                    message: "Usage field contains directive markers".to_string(),
                    details: Some(
                        "The 'usage' field should contain the spec directly, not #USAGE directives"
                            .to_string(),
                    ),
                });
            }
        }

        issues
    }

    fn validate_timeout(&self, task: &Task) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if let Some(ref timeout) = task.timeout {
            // Try to parse as duration
            if let Err(e) = duration::parse_duration(timeout) {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "invalid-timeout".to_string(),
                    message: format!("Invalid timeout format: '{}'", timeout),
                    details: Some(format!("Parse error: {}", e)),
                });
            }
        }

        issues
    }

    fn validate_aliases(
        &self,
        task: &Task,
        all_tasks: &BTreeMap<String, Task>,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Build a map of aliases to tasks
        let mut alias_map: HashMap<String, Vec<String>> = HashMap::new();
        for t in all_tasks.values() {
            for alias in &t.aliases {
                alias_map
                    .entry(alias.clone())
                    .or_default()
                    .push(t.name.clone());
            }
        }

        // Check for conflicts - only report once for the first task alphabetically
        for alias in &task.aliases {
            if let Some(tasks) = alias_map.get(alias)
                && tasks.len() > 1
            {
                // Only report the conflict for the first task (alphabetically) to avoid duplicates
                let mut sorted_tasks = tasks.clone();
                sorted_tasks.sort();
                if sorted_tasks[0] == task.name {
                    issues.push(ValidationIssue {
                        task: task.name.clone(),
                        severity: Severity::Error,
                        category: "alias-conflict".to_string(),
                        message: format!("Alias '{}' is used by multiple tasks", alias),
                        details: Some(format!("Tasks: {}", tasks.join(", "))),
                    });
                }
            }

            // Check if alias conflicts with a task name
            if all_tasks.contains_key(alias) {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "alias-conflict".to_string(),
                    message: format!("Alias '{}' conflicts with task name", alias),
                    details: Some(format!("A task named '{}' already exists", alias)),
                });
            }
        }

        issues
    }

    fn validate_file_existence(&self, task: &Task) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if let Some(ref file) = task.file {
            if !file.exists() {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "missing-file".to_string(),
                    message: format!("Task file not found: {}", file::display_path(file)),
                    details: None,
                });
            } else if !file::is_executable(file) {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Warning,
                    category: "not-executable".to_string(),
                    message: format!("Task file is not executable: {}", file::display_path(file)),
                    details: Some(format!("Run: chmod +x {}", file::display_path(file))),
                });
            }
        }

        issues
    }

    async fn validate_directory(&self, task: &Task, config: &Arc<Config>) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if let Some(ref dir) = task.dir {
            // Try to render the directory template
            if dir.contains("{{") || dir.contains("{%") {
                // Contains template syntax - try to render it
                match task.dir(config).await {
                    Ok(rendered_dir) => {
                        if let Some(rendered) = rendered_dir
                            && !rendered.exists()
                        {
                            issues.push(ValidationIssue {
                                task: task.name.clone(),
                                severity: Severity::Warning,
                                category: "missing-directory".to_string(),
                                message: format!(
                                    "Task directory does not exist: {}",
                                    file::display_path(&rendered)
                                ),
                                details: Some(format!("Template: {}", dir)),
                            });
                        }
                    }
                    Err(e) => {
                        issues.push(ValidationIssue {
                            task: task.name.clone(),
                            severity: Severity::Error,
                            category: "invalid-directory-template".to_string(),
                            message: "Failed to render directory template".to_string(),
                            details: Some(format!("Template: {}, Error: {:#}", dir, e)),
                        });
                    }
                }
            } else {
                // Static path - check if it exists
                let dir_path = PathBuf::from(dir);
                if dir_path.is_absolute() && !dir_path.exists() {
                    issues.push(ValidationIssue {
                        task: task.name.clone(),
                        severity: Severity::Warning,
                        category: "missing-directory".to_string(),
                        message: format!("Task directory does not exist: {}", dir),
                        details: None,
                    });
                }
            }
        }

        issues
    }

    fn validate_shell(&self, task: &Task) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if let Some(ref shell) = task.shell {
            // Parse shell command (could be "bash -c" or just "bash")
            let shell_parts: Vec<&str> = shell.split_whitespace().collect();
            if let Some(shell_cmd) = shell_parts.first() {
                // Check if it's an absolute path
                let shell_path = PathBuf::from(shell_cmd);
                if shell_path.is_absolute() && !shell_path.exists() {
                    issues.push(ValidationIssue {
                        task: task.name.clone(),
                        severity: Severity::Error,
                        category: "invalid-shell".to_string(),
                        message: format!("Shell command not found: {}", shell_cmd),
                        details: Some(format!("Full shell: {}", shell)),
                    });
                }
            }
        }

        issues
    }

    fn validate_source_patterns(&self, task: &Task) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        for source in &task.sources {
            // Try to compile as glob pattern
            if let Err(e) = globset::GlobBuilder::new(source).build() {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "invalid-glob-pattern".to_string(),
                    message: format!("Invalid source glob pattern: '{}'", source),
                    details: Some(format!("{}", e)),
                });
            }
        }

        issues
    }

    fn validate_output_patterns(&self, task: &Task) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Validate output patterns if they exist
        let paths = task.outputs.patterns();
        for path in paths {
            // Try to compile as glob pattern
            if let Err(e) = globset::GlobBuilder::new(&path).build() {
                issues.push(ValidationIssue {
                    task: task.name.clone(),
                    severity: Severity::Error,
                    category: "invalid-glob-pattern".to_string(),
                    message: format!("Invalid output glob pattern: '{}'", path),
                    details: Some(format!("{}", e)),
                });
            }
        }

        issues
    }

    fn validate_run_entries(
        &self,
        task: &Task,
        all_tasks: &BTreeMap<String, Task>,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Validate run entries
        for entry in task.run() {
            match entry {
                crate::task::RunEntry::Script(script) => {
                    // Check if script is empty
                    if script.trim().is_empty() {
                        issues.push(ValidationIssue {
                            task: task.name.clone(),
                            severity: Severity::Warning,
                            category: "empty-script".to_string(),
                            message: "Task contains empty script entry".to_string(),
                            details: None,
                        });
                    }
                }
                crate::task::RunEntry::SingleTask { task: task_name } => {
                    // Check if referenced task exists (by name, display_name, or alias)
                    if !Self::task_exists(all_tasks, task_name) {
                        issues.push(ValidationIssue {
                            task: task.name.clone(),
                            severity: Severity::Error,
                            category: "missing-task-reference".to_string(),
                            message: format!(
                                "Task '{}' referenced in run entry not found",
                                task_name
                            ),
                            details: None,
                        });
                    }
                }
                crate::task::RunEntry::TaskGroup { tasks } => {
                    // Check if all tasks in group exist (by name, display_name, or alias)
                    for task_name in tasks {
                        if !Self::task_exists(all_tasks, task_name) {
                            issues.push(ValidationIssue {
                                task: task.name.clone(),
                                severity: Severity::Error,
                                category: "missing-task-reference".to_string(),
                                message: format!("Task '{}' in task group not found", task_name),
                                details: Some("Referenced in parallel task group".to_string()),
                            });
                        }
                    }
                }
            }
        }

        // Check if task has no run entries and no file
        // Allow tasks with dependencies but no run entries (meta/group tasks)
        if task.run().is_empty()
            && task.file.is_none()
            && task.depends.is_empty()
            && task.depends_post.is_empty()
        {
            issues.push(ValidationIssue {
                task: task.name.clone(),
                severity: Severity::Error,
                category: "no-execution".to_string(),
                message: "Task has no executable content".to_string(),
                details: Some(
                    "Task must have either 'run', 'run_windows', 'file', or 'depends' defined"
                        .to_string(),
                ),
            });
        }

        issues
    }

    fn output_json(&self, results: &ValidationResults) -> Result<()> {
        let json = serde_json::to_string_pretty(results)?;
        miseprintln!("{}", json);
        Ok(())
    }

    fn output_human(&self, results: &ValidationResults) -> Result<()> {
        if results.issues.is_empty() {
            miseprintln!(
                "{}",
                console_style(format!(
                    "✓ All {} task(s) validated successfully",
                    results.tasks_validated
                ))
                .green()
            );
            return Ok(());
        }

        // Group issues by task
        let mut issues_by_task: IndexMap<String, Vec<&ValidationIssue>> = IndexMap::new();
        for issue in &results.issues {
            issues_by_task
                .entry(issue.task.clone())
                .or_insert_with(Vec::new)
                .push(issue);
        }

        // Print summary
        miseprintln!(
            "\n{} task(s) validated with {} issue(s):\n",
            console_style(results.tasks_validated).bold(),
            console_style(results.errors + results.warnings).bold()
        );

        if results.errors > 0 {
            miseprintln!(
                "  {} {}",
                console_style("✗").red().bold(),
                console_style(format!("{} error(s)", results.errors))
                    .red()
                    .bold()
            );
        }
        if results.warnings > 0 {
            miseprintln!(
                "  {} {}",
                console_style("⚠").yellow().bold(),
                console_style(format!("{} warning(s)", results.warnings))
                    .yellow()
                    .bold()
            );
        }

        miseprintln!();

        // Print issues grouped by task
        for (task_name, task_issues) in issues_by_task {
            miseprintln!(
                "{} {}",
                console_style("Task:").bold(),
                console_style(&task_name).cyan()
            );

            for issue in task_issues {
                let severity_icon = match issue.severity {
                    Severity::Error => console_style("✗").red().bold(),
                    Severity::Warning => console_style("⚠").yellow().bold(),
                };

                miseprintln!(
                    "  {} {} [{}]",
                    severity_icon,
                    console_style(&issue.message).bold(),
                    console_style(&issue.category).dim()
                );

                if let Some(ref details) = issue.details {
                    for line in details.lines() {
                        miseprintln!("      {}", console_style(line).dim());
                    }
                }
            }

            miseprintln!();
        }

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Validate all tasks
    $ <bold>mise tasks validate</bold>

    # Validate specific tasks
    $ <bold>mise tasks validate build test</bold>

    # Output results as JSON
    $ <bold>mise tasks validate --json</bold>

    # Only show errors (skip warnings)
    $ <bold>mise tasks validate --errors-only</bold>

<bold><underline>Validation Checks:</underline></bold>

The validate command performs the following checks:

  • <bold>Circular Dependencies</bold>: Detects dependency cycles
  • <bold>Missing References</bold>: Finds references to non-existent tasks
  • <bold>Usage Spec Parsing</bold>: Validates #USAGE directives and specs
  • <bold>Timeout Format</bold>: Checks timeout values are valid durations
  • <bold>Alias Conflicts</bold>: Detects duplicate aliases across tasks
  • <bold>File Existence</bold>: Verifies file-based tasks exist
  • <bold>Directory Templates</bold>: Validates directory paths and templates
  • <bold>Shell Commands</bold>: Checks shell executables exist
  • <bold>Glob Patterns</bold>: Validates source and output patterns
  • <bold>Run Entries</bold>: Ensures tasks reference valid dependencies
"#
);
