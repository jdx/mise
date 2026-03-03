use crate::config::Config;
use crate::task::Task;
use crate::task::task_execution_plan::{
    TaskDeclarationRef, collect_toml_declaration_sources, format_declaration_location,
    task_declaration_ref,
};
use serde::Serialize;
use std::collections::BTreeMap;

pub const DEFAULT_AVAILABLE_TASKS_PREVIEW_LIMIT: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvailableTaskDiagnostic {
    pub name: String,
    pub declaration: TaskDeclarationRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ResolutionScope {
    pub config_files: Vec<String>,
    pub available_tasks: Vec<AvailableTaskDiagnostic>,
}

impl ResolutionScope {
    pub fn from_tasks<'a>(tasks: impl IntoIterator<Item = &'a Task>) -> Self {
        let tasks = tasks.into_iter().collect::<Vec<_>>();
        Self {
            config_files: config_files_from_tasks(tasks.iter().copied()),
            available_tasks: available_tasks_from_tasks(tasks.iter().copied()),
        }
    }

    pub fn from_config_and_tasks(config: &Config, tasks: &BTreeMap<String, Task>) -> Self {
        let config_sources = config
            .config_files
            .keys()
            .map(|path| path.to_string_lossy().to_string());
        let task_sources = tasks
            .values()
            .filter(|task| !task.config_source.as_os_str().is_empty())
            .map(|task| task.config_source.to_string_lossy().to_string());
        let combined_sources = config_sources.chain(task_sources).collect::<Vec<_>>();

        Self {
            config_files: collect_toml_declaration_sources(
                combined_sources.iter().map(String::as_str),
            ),
            available_tasks: available_tasks_from_tasks(tasks.values()),
        }
    }

    pub fn append_to(&self, lines: &mut Vec<String>, preview_limit: usize) {
        append_resolution_sections(
            lines,
            &self.config_files,
            &self.available_tasks,
            preview_limit,
        );
    }

    pub fn append_to_with_name(
        &self,
        lines: &mut Vec<String>,
        preview_limit: usize,
        format_name: impl Fn(&str) -> String,
    ) {
        append_resolution_sections_with_name(
            lines,
            &self.config_files,
            &self.available_tasks,
            preview_limit,
            format_name,
        );
    }
}

pub fn available_tasks_from_tasks<'a>(
    tasks: impl IntoIterator<Item = &'a Task>,
) -> Vec<AvailableTaskDiagnostic> {
    let mut by_name = BTreeMap::new();
    for task in tasks {
        by_name
            .entry(task.name.clone())
            .or_insert_with(|| AvailableTaskDiagnostic {
                name: task.name.clone(),
                declaration: task_declaration_ref(task),
            });
    }
    by_name.into_values().collect()
}

pub fn config_files_from_tasks<'a>(tasks: impl IntoIterator<Item = &'a Task>) -> Vec<String> {
    let sources = tasks
        .into_iter()
        .map(|task| task.config_source.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    collect_toml_declaration_sources(sources.iter().map(String::as_str))
}

pub fn append_resolution_sections(
    lines: &mut Vec<String>,
    config_files: &[String],
    available_tasks: &[AvailableTaskDiagnostic],
    preview_limit: usize,
) {
    append_resolution_sections_with_name(
        lines,
        config_files,
        available_tasks,
        preview_limit,
        |n| n.to_string(),
    );
}

pub fn append_resolution_sections_with_name(
    lines: &mut Vec<String>,
    config_files: &[String],
    available_tasks: &[AvailableTaskDiagnostic],
    preview_limit: usize,
    format_name: impl Fn(&str) -> String,
) {
    lines.push(String::new());
    lines.push(format!(
        "Config files loaded for task resolution ({}):",
        config_files.len()
    ));
    if config_files.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        for file in config_files {
            lines.push(format!("  - {file}"));
        }
    }

    lines.push(String::new());
    lines.push(format!("Available tasks ({}):", available_tasks.len()));
    if available_tasks.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        for task in available_tasks.iter().take(preview_limit) {
            lines.push(format!(
                "  - {} ({})",
                format_name(&task.name),
                format_declaration_location(&task.declaration)
            ));
        }
        let remaining = available_tasks.len().saturating_sub(preview_limit);
        if remaining > 0 {
            lines.push(format!("  - ... and {remaining} more"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_available_tasks_from_tasks_dedupes_by_name() {
        let task_a = Task {
            name: "build".to_string(),
            config_source: PathBuf::from("/tmp/a/mise.toml"),
            ..Default::default()
        };
        let task_b = Task {
            name: "build".to_string(),
            config_source: PathBuf::from("/tmp/b/mise.toml"),
            ..Default::default()
        };
        let task_c = Task {
            name: "test".to_string(),
            config_source: PathBuf::from("/tmp/c/mise.toml"),
            ..Default::default()
        };

        let available = available_tasks_from_tasks([&task_a, &task_b, &task_c]);
        assert_eq!(available.len(), 2);
        assert_eq!(available[0].name, "build");
        assert_eq!(available[1].name, "test");
    }

    #[test]
    fn test_append_resolution_sections_renders_preview_and_remainder() {
        let mut lines = vec!["task not found: missing".to_string()];
        let config_files = vec!["~/project/mise.toml".to_string()];
        let available = vec![
            AvailableTaskDiagnostic {
                name: "a".to_string(),
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(1),
                },
            },
            AvailableTaskDiagnostic {
                name: "b".to_string(),
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(2),
                },
            },
            AvailableTaskDiagnostic {
                name: "c".to_string(),
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(3),
                },
            },
        ];

        append_resolution_sections(&mut lines, &config_files, &available, 2);
        let message = lines.join("\n");
        assert!(message.contains("Config files loaded for task resolution (1):"));
        assert!(message.contains("Available tasks (3):"));
        assert!(message.contains("  - a ("));
        assert!(message.contains("  - b ("));
        assert!(message.contains("  - ... and 1 more"));
    }

    #[test]
    fn test_resolution_scope_from_tasks_collects_and_renders_sections() {
        let task_a = Task {
            name: "build".to_string(),
            config_source: PathBuf::from("/tmp/a/mise.toml"),
            ..Default::default()
        };
        let task_b = Task {
            name: "test".to_string(),
            config_source: PathBuf::from("/tmp/b/mise.toml"),
            ..Default::default()
        };

        let scope = ResolutionScope::from_tasks([&task_a, &task_b]);
        assert_eq!(
            scope.config_files,
            vec![
                "/tmp/a/mise.toml".to_string(),
                "/tmp/b/mise.toml".to_string()
            ]
        );
        assert_eq!(
            scope
                .available_tasks
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>(),
            vec!["build".to_string(), "test".to_string()]
        );

        let mut lines = vec!["task not found: missing".to_string()];
        scope.append_to(&mut lines, 30);
        let message = lines.join("\n");
        assert!(message.contains("Config files loaded for task resolution (2):"));
        assert!(message.contains("Available tasks (2):"));
        assert!(message.contains("  - build ("));
        assert!(message.contains("  - test ("));
    }

    #[test]
    fn test_resolution_scope_append_with_name_formats_task_names() {
        let task = Task {
            name: "deploy".to_string(),
            config_source: PathBuf::from("/tmp/mise.toml"),
            ..Default::default()
        };
        let scope = ResolutionScope::from_tasks([&task]);
        let mut lines = vec!["Task 'depoy' not found.".to_string()];
        scope.append_to_with_name(&mut lines, 30, |name| format!("**{name}**"));
        let message = lines.join("\n");
        assert!(message.contains("  - **deploy** ("));
    }
}
