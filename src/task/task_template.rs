use crate::config::config_file::mise_toml::{EnvList, deserialize_vars};
use crate::config::config_file::toml::deserialize_arr;
use crate::task::task_sources::TaskOutputs;
use crate::task::{
    RunEntry, Silent, Task, TaskCacheConfig, TaskConfirm, TaskDep, TaskOutput, TaskRustCacheConfig,
    TaskToolValue, TaskWatchOptions,
};
use indexmap::IndexMap;
use serde::Deserialize;

/// A task template definition that can be extended by tasks via `extends`
/// Templates are defined in [task_templates.*] sections of mise.toml
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct TaskTemplate {
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
    pub daemons: Option<crate::task::TaskDaemons>,
    #[serde(default)]
    pub env: EnvList,
    #[serde(default, deserialize_with = "deserialize_vars")]
    pub vars: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub watch: Option<TaskWatchOptions>,
    #[serde(default)]
    pub outputs: TaskOutputs,
    #[serde(default)]
    pub cache: Option<TaskCacheConfig>,
    #[serde(default)]
    pub rust_cache: Option<TaskRustCacheConfig>,
    #[serde(default)]
    pub output: Option<TaskOutput>,
    #[serde(default)]
    pub shell: Option<String>,
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
    /// Preserve ambient env vars when env inheritance is denied without hashing their values
    #[serde(default)]
    pub pass_through_env: Vec<String>,
}

/// What a merge does with the template's `usage` spec.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UsageMerge {
    /// Prepend the template's spec to the task's. For a template the task named with
    /// `extends`: it asked for what the template declares, so adding a flag of its own must
    /// not drop the shared ones.
    Compose,
    /// Use the template's spec only when the task has none, like every other field a
    /// workspace default contributes. Composing there could add a required argument to a
    /// command that parsed fine before, which is not what "fills anything still unset" means.
    FillOnly,
}

impl Task {
    /// Merge a template into this task, using template values only where the task
    /// doesn't already have values set. This allows tasks to override template values.
    ///
    /// This is the fill-only merge, used for a workspace-root task default. For a template
    /// the task named with `extends`, see [`Self::merge_extended_template`], which differs
    /// only in what it does with `usage`.
    ///
    /// Merge semantics:
    /// - run, run_windows: Local overrides completely (if non-empty)
    /// - tools: Deep merge (local tools added/override template)
    /// - env: Deep merge (template first, then local overrides)
    /// - vars: Deep merge (template first, then local overrides)
    /// - depends, depends_post, wait_for: Local overrides completely (if non-empty)
    /// - dir: Local overrides; defaults to None if not in template
    /// - sources, outputs: Local overrides completely (if non-empty)
    /// - usage: Template spec used only when the task has none
    /// - Other fields: Local overrides template (if set)
    pub(crate) fn merge_template(&mut self, template: &TaskTemplate) {
        self.merge_template_with(template, UsageMerge::FillOnly)
    }

    /// Merge a template the task named with `extends`.
    ///
    /// Every field behaves as it does in [`Self::merge_template`], except `usage`: the
    /// template's spec is prepended to the task's rather than used only when the task has
    /// none, so a task adding a flag of its own keeps the shared ones.
    pub(crate) fn merge_extended_template(&mut self, template: &TaskTemplate) {
        self.merge_template_with(template, UsageMerge::Compose)
    }

    fn merge_template_with(&mut self, template: &TaskTemplate, usage_merge: UsageMerge) {
        // A task whose command is a script file does not also get a `run`. `file` wins over
        // `run` in the executor, so a task holding both carries a script that can never
        // execute and yet shows up wherever the task is described.
        //
        // Either side can supply the `file`: the task's own (every file task, which reaches a
        // template through `#MISE extends=...` with `run` necessarily empty), or the
        // template's, which is merged further down and would otherwise arrive *after* its own
        // `run` had already been copied onto a task that had neither.
        let command_is_a_file = self.file.is_some() || template.file.is_some();

        // run: only use template if local is empty
        if self.run.is_empty() && !command_is_a_file {
            self.run = template.run.clone();
        }

        // run_windows: only use template if local is empty
        if self.run_windows.is_empty() && !command_is_a_file {
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

        // daemons: local overrides; use template only if local not set
        if self.daemons.is_none() {
            self.daemons = template.daemons.clone();
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

        if self.watch.is_none() {
            self.watch = template.watch.clone();
        }

        // outputs: local overrides completely if default
        if self.outputs == TaskOutputs::default() && template.outputs != TaskOutputs::default() {
            self.outputs = template.outputs.clone();
        }

        if self.cache.is_none() {
            self.cache = template.cache.clone();
        }

        if self.rust_cache.is_none() {
            self.rust_cache = template.rust_cache.clone();
        }

        // output: use template only if local not set
        if self.output.is_none() {
            self.output = template.output;
        }

        // shell: use template only if local not set
        if self.shell.is_none() {
            self.shell = template.shell.clone();
        }

        // Note: quiet, hide, raw, interactive, and raw_args are `bool` in Task (not
        // Option<bool>), so we cannot distinguish between "not set" (defaults to false)
        // and "explicitly set to false". Therefore, we do NOT merge these boolean
        // fields from templates to avoid the case where a task explicitly sets
        // `quiet = false` but gets overridden by a template's `quiet = true`. Users
        // must explicitly set these in their task if needed.

        // Preserve whether `silent = false` was explicitly selected. The resolved
        // value alone cannot distinguish that from an omitted `silent` field, and
        // overlays need the same provenance after a template is applied.
        if !self.toml_bool_presence.silent
            && let Some(ref silent) = template.silent
        {
            self.silent = silent.clone();
            self.toml_bool_presence.record("silent");
        }

        // usage: a task with no spec of its own takes the template's either way. A task that
        // has one keeps it, and additionally gets the template's declarations prepended when
        // the template is one it named with `extends` -- a usage spec is a list of
        // declarations, so a task adding a flag of its own should not thereby drop every
        // shared one. Template first, so the shared flags lead in `--help` and a template's
        // positional args stay ahead of the task's. Both halves are rendered together
        // afterwards, so tera in either still resolves.
        if !template.usage.is_empty() {
            if self.usage.trim().is_empty() {
                self.usage = template.usage.clone();
            } else if usage_merge == UsageMerge::Compose {
                self.usage = format!("{}\n{}", template.usage.trim_end(), self.usage.trim());
            }
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
        self.pass_through_env
            .splice(0..0, template.pass_through_env.clone());
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
    fn test_merge_template_run_not_inherited_by_file_task() {
        let mut task = Task {
            file: Some("mise-tasks/build".into()),
            ..Default::default()
        };
        let template = TaskTemplate {
            run: vec![RunEntry::Script("template command".to_string())],
            run_windows: vec![RunEntry::Script("template command".to_string())],
            description: "template description".to_string(),
            ..Default::default()
        };

        task.merge_template(&template);

        // The script file is the command, so the template's `run` is not a second one.
        assert!(task.run.is_empty());
        assert!(task.run_windows.is_empty());
        // Everything else still inherits.
        assert_eq!(task.description, "template description");
    }

    #[test]
    fn test_merge_template_run_not_inherited_when_template_has_a_file() {
        let mut task = Task::default();
        let template = TaskTemplate {
            file: Some("mise-tasks/build".to_string()),
            run: vec![RunEntry::Script("template command".to_string())],
            run_windows: vec![RunEntry::Script("template command".to_string())],
            ..Default::default()
        };

        task.merge_template(&template);

        // The template's `file` is the command it contributes; its `run` would only ever be
        // dead weight on the task, since `file` wins in the executor.
        assert_eq!(task.file, Some("mise-tasks/build".into()));
        assert!(task.run.is_empty());
        assert!(task.run_windows.is_empty());
    }

    #[test]
    fn test_merge_template_usage_is_composed() {
        let mut task = Task {
            usage: "flag \"--own <v>\"".to_string(),
            ..Default::default()
        };
        let template = TaskTemplate {
            usage: "flag \"--shared <v>\"\n".to_string(),
            ..Default::default()
        };

        task.merge_extended_template(&template);

        assert_eq!(task.usage, "flag \"--shared <v>\"\nflag \"--own <v>\"");
    }

    #[test]
    fn test_merge_template_usage_from_template_only() {
        let mut task = Task::default();
        let template = TaskTemplate {
            usage: "flag \"--shared <v>\"".to_string(),
            ..Default::default()
        };

        task.merge_extended_template(&template);

        assert_eq!(task.usage, "flag \"--shared <v>\"");
    }

    #[test]
    fn test_workspace_default_usage_only_fills() {
        let mut task = Task {
            usage: "flag \"--own <v>\"".to_string(),
            ..Default::default()
        };
        let default = TaskTemplate {
            usage: "arg \"<required>\"".to_string(),
            ..Default::default()
        };

        // A workspace default fills what the task left unset; composing here would add a
        // required argument to a command line that parsed fine before.
        task.merge_template(&default);
        assert_eq!(task.usage, "flag \"--own <v>\"");

        // With no spec of its own, the task still takes the default's.
        let mut bare = Task::default();
        bare.merge_template(&default);
        assert_eq!(bare.usage, "arg \"<required>\"");
    }

    #[test]
    fn test_merge_template_usage_without_template_spec() {
        let mut task = Task {
            usage: "flag \"--own <v>\"".to_string(),
            ..Default::default()
        };

        task.merge_extended_template(&TaskTemplate::default());

        assert_eq!(task.usage, "flag \"--own <v>\"");
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
    fn test_merge_template_output() {
        let template = TaskTemplate {
            output: Some(TaskOutput::KeepOrder),
            ..Default::default()
        };

        let mut inherited = Task::default();
        inherited.merge_template(&template);
        assert_eq!(inherited.output, Some(TaskOutput::KeepOrder));

        let mut overridden = Task {
            output: Some(TaskOutput::Interleave),
            ..Default::default()
        };
        overridden.merge_template(&template);
        assert_eq!(overridden.output, Some(TaskOutput::Interleave));
    }

    #[test]
    fn test_merge_template_watch_options() {
        let template = TaskTemplate {
            watch: Some(TaskWatchOptions {
                no_vcs_ignore: true,
            }),
            ..Default::default()
        };

        let mut inherited = Task::default();
        inherited.merge_template(&template);
        assert_eq!(inherited.watch, template.watch);

        let mut overridden = Task {
            watch: Some(TaskWatchOptions {
                no_vcs_ignore: false,
            }),
            ..Default::default()
        };
        overridden.merge_template(&template);
        assert_eq!(
            overridden.watch,
            Some(TaskWatchOptions {
                no_vcs_ignore: false
            })
        );
    }

    #[test]
    fn test_merge_template_cache_can_be_disabled_locally() {
        let template = TaskTemplate {
            cache: Some(TaskCacheConfig {
                enabled: true,
                audit: false,
                env: vec!["PROFILE".to_string()],
                command_inputs: vec![],
            }),
            ..Default::default()
        };

        let mut inherited = Task::default();
        inherited.merge_template(&template);
        assert!(inherited.cache.as_ref().is_some_and(|cache| cache.enabled));

        let mut disabled = Task {
            cache: Some(TaskCacheConfig::default()),
            ..Default::default()
        };
        disabled.merge_template(&template);
        assert_eq!(disabled.cache, Some(TaskCacheConfig::default()));
    }

    #[test]
    fn test_merge_template_depends_override() {
        let mut task = Task {
            depends: vec![TaskDep {
                task: "local-dep".to_string(),
                args: vec![],
                env: Default::default(),
                optional: false,
            }],
            ..Default::default()
        };
        let template = TaskTemplate {
            depends: vec![TaskDep {
                task: "template-dep".to_string(),
                args: vec![],
                env: Default::default(),
                optional: false,
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
            pass_through_env: vec!["TASK_SECRET".to_string()],
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
            pass_through_env: vec!["TEMPLATE_SECRET".to_string()],
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
        assert_eq!(
            task.pass_through_env,
            vec!["TEMPLATE_SECRET".to_string(), "TASK_SECRET".to_string()]
        );
    }
}
