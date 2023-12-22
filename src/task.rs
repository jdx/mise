use console::truncate_str;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use eyre::Result;
use itertools::Itertools;

use crate::config::config_file::toml::TomlParser;
use crate::config::Config;
use crate::file;

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
    pub fn from_path(path: PathBuf) -> Result<Task> {
        let info = file::read_to_string(&path)?
            .lines()
            .filter_map(|line| regex!(r"^# rtx ([a-z]+=.+)$").captures(line))
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
        let p = TomlParser::new(&info);
        // trace!("task info: {:#?}", info);

        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let task = Task {
            hide: !file::is_executable(&path) || p.parse_bool("hide").unwrap_or_default(),
            description: p.parse_str("description").unwrap_or_default(),
            sources: p.parse_array("sources").unwrap_or_default(),
            outputs: p.parse_array("outputs").unwrap_or_default(),
            depends: p.parse_array("depends").unwrap_or_default(),
            dir: p.parse_str("dir").map(PathBuf::from),
            env: p.parse_hashmap("env").unwrap_or_default(),
            file: Some(path.clone()),
            ..Task::new(name, path)
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
        let tasks = config.tasks();
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

    // pub fn project_root(&self) -> &Path {
    //     match self
    //         .config_source
    //         .parent()
    //         .expect("task source has no parent")
    //     {
    //         dir if dir.ends_with(".rtx/tasks") => dir.parent().unwrap(),
    //         dir if dir.ends_with(".config/rtx/tasks") => dir.parent().unwrap().parent().unwrap(),
    //         dir => dir,
    //     }
    // }
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
