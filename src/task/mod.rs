use crate::cli::version::VERSION;
use crate::config::config_file::mise_toml::EnvList;
use crate::config::config_file::toml::{TrackingTomlParser, deserialize_arr};
use crate::config::env_directive::{EnvDirective, EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::{self, Config};
use crate::path_env::PathEnv;
use crate::task::task_script_parser::TaskScriptParser;
use crate::tera::get_tera;
use crate::ui::tree::TreeItem;
use crate::{dirs, env, file};
use console::{Color, measure_text_width, truncate_str};
use eyre::{Result, bail, eyre};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use globset::GlobBuilder;
use indexmap::IndexMap;
use itertools::Itertools;
use petgraph::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock as Lazy;
use std::{ffi, fmt, path};
use xx::regex;

static FUZZY_MATCHER: Lazy<SkimMatcherV2> =
    Lazy::new(|| SkimMatcherV2::default().use_cache(true).smart_case());

/// Type alias for tracking failed tasks with their exit codes
pub type FailedTasks = Arc<std::sync::Mutex<Vec<(Task, Option<i32>)>>>;

mod deps;
pub mod task_context_builder;
mod task_dep;
pub mod task_executor;
pub mod task_fetcher;
pub mod task_file_providers;
pub mod task_helpers;
pub mod task_list;
mod task_load_context;
pub mod task_output;
pub mod task_output_handler;
pub mod task_results_display;
pub mod task_scheduler;
mod task_script_parser;
pub mod task_source_checker;
pub mod task_sources;
pub mod task_template;
pub mod task_tool_installer;

pub use task_load_context::{TaskLoadContext, expand_colon_task_syntax};
pub use task_output::TaskOutput;
pub use task_script_parser::has_any_args_defined;
pub use task_template::TaskTemplate;

use crate::config::config_file::ConfigFile;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::toolset::Toolset;
use crate::ui::style;
pub use deps::Deps;
use task_dep::TaskDep;
use task_sources::{RawOutputTemplates, TaskOutputs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(untagged)]
pub enum RunEntry {
    /// Shell script entry
    Script(String),
    /// Run a single task with optional args
    SingleTask { task: String },
    /// Run multiple tasks in parallel
    TaskGroup { tasks: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Silent {
    #[default]
    Off,
    Bool(bool),
    Stdout,
    Stderr,
}

impl Silent {
    pub fn is_silent(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stdout | Silent::Stderr)
    }

    pub fn suppresses_stdout(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stdout)
    }

    pub fn suppresses_stderr(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stderr)
    }

    pub fn suppresses_both(&self) -> bool {
        matches!(self, Silent::Bool(true))
    }
}

impl Serialize for Silent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Silent::Off | Silent::Bool(false) => serializer.serialize_bool(false),
            Silent::Bool(true) => serializer.serialize_bool(true),
            Silent::Stdout => serializer.serialize_str("stdout"),
            Silent::Stderr => serializer.serialize_str("stderr"),
        }
    }
}

impl From<bool> for Silent {
    fn from(b: bool) -> Self {
        if b { Silent::Bool(true) } else { Silent::Off }
    }
}

impl std::str::FromStr for Silent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "true" => Ok(Silent::Bool(true)),
            "false" => Ok(Silent::Off),
            "stdout" => Ok(Silent::Stdout),
            "stderr" => Ok(Silent::Stderr),
            _ => Err(format!(
                "invalid silent value: {}, expected true, false, 'stdout', or 'stderr'",
                s
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Silent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SilentVisitor;

        impl<'de> serde::de::Visitor<'de> for SilentVisitor {
            type Value = Silent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a boolean or a string ('stdout' or 'stderr')")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Silent, E>
            where
                E: serde::de::Error,
            {
                Ok(Silent::from(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Silent, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "stdout" => Ok(Silent::Stdout),
                    "stderr" => Ok(Silent::Stderr),
                    _ => Err(E::custom(format!(
                        "invalid silent value: '{}', expected 'stdout' or 'stderr'",
                        value
                    ))),
                }
            }
        }

        deserializer.deserialize_any(SilentVisitor)
    }
}

impl std::str::FromStr for RunEntry {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RunEntry::Script(s.to_string()))
    }
}

impl Display for RunEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RunEntry::Script(s) => write!(f, "{}", s),
            RunEntry::SingleTask { task } => write!(f, "task: {task}"),
            RunEntry::TaskGroup { tasks } => write!(f, "tasks: {}", tasks.join(", ")),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "alias", deserialize_with = "deserialize_arr")]
    pub aliases: Vec<String>,
    #[serde(skip)]
    pub config_source: PathBuf,
    #[serde(skip)]
    pub cf: Option<Arc<dyn ConfigFile>>,
    #[serde(skip)]
    pub config_root: Option<PathBuf>,
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
    /// Env vars inherited from parent tasks at runtime (not used for task identity/deduplication)
    #[serde(skip)]
    pub inherited_env: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub hide: bool,
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub raw: bool,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub outputs: TaskOutputs,
    #[serde(skip)]
    pub raw_outputs: RawOutputTemplates,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub silent: Silent,
    #[serde(default)]
    pub tools: IndexMap<String, String>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub timeout: Option<String>,

    // normal type
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run: Vec<RunEntry>,

    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run_windows: Vec<RunEntry>,

    // command type
    // pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,

    // script type
    // pub script: Option<String>,

    // file type
    #[serde(default)]
    pub file: Option<PathBuf>,

    // Store the original remote file source (git::/http:/https:) before it's replaced with local path
    // This is used to determine if the task should use monorepo config file context
    #[serde(skip)]
    pub remote_file_source: Option<String>,

    /// Name of the task template to extend (requires experimental = true)
    #[serde(default)]
    pub extends: Option<String>,
}

impl Task {
    pub fn new(path: &Path, prefix: &Path, config_root: &Path) -> Result<Task> {
        Ok(Self {
            name: name_from_path(prefix, path)?,
            config_source: path.to_path_buf(),
            config_root: Some(config_root.to_path_buf()),
            ..Default::default()
        })
    }

    pub async fn from_path(
        config: &Arc<Config>,
        path: &Path,
        prefix: &Path,
        config_root: &Path,
    ) -> Result<Task> {
        let mut task = Task::new(path, prefix, config_root)?;
        let info = file::read_to_string(path)?
            .lines()
            .filter_map(|line| {
                debug_assert!(
                    !VERSION.starts_with("2026.3"),
                    "remove old syntax `# mise`"
                );
                if let Some(captures) =
                    regex!(r"^(?:#|//|::)(?:MISE| ?\[MISE\]) ([a-z0-9_.-]+=[^\n]+)$").captures(line)
                {
                    Some(captures)
                } else if let Some(captures) = regex!(r"^(?:#|//) mise ([a-z0-9_.-]+=[^\n]+)$")
                    .captures(line)
                {
                    deprecated!(
                        "file_task_headers_old_syntax",
                        "The `# mise ...` syntax for task headers is deprecated and will be removed in mise 2026.3.0. Use the new `#MISE ...` syntax instead."
                    );
                    Some(captures)
                } else {
                    None
                }
            })
            .map(|captures| captures.extract().1)
            .map(|[toml]| {
                toml::de::from_str::<toml::Value>(toml)
                    .map_err(|e| eyre::eyre!("failed to parse task header TOML {toml:?}: {e}"))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|toml| toml.as_table().cloned())
            .flatten()
            .fold(toml::Table::new(), |mut map, (key, value)| {
                // Deep-merge tables when both existing and new values are tables
                // This allows multiple #MISE lines like:
                //   #MISE tools.terraform="1"
                //   #MISE tools.tflint="0"
                // to be merged into a single tools table
                // See: https://github.com/jdx/mise/discussions/7839
                if let Some(existing) = map.get_mut(&key) {
                    if let (toml::Value::Table(existing_table), toml::Value::Table(new_table)) =
                        (existing, &value)
                    {
                        for (k, v) in new_table {
                            existing_table.insert(k.clone(), v.clone());
                        }
                    } else {
                        map.insert(key, value);
                    }
                } else {
                    map.insert(key, value);
                }
                map
            });
        let info = toml::Value::Table(info);

        let mut p = TrackingTomlParser::new(&info);
        // trace!("task info: {:#?}", info);

        task.description = p.parse_str("description").unwrap_or_default();
        // Check for multiple alias fields before parsing
        let alias_fields: Vec<&str> = ["alias", "aliases"]
            .iter()
            .filter(|&field| info.get(field).is_some())
            .copied()
            .collect();

        if alias_fields.len() > 1 {
            return Err(eyre::eyre!(
                "Cannot define both 'alias' and 'aliases' fields in task file header: {}. Use only one.",
                display_path(path)
            ));
        }

        task.aliases = p
            .parse_array("alias")
            .or(p.parse_array("aliases"))
            .or(p.parse_str("alias").map(|s| vec![s]))
            .or(p.parse_str("aliases").map(|s| vec![s]))
            .unwrap_or_default();
        task.confirm = p.parse_str("confirm");
        task.depends = p.parse_array("depends").unwrap_or_default();
        task.depends_post = p.parse_array("depends_post").unwrap_or_default();
        task.wait_for = p.parse_array("wait_for").unwrap_or_default();
        task.env = p.parse_env("env")?.unwrap_or_default();
        task.dir = p.parse_str("dir");
        task.hide = !file::is_executable(path) || p.parse_bool("hide").unwrap_or_default();
        task.raw = p.parse_bool("raw").unwrap_or_default();
        task.sources = p.parse_array("sources").unwrap_or_default();
        task.outputs = p.get_raw("outputs").map(|to| to.into()).unwrap_or_default();
        task.file = Some(path.to_path_buf());
        task.shell = p.parse_str("shell");
        task.quiet = p.parse_bool("quiet").unwrap_or_default();
        task.silent = p
            .get_raw("silent")
            .and_then(|v| Silent::deserialize(v.clone()).ok())
            .unwrap_or_default();
        task.tools = p
            .parse_table("tools")
            .map(|t| {
                t.into_iter()
                    .filter_map(|(k, v)| v.as_str().map(|vs| (k, vs.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let mut unparsed = p.unparsed_keys();
        unparsed.sort();

        if !unparsed.is_empty() {
            return Err(eyre::eyre!(
                "unknown field(s) {:?} in task file header: {}",
                unparsed,
                display_path(path)
            ));
        }

        #[cfg(test)]
        {
            let fields: Vec<String> = p.parsed_keys().map(|s| s.to_string()).collect();
            tests::capture_parsed_fields(fields);
        }
        task.render(config, config_root).await?;
        Ok(task)
    }

    /// Add env vars that were inherited from parent tasks (e.g., via `run = [{ task = "..." }]`)
    /// These do NOT affect task identity/deduplication
    pub fn derive_env(&self, env_directives: &[EnvDirective]) -> Self {
        let mut new_task = self.clone();
        new_task.inherited_env.0.extend_from_slice(env_directives);
        new_task
    }

    /// Add env vars specified in dependency declarations (e.g., `depends = ["FOO=bar task"]`)
    /// These DO affect task identity/deduplication
    pub fn with_dependency_env(&self, env_directives: &[EnvDirective]) -> Self {
        let mut new_task = self.clone();
        new_task.env.0.extend_from_slice(env_directives);
        new_task
    }

    /// prints the task name without an extension
    pub fn display_name(&self, all_tasks: &BTreeMap<String, Task>) -> String {
        // For task names, only strip extensions after the last colon (:)
        // This handles monorepo task names like "//projects/my.app:build.sh"
        // where we want to strip ".sh" but keep "my.app" intact
        let display_name = if let Some((prefix, task_part)) = self.name.rsplit_once(':') {
            // Has a colon separator (e.g., "//projects/my.app:build.sh")
            // Strip extension from the task part only
            let task_without_ext = task_part.rsplitn(2, '.').last().unwrap_or_default();
            format!("{}:{}", prefix, task_without_ext)
        } else {
            // No colon separator (e.g., "build.sh")
            // Strip extension from the whole name
            self.name
                .rsplitn(2, '.')
                .last()
                .unwrap_or_default()
                .to_string()
        };

        if all_tasks.contains_key(&display_name) {
            // this means another task has the name without an extension so use the full name
            self.name.clone()
        } else {
            display_name
        }
    }

    pub fn is_match(&self, pat: &str) -> bool {
        if self.name == pat || self.aliases.contains(&pat.to_string()) {
            return true;
        }

        // For pattern matching, we need to handle several cases:
        // 1. Simple pattern (e.g., "build") should match monorepo tasks (e.g., "//projects/my.app:build")
        // 2. Full pattern (e.g., "//projects/my.app:build") should only match exact path
        // 3. Extensions should be stripped for comparison

        let matches = if let Some((prefix, task_part)) = self.name.rsplit_once(':') {
            // Task name has a colon (e.g., "//projects/my.app:build.sh")
            let task_stripped = task_part.rsplitn(2, '.').last().unwrap_or_default();

            if let Some((pat_prefix, pat_task)) = pat.rsplit_once(':') {
                // Pattern also has a colon - compare full paths
                let pat_task_stripped = pat_task.rsplitn(2, '.').last().unwrap_or_default();
                prefix == pat_prefix && task_stripped == pat_task_stripped
            } else {
                // Pattern is simple (no colon) - just compare task names
                let pat_stripped = pat.rsplitn(2, '.').last().unwrap_or_default();
                task_stripped == pat_stripped
            }
        } else {
            // Simple task name without colon (e.g., "build.sh")
            let name_stripped = self.name.rsplitn(2, '.').last().unwrap_or_default();
            let pat_stripped = pat.rsplitn(2, '.').last().unwrap_or_default();
            name_stripped == pat_stripped
        };

        matches || self.aliases.contains(&pat.to_string())
    }

    pub async fn task_dir() -> PathBuf {
        let config = Config::get().await.unwrap();
        let cwd = dirs::CWD.clone().unwrap_or_default();
        let project_root = config.project_root.clone().unwrap_or(cwd);
        for dir in config::task_includes_for_dir(&project_root, &config.config_files) {
            if dir.is_dir() && project_root.join(&dir).exists() {
                return project_root.join(dir);
            }
        }
        project_root.join("mise-tasks")
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn prefix(&self) -> String {
        format!("[{}]", self.display_name)
    }

    pub fn run(&self) -> &Vec<RunEntry> {
        if cfg!(windows) && !self.run_windows.is_empty() {
            &self.run_windows
        } else {
            &self.run
        }
    }

    /// Returns only the script strings from the run entries (without rendering)
    pub fn run_script_strings(&self) -> Vec<String> {
        self.run()
            .iter()
            .filter_map(|e| match e {
                RunEntry::Script(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn all_depends(&self, tasks: &BTreeMap<String, Task>) -> Result<Vec<Task>> {
        let tasks_ref = build_task_ref_map(tasks.iter());
        let mut path = vec![self.name.clone()];
        self.all_depends_recursive(&tasks_ref, &mut path)
    }

    fn all_depends_recursive(
        &self,
        tasks: &BTreeMap<String, &Task>,
        path: &mut Vec<String>,
    ) -> Result<Vec<Task>> {
        let mut depends: Vec<Task> = self
            .depends
            .iter()
            .chain(self.depends_post.iter())
            .map(|td| match_tasks_with_context(tasks, td, Some(self)))
            .flatten_ok()
            .filter_ok(|t| t.name != self.name)
            .collect::<Result<Vec<_>>>()?;

        // Collect transitive dependencies with cycle detection
        for dep in depends.clone() {
            if path.contains(&dep.name) {
                // Circular dependency detected - build path string for error message
                let cycle_path = path
                    .iter()
                    .skip_while(|&name| name != &dep.name)
                    .chain(std::iter::once(&dep.name))
                    .join(" -> ");
                return Err(eyre!("circular dependency detected: {}", cycle_path));
            }
            path.push(dep.name.clone());
            let mut extra = dep.all_depends_recursive(tasks, path)?;
            path.pop(); // Remove from path after processing this branch
            extra.retain(|t| t.name != self.name); // prevent depending on ourself
            depends.extend(extra);
        }
        let depends = depends.into_iter().unique().collect();
        Ok(depends)
    }

    pub async fn resolve_depends(
        &self,
        config: &Arc<Config>,
        tasks_to_run: &[Task],
    ) -> Result<(Vec<Task>, Vec<Task>)> {
        use crate::task::TaskLoadContext;

        let tasks_to_run: HashSet<&Task> = tasks_to_run.iter().collect();

        // Build context with path hints from self, tasks_to_run, and dependency patterns
        // Resolve patterns before extracting paths to handle local deps (e.g., ":A")
        let path_hints: Vec<String> = once(&self.name)
            .chain(tasks_to_run.iter().map(|t| &t.name))
            .filter_map(|name| extract_monorepo_path(name))
            .chain(
                self.depends
                    .iter()
                    .chain(self.wait_for.iter())
                    .chain(self.depends_post.iter())
                    .map(|td| resolve_task_pattern(&td.task, Some(self)))
                    .filter_map(|resolved| extract_monorepo_path(&resolved)),
            )
            .unique()
            .collect();

        let ctx = if !path_hints.is_empty() {
            Some(TaskLoadContext {
                path_hints,
                load_all: false,
            })
        } else {
            None
        };

        let all_tasks = config.tasks_with_context(ctx.as_ref()).await?;
        let tasks = build_task_ref_map(all_tasks.iter());
        let depends = self
            .depends
            .iter()
            .map(|td| match_tasks_with_context(&tasks, td, Some(self)))
            .flatten_ok()
            .collect_vec();
        let wait_for = self
            .wait_for
            .iter()
            .map(|td| {
                match_tasks_with_context(&tasks, td, Some(self))
                    .map(|tasks| tasks.into_iter().map(|t| (t, td)).collect_vec())
            })
            .flatten_ok()
            .filter_map_ok(|(t, td)| {
                if td.env.is_empty() && td.args.is_empty() {
                    // Name-based matching: wait for any running instance of this task
                    // regardless of env/args variant (e.g., "VERBOSE=1 setup" matches "setup").
                    // Return the actual task from tasks_to_run so the dependency graph
                    // gets the correct env/args-variant node.
                    tasks_to_run
                        .iter()
                        .find(|tr| tr.name == t.name)
                        .map(|tr| (*tr).clone())
                } else {
                    // Full identity matching: user explicitly wants a specific env/args variant
                    tasks_to_run.contains(&t).then_some(t)
                }
            })
            .collect_vec();
        let depends_post = self
            .depends_post
            .iter()
            .map(|td| match_tasks_with_context(&tasks, td, Some(self)))
            .flatten_ok()
            .filter_ok(|t| t.name != self.name)
            .collect::<Result<Vec<_>>>()?;
        let depends = depends
            .into_iter()
            .chain(wait_for)
            .filter_ok(|t| t.name != self.name)
            .collect::<Result<_>>()?;
        Ok((depends, depends_post))
    }

    fn populate_spec_metadata(&self, spec: &mut usage::Spec) {
        spec.name = self.display_name.clone();
        spec.bin = self.display_name.clone();
        if spec.cmd.help.is_none() {
            spec.cmd.help = Some(self.description.clone());
        }
        spec.cmd.name = self.display_name.clone();
        spec.cmd.aliases = self.aliases.clone();
        if spec.cmd.before_help.is_none()
            && spec.cmd.before_help_long.is_none()
            && !self.depends.is_empty()
        {
            spec.cmd.before_help_long =
                Some(format!("- Depends: {}", self.depends.iter().join(", ")));
        }
        spec.cmd.usage = spec.cmd.usage();
    }

    pub async fn parse_usage_spec(
        &self,
        config: &Arc<Config>,
        cwd: Option<PathBuf>,
        env: &EnvMap,
    ) -> Result<(usage::Spec, Vec<String>)> {
        let (mut spec, scripts) = if let Some(file) = self.file_path(config).await? {
            let spec = usage::Spec::parse_script(&file)
                .inspect_err(|e| {
                    warn!(
                        "failed to parse task file {} with usage: {e:?}",
                        file::display_path(&file)
                    )
                })
                .unwrap_or_default();
            (spec, vec![])
        } else {
            let scripts_only = self.run_script_strings();
            let (scripts, spec) = TaskScriptParser::new(cwd)
                .parse_run_scripts(config, self, &scripts_only, env)
                .await?;
            (spec, scripts)
        };
        self.populate_spec_metadata(&mut spec);
        Ok((spec, scripts))
    }

    /// Parse usage spec for display purposes without expensive environment rendering
    pub async fn parse_usage_spec_for_display(&self, config: &Arc<Config>) -> Result<usage::Spec> {
        let dir = self.dir(config).await?;
        let mut spec = if let Some(file) = self.file_path(config).await? {
            usage::Spec::parse_script(&file)
                .inspect_err(|e| {
                    warn!(
                        "failed to parse task file {} with usage: {e:?}",
                        file::display_path(&file)
                    )
                })
                .unwrap_or_default()
        } else {
            let scripts_only = self.run_script_strings();
            TaskScriptParser::new(dir)
                .parse_run_scripts_for_spec_only(config, self, &scripts_only)
                .await?
        };
        self.populate_spec_metadata(&mut spec);
        Ok(spec)
    }

    pub async fn render_run_scripts_with_args(
        &self,
        config: &Arc<Config>,
        cwd: Option<PathBuf>,
        args: &[String],
        env: &EnvMap,
    ) -> Result<Vec<(String, Vec<String>)>> {
        let (spec, scripts) = self.parse_usage_spec(config, cwd.clone(), env).await?;
        if has_any_args_defined(&spec) {
            let scripts_only = self.run_script_strings();
            let scripts = TaskScriptParser::new(cwd)
                .parse_run_scripts_with_args(config, self, &scripts_only, env, args, &spec)
                .await?;
            Ok(scripts.into_iter().map(|s| (s, vec![])).collect())
        } else {
            Ok(scripts
                .iter()
                .enumerate()
                .map(|(i, script)| {
                    // only pass args to the last script if no formal args are defined
                    match i == self.run_script_strings().len() - 1 {
                        true => (script.clone(), args.iter().cloned().collect_vec()),
                        false => (script.clone(), vec![]),
                    }
                })
                .collect())
        }
    }

    pub async fn render_markdown(&self, config: &Arc<Config>) -> Result<String> {
        let spec = self.parse_usage_spec_for_display(config).await?;
        let ctx = usage::docs::markdown::MarkdownRenderer::new(spec)
            .with_replace_pre_with_code_fences(true)
            .with_header_level(2);
        Ok(ctx.render_spec()?)
    }

    pub fn estyled_prefix(&self) -> String {
        static COLORS: Lazy<Vec<Color>> = Lazy::new(|| {
            vec![
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::Green,
                Color::Yellow,
                Color::Red,
            ]
        });
        let idx = self.display_name.chars().map(|c| c as usize).sum::<usize>() % COLORS.len();

        style::ereset() + &style::estyle(self.prefix()).fg(COLORS[idx]).to_string()
    }

    pub async fn dir(&self, config: &Arc<Config>) -> Result<Option<PathBuf>> {
        if let Some(dir) = self.dir.clone().or_else(|| {
            self.cf(config)
                .as_ref()
                .and_then(|cf| cf.task_config().dir.clone())
        }) {
            let config_root = self.config_root.clone().unwrap_or_default();
            let mut tera = get_tera(Some(&config_root));
            let tera_ctx = self.tera_ctx(config).await?;
            let dir = tera.render_str(&dir, &tera_ctx)?;
            let dir = file::replace_path(&dir);
            if dir.is_absolute() {
                Ok(Some(dir.to_path_buf()))
            } else if let Some(root) = &self.config_root {
                Ok(Some(root.join(dir)))
            } else {
                Ok(Some(dir.clone()))
            }
        } else {
            Ok(self.config_root.clone())
        }
    }

    pub async fn file_path(&self, config: &Arc<Config>) -> Result<Option<PathBuf>> {
        if let Some(file) = &self.file {
            let file_str = file.to_string_lossy().to_string();
            let config_root = self.config_root.clone().unwrap_or_default();
            let mut tera = get_tera(Some(&config_root));
            let tera_ctx = self.tera_ctx(config).await?;
            let rendered = tera.render_str(&file_str, &tera_ctx)?;
            let rendered_path = file::replace_path(&rendered);
            if rendered_path.is_absolute() {
                Ok(Some(rendered_path))
            } else if let Some(root) = &self.config_root {
                Ok(Some(root.join(rendered_path)))
            } else {
                Ok(Some(rendered_path))
            }
        } else {
            Ok(None)
        }
    }

    /// Get file path without templating (for display purposes)
    /// This is a non-async version used when we just need the path for display
    fn file_path_raw(&self) -> Option<PathBuf> {
        self.file.as_ref().map(|file| {
            if file.is_absolute() {
                file.clone()
            } else if let Some(root) = &self.config_root {
                root.join(file)
            } else {
                file.clone()
            }
        })
    }

    pub async fn tera_ctx(&self, config: &Arc<Config>) -> Result<tera::Context> {
        let ts = config.get_toolset().await?;
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        tera_ctx.insert("config_root", &self.config_root);
        Ok(tera_ctx)
    }

    pub fn cf<'a>(&'a self, config: &'a Config) -> Option<&'a Arc<dyn ConfigFile>> {
        // For monorepo tasks, use the stored config file reference
        if let Some(ref cf) = self.cf {
            return Some(cf);
        }
        // Fallback to looking up in config.config_files
        config.config_files.get(&self.config_source)
    }

    /// Check if this task is a remote task (loaded from git:// or http:// URL)
    /// Remote tasks should not use monorepo config file context because they need
    /// access to tools from the full config hierarchy, not just the local config file
    pub fn is_remote(&self) -> bool {
        // Check the stored remote file source (set before file is replaced with local path)
        if let Some(source) = &self.remote_file_source {
            return source.starts_with("git::")
                || source.starts_with("http://")
                || source.starts_with("https://");
        }
        false
    }

    pub fn shell(&self) -> Option<Vec<String>> {
        self.shell.as_ref().and_then(|shell| {
            let shell_cmd = shell
                .split_whitespace()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            if shell_cmd.is_empty() || shell_cmd[0].trim().is_empty() {
                warn!("invalid shell '{shell}', expected '<program> <argument>' (e.g. sh -c)");
                None
            } else {
                Some(shell_cmd)
            }
        })
    }

    pub async fn render(&mut self, config: &Arc<Config>, config_root: &Path) -> Result<()> {
        let mut tera = get_tera(Some(config_root));
        let tera_ctx = self.tera_ctx(config).await?;
        for a in &mut self.aliases {
            *a = tera.render_str(a, &tera_ctx)?;
        }

        self.description = tera.render_str(&self.description, &tera_ctx)?;
        for s in &mut self.sources {
            *s = tera.render_str(s, &tera_ctx)?;
        }
        if !self.sources.is_empty() && self.outputs.is_empty() {
            self.outputs = TaskOutputs::Auto;
        }
        self.raw_outputs = self.outputs.render(&mut tera, &tera_ctx)?;
        for d in &mut self.depends {
            d.render(&mut tera, &tera_ctx)?;
        }
        for d in &mut self.depends_post {
            d.render(&mut tera, &tera_ctx)?;
        }
        for d in &mut self.wait_for {
            d.render(&mut tera, &tera_ctx)?;
        }
        if let Some(dir) = &mut self.dir {
            *dir = tera.render_str(dir, &tera_ctx)?;
        }
        if let Some(shell) = &mut self.shell {
            *shell = tera.render_str(shell, &tera_ctx)?;
        }
        for (_, v) in &mut self.tools {
            *v = tera.render_str(v, &tera_ctx)?;
        }
        Ok(())
    }

    pub fn name_to_path(&self) -> PathBuf {
        self.name.replace(':', path::MAIN_SEPARATOR_STR).into()
    }

    pub async fn render_env(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
    ) -> Result<(EnvMap, Vec<(String, String)>)> {
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        let mut env = ts.full_env(config).await?;
        if let Some(root) = &config.project_root {
            tera_ctx.insert("config_root", &root);
        }

        // Convert task env directives to (EnvDirective, PathBuf) pairs
        // Use the config file path as source for proper path resolution
        // Include inherited_env first (so task's own env can override it)
        let env_directives: Vec<_> = self
            .inherited_env
            .0
            .iter()
            .chain(self.env.0.iter())
            .map(|directive| (directive.clone(), self.config_source.clone()))
            .collect();

        // Resolve environment directives using the same system as global env
        let env_results = EnvResults::resolve(
            config,
            tera_ctx.clone(),
            &env,
            env_directives,
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await?;
        let task_env = env_results.env.into_iter().map(|(k, (v, _))| (k, v));
        // Apply the resolved environment variables
        env.extend(task_env.clone());

        // Remove environment variables that were explicitly unset
        for key in &env_results.env_remove {
            env.remove(key);
        }

        // Apply path additions from _.path directives
        if !env_results.env_paths.is_empty() {
            let mut path_env = PathEnv::from_iter(env::split_paths(
                &env.get(&*env::PATH_KEY).cloned().unwrap_or_default(),
            ));
            for path in env_results.env_paths {
                path_env.add(path);
            }
            env.insert(env::PATH_KEY.to_string(), path_env.to_string());
        }

        Ok((env, task_env.collect()))
    }
}

fn name_from_path(prefix: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<String> {
    let name = path
        .as_ref()
        .strip_prefix(prefix)
        .map(|p| match p {
            p if p.starts_with("mise-tasks") => p.strip_prefix("mise-tasks"),
            p if p.starts_with(".mise-tasks") => p.strip_prefix(".mise-tasks"),
            p if p.starts_with(".mise/tasks") => p.strip_prefix(".mise/tasks"),
            p if p.starts_with("mise/tasks") => p.strip_prefix("mise/tasks"),
            p if p.starts_with(".config/mise/tasks") => p.strip_prefix(".config/mise/tasks"),
            _ => Ok(p),
        })??
        .components()
        .map(path::Component::as_os_str)
        .map(ffi::OsStr::to_string_lossy)
        .map(|s| s.replace(':', "_"))
        .join(":");
    if let Some(name) = name.strip_suffix(":_default") {
        Ok(name.to_string())
    } else {
        Ok(name)
    }
}

/// Extract monorepo path from a task name
/// e.g., "//projects/frontend:test" -> Some("projects/frontend")
/// e.g., "//projects/frontend:test:nested" -> Some("projects/frontend")
/// Returns None if the task name doesn't have monorepo syntax
pub(crate) fn extract_monorepo_path(name: &str) -> Option<String> {
    name.strip_prefix("//").and_then(|stripped| {
        // Find the FIRST colon after "//" prefix to handle task names with colons like "do:item-1"
        stripped.find(':').map(|idx| stripped[..idx].to_string())
    })
}

/// Build a map of task names and aliases to task references
/// For monorepo tasks, creates entries for both prefixed and unprefixed aliases
/// e.g., task "//:format" with alias "fmt" creates both "//:fmt" and "fmt"
pub(crate) fn build_task_ref_map<'a, I>(tasks: I) -> BTreeMap<String, &'a Task>
where
    I: Iterator<Item = (&'a String, &'a Task)> + 'a,
{
    tasks
        .flat_map(|(_, t)| {
            t.aliases
                .iter()
                .flat_map(|a| {
                    // For monorepo tasks, create entries for both prefixed and unprefixed aliases
                    // This allows references like "fmt" to resolve to "//:format"
                    if let Some(path) = extract_monorepo_path(&t.name) {
                        vec![(format!("//{}:{}", path, a), t), (a.to_string(), t)]
                    } else {
                        // Non-monorepo task, use alias as-is
                        vec![(a.to_string(), t)]
                    }
                })
                .chain(once((t.name.clone(), t)))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve a task dependency pattern, optionally relative to a parent task
/// If pattern starts with ":" and parent_task is provided, resolve relative to parent's path
/// For example: parent "//projects/frontend:test" with pattern ":build" -> "//projects/frontend:build"
pub(crate) fn resolve_task_pattern(pattern: &str, parent_task: Option<&Task>) -> String {
    // Check if this is a bare task name that should be treated as relative
    let is_bare_name =
        !pattern.starts_with("//") && !pattern.starts_with("::") && !pattern.starts_with(':');

    // If pattern starts with ":" or is a bare name in monorepo context, resolve relatively
    let should_resolve_relatively = pattern.starts_with(':') && !pattern.starts_with("::")
        || (is_bare_name && parent_task.is_some_and(|p| p.name.starts_with("//")));

    if should_resolve_relatively && let Some(parent) = parent_task {
        // Extract the path portion from the parent task name
        // For monorepo tasks like "//projects/frontend:test:nested", we need to extract "//projects/frontend"
        // by finding the FIRST colon after the "//" prefix, not the last one
        if let Some(stripped) = parent.name.strip_prefix("//") {
            // Find the first colon after "//" prefix
            if let Some(colon_idx) = stripped.find(':') {
                let path = format!("//{}", &stripped[..colon_idx]);
                // If pattern is a bare name, add the colon prefix
                return if is_bare_name {
                    format!("{}:{}", path, pattern)
                } else {
                    format!("{}{}", path, pattern)
                };
            }
        } else if let Some((path, _)) = parent.name.rsplit_once(':') {
            // For non-monorepo tasks, use the old logic
            return format!("{}{}", path, pattern);
        }
    }
    pattern.to_string()
}

fn match_tasks_with_context(
    tasks: &BTreeMap<String, &Task>,
    td: &TaskDep,
    parent_task: Option<&Task>,
) -> Result<Vec<Task>> {
    let resolved_pattern = resolve_task_pattern(&td.task, parent_task);
    let matches = tasks
        .get_matching(&resolved_pattern)?
        .into_iter()
        .map(|t| {
            let mut t = (*t).clone();
            t.args = td.args.clone();
            // Apply env vars from dependency - these affect task identity/deduplication
            if !td.env.is_empty() {
                let env_directives: Vec<EnvDirective> = td
                    .env
                    .iter()
                    .map(|(k, v)| EnvDirective::Val(k.clone(), v.clone(), Default::default()))
                    .collect();
                t = t.with_dependency_env(&env_directives);
                if let Some(config_root) = &t.config_root {
                    let config_root = config_root.clone();
                    t.outputs
                        .re_render_with_env(&t.raw_outputs.clone(), &td.env, &config_root)?;
                }
            }
            Ok(t)
        })
        .collect::<Result<Vec<_>>>()?;
    if matches.is_empty() {
        let mut err_msg = format!("task not found: {}", td.task);

        // In monorepo mode, suggest similar tasks using fuzzy matching
        if resolved_pattern.starts_with("//") {
            let similar: Vec<String> = tasks
                .keys()
                .filter(|k| k.starts_with("//"))
                .filter_map(|k| {
                    FUZZY_MATCHER
                        .fuzzy_match(&k.to_lowercase(), &resolved_pattern.to_lowercase())
                        .map(|score| (score, k.clone()))
                })
                .sorted_by_key(|(score, _)| -1 * *score)
                .take(5)
                .map(|(_, k)| k)
                .collect();

            if !similar.is_empty() {
                err_msg.push_str("\n\nDid you mean one of these?");
                for task_name in similar {
                    err_msg.push_str(&format!("\n  - {}", task_name));
                }
            }
        }

        return Err(eyre!(err_msg));
    };

    Ok(matches)
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: "".to_string(),
            display_name: "".to_string(),
            description: "".to_string(),
            aliases: vec![],
            config_source: PathBuf::new(),
            cf: None,
            config_root: None,
            confirm: None,
            depends: vec![],
            depends_post: vec![],
            wait_for: vec![],
            env: Default::default(),
            inherited_env: Default::default(),
            dir: None,
            hide: false,
            global: false,
            raw: false,
            sources: vec![],
            outputs: Default::default(),
            raw_outputs: Default::default(),
            shell: None,
            silent: Silent::Off,
            run: vec![],
            run_windows: vec![],
            args: vec![],
            file: None,
            quiet: false,
            tools: Default::default(),
            usage: "".to_string(),
            timeout: None,
            remote_file_source: None,
            extends: None,
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let cmd = self
            .run()
            .iter()
            .map(|e| e.to_string())
            .next()
            .or_else(|| self.file_path_raw().as_ref().map(display_path));

        if let Some(cmd) = cmd {
            let cmd = cmd.lines().next().unwrap_or_default();
            let prefix = self.prefix();
            let prefix_len = measure_text_width(&prefix);
            // Ensure we have at least 20 characters for the command, even with very long prefixes
            let available_width = (*env::TERM_WIDTH).saturating_sub(prefix_len + 4); // 4 chars buffer for spacing and ellipsis
            let max_width = available_width.max(20); // Always show at least 20 chars of command
            let truncated_cmd = truncate_str(cmd, max_width, "…");
            write!(f, "{} {}", prefix, truncated_cmd)
        } else {
            write!(f, "{}", self.prefix())
        }
    }
}

impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Extract sorted env key-value pairs from task's own env (not inherited_env)
/// Used for consistent comparison/hashing of task identity
fn env_key(task: &Task) -> Vec<(&String, &String)> {
    task.env
        .0
        .iter()
        .filter_map(|d| match d {
            EnvDirective::Val(k, v, _) => Some((k, v)),
            _ => None,
        })
        .sorted()
        .collect()
}

impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => match self.args.cmp(&other.args) {
                Ordering::Equal => env_key(self).cmp(&env_key(other)),
                o => o,
            },
            o => o,
        }
    }
}

impl Hash for Task {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.args.iter().for_each(|arg| arg.hash(state));
        // Include task's own env (not inherited_env) for deduplication
        for (k, v) in env_key(self) {
            k.hash(state);
            v.hash(state);
        }
    }
}

impl Eq for Task {}
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.args == other.args && env_key(self) == env_key(other)
    }
}

impl TreeItem for (&Graph<Task, ()>, NodeIndex) {
    type Child = Self;

    fn write_self(&self) -> std::io::Result<()> {
        if let Some(w) = self.0.node_weight(self.1) {
            miseprint!("{}", w.display_name)?;
        }
        Ok(())
    }

    fn children(&self) -> Cow<'_, [Self::Child]> {
        let v: Vec<_> = self.0.neighbors(self.1).map(|i| (self.0, i)).collect();
        Cow::from(v)
    }
}

pub trait GetMatchingExt<T> {
    fn get_matching(&self, pat: &str) -> Result<Vec<&T>>;
}

/// Helper function to strip file extension from a task name
/// e.g., "test.js" -> "test", "build" -> "build"
/// Special case: hidden files like ".hidden" are preserved to avoid empty strings
fn strip_extension(name: &str) -> &str {
    let result = name.rsplitn(2, '.').last().unwrap_or(name);
    // Don't strip extension if it would result in empty string (hidden files)
    if result.is_empty() { name } else { result }
}

impl<T> GetMatchingExt<T> for BTreeMap<String, T>
where
    T: Eq + Hash,
{
    fn get_matching(&self, pat: &str) -> Result<Vec<&T>> {
        // === Monorepo pattern matching ===
        // Only patterns starting with '//' or ':' are monorepo patterns
        // Reject patterns that look like monorepo paths but use wrong syntax (have / and : but don't start with // or :)
        if !pat.starts_with("//") && !pat.starts_with(':') {
            // Check if this looks like an attempt at a monorepo path with wrong syntax
            if pat.contains('/') && pat.contains(':') {
                bail!(
                    "relative path syntax '{}' is not supported, use '//{}'  or ':task' for current directory",
                    pat,
                    pat
                )
            }
            // If it doesn't contain wildcards or ':', it's a simple task name
            if !pat.contains('*') && !pat.contains("...") && !pat.contains(':') {
                return Ok(self
                    .iter()
                    .filter(|(k, _)| {
                        // Check if task name exactly matches, or matches without extension
                        k.as_str() == pat || strip_extension(k) == pat
                    })
                    .map(|(_, v)| v)
                    .collect());
            }
            // Has wildcards or colon but no /, so it's a regular task pattern like "render:*" or "build:linux"
            // Process with glob matching below
        }

        // === Parse monorepo pattern ===
        let normalized_pat = if pat.starts_with("//") {
            pat.to_string()
        } else if pat.starts_with(':') {
            // Special case: :task should have been expanded before calling get_matching
            // If we reach here, it means the expansion didn't happen properly
            bail!("':task' pattern should be expanded before matching")
        } else {
            pat.to_string()
        };

        // Split pattern into path and task parts
        // Pattern format: //path/...:task* or //path:task*
        let parts: Vec<&str> = normalized_pat.splitn(2, ':').collect();
        let has_explicit_task_glob = parts.len() > 1;
        let (path_pattern, task_pattern) = match parts.as_slice() {
            [path, task] => (*path, *task),
            [path] => (*path, "*"),
            _ => (normalized_pat.as_str(), "*"),
        };

        // === Convert ellipsis to glob syntax ===
        // Convert ellipsis (...) to glob pattern (**)
        // //... matches everything, //foo/... matches foo and all subdirs
        let path_glob = path_pattern.replace("...", "**");

        // For task patterns, * only matches within the task name portion (after final :)
        // e.g., test:* matches test:unit, test:integration, etc.
        let task_glob = task_pattern;

        // === Build glob matchers once (performance optimization) ===
        // Build path matcher for absolute patterns
        let path_matcher = GlobBuilder::new(&path_glob)
            .literal_separator(true)
            .build()
            .ok()
            .map(|b| b.compile_matcher());

        // Build task matcher if not wildcard
        let task_matcher = if task_glob != "*" {
            GlobBuilder::new(task_glob)
                .literal_separator(false) // Allow * to match : in task names
                .build()
                .ok()
                .map(|b| b.compile_matcher())
        } else {
            None
        };

        // Build relative pattern matchers if needed
        let (rel_path_matcher, rel_task_matcher) = if !pat.starts_with("//") {
            let rel_path_pattern = path_pattern.strip_prefix("//").unwrap_or(path_pattern);
            let rel_path_glob = rel_path_pattern.replace("...", "**");

            let rel_path = GlobBuilder::new(&rel_path_glob)
                .literal_separator(true)
                .build()
                .ok()
                .map(|b| b.compile_matcher());

            let rel_task = if task_glob != "*" {
                GlobBuilder::new(task_glob)
                    .literal_separator(false)
                    .build()
                    .ok()
                    .map(|b| b.compile_matcher())
            } else {
                None
            };

            (rel_path, rel_task)
        } else {
            (None, None)
        };

        // === Match tasks with extension stripping ===
        Ok(self
            .iter()
            .filter(|(k, _)| {
                // Split task name into path and task parts
                let key_parts: Vec<&str> = k.splitn(2, ':').collect();
                let (key_path, key_task) = match key_parts.as_slice() {
                    [path, task] => (*path, *task),
                    [path] => (*path, ""),
                    _ => (k.as_str(), ""),
                };

                // Match path part with ellipsis support
                let path_matches = if let Some(ref matcher) = path_matcher {
                    matcher.is_match(key_path)
                } else {
                    false
                };

                // Match task part with asterisk support and extension stripping
                // When the pattern explicitly uses a wildcard after `:` (e.g., "test:*"),
                // require the key to actually have a task part (i.e., contain a `:`
                // separator). This prevents "test" from matching "test:*", which would
                // cause circular dependencies. Implicit wildcards (bare names like "test")
                // should still match the exact task.
                let task_matches = if task_glob == "*" {
                    !has_explicit_task_glob || !key_task.is_empty()
                } else if let Some(ref matcher) = task_matcher {
                    // Check exact match OR match without extension
                    matcher.is_match(key_task) || matcher.is_match(strip_extension(key_task))
                } else {
                    false
                };

                // Try matching without // prefix for relative patterns
                let relative_match = if !pat.starts_with("//") {
                    let stripped_key = k.strip_prefix("//").unwrap_or(k);
                    let stripped_parts: Vec<&str> = stripped_key.splitn(2, ':').collect();
                    let (stripped_path, stripped_task) = match stripped_parts.as_slice() {
                        [path, task] => (*path, *task),
                        [path] => (*path, ""),
                        _ => (stripped_key, ""),
                    };

                    let rel_path_matches = if let Some(ref matcher) = rel_path_matcher {
                        matcher.is_match(stripped_path)
                    } else {
                        false
                    };

                    let rel_task_matches = if task_glob == "*" {
                        !has_explicit_task_glob || !stripped_task.is_empty()
                    } else if let Some(ref matcher) = rel_task_matcher {
                        // Check exact match OR match without extension
                        matcher.is_match(stripped_task)
                            || matcher.is_match(strip_extension(stripped_task))
                    } else {
                        false
                    };

                    rel_path_matches && rel_task_matches
                } else {
                    false
                };

                (path_matches && task_matches) || relative_match
            })
            .map(|(_, t)| t)
            .unique()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Mutex;

    use crate::task::Task;
    use crate::{config::Config, dirs};
    use pretty_assertions::assert_eq;

    use super::name_from_path;

    // Thread-local storage to capture parser state during tests
    thread_local! {
        static CAPTURED_PARSER_FIELDS: Mutex<Option<Vec<String>>> = const { Mutex::new(None) };
    }

    pub(super) fn capture_parsed_fields(fields: Vec<String>) {
        CAPTURED_PARSER_FIELDS.with(|captured| {
            *captured.lock().unwrap() = Some(fields);
        });
    }

    fn take_captured_fields() -> Option<Vec<String>> {
        CAPTURED_PARSER_FIELDS.with(|captured| captured.lock().unwrap().take())
    }

    #[tokio::test]
    async fn test_from_path() {
        let test_cases = [(".mise/tasks/filetask", "filetask", vec!["ft"])];
        let config = Config::get().await.unwrap();
        for (path, name, aliases) in test_cases {
            let t = Task::from_path(
                &config,
                Path::new(path),
                Path::new(".mise/tasks"),
                Path::new(dirs::CWD.as_ref().unwrap()),
            )
            .await
            .unwrap();
            assert_eq!(t.name, name);
            assert_eq!(t.aliases, aliases);
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_name_from_path() {
        let test_cases = [
            (("/.mise/tasks", "/.mise/tasks/a"), "a"),
            (("/.mise/tasks", "/.mise/tasks/a/b"), "a:b"),
            (("/.mise/tasks", "/.mise/tasks/a/b/c"), "a:b:c"),
            (("/.mise/tasks", "/.mise/tasks/a:b"), "a_b"),
            (("/.mise/tasks", "/.mise/tasks/a:b/c"), "a_b:c"),
        ];

        for ((root, path), expected) in test_cases {
            assert_eq!(name_from_path(root, path).unwrap(), expected)
        }
    }

    #[test]
    fn test_name_from_path_invalid() {
        let test_cases = [("/some/other/dir", "/.mise/tasks/a")];

        for (root, path) in test_cases {
            assert!(name_from_path(root, path).is_err())
        }
    }

    // This test verifies that resolve_depends correctly uses self.depends_post
    // instead of iterating through all tasks_to_run (which was the bug)
    #[tokio::test]
    async fn test_resolve_depends_post_uses_self_only() {
        use crate::task::task_dep::TaskDep;

        // Create a task with depends_post
        let task_with_post_deps = Task {
            name: "task_with_post".to_string(),
            depends_post: vec![
                TaskDep {
                    task: "post1".to_string(),
                    args: vec![],
                    env: Default::default(),
                },
                TaskDep {
                    task: "post2".to_string(),
                    args: vec![],
                    env: Default::default(),
                },
            ],
            ..Default::default()
        };

        // Create another task with different depends_post
        let other_task = Task {
            name: "other_task".to_string(),
            depends_post: vec![TaskDep {
                task: "other_post".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        // Verify that task_with_post_deps has the expected depends_post
        assert_eq!(task_with_post_deps.depends_post.len(), 2);
        assert_eq!(task_with_post_deps.depends_post[0].task, "post1");
        assert_eq!(task_with_post_deps.depends_post[1].task, "post2");

        // Verify that other_task doesn't interfere (would have before the fix)
        assert_eq!(other_task.depends_post.len(), 1);
        assert_eq!(other_task.depends_post[0].task, "other_post");
    }

    #[tokio::test]
    async fn test_from_path_toml_headers() {
        use std::fs;
        use tempfile::tempdir;

        let config = Config::get().await.unwrap();
        let temp_dir = tempdir().unwrap();
        let task_path = temp_dir.path().join("test_task");

        fs::write(
            &task_path,
            r#"#!/bin/bash
#MISE description="Build the CLI"
# MISE alias="b"
# [MISE] sources=["Cargo.toml", "src/**/*.rs"]
echo "hello world"
"#,
        )
        .unwrap();

        let result = Task::from_path(&config, &task_path, temp_dir.path(), temp_dir.path()).await;
        let mut expected = Task::new(&task_path, temp_dir.path(), temp_dir.path()).unwrap();
        expected.description = "Build the CLI".to_string();
        expected.aliases = vec!["b".to_string()];
        expected.sources = vec!["Cargo.toml".to_string(), "src/**/*.rs".to_string()];
        assert_eq!(result.unwrap(), expected);
    }

    #[tokio::test]
    async fn test_from_path_invalid_toml() {
        use std::fs;
        use tempfile::tempdir;

        let config = Config::get().await.unwrap();
        let temp_dir = tempdir().unwrap();
        let task_path = temp_dir.path().join("test_task");

        // Create a task file with invalid TOML in the header
        fs::write(
            &task_path,
            r#"#!/bin/bash
# mise description="test task"
# mise env={invalid=toml=here}
echo "hello world"
"#,
        )
        .unwrap();

        let result = Task::from_path(&config, &task_path, temp_dir.path(), temp_dir.path()).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to parse task header TOML")
        );
    }

    #[test]
    fn test_resolve_task_pattern() {
        use super::resolve_task_pattern;

        // Test 1: Relative pattern with monorepo parent task
        let parent_task = Task {
            name: "//projects/frontend:test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":build", Some(&parent_task)),
            "//projects/frontend:build"
        );

        // Test 2: Relative pattern with different parent
        let parent_task = Task {
            name: "//libs/shared:lint".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":compile", Some(&parent_task)),
            "//libs/shared:compile"
        );

        // Test 3: Absolute pattern should not be modified
        let parent_task = Task {
            name: "//projects/frontend:test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("//projects/backend:build", Some(&parent_task)),
            "//projects/backend:build"
        );

        // Test 4: Simple task name with monorepo parent should resolve relatively (NEW BEHAVIOR)
        assert_eq!(
            resolve_task_pattern("build", Some(&parent_task)),
            "//projects/frontend:build"
        );

        // Test 5: Relative pattern without parent task (no resolution)
        assert_eq!(resolve_task_pattern(":build", None), ":build");

        // Test 6: Non-monorepo task - colon pattern should not resolve
        let parent_task = Task {
            name: "test".to_string(),
            ..Default::default()
        };
        assert_eq!(resolve_task_pattern(":build", Some(&parent_task)), ":build");

        // Test 6a: Non-monorepo task - bare name should not resolve
        assert_eq!(resolve_task_pattern("build", Some(&parent_task)), "build");

        // Test 7: Root monorepo task (empty path)
        let parent_task = Task {
            name: "//:root-task".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":other", Some(&parent_task)),
            "//:other"
        );

        // Test 8: Double colon should not be treated as relative
        let parent_task = Task {
            name: "//projects/frontend:test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("::global", Some(&parent_task)),
            "::global"
        );

        // Test 9: Pattern with wildcards
        let parent_task = Task {
            name: "//projects/frontend:test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":test*", Some(&parent_task)),
            "//projects/frontend:test*"
        );

        // Test 10: Deep nested path
        let parent_task = Task {
            name: "//a/b/c/d:task".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":dep", Some(&parent_task)),
            "//a/b/c/d:dep"
        );

        // Test 11: Task name with colon (e.g., "do:item-1")
        // This is the bug that was fixed - we need to split on the FIRST colon after //
        let parent_task = Task {
            name: "//submodule:do:item-1".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":before", Some(&parent_task)),
            "//submodule:before"
        );

        // Test 12: Another task name with multiple colons
        let parent_task = Task {
            name: "//project:test:unit:fast".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern(":setup", Some(&parent_task)),
            "//project:setup"
        );

        // Test 13: Bare name without parent task (no resolution)
        assert_eq!(resolve_task_pattern("build", None), "build");

        // Test 14: Bare name with different monorepo parent
        let parent_task = Task {
            name: "//libs/shared:lint".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("compile", Some(&parent_task)),
            "//libs/shared:compile"
        );

        // Test 15: Bare name with root monorepo task
        let parent_task = Task {
            name: "//:root-task".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("other", Some(&parent_task)),
            "//:other"
        );

        // Test 16: Bare name with task containing colons
        let parent_task = Task {
            name: "//submodule:do:item-1".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("before", Some(&parent_task)),
            "//submodule:before"
        );

        // Test 17: Absolute path should not be modified even with monorepo parent
        let parent_task = Task {
            name: "//projects/frontend:test".to_string(),
            ..Default::default()
        };
        assert_eq!(
            resolve_task_pattern("//other/module:task", Some(&parent_task)),
            "//other/module:task"
        );

        // Test 18: Global task (::) should not be modified
        assert_eq!(
            resolve_task_pattern("::global", Some(&parent_task)),
            "::global"
        );
    }

    #[test]
    fn test_extract_monorepo_path() {
        use super::extract_monorepo_path;

        // Test 1: Simple monorepo task
        assert_eq!(
            extract_monorepo_path("//projects/frontend:test"),
            Some("projects/frontend".to_string())
        );

        // Test 2: Root level task
        assert_eq!(extract_monorepo_path("//:root-task"), Some("".to_string()));

        // Test 3: Deep nested path
        assert_eq!(
            extract_monorepo_path("//a/b/c/d:task"),
            Some("a/b/c/d".to_string())
        );

        // Test 4: Non-monorepo task (no // prefix)
        assert_eq!(extract_monorepo_path("regular-task"), None);

        // Test 5: Task name with colon (e.g., "do:item-1")
        // This was the bug - we need to extract based on FIRST colon after //
        assert_eq!(
            extract_monorepo_path("//submodule:do:item-1"),
            Some("submodule".to_string())
        );

        // Test 6: Multiple colons in task name
        assert_eq!(
            extract_monorepo_path("//project:test:unit:fast"),
            Some("project".to_string())
        );

        // Test 7: Complex path with colons in task name
        assert_eq!(
            extract_monorepo_path("//apps/backend:build:prod"),
            Some("apps/backend".to_string())
        );
    }

    #[test]
    fn test_strip_extension() {
        use super::strip_extension;

        // Test 1: Single extension
        assert_eq!(strip_extension("task.sh"), "task");
        assert_eq!(strip_extension("build.js"), "build");
        assert_eq!(strip_extension("test.py"), "test");

        // Test 2: Multiple extensions (only strips rightmost one)
        assert_eq!(strip_extension("backup.test.js"), "backup.test");
        assert_eq!(strip_extension("file.tar.gz"), "file.tar");
        assert_eq!(strip_extension("archive.tar.bz2"), "archive.tar");

        // Test 3: No extension
        assert_eq!(strip_extension("task"), "task");
        assert_eq!(strip_extension("build"), "build");

        // Test 4: Hidden files (starting with dot)
        // Now preserved to avoid empty strings
        assert_eq!(strip_extension(".hidden"), ".hidden");
        assert_eq!(strip_extension(".gitignore"), ".gitignore");

        // Test 5: Hidden files with extension
        assert_eq!(strip_extension(".hidden.sh"), ".hidden");
        assert_eq!(strip_extension(".config.json"), ".config");

        // Test 6: Empty string
        assert_eq!(strip_extension(""), "");

        // Test 7: Only extension separator (preserved to avoid empty string)
        assert_eq!(strip_extension("."), ".");

        // Test 8: Multiple dots with extension
        assert_eq!(strip_extension("my.task.name.js"), "my.task.name");

        // Test 9: Path-like names (shouldn't treat / as special)
        assert_eq!(strip_extension("path/to/task.sh"), "path/to/task");
        assert_eq!(strip_extension("path/task"), "path/task");

        // Test 10: Task names with dots in the middle
        assert_eq!(strip_extension("test.unit"), "test");
        assert_eq!(strip_extension("build.prod.js"), "build.prod");
    }

    #[test]
    fn test_circular_dependency_detection() {
        use super::Task;
        use std::collections::BTreeMap;

        let mut tasks = BTreeMap::new();

        // Create circular dependency: task_a -> task_b -> task_a
        let task_a = Task {
            name: "task_a".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "task_b".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        let task_b = Task {
            name: "task_b".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "task_a".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        tasks.insert("task_a".to_string(), task_a.clone());
        tasks.insert("task_b".to_string(), task_b);

        // Should detect circular dependency
        let result = task_a.all_depends(&tasks);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("circular dependency detected"));
    }

    #[test]
    fn test_transitive_circular_dependency_detection() {
        use super::Task;
        use std::collections::BTreeMap;

        let mut tasks = BTreeMap::new();

        // Create transitive circular dependency: a -> b -> c -> a
        let task_a = Task {
            name: "task_a".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "task_b".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        let task_b = Task {
            name: "task_b".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "task_c".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        let task_c = Task {
            name: "task_c".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "task_a".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        tasks.insert("task_a".to_string(), task_a.clone());
        tasks.insert("task_b".to_string(), task_b);
        tasks.insert("task_c".to_string(), task_c);

        // Should detect circular dependency
        let result = task_a.all_depends(&tasks);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("circular dependency detected"));
    }

    #[test]
    fn test_no_false_positive_for_diamond_dependency() {
        use super::Task;
        use std::collections::BTreeMap;

        let mut tasks = BTreeMap::new();

        // Create diamond dependency (NOT circular): root -> [a, b] -> common
        let root = Task {
            name: "root".to_string(),
            depends: vec![
                crate::task::task_dep::TaskDep {
                    task: "task_a".to_string(),
                    args: vec![],
                    env: Default::default(),
                },
                crate::task::task_dep::TaskDep {
                    task: "task_b".to_string(),
                    args: vec![],
                    env: Default::default(),
                },
            ],
            ..Default::default()
        };

        let task_a = Task {
            name: "task_a".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "common".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        let task_b = Task {
            name: "task_b".to_string(),
            depends: vec![crate::task::task_dep::TaskDep {
                task: "common".to_string(),
                args: vec![],
                env: Default::default(),
            }],
            ..Default::default()
        };

        let common = Task {
            name: "common".to_string(),
            ..Default::default()
        };

        tasks.insert("root".to_string(), root.clone());
        tasks.insert("task_a".to_string(), task_a);
        tasks.insert("task_b".to_string(), task_b);
        tasks.insert("common".to_string(), common);

        // Should NOT detect circular dependency (diamond is OK)
        let result = root.all_depends(&tasks);
        assert!(result.is_ok());
        let deps = result.unwrap();
        // Should have task_a, task_b, and common (deduplicated)
        assert_eq!(deps.len(), 3);
    }

    #[test]
    fn test_file_path_raw_absolute() {
        use std::path::PathBuf;

        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("/absolute/path/script.sh")),
            config_root: Some(PathBuf::from("/project/root")),
            ..Default::default()
        };

        let result = task.file_path_raw();
        assert_eq!(result, Some(PathBuf::from("/absolute/path/script.sh")));
    }

    #[test]
    fn test_file_path_raw_relative() {
        use std::path::PathBuf;

        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("scripts/test.sh")),
            config_root: Some(PathBuf::from("/project/root")),
            ..Default::default()
        };

        let result = task.file_path_raw();
        assert_eq!(result, Some(PathBuf::from("/project/root/scripts/test.sh")));
    }

    #[test]
    fn test_file_path_raw_relative_no_config_root() {
        use std::path::PathBuf;

        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("scripts/test.sh")),
            config_root: None,
            ..Default::default()
        };

        let result = task.file_path_raw();
        assert_eq!(result, Some(PathBuf::from("scripts/test.sh")));
    }

    #[test]
    fn test_file_path_raw_none() {
        let task = Task {
            name: "test".to_string(),
            file: None,
            config_root: None,
            ..Default::default()
        };

        let result = task.file_path_raw();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_file_path_absolute() {
        use std::path::PathBuf;

        let config = Config::get().await.unwrap();
        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("/absolute/path/script.sh")),
            config_root: Some(PathBuf::from("/project/root")),
            ..Default::default()
        };

        let result = task.file_path(&config).await.unwrap();
        assert_eq!(result, Some(PathBuf::from("/absolute/path/script.sh")));
    }

    #[tokio::test]
    async fn test_file_path_relative() {
        use std::path::PathBuf;

        let config = Config::get().await.unwrap();
        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("scripts/test.sh")),
            config_root: Some(PathBuf::from("/project/root")),
            ..Default::default()
        };

        let result = task.file_path(&config).await.unwrap();
        assert_eq!(result, Some(PathBuf::from("/project/root/scripts/test.sh")));
    }

    #[tokio::test]
    async fn test_file_path_none() {
        let config = Config::get().await.unwrap();
        let task = Task {
            name: "test".to_string(),
            file: None,
            config_root: None,
            ..Default::default()
        };

        let result = task.file_path(&config).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_file_path_with_templating() {
        use std::path::PathBuf;

        let config = Config::get().await.unwrap();
        let task = Task {
            name: "test".to_string(),
            file: Some(PathBuf::from("scripts/{{config_root}}/test.sh")),
            config_root: Some(PathBuf::from("/project/root")),
            ..Default::default()
        };

        // This test verifies that templating is processed in file_path
        let result = task.file_path(&config).await;
        // Should succeed (not error on template rendering)
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_parses_all_fields() {
        use std::fs;
        use tempfile::tempdir;

        // Create a temporary directory for the test
        let temp_dir = tempdir().unwrap();
        let tasks_dir = temp_dir.path().join("tasks");
        fs::create_dir(&tasks_dir).unwrap();
        let task_file = tasks_dir.join("test-task");

        // Create a file task with ALL possible header fields
        let script_content = r#"#!/usr/bin/env bash
#MISE description="Test task with all fields"
#MISE aliases=["alias1", "alias2"]
#MISE depends=["dep1", "dep2"]
#MISE depends_post=["post1"]
#MISE wait_for=["wait1"]
#MISE env={TEST_VAR="value"}
#MISE dir="/some/dir"
#MISE hide=true
#MISE raw=true
#MISE sources=["src1.txt", "src2.txt"]
#MISE outputs=["out1.txt"]
#MISE shell="bash -c"
#MISE quiet=true
#MISE silent=true
#MISE tools={node="20", python="3.11"}
#MISE confirm="Are you sure?"
echo "test"
"#;
        fs::write(&task_file, script_content).unwrap();
        fs::set_permissions(&task_file, std::fs::Permissions::from_mode(0o755)).unwrap();

        let config = Config::get().await.unwrap();
        let task = Task::from_path(&config, &task_file, &tasks_dir, temp_dir.path())
            .await
            .unwrap();

        assert_eq!(task.description, "Test task with all fields");
        assert_eq!(task.aliases, vec!["alias1", "alias2"]);
        assert_eq!(task.depends.len(), 2);
        assert_eq!(task.depends_post.len(), 1);
        assert_eq!(task.wait_for.len(), 1);
        assert_eq!(task.dir, Some("/some/dir".to_string()));
        assert_eq!(task.hide, true);
        assert_eq!(task.raw, true);
        assert_eq!(task.sources, vec!["src1.txt", "src2.txt"]);
        assert_eq!(task.shell, Some("bash -c".to_string()));
        assert_eq!(task.quiet, true);
        assert!(!task.tools.is_empty());
        assert_eq!(task.confirm, Some("Are you sure?".to_string()));

        let mut parsed_fields =
            take_captured_fields().expect("Parser fields should have been captured");

        // Group "alias" and "aliases" as they are alternate forms (count as 1)
        let has_alias = parsed_fields.iter().any(|k| k == "alias");
        parsed_fields.retain(|k| k != "aliases" || !has_alias);

        // Count property lines in script (exclude shebang and echo command)
        let script_lines = script_content.lines().count() - 2;

        assert_eq!(
            parsed_fields.len(),
            script_lines,
            "Parser looks for {} properties but test script has {} field lines.\n\
             If you added (or removed) parseable fields, add it to the test script.\n\
             Parser fields: {:?}",
            parsed_fields.len(),
            script_lines,
            parsed_fields
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_multi_line_tools_merge() {
        // Regression test for https://github.com/jdx/mise/discussions/7839
        // Multiple #MISE tools.X=Y lines should be merged into a single tools table
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let tasks_dir = temp_dir.path().join("tasks");
        fs::create_dir(&tasks_dir).unwrap();
        let task_file = tasks_dir.join("multi-tools-task");

        // Create a file task with multiple tools on separate lines
        let script_content = r#"#!/usr/bin/env bash
#MISE tools.node="20"
#MISE tools.python="3.11"
#MISE tools.ruby="3.2"
echo "test"
"#;
        fs::write(&task_file, script_content).unwrap();
        fs::set_permissions(&task_file, std::fs::Permissions::from_mode(0o755)).unwrap();

        let config = Config::get().await.unwrap();
        let task = Task::from_path(&config, &task_file, &tasks_dir, temp_dir.path())
            .await
            .unwrap();

        // All three tools should be present
        assert_eq!(
            task.tools.len(),
            3,
            "Expected 3 tools, got: {:?}",
            task.tools
        );
        assert!(
            task.tools.contains_key("node"),
            "Expected 'node' in tools: {:?}",
            task.tools
        );
        assert!(
            task.tools.contains_key("python"),
            "Expected 'python' in tools: {:?}",
            task.tools
        );
        assert!(
            task.tools.contains_key("ruby"),
            "Expected 'ruby' in tools: {:?}",
            task.tools
        );
        assert_eq!(task.tools.get("node").unwrap(), "20");
        assert_eq!(task.tools.get("python").unwrap(), "3.11");
        assert_eq!(task.tools.get("ruby").unwrap(), "3.2");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_hyphenated_and_numeric_tool_names() {
        // Test that tool names with hyphens and numbers are parsed correctly
        // e.g., git-cliff, 1password
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let tasks_dir = temp_dir.path().join("tasks");
        fs::create_dir(&tasks_dir).unwrap();
        let task_file = tasks_dir.join("hyphenated-tools-task");

        // Create a file task with hyphenated and numeric tool names
        let script_content = r#"#!/usr/bin/env bash
#MISE tools.git-cliff="1.0"
#MISE tools.1password-cli="2.0"
echo "test"
"#;
        fs::write(&task_file, script_content).unwrap();
        fs::set_permissions(&task_file, std::fs::Permissions::from_mode(0o755)).unwrap();

        let config = Config::get().await.unwrap();
        let task = Task::from_path(&config, &task_file, &tasks_dir, temp_dir.path())
            .await
            .unwrap();

        // Both tools should be present
        assert_eq!(
            task.tools.len(),
            2,
            "Expected 2 tools, got: {:?}",
            task.tools
        );
        assert!(
            task.tools.contains_key("git-cliff"),
            "Expected 'git-cliff' in tools: {:?}",
            task.tools
        );
        assert!(
            task.tools.contains_key("1password-cli"),
            "Expected '1password-cli' in tools: {:?}",
            task.tools
        );
        assert_eq!(task.tools.get("git-cliff").unwrap(), "1.0");
        assert_eq!(task.tools.get("1password-cli").unwrap(), "2.0");
    }

    #[test]
    fn test_get_matching_wildcard_does_not_match_parent() {
        use std::collections::BTreeMap;

        use super::GetMatchingExt;

        let mut tasks: BTreeMap<String, String> = BTreeMap::new();
        tasks.insert("test".to_string(), "test".to_string());
        tasks.insert("test:foo".to_string(), "test:foo".to_string());
        tasks.insert("test:bar".to_string(), "test:bar".to_string());

        // "test:*" should match "test:foo" and "test:bar" but NOT "test" itself
        let matches = tasks.get_matching("test:*").unwrap();
        assert_eq!(
            matches,
            vec![&"test:bar".to_string(), &"test:foo".to_string()]
        );

        // Bare name "test" should still match the "test" task (implicit wildcard)
        let matches = tasks.get_matching("test").unwrap();
        assert!(matches.contains(&&"test".to_string()));
    }
}
