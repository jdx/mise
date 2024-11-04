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
use std::collections::BTreeMap;
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
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, EitherStringOrBool>,
    #[serde(default)]
    pub dir: Option<PathBuf>,
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
pub struct EitherStringOrBool(#[serde(with = "either::serde_untagged")] pub Either<String, bool>);

impl Display for EitherStringOrBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Either::Left(s) => write!(f, "\"{}\"", s),
            Either::Right(b) => write!(f, "{}", b),
        }
    }
}

impl Debug for EitherStringOrBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Task {
    pub fn from_path(path: &Path) -> Result<Task> {
        let info = file::read_to_string(path)?
            .lines()
            .filter_map(|line| {
                regex!(r"^(#|//) mise ([a-z]+=.+)$")
                    .captures(line)
                    .or(regex!(r"^(#|//)MISE ([a-z]+=.+)$").captures(line))
            })
            .map(|captures| captures.extract())
            .flat_map(|(_, [_, toml])| {
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
        let config_root =
            config_root(&path).ok_or_else(|| eyre!("config root not found: {}", path.display()))?;
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &config_root);
        let p = TomlParser::new(&info, get_tera(Some(config_root)), tera_ctx);
        // trace!("task info: {:#?}", info);

        let task = Task {
            name: name_from_path(config_root, path)?,
            config_source: path.to_path_buf(),
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

    pub fn resolve_depends<'a>(&self, config: &'a Config) -> Result<Vec<&'a Task>> {
        let tasks = config.tasks_with_aliases()?;
        self.depends
            .iter()
            .map(|pat| match_tasks(tasks.clone(), pat))
            .flatten_ok()
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
            let (scripts, spec) = TaskScriptParser::new(cwd).parse_run_scripts(&self.run)?;
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
                    match i == self.run.len() - 1 {
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
}

fn name_from_path(root: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<String> {
    Ok(path
        .as_ref()
        .strip_prefix(root)
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

fn match_tasks<'a>(tasks: BTreeMap<String, &'a Task>, pat: &str) -> Result<Vec<&'a Task>> {
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
            depends: vec![],
            env: BTreeMap::new(),
            dir: None,
            hide: false,
            raw: false,
            sources: vec![],
            outputs: vec![],
            shell: None,
            run: vec![],
            args: vec![],
            file: None,
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let cmd = if let Some(command) = self.run.first() {
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

fn config_root(config_source: &impl AsRef<Path>) -> Option<&Path> {
    for ancestor in config_source.as_ref().ancestors() {
        if ancestor.ends_with("mise-tasks") {
            return ancestor.parent();
        }

        if ancestor.ends_with(".mise-tasks") {
            return ancestor.parent();
        }

        if ancestor.ends_with(".mise/tasks") {
            return ancestor.parent()?.parent();
        }

        if ancestor.ends_with(".config/mise/tasks") {
            return ancestor.parent()?.parent()?.parent();
        }

        if ancestor.ends_with("mise/tasks") {
            return ancestor.parent()?.parent();
        }
    }

    config_source.as_ref().parent()
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
                let p: PathBuf = k.split(':').collect();

                matcher.is_match(p)
            })
            .map(|(_, t)| t)
            .unique()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use crate::task::Task;
    use crate::test::reset;

    use super::{config_root, name_from_path};

    #[test]
    fn test_from_path() {
        reset();
        let test_cases = [(".mise/tasks/filetask", "filetask", vec!["ft"])];

        for (path, name, aliases) in test_cases {
            let t = Task::from_path(Path::new(path)).unwrap();
            assert_eq!(t.name, name);
            assert_eq!(t.aliases, aliases);
        }
    }

    #[test]
    fn test_name_from_path() {
        reset();
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
        reset();
        let test_cases = [("/some/other/dir", "/.mise/tasks/a")];

        for (root, path) in test_cases {
            assert!(name_from_path(root, path).is_err())
        }
    }

    #[test]
    fn test_config_root() {
        reset();
        let test_cases = [
            ("/base", Some(Path::new("/"))),
            ("/base/.mise/tasks", Some(Path::new("/base"))),
            ("/base/.config/mise/tasks", Some(Path::new("/base"))),
            ("/base/mise/tasks", Some(Path::new("/base"))),
        ];

        for (src, expected) in test_cases {
            assert_eq!(config_root(&src), expected)
        }
    }
}
