use petgraph::graph::DiGraph;
use petgraph::prelude::*;
use petgraph::{Direction, Graph};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use console::truncate_str;
use eyre::Result;
use itertools::Itertools;

use crate::config::config_file::toml::TomlParser;
use crate::config::Config;
use crate::file;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::ui::tree::TreeItem;

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct Task {
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub config_source: PathBuf,
    pub depends: Vec<String>,
    pub env: HashMap<String, String>,
    pub dir: Option<PathBuf>,
    pub hide: bool,
    pub raw: bool,
    pub sources: Vec<String>,
    pub outputs: Vec<String>,

    // normal type
    pub run: Vec<String>,

    // command type
    // pub command: Option<String>,
    pub args: Vec<String>,

    // script type
    // pub script: Option<String>,

    // file type
    pub file: Option<PathBuf>,
}

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
        let config_root = config_root(path);
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &config_root);
        let p = TomlParser::new(&info, get_tera(config_root), tera_ctx);
        // trace!("task info: {:#?}", info);

        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let task = Task {
            hide: !file::is_executable(path) || p.parse_bool("hide").unwrap_or_default(),
            description: p.parse_str("description")?.unwrap_or_default(),
            sources: p.parse_array("sources")?.unwrap_or_default(),
            outputs: p.parse_array("outputs")?.unwrap_or_default(),
            depends: p.parse_array("depends")?.unwrap_or_default(),
            dir: p.parse_str("dir")?,
            env: p.parse_hashmap("env")?.unwrap_or_default(),
            file: Some(path.to_path_buf()),
            ..Task::new(name, path.to_path_buf())
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
        let tasks = config.tasks_with_aliases();
        let depends = self
            .depends
            .iter()
            .map(|name| match name.strip_suffix('*') {
                Some(prefix) => Ok(tasks
                    .values()
                    .unique()
                    .filter(|t| *t != self && t.name.starts_with(prefix))
                    .collect::<Vec<_>>()),
                None => tasks
                    .get(name)
                    .map(|task| vec![task])
                    .ok_or_else(|| eyre!("task not found: {name}")),
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(depends)
    }
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

    // fn pop(&'a mut self) -> Option<&'a Task> {
    //     if let Some(leaf) = self.leaves().first() {
    //         self.remove(&leaf.clone())
    //     } else {
    //         None
    //     }
    // }
}

impl TreeItem for (&Graph<Task, ()>, NodeIndex) {
    type Child = Self;

    fn write_self(&self) {
        if let Some(w) = self.0.node_weight(self.1) {
            miseprint!("{}", w.name);
        }
    }

    fn children(&self) -> Cow<[Self::Child]> {
        let v: Vec<_> = self.0.neighbors(self.1).map(|i| (self.0, i)).collect();
        Cow::from(v)
    }
}

fn config_root(config_source: &Path) -> &Path {
    match config_source.parent().expect("task source has no parent") {
        dir if dir.ends_with(".mise/tasks") => dir.parent().unwrap().parent().unwrap(),
        dir if dir.ends_with(".config/mise/tasks") => {
            dir.parent().unwrap().parent().unwrap().parent().unwrap()
        }
        dir => dir,
    }
}
