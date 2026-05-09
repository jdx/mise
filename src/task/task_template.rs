use crate::config::config_file::mise_toml::EnvList;
use crate::config::config_file::toml::deserialize_arr;
use crate::task::task_sources::TaskOutputs;
use crate::task::{
    RunEntry, Silent, Task, TaskConfirm, TaskDep, TaskPlatformOverride, TaskToolValue,
};
use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::BTreeMap;

/// A task template definition that can be extended by tasks via `extends`
/// Templates are defined in [task_templates.*] sections of mise.toml
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskTemplate {
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "alias", deserialize_with = "deserialize_arr")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub confirm: Option<TaskConfirm>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub depends: Vec<TaskDep>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub depends_post: Vec<TaskDep>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub wait_for: Vec<TaskDep>,
    #[serde(default)]
    pub env: EnvList,
    #[serde(default)]
    pub vars: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub hide: Option<bool>,
    #[serde(default)]
    pub raw: Option<bool>,
    #[serde(default)]
    pub raw_args: Option<bool>,
    #[serde(default)]
    pub interactive: Option<bool>,
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
    pub tools: IndexMap<String, TaskToolValue>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run: Vec<RunEntry>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run_windows: Vec<RunEntry>,
    #[serde(skip)]
    pub platform_tasks: BTreeMap<String, TaskPlatformOverride>,
    #[serde(default)]
    pub file: Option<String>,
    /// Block reads, writes, network, and env vars
    #[serde(default)]
    pub deny_all: bool,
    /// Block filesystem reads
    #[serde(default)]
    pub deny_read: bool,
    /// Block all filesystem writes
    #[serde(default)]
    pub deny_write: bool,
    /// Block all network access
    #[serde(default)]
    pub deny_net: bool,
    /// Block env var inheritance
    #[serde(default)]
    pub deny_env: bool,
    /// Allow reads from specific paths
    #[serde(default)]
    pub allow_read: Vec<std::path::PathBuf>,
    /// Allow writes to specific paths
    #[serde(default)]
    pub allow_write: Vec<std::path::PathBuf>,
    /// Allow network to specific hosts
    #[serde(default)]
    pub allow_net: Vec<String>,
    /// Allow specific env vars through
    #[serde(default)]
    pub allow_env: Vec<String>,
}

fn platform_key_covers(local_key: &str, template_key: &str) -> bool {
    if local_key == template_key {
        return true;
    }
    if !local_key.contains('/') {
        return template_key
            .strip_prefix(local_key)
            .is_some_and(|rest| rest.starts_with('/'));
    }
    false
}

impl Task {
    /// Merge a template into this task, using template values only where the task
    /// doesn't already have values set. This allows tasks to override template values.
    ///
    /// Merge semantics:
    /// - run, run_windows, platform override fields: Local overrides where set
    /// - tools: Deep merge (local tools added/override template)
    /// - env: Deep merge (template first, then local overrides)
    /// - vars: Deep merge (template first, then local overrides)
    /// - depends, depends_post, wait_for: Local overrides completely (if non-empty)
    /// - dir: Local overrides; defaults to None if not in template
    /// - sources, outputs: Local overrides completely (if non-empty)
    /// - Other fields: Local overrides template (if set)
    pub fn merge_template(&mut self, template: &TaskTemplate) {
        let has_local_run = !self.run.is_empty();
        let has_local_shell = self.shell.is_some();

        // run: only use template if local is empty
        if !has_local_run {
            self.run = template.run.clone();
        }

        let local_run_windows = !self.run_windows.is_empty();
        let local_platform_tasks = self.platform_tasks.clone();

        // run_windows: only use template if local run/run_windows/windows platform run overrides are empty
        if !has_local_run
            && !local_run_windows
            && !local_platform_tasks.iter().any(|(key, platform)| {
                platform.run.is_some() && platform_key_covers(key, "windows")
            })
        {
            self.run_windows = template.run_windows.clone();
        }

        // platform_tasks: merge per field. A local base `run` suppresses all
        // template platform runs, but not template platform shells. A local
        // platform `run` only overrides covered template `run` fields; a local
        // `shell` only overrides covered template `shell` fields. Legacy local
        // run_windows also overrides template Windows platform runs, but not
        // template Windows shells.
        let mut platform_tasks = BTreeMap::new();

        for (template_key, template_platform) in &template.platform_tasks {
            let local_run_covers =
                local_platform_tasks
                    .iter()
                    .any(|(local_key, local_platform)| {
                        local_platform.run.is_some() && platform_key_covers(local_key, template_key)
                    });
            let local_shell_covers =
                local_platform_tasks
                    .iter()
                    .any(|(local_key, local_platform)| {
                        local_platform.shell.is_some()
                            && platform_key_covers(local_key, template_key)
                    });

            let run = if has_local_run
                || local_run_covers
                || (local_run_windows && platform_key_covers("windows", template_key))
            {
                None
            } else {
                template_platform.run.clone()
            };
            let shell = if has_local_shell || local_shell_covers {
                None
            } else {
                template_platform.shell.clone()
            };

            if run.is_some() || shell.is_some() {
                platform_tasks.insert(template_key.clone(), TaskPlatformOverride { run, shell });
            }
        }

        for (local_key, local_platform) in local_platform_tasks {
            let platform = platform_tasks.entry(local_key).or_default();
            if local_platform.run.is_some() {
                platform.run = local_platform.run;
            }
            if local_platform.shell.is_some() {
                platform.shell = local_platform.shell;
            }
        }

        self.platform_tasks = platform_tasks;

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

        // vars: deep merge (template first, then local overrides)
        let mut merged_vars = template.vars.clone();
        merged_vars.0.extend(self.vars.0.clone());
        self.vars = merged_vars;

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

        if template.raw_args == Some(true) {
            self.raw_args = true;
        }
        if template.interactive == Some(true) {
            self.interactive = true;
        }

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

        // sandbox: restrictions compose with task-local settings, matching how
        // task and global sandbox config are combined in the executor.
        self.deny_all |= template.deny_all;
        self.deny_read |= template.deny_read;
        self.deny_write |= template.deny_write;
        self.deny_net |= template.deny_net;
        self.deny_env |= template.deny_env;

        self.allow_read.splice(0..0, template.allow_read.clone());
        self.allow_write.splice(0..0, template.allow_write.clone());
        self.allow_net.splice(0..0, template.allow_net.clone());
        self.allow_env.splice(0..0, template.allow_env.clone());
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
    fn test_merge_template_platform_overrides_per_key() {
        let mut task = Task {
            platform_tasks: BTreeMap::from([(
                "linux".to_string(),
                TaskPlatformOverride {
                    run: Some(vec![RunEntry::Script("local linux".to_string())]),
                    shell: None,
                },
            )]),
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([
                (
                    "linux".to_string(),
                    TaskPlatformOverride {
                        run: Some(vec![RunEntry::Script("template linux".to_string())]),
                        shell: None,
                    },
                ),
                (
                    "windows".to_string(),
                    TaskPlatformOverride {
                        run: Some(vec![RunEntry::Script("template windows".to_string())]),
                        shell: None,
                    },
                ),
            ]),
            ..Default::default()
        };

        task.merge_template(&template);

        assert!(matches!(
            &task.platform_tasks["linux"].run.as_ref().unwrap()[0],
            RunEntry::Script(s) if s == "local linux"
        ));
        assert!(matches!(
            &task.platform_tasks["windows"].run.as_ref().unwrap()[0],
            RunEntry::Script(s) if s == "template windows"
        ));
    }

    #[test]
    fn test_merge_template_platform_overrides_per_field() {
        let mut task = Task {
            platform_tasks: BTreeMap::from([(
                "linux".to_string(),
                TaskPlatformOverride {
                    run: None,
                    shell: Some("bash -c".to_string()),
                },
            )]),
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([(
                "linux".to_string(),
                TaskPlatformOverride {
                    run: Some(vec![RunEntry::Script("template linux".to_string())]),
                    shell: Some("sh -c".to_string()),
                },
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        let linux = &task.platform_tasks["linux"];
        assert!(matches!(
            &linux.run.as_ref().unwrap()[0],
            RunEntry::Script(s) if s == "template linux"
        ));
        assert_eq!(linux.shell.as_deref(), Some("bash -c"));
    }

    #[test]
    fn test_merge_template_run_windows_keeps_template_windows_platform_shell() {
        let mut task = Task {
            run_windows: vec![RunEntry::Script("local windows".to_string())],
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([(
                "windows".to_string(),
                TaskPlatformOverride {
                    run: Some(vec![RunEntry::Script("template windows".to_string())]),
                    shell: Some("pwsh -Command".to_string()),
                },
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        assert_eq!(task.run_windows.len(), 1);
        let windows = &task.platform_tasks["windows"];
        assert!(windows.run.is_none());
        assert_eq!(windows.shell.as_deref(), Some("pwsh -Command"));
    }

    #[test]
    fn test_merge_template_local_run_keeps_template_platform_shell() {
        let mut task = Task {
            run: vec![RunEntry::Script("local run".to_string())],
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([(
                "windows".to_string(),
                TaskPlatformOverride {
                    run: Some(vec![RunEntry::Script("template windows".to_string())]),
                    shell: Some("pwsh -Command".to_string()),
                },
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        assert_eq!(task.run.len(), 1);
        let windows = &task.platform_tasks["windows"];
        assert!(windows.run.is_none());
        assert_eq!(windows.shell.as_deref(), Some("pwsh -Command"));
    }

    #[test]
    fn test_merge_template_local_shell_overrides_template_platform_shell() {
        let mut task = Task {
            shell: Some("bash -c".to_string()),
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([(
                "windows".to_string(),
                TaskPlatformOverride {
                    run: None,
                    shell: Some("pwsh -Command".to_string()),
                },
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        assert_eq!(task.shell.as_deref(), Some("bash -c"));
        assert!(!task.platform_tasks.contains_key("windows"));
    }

    #[test]
    fn test_merge_template_run_windows_overrides_template_windows_platform() {
        let mut task = Task {
            run_windows: vec![RunEntry::Script("local windows".to_string())],
            ..Default::default()
        };
        let template = TaskTemplate {
            platform_tasks: BTreeMap::from([(
                "windows".to_string(),
                TaskPlatformOverride {
                    run: Some(vec![RunEntry::Script("template windows".to_string())]),
                    shell: None,
                },
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        assert_eq!(task.run_windows.len(), 1);
        assert!(!task.platform_tasks.contains_key("windows"));
    }

    #[test]
    fn test_merge_template_raw_args_and_interactive() {
        let mut task = Task::default();
        let template = TaskTemplate {
            raw_args: Some(true),
            interactive: Some(true),
            ..Default::default()
        };

        task.merge_template(&template);

        assert!(task.raw_args);
        assert!(task.interactive);
    }

    #[test]
    fn test_merge_template_tools_deep_merge() {
        let mut task = Task {
            tools: IndexMap::from([("node".to_string(), TaskToolValue::String("20".to_string()))]),
            ..Default::default()
        };
        let template = TaskTemplate {
            tools: IndexMap::from([
                (
                    "python".to_string(),
                    TaskToolValue::String("3.12".to_string()),
                ),
                ("node".to_string(), TaskToolValue::String("18".to_string())), // Should be overridden by task
            ]),
            ..Default::default()
        };

        task.merge_template(&template);

        // Should have both tools, with task's node version
        assert_eq!(task.tools.len(), 2);
        assert_eq!(
            task.tools.get("node"),
            Some(&TaskToolValue::String("20".to_string()))
        );
        assert_eq!(
            task.tools.get("python"),
            Some(&TaskToolValue::String("3.12".to_string()))
        );
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

    #[test]
    fn test_merge_template_vars_deep_merge() {
        let mut task = Task {
            vars: EnvList(vec![crate::config::env_directive::EnvDirective::Val(
                "target".to_string(),
                "linux".to_string(),
                Default::default(),
            )]),
            ..Default::default()
        };
        let template = TaskTemplate {
            vars: EnvList(vec![crate::config::env_directive::EnvDirective::Val(
                "profile".to_string(),
                "release".to_string(),
                Default::default(),
            )]),
            ..Default::default()
        };

        task.merge_template(&template);

        // Should contain template vars + local vars (local appended)
        assert_eq!(task.vars.0.len(), 2);
    }

    #[test]
    fn test_merge_template_vars_override() {
        let mut task = Task {
            vars: EnvList(vec![
                crate::config::env_directive::EnvDirective::Val(
                    "target".to_string(),
                    "linux".to_string(),
                    Default::default(),
                ),
                crate::config::env_directive::EnvDirective::Val(
                    "shared".to_string(),
                    "task_value".to_string(),
                    Default::default(),
                ),
            ]),
            ..Default::default()
        };
        let template = TaskTemplate {
            vars: EnvList(vec![
                crate::config::env_directive::EnvDirective::Val(
                    "profile".to_string(),
                    "release".to_string(),
                    Default::default(),
                ),
                crate::config::env_directive::EnvDirective::Val(
                    "shared".to_string(),
                    "template_value".to_string(),
                    Default::default(),
                ),
            ]),
            ..Default::default()
        };

        task.merge_template(&template);

        // Last matching directive should win when vars are resolved.
        let shared_val = task.vars.0.iter().rev().find_map(|d| match d {
            crate::config::env_directive::EnvDirective::Val(name, value, _) if name == "shared" => {
                Some(value.as_str())
            }
            _ => None,
        });
        assert_eq!(shared_val, Some("task_value"));
    }

    #[test]
    fn test_merge_template_sandbox_config() {
        let mut task = Task {
            deny_net: true,
            allow_read: vec!["task-read".into()],
            allow_env: vec!["TASK_*".to_string()],
            ..Default::default()
        };
        let template = TaskTemplate {
            deny_all: true,
            deny_read: true,
            deny_write: true,
            deny_env: true,
            allow_read: vec!["template-read".into()],
            allow_write: vec!["template-write".into()],
            allow_net: vec!["example.com".to_string()],
            allow_env: vec!["TEMPLATE_*".to_string()],
            ..Default::default()
        };

        task.merge_template(&template);

        assert!(task.deny_all);
        assert!(task.deny_read);
        assert!(task.deny_write);
        assert!(task.deny_net);
        assert!(task.deny_env);
        assert_eq!(
            task.allow_read,
            vec![
                std::path::PathBuf::from("template-read"),
                std::path::PathBuf::from("task-read")
            ]
        );
        assert_eq!(
            task.allow_write,
            vec![std::path::PathBuf::from("template-write")]
        );
        assert_eq!(task.allow_net, vec!["example.com".to_string()]);
        assert_eq!(
            task.allow_env,
            vec!["TEMPLATE_*".to_string(), "TASK_*".to_string()]
        );
    }
}
