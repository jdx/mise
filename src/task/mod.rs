use crate::config::config_file::toml::{TomlParser, deserialize_arr};
use crate::config::{self, Config};
use crate::task::task_script_parser::{TaskScriptParser, has_any_args_defined};
use crate::tera::get_tera;
use crate::ui::tree::TreeItem;
use crate::{dirs, env, file};
use console::{Color, truncate_str};
use either::Either;
use eyre::{Result, eyre};
use globset::GlobBuilder;
use indexmap::IndexMap;
use itertools::Itertools;
use petgraph::prelude::*;
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock as Lazy;
use std::{ffi, fmt, path};
use xx::regex;

mod deps;
mod task_dep;
pub mod task_file_providers;
mod task_script_parser;
pub mod task_sources;

use crate::config::config_file::ConfigFile;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::toolset::Toolset;
use crate::ui::style;
pub use deps::Deps;
use task_dep::TaskDep;
use task_sources::TaskOutputs;

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
    pub env: BTreeMap<String, EitherStringOrIntOrBool>,
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
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub tools: IndexMap<String, String>,
    #[serde(default)]
    pub usage: String,

    // normal type
    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run: Vec<String>,

    #[serde(default, deserialize_with = "deserialize_arr")]
    pub run_windows: Vec<String>,

    // command type
    // pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,

    // script type
    // pub script: Option<String>,

    // file type
    #[serde(default)]
    pub file: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct EitherStringOrIntOrBool(
    #[serde(with = "either::serde_untagged")] pub Either<String, EitherIntOrBool>,
);

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct EitherIntOrBool(#[serde(with = "either::serde_untagged")] pub Either<i64, bool>);

impl Display for EitherIntOrBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Either::Left(i) => write!(f, "{i}"),
            Either::Right(b) => write!(f, "{b}"),
        }
    }
}

impl Display for EitherStringOrIntOrBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Either::Left(s) => write!(f, "\"{s}\""),
            Either::Right(b) => write!(f, "{b}"),
        }
    }
}

impl Debug for EitherStringOrIntOrBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
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
                regex!(r"^(#|//) mise ([a-z_]+=.+)$")
                    .captures(line)
                    .or_else(|| regex!(r"^(#|//|::)MISE ([a-z_]+=.+)$").captures(line))
            })
            .map(|captures| captures.extract().1)
            .flat_map(|[_, toml]| {
                toml.parse::<toml::Value>()
                    .map_err(|e| debug!("failed to parse toml: {e}"))
            })
            .filter_map(|toml| toml.as_table().cloned())
            .flatten()
            .fold(toml::Table::new(), |mut map, (key, value)| {
                map.insert(key, value);
                map
            });
        let info = toml::Value::Table(info);
        let p = TomlParser::new(&info);
        // trace!("task info: {:#?}", info);

        task.description = p.parse_str("description").unwrap_or_default();
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
        task.outputs = info.get("outputs").map(|to| to.into()).unwrap_or_default();
        task.file = Some(path.to_path_buf());
        task.shell = p.parse_str("shell");
        task.quiet = p.parse_bool("quiet").unwrap_or_default();
        task.silent = p.parse_bool("silent").unwrap_or_default();
        task.tools = p
            .parse_table("tools")
            .map(|t| {
                t.into_iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        task.render(config, config_root).await?;
        Ok(task)
    }

    /// prints the task name without an extension
    pub fn display_name(&self, all_tasks: &BTreeMap<String, Task>) -> String {
        let display_name = self
            .name
            .rsplitn(2, '.')
            .last()
            .unwrap_or_default()
            .to_string();
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
        let pat = pat.rsplitn(2, '.').last().unwrap_or_default();
        self.name.rsplitn(2, '.').last().unwrap_or_default() == pat
            || self.aliases.contains(&pat.to_string())
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

    pub fn run(&self) -> &Vec<String> {
        if cfg!(windows) && !self.run_windows.is_empty() {
            &self.run_windows
        } else {
            &self.run
        }
    }

    pub fn all_depends(&self, tasks: &BTreeMap<String, &Task>) -> Result<Vec<Task>> {
        let mut depends: Vec<Task> = self
            .depends
            .iter()
            .chain(self.depends_post.iter())
            .map(|td| match_tasks(tasks, td))
            .flatten_ok()
            .filter_ok(|t| t.name != self.name)
            .collect::<Result<Vec<_>>>()?;
        for dep in depends.clone() {
            let mut extra = dep.all_depends(tasks)?;
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
        let tasks_to_run: HashSet<&Task> = tasks_to_run.iter().collect();
        let tasks = config.tasks_with_aliases().await?;
        let depends = self
            .depends
            .iter()
            .map(|td| match_tasks(&tasks, td))
            .flatten_ok()
            .collect_vec();
        let wait_for = self
            .wait_for
            .iter()
            .map(|td| match_tasks(&tasks, td))
            .flatten_ok()
            .filter_ok(|t| tasks_to_run.contains(t))
            .collect_vec();
        let depends_post = tasks_to_run
            .iter()
            .flat_map(|t| t.depends_post.iter().map(|td| match_tasks(&tasks, td)))
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

    pub async fn parse_usage_spec(
        &self,
        config: &Arc<Config>,
        cwd: Option<PathBuf>,
        env: &EnvMap,
    ) -> Result<(usage::Spec, Vec<String>)> {
        let (mut spec, scripts) = if let Some(file) = &self.file {
            let spec = usage::Spec::parse_script(file)
                .inspect_err(|e| {
                    warn!(
                        "failed to parse task file {} with usage: {e:?}",
                        file::display_path(file)
                    )
                })
                .unwrap_or_default();
            (spec, vec![])
        } else {
            let (scripts, spec) = TaskScriptParser::new(cwd)
                .parse_run_scripts(config, self, self.run(), env)
                .await?;
            (spec, scripts)
        };
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
        Ok((spec, scripts))
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
            let scripts = TaskScriptParser::new(cwd)
                .parse_run_scripts_with_args(config, self, self.run(), env, args, &spec)
                .await?;
            Ok(scripts.into_iter().map(|s| (s, vec![])).collect())
        } else {
            Ok(scripts
                .iter()
                .enumerate()
                .map(|(i, script)| {
                    // only pass args to the last script if no formal args are defined
                    match i == self.run().len() - 1 {
                        true => (script.clone(), args.iter().cloned().collect_vec()),
                        false => (script.clone(), vec![]),
                    }
                })
                .collect())
        }
    }

    pub async fn render_markdown(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        dir: &Path,
    ) -> Result<String> {
        let env = self.render_env(config, ts).await?;
        let (spec, _) = self
            .parse_usage_spec(config, Some(dir.to_path_buf()), &env)
            .await?;
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
        let prefix = style::ereset() + &style::estyle(self.prefix()).fg(COLORS[idx]).to_string();
        if *env::MISE_TASK_LEVEL > 0 {
            format!(
                "MISE_TASK_UNNEST:{}:MISE_TASK_UNNEST {prefix}",
                *env::MISE_TASK_LEVEL
            )
        } else {
            prefix
        }
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

    pub async fn tera_ctx(&self, config: &Arc<Config>) -> Result<tera::Context> {
        let ts = config.get_toolset().await?;
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        tera_ctx.insert("config_root", &self.config_root);
        Ok(tera_ctx)
    }

    pub fn cf<'a>(&self, config: &'a Config) -> Option<&'a Arc<dyn ConfigFile>> {
        config.config_files.get(&self.config_source)
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
        self.outputs.render(&mut tera, &tera_ctx)?;
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

    pub async fn render_env(&self, config: &Arc<Config>, ts: &Toolset) -> Result<EnvMap> {
        let mut tera = get_tera(self.config_root.as_deref());
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        let mut env = ts.full_env(config).await?;
        if let Some(root) = &config.project_root {
            tera_ctx.insert("config_root", &root);
        }
        let task_env: Vec<(String, String)> = self
            .env
            .iter()
            .filter_map(|(k, v)| match &v.0 {
                Either::Left(v) => Some((k.to_string(), v.to_string())),
                Either::Right(EitherIntOrBool(Either::Left(v))) => {
                    Some((k.to_string(), v.to_string()))
                }
                _ => None,
            })
            .collect_vec();
        for (k, v) in task_env {
            tera_ctx.insert("env", &env);
            env.insert(k, tera.render_str(&v, &tera_ctx)?);
        }
        let rm_env = self
            .env
            .iter()
            .filter(|(_, v)| v.0 == Either::Right(EitherIntOrBool(Either::Right(false))))
            .map(|(k, _)| k)
            .collect::<HashSet<_>>();

        Ok(env
            .into_iter()
            .filter(|(k, _)| !rm_env.contains(k))
            .collect())
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

fn match_tasks(tasks: &BTreeMap<String, &Task>, td: &TaskDep) -> Result<Vec<Task>> {
    let matches = tasks
        .get_matching(&td.task)?
        .into_iter()
        .map(|t| {
            let mut t = (*t).clone();
            t.args = td.args.clone();
            t
        })
        .collect_vec();
    if matches.is_empty() {
        return Err(eyre!("task not found: {td}"));
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
            env: BTreeMap::new(),
            dir: None,
            hide: false,
            global: false,
            raw: false,
            sources: vec![],
            outputs: Default::default(),
            shell: None,
            silent: false,
            run: vec![],
            run_windows: vec![],
            args: vec![],
            file: None,
            quiet: false,
            tools: Default::default(),
            usage: "".to_string(),
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let cmd = if let Some(command) = self.run().first() {
            Some(command.to_string())
        } else {
            self.file.as_ref().map(display_path)
        };

        if let Some(cmd) = cmd {
            let cmd = cmd.lines().next().unwrap_or_default();
            write!(f, "{} {}", self.prefix(), truncate_str(cmd, 60, "…"))
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

impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => self.args.cmp(&other.args),
            o => o,
        }
    }
}

impl Hash for Task {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.args.iter().for_each(|arg| arg.hash(state));
    }
}

impl Eq for Task {}
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.args == other.args
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

    fn children(&self) -> Cow<[Self::Child]> {
        let v: Vec<_> = self.0.neighbors(self.1).map(|i| (self.0, i)).collect();
        Cow::from(v)
    }
}

pub trait GetMatchingExt<T> {
    fn get_matching(&self, pat: &str) -> Result<Vec<&T>>;
}

impl<T> GetMatchingExt<T> for BTreeMap<String, T>
where
    T: Eq + Hash,
{
    fn get_matching(&self, pat: &str) -> Result<Vec<&T>> {
        let normalized = pat.split(':').collect::<PathBuf>();
        let matcher = GlobBuilder::new(&normalized.to_string_lossy())
            .literal_separator(true)
            .build()?
            .compile_matcher();

        Ok(self
            .iter()
            .filter(|(k, _)| {
                let path: PathBuf = k.split(':').collect();
                if matcher.is_match(&path) {
                    return true;
                }
                if let Some(stem) = path.file_stem() {
                    let base_path = path.with_file_name(stem);
                    return matcher.is_match(&base_path);
                }
                false
            })
            .map(|(_, t)| t)
            .unique()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::task::Task;
    use crate::{config::Config, dirs};
    use pretty_assertions::assert_eq;

    use super::name_from_path;

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
}
