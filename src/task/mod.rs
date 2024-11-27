use crate::config::config_file::toml::{deserialize_arr, TomlParser};
use crate::config::Config;
use crate::file;
use crate::task::task_script_parser::{
    has_any_args_defined, replace_template_placeholders_with_args, TaskScriptParser,
};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::ui::tree::TreeItem;
use console::{truncate_str, Color};
use either::Either;
use eyre::{eyre, Result};
use globset::Glob;
use itertools::Itertools;
use once_cell::sync::Lazy;
use petgraph::prelude::*;
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{ffi, fmt, path};
use xx::regex;

mod deps;
mod task_script_parser;

use crate::file::display_path;
use crate::ui::style;
pub use deps::Deps;

#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct Task {
    #[serde(skip)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "alias", deserialize_with = "deserialize_arr")]
    pub aliases: Vec<String>,
    #[serde(skip)]
    pub config_source: PathBuf,
    #[serde(skip)]
    pub config_root: Option<PathBuf>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub wait_for: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, EitherStringOrIntOrBool>,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub hide: bool,
    #[serde(default)]
    pub raw: bool,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub shell: Option<String>,

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
        write!(f, "{}", self)
    }
}

impl Task {
    pub fn from_path(path: &Path, prefix: &Path, config_root: &Path) -> Result<Task> {
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
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &config_root);
        let p = TomlParser::new(&info, get_tera(Some(config_root)), tera_ctx);
        // trace!("task info: {:#?}", info);

        let task = Task {
            name: name_from_path(prefix, path)?,
            config_source: path.to_path_buf(),
            config_root: Some(config_root.to_path_buf()),
            hide: !file::is_executable(path) || p.parse_bool("hide").unwrap_or_default(),
            aliases: p
                .parse_array("alias")?
                .or(p.parse_array("aliases")?)
                .or(p.parse_str("alias")?.map(|s| vec![s]))
                .or(p.parse_str("aliases")?.map(|s| vec![s]))
                .unwrap_or_default(),
            description: p.parse_str("description")?.unwrap_or_default(),
            sources: p.parse_array("sources")?.unwrap_or_default(),
            outputs: p.parse_array("outputs")?.unwrap_or_default(),
            depends: p.parse_array("depends")?.unwrap_or_default(),
            wait_for: p.parse_array("wait_for")?.unwrap_or_default(),
            dir: p.parse_str("dir")?,
            env: p.parse_env("env")?.unwrap_or_default(),
            file: Some(path.to_path_buf()),
            shell: p.parse_str("shell")?,
            ..Default::default()
        };
        Ok(task)
    }

    // pub fn args(&self) -> impl Iterator<Item = String> {
    //     if let Some(script) = &self.script {
    //         // TODO: cli_args
    //         vec!["-c".to_string(), script.to_string()].into_iter()
    //     } else {
    //         self.args
    //             .iter()
    //             .chain(self.cli_args.iter())
    //             .cloned()
    //             .collect_vec()
    //             .into_iter()
    //     }
    // }
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn prefix(&self) -> String {
        format!("[{}]", self.name)
    }

    pub fn run(&self) -> &Vec<String> {
        if cfg!(windows) && !self.run_windows.is_empty() {
            &self.run_windows
        } else {
            &self.run
        }
    }

    pub fn all_depends<'a>(&self, config: &'a Config) -> Result<Vec<&'a Task>> {
        let tasks = config.tasks_with_aliases()?;
        let mut depends: Vec<&Task> = self
            .depends
            .iter()
            .map(|pat| match_tasks(&tasks, pat))
            .flatten_ok()
            .filter_ok(|t| t.name != self.name)
            .collect::<Result<Vec<_>>>()?;
        for dep in depends.clone() {
            depends.extend(dep.all_depends(config)?);
        }
        Ok(depends)
    }

    pub fn resolve_depends<'a>(
        &self,
        config: &'a Config,
        tasks_to_run: &[&Task],
    ) -> Result<Vec<&'a Task>> {
        let tasks_to_run: HashSet<&Task> = tasks_to_run.iter().copied().collect();
        let tasks = config.tasks_with_aliases()?;
        self.wait_for
            .iter()
            .map(|pat| match_tasks(&tasks, pat))
            .flatten_ok()
            .filter_ok(|t| tasks_to_run.contains(*t))
            .chain(
                self.depends
                    .iter()
                    .map(|pat| match_tasks(&tasks, pat))
                    .flatten_ok(),
            )
            .filter_ok(|t| t.name != self.name)
            .collect()
    }

    pub fn parse_usage_spec(&self, cwd: Option<PathBuf>) -> Result<(usage::Spec, Vec<String>)> {
        let (mut spec, scripts) = if let Some(file) = &self.file {
            let mut spec = usage::Spec::parse_script(file)
                .inspect_err(|e| debug!("failed to parse task file with usage: {e}"))
                .unwrap_or_default();
            spec.cmd.name = self.name.clone();
            (spec, vec![])
        } else {
            let (scripts, spec) =
                TaskScriptParser::new(cwd).parse_run_scripts(&self.config_root, self.run())?;
            (spec, scripts)
        };
        spec.name = self.name.clone();
        spec.bin = self.name.clone();
        if spec.cmd.help.is_none() {
            spec.cmd.help = Some(self.description.clone());
        }
        spec.cmd.name = self.name.clone();
        spec.cmd.aliases = self.aliases.clone();
        if spec.cmd.before_help.is_none()
            && spec.cmd.before_help_long.is_none()
            && !self.depends.is_empty()
        {
            spec.cmd.before_help_long = Some(format!("- Depends: {}", self.depends.join(", ")));
        }
        spec.cmd.usage = spec.cmd.usage();
        Ok((spec, scripts))
    }

    pub fn render_run_scripts_with_args(
        &self,
        cwd: Option<PathBuf>,
        args: &[String],
    ) -> Result<Vec<(String, Vec<String>)>> {
        let (spec, scripts) = self.parse_usage_spec(cwd)?;
        if has_any_args_defined(&spec) {
            Ok(
                replace_template_placeholders_with_args(&spec, &scripts, args)?
                    .into_iter()
                    .map(|s| (s, vec![]))
                    .collect(),
            )
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

    pub fn render_markdown(&self, dir: &Path) -> Result<String> {
        let (spec, _) = self.parse_usage_spec(Some(dir.to_path_buf()))?;
        let ctx = usage::docs::markdown::MarkdownRenderer::new(&spec).with_header_level(2);
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
        let idx = self.name.chars().map(|c| c as usize).sum::<usize>() % COLORS.len();
        style::ereset() + &style::estyle(self.prefix()).fg(COLORS[idx]).to_string()
    }

    pub fn dir(&self) -> Result<Option<PathBuf>> {
        if let Some(dir) = &self.dir {
            // TODO: memoize
            // let dir = self.dir_rendered.get_or_try_init(|| -> Result<PathBuf> {
            let mut tera = get_tera(self.config_root.as_deref());
            let mut ctx = BASE_CONTEXT.clone();
            if let Some(config_root) = &self.config_root {
                ctx.insert("config_root", config_root);
            }
            let dir = tera.render_str(dir, &ctx)?;
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
}

fn name_from_path(prefix: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<String> {
    Ok(path
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
        .join(":"))
}

fn match_tasks<'a>(tasks: &BTreeMap<String, &'a Task>, pat: &str) -> Result<Vec<&'a Task>> {
    let matches = tasks.get_matching(pat)?.into_iter().cloned().collect_vec();
    if matches.is_empty() {
        return Err(eyre!("task not found: {pat}"));
    };

    Ok(matches)
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: "".to_string(),
            description: "".to_string(),
            aliases: vec![],
            config_source: PathBuf::new(),
            config_root: None,
            depends: vec![],
            wait_for: vec![],
            env: BTreeMap::new(),
            dir: None,
            hide: false,
            raw: false,
            sources: vec![],
            outputs: vec![],
            shell: None,
            run: vec![],
            run_windows: vec![],
            args: vec![],
            file: None,
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
        self.name.cmp(&other.name)
    }
}

impl Hash for Task {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl TreeItem for (&Graph<Task, ()>, NodeIndex) {
    type Child = Self;

    fn write_self(&self) -> std::io::Result<()> {
        if let Some(w) = self.0.node_weight(self.1) {
            miseprint!("{}", w.name)?;
        }
        std::io::Result::Ok(())
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
        let matcher = Glob::new(&normalized.to_string_lossy())?.compile_matcher();

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

    use crate::dirs;
    use crate::task::Task;
    use pretty_assertions::assert_eq;

    use super::name_from_path;

    #[test]
    fn test_from_path() {
        let test_cases = [(".mise/tasks/filetask", "filetask", vec!["ft"])];

        for (path, name, aliases) in test_cases {
            let t = Task::from_path(
                Path::new(path),
                Path::new(".mise/tasks"),
                Path::new(dirs::CWD.as_ref().unwrap()),
            )
            .unwrap();
            assert_eq!(t.name, name);
            assert_eq!(t.aliases, aliases);
        }
    }

    #[test]
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
