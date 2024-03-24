use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use console::truncate_str;
use either::Either;
use eyre::Result;
use globset::Glob;
use itertools::Itertools;
use petgraph::prelude::*;
use serde_derive::Deserialize;

use crate::config::config_file::toml::deserialize_arr;
use crate::config::config_file::toml::TomlParser;
use crate::config::Config;
use crate::file;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::ui::tree::TreeItem;

#[derive(Debug, Default, Clone, Eq, PartialEq, Deserialize)]
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
    pub env: HashMap<String, EitherStringOrBool>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EitherStringOrBool(#[serde(with = "either::serde_untagged")] pub Either<String, bool>);

impl Task {
    pub fn new(name: String, config_source: PathBuf) -> Task {
        Task {
            name: name.clone(),
            config_source,
            ..Default::default()
        }
    }
    pub fn from_path(path: &Path) -> Result<Task> {
        let info = file::read_to_string(path)?
            .lines()
            .filter_map(|line| regex!(r"^# mise ([a-z]+=.+)$").captures(line))
            .map(|captures| captures.extract())
            .flat_map(|(_, [toml])| {
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
            hide: !file::is_executable(path) || p.parse_bool("hide").unwrap_or_default(),
            aliases: p
                .parse_array("alias")?
                .unwrap_or(vec![p.parse_str("alias")?.unwrap_or_default()]),
            description: p.parse_str("description")?.unwrap_or_default(),
            sources: p.parse_array("sources")?.unwrap_or_default(),
            outputs: p.parse_array("outputs")?.unwrap_or_default(),
            depends: p.parse_array("depends")?.unwrap_or_default(),
            dir: p.parse_str("dir")?,
            env: p.parse_env("env")?.unwrap_or_default(),
            file: Some(path.to_path_buf()),
            ..Task::new(name_from_path(config_root, path)?, path.to_path_buf())
        };
        Ok(task)
    }

    pub fn command_string(&self) -> Option<String> {
        if let Some(command) = self.run.first() {
            Some(command.to_string())
            // } else if let Some(script) = &self.script {
            //     Some(script.to_string())
        } else {
            self.file
                .as_ref()
                .map(|file| file.to_str().unwrap().to_string())
        }
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
}

fn name_from_path(root: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<String> {
    Ok(path
        .as_ref()
        .strip_prefix(root)
        .map(|p| match p {
            p if p.starts_with(".mise/tasks") => p.strip_prefix(".mise/tasks"),
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

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(cmd) = self.command_string() {
            write!(f, "{} {}", self.prefix(), truncate_str(&cmd, 60, "…"))
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

#[derive(Debug)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<String>,
    tx: mpsc::Sender<Option<Task>>,
}

impl Deps {
    pub fn new(config: &Config, tasks: Vec<Task>) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];
        for t in tasks {
            stack.push(t.clone());
            indexes
                .entry(t.name.clone())
                .or_insert_with(|| graph.add_node(t));
        }
        while let Some(a) = stack.pop() {
            let a_idx = *indexes
                .entry(a.name.clone())
                .or_insert_with(|| graph.add_node(a.clone()));
            for b in a.resolve_depends(config)? {
                let b_idx = *indexes
                    .entry(b.name.clone())
                    .or_insert_with(|| graph.add_node(b.clone()));
                if !graph.contains_edge(a_idx, b_idx) {
                    graph.add_edge(a_idx, b_idx, ());
                }
                stack.push(b.clone());
            }
        }
        let (tx, _) = mpsc::channel();
        let sent = HashSet::new();
        Ok(Self { graph, tx, sent })
    }

    fn leaves(&self) -> Vec<Task> {
        self.graph
            .externals(Direction::Outgoing)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    fn emit_leaves(&mut self) {
        let leaves = self.leaves().into_iter().collect_vec();
        for task in leaves {
            if self.sent.contains(&task.name) {
                continue;
            }
            self.sent.insert(task.name.clone());
            self.tx.send(Some(task)).unwrap();
        }
        if self.graph.node_count() == 0 {
            self.tx.send(None).unwrap();
        }
    }

    pub fn subscribe(&mut self) -> mpsc::Receiver<Option<Task>> {
        let (tx, rx) = mpsc::channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    // #[requires(self.graph.node_count() > 0)]
    // #[ensures(self.graph.node_count() == old(self.graph.node_count()) - 1)]
    pub fn remove(&mut self, task: &Task) {
        if let Some(idx) = self
            .graph
            .node_indices()
            .find(|&idx| &self.graph[idx] == task)
        {
            self.graph.remove_node(idx);
            self.emit_leaves();
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &Task> {
        self.graph.node_indices().map(|idx| &self.graph[idx])
    }

    pub fn is_linear(&self) -> bool {
        !self.graph.node_indices().any(|idx| {
            self.graph
                .neighbors_directed(idx, Direction::Outgoing)
                .count()
                > 1
        })
    }

    // fn pop(&'a mut self) -> Option<&'a Tasks> {
    //     if let Some(leaf) = self.leaves().first() {
    //         self.remove(&leaf.clone())
    //     } else {
    //         None
    //     }
    // }
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
        if ancestor.ends_with(".mise/tasks") {
            return ancestor.parent()?.parent();
        }

        if ancestor.ends_with(".config/mise/tasks") {
            return ancestor.parent()?.parent()?.parent();
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

    use crate::task::Task;

    use super::{config_root, name_from_path};

    #[test]
    fn test_from_path() {
        let test_cases = [(".mise/tasks/filetask", "filetask", vec!["ft"])];

        for (path, name, aliases) in test_cases {
            let t = Task::from_path(Path::new(path)).unwrap();
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

    #[test]
    fn test_config_root() {
        let test_cases = [
            ("/base", Some(Path::new("/"))),
            ("/base/.mise/tasks", Some(Path::new("/base"))),
            ("/base/.config/mise/tasks", Some(Path::new("/base"))),
        ];

        for (src, expected) in test_cases {
            assert_eq!(config_root(&src), expected)
        }
    }
}
