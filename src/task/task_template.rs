use crate::config::config_file::mise_toml::EnvList;
use crate::config::config_file::toml::deserialize_arr;
use crate::task::task_sources::TaskOutputs;
use crate::task::{RunEntry, Silent, Task, TaskDep};
use indexmap::IndexMap;
use serde::Deserialize;

/// A task template definition that can be extended by tasks via `extends`
/// Templates are defined in [task_templates.*] sections of mise.toml
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskTemplate {
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "alias", deserialize_with = "deserialize_arr")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub confirm: Option<String>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub depends: Vec<TaskDep>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub depends_post: Vec<TaskDep>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub wait_for: Vec<TaskDep>,
    #[serde(default)]
    pub env: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub hide: Option<bool>,
    #[serde(default)]
    pub raw: Option<bool>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub outputs: TaskOutputs,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub quiet: Option<bool>,
    #[serde(default)]
    pub silent: Option<Silent>,
    #[serde(default)]
    pub tools: IndexMap<String, String>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run: Vec<RunEntry>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run_windows: Vec<RunEntry>,
    #[serde(default)]
    pub file: Option<String>,
}

impl Task {
    /// Merge a template into this task, using template values only where the task
    /// doesn't already have values set. This allows tasks to override template values.
    ///
    /// Merge semantics:
    /// - run, run_windows: Local overrides completely (if non-empty)
    /// - tools: Deep merge (local tools added/override template)
    /// - env: Deep merge (template first, then local overrides)
    /// - depends, depends_post, wait_for: Local overrides completely (if non-empty)
    /// - dir: Local overrides; defaults to None if not in template
    /// - sources, outputs: Local overrides completely (if non-empty)
    /// - Other fields: Local overrides template (if set)
    pub fn merge_template(&mut self, template: &TaskTemplate) {
        // run: only use template if local is empty
        if self.run.is_empty() {
            self.run = template.run.clone();
        }

        // run_windows: only use template if local is empty
        if self.run_windows.is_empty() {
            self.run_windows = template.run_windows.clone();
        }

        // tools: deep merge (template first, then local overrides)
        let mut merged_tools = template.tools.clone();
        for (tool, version) in &self.tools {
            merged_tools.insert(tool.clone(), version.clone());
        }
        self.tools = merged_tools;

        // env: deep merge (template first, then local overrides)
        let mut merged_env = template.env.clone();
        merged_env.0.extend(self.env.0.clone());
        self.env = merged_env;

        // depends: local overrides completely if non-empty
        if self.depends.is_empty() && !template.depends.is_empty() {
            self.depends = template.depends.clone();
        }

        // depends_post: local overrides completely if non-empty
        if self.depends_post.is_empty() && !template.depends_post.is_empty() {
            self.depends_post = template.depends_post.clone();
        }

        // wait_for: local overrides completely if non-empty
        if self.wait_for.is_empty() && !template.wait_for.is_empty() {
            self.wait_for = template.wait_for.clone();
        }

        // dir: local overrides; use template only if local not set
        if self.dir.is_none() {
            self.dir = template.dir.clone();
        }

        // description: use template only if local is empty
        if self.description.is_empty() && !template.description.is_empty() {
            self.description = template.description.clone();
        }

        // aliases: local overrides completely if non-empty
        if self.aliases.is_empty() && !template.aliases.is_empty() {
            self.aliases = template.aliases.clone();
        }

        // confirm: use template only if local not set
        if self.confirm.is_none() {
            self.confirm = template.confirm.clone();
        }

        // sources: local overrides completely if non-empty
        if self.sources.is_empty() && !template.sources.is_empty() {
            self.sources = template.sources.clone();
        }

        // outputs: local overrides completely if default
        if self.outputs == TaskOutputs::default() && template.outputs != TaskOutputs::default() {
            self.outputs = template.outputs.clone();
        }

        // shell: use template only if local not set
        if self.shell.is_none() {
            self.shell = template.shell.clone();
        }

        // Note: quiet, hide, and raw are `bool` in Task (not Option<bool>), so we cannot
        // distinguish between "not set" (defaults to false) and "explicitly set to false".
        // Therefore, we do NOT merge these boolean fields from templates to avoid the case
        // where a task explicitly sets `quiet = false` but gets overridden by a template's
        // `quiet = true`. Users must explicitly set these in their task if needed.

        // silent: use template only if local is Off (Silent is an enum, so we can distinguish)
        if matches!(self.silent, Silent::Off)
            && let Some(ref silent) = template.silent
        {
            self.silent = silent.clone();
        }

        // usage: use template only if local is empty
        if self.usage.is_empty() && !template.usage.is_empty() {
            self.usage = template.usage.clone();
        }

        // timeout: use template only if local not set
        if self.timeout.is_none() {
            self.timeout = template.timeout.clone();
        }

        // file: use template only if local not set
        if self.file.is_none()
            && let Some(ref file) = template.file
        {
            self.file = Some(file.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_template_run_override() {
        let mut task = Task {
            run: vec![RunEntry::Script("local command".to_string())],
            ..Default::default()
        };
        let template = TaskTemplate {
            run: vec![RunEntry::Script("template command".to_string())],
            ..Default::default()
        };

        task.merge_template(&template);

        // Local run should be preserved
        assert_eq!(task.run.len(), 1);
        assert!(matches!(&task.run[0], RunEntry::Script(s) if s == "local command"));
    }

    #[test]
    fn test_merge_template_run_from_template() {
        let mut task = Task::default();
        let template = TaskTemplate {
            run: vec![RunEntry::Script("template command".to_string())],
            ..Default::default()
        };

        task.merge_template(&template);

        // Template run should be used when local is empty
        assert_eq!(task.run.len(), 1);
        assert!(matches!(&task.run[0], RunEntry::Script(s) if s == "template command"));
    }

    #[test]
    fn test_merge_template_tools_deep_merge() {
        let mut task = Task {
            tools: IndexMap::from([("node".to_string(), "20".to_string())]),
            ..Default::default()
        };
        let template = TaskTemplate {
            tools: IndexMap::from([
                ("python".to_string(), "3.12".to_string()),
                ("node".to_string(), "18".to_string()), // Should be overridden by task
            ]),
            ..Default::default()
        };

        task.merge_template(&template);

        // Should have both tools, with task's node version
        assert_eq!(task.tools.len(), 2);
        assert_eq!(task.tools.get("node"), Some(&"20".to_string()));
        assert_eq!(task.tools.get("python"), Some(&"3.12".to_string()));
    }

    #[test]
    fn test_merge_template_description() {
        let mut task = Task::default();
        let template = TaskTemplate {
            description: "Template description".to_string(),
            ..Default::default()
        };

        task.merge_template(&template);

        assert_eq!(task.description, "Template description");

        // Now test that local description is preserved
        let mut task2 = Task {
            description: "Local description".to_string(),
            ..Default::default()
        };
        task2.merge_template(&template);
        assert_eq!(task2.description, "Local description");
    }

    #[test]
    fn test_merge_template_depends_override() {
        let mut task = Task {
            depends: vec![TaskDep {
                task: "local-dep".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };
        let template = TaskTemplate {
            depends: vec![TaskDep {
                task: "template-dep".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        task.merge_template(&template);

        // Local depends should be completely preserved (not merged)
        assert_eq!(task.depends.len(), 1);
        assert_eq!(task.depends[0].task, "local-dep");
    }
}
