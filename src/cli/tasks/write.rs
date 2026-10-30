//! What `mise tasks add`, `mise tasks set` and `mise tasks rm` share: the flags that choose a
//! config file, the flags that set task fields, finding the files that define a task, and the
//! report each prints.

use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, bail};
use indexmap::IndexMap;
use itertools::Itertools;
use serde::Serialize;
use toml_edit::{Array, DocumentMut, InlineTable, Item, TableLike, Value};
use toml_writer::{ToTomlKey, ToTomlValue, TomlKeyBuilder, TomlString, TomlStringBuilder};

use crate::args::split_tool_version;
use crate::config::config_file;
use crate::config::{
    self, Config, ConfigPathOptions, Settings, SettingsExt, resolve_target_config_path,
};
use crate::file::{self, display_path};
use crate::task::{Task, TaskDecl};

/// The flags that choose the config file a task command uses, as `mise use` has them
#[derive(Debug, Default, usage_rs::Args)]
pub(super) struct TargetArgs {
    /// Use mise.<ENV>.toml in the current directory, or .mise.<ENV>.toml if that exists
    ///
    /// `--env local` uses mise.local.toml, which is for tasks you keep to
    /// yourself. Not the same as the global -E, which only chooses which config
    /// files are loaded.
    #[usage(long, short, overrides = ["global", "path"])]
    pub(super) env: Option<String>,
    /// Use the global config file
    #[usage(long, short, overrides = ["path", "env"])]
    pub(super) global: bool,
    /// Use this config file, or the config file in this directory
    #[usage(long, short, overrides = ["global", "env"], value_hint = usage_rs::ValueHint::FilePath)]
    pub(super) path: Option<PathBuf>,
}

impl TargetArgs {
    /// Whether any of the flags was given.
    pub(super) fn is_set(&self) -> bool {
        self.global || self.path.is_some() || self.env.is_some()
    }

    /// The config file the flags choose, or the default write target when none was given:
    /// https://mise.jdx.dev/configuration.html#target-file-for-write-operations
    pub(super) fn config_path(&self) -> Result<PathBuf> {
        resolve_target_config_path(ConfigPathOptions {
            global: self.global,
            path: self.path.clone(),
            env: self.env.clone(),
            prefer_toml: true,
            ..Default::default()
        })
    }
}

/// The flags that set a task's fields, shared by `mise tasks add` and `mise tasks set`
#[derive(Debug, Default, usage_rs::Args)]
pub(super) struct TaskFields {
    /// Other names for the task
    #[usage(long, short)]
    pub(super) alias: Vec<String>,
    /// Tasks to run before this one
    #[usage(long, short, value_name = "TASK")]
    pub(super) depends: Vec<String>,
    /// Run the task in a specific directory
    #[usage(long, short = 'D')]
    pub(super) dir: Option<String>,
    /// Hide the task from `mise tasks` and completions
    #[usage(long, short = 'H')]
    pub(super) hide: bool,
    /// Do not print the command before running
    #[usage(long, short)]
    pub(super) quiet: bool,
    /// Connect the task directly to the terminal's stdin, stdout, and stderr
    #[usage(long, short)]
    pub(super) raw: bool,
    /// Glob patterns of files this task uses as input
    #[usage(long, short, value_name = "GLOB")]
    pub(super) sources: Vec<String>,
    /// Wait for these tasks to finish if they are also being run
    #[usage(long, short, value_name = "TASK")]
    pub(super) wait_for: Vec<String>,

    /// Ask this question before the task runs, and run it only if the answer is yes
    #[usage(long, value_name = "MESSAGE")]
    pub(super) confirm: Option<String>,
    /// Tasks to run after this one
    #[usage(long, value_name = "TASK")]
    pub(super) depends_post: Vec<String>,
    /// Description of the task
    #[usage(long)]
    pub(super) description: Option<String>,
    /// Set an environment variable for the task; repeat for several
    ///
    /// Written to the task's `env` table. `--env` chooses the config file instead.
    #[usage(long, value_name = "KEY=VALUE")]
    pub(super) env_var: Vec<String>,
    /// Glob patterns of files this task creates, used to skip it when they are up to date
    #[usage(long, value_name = "GLOB")]
    pub(super) outputs: Vec<String>,
    /// Command to run on Windows (TOML tasks only; ignored with `--file`)
    #[usage(long, value_name = "COMMAND")]
    pub(super) run_windows: Option<String>,
    /// Run the task in a specific shell
    #[usage(long)]
    pub(super) shell: Option<String>,
    /// Do not print the command or its output
    #[usage(long)]
    pub(super) silent: bool,
    /// Tool the task needs, such as node@24 or npm:prettier@3; repeat for several
    ///
    /// Written to the task's `tools` table. Without a version, writes `latest`.
    #[usage(long, value_name = "TOOL@VERSION")]
    pub(super) tools: Vec<String>,
    /// Usage spec for the task's arguments and flags: https://usage.jdx.dev/spec/
    #[usage(long, value_name = "SPEC")]
    pub(super) usage: Option<String>,
}

impl TaskFields {
    /// Whether any field flag was given.
    pub(super) fn is_empty(&self) -> bool {
        let Self {
            alias,
            depends,
            dir,
            hide,
            quiet,
            raw,
            sources,
            wait_for,
            confirm,
            depends_post,
            description,
            env_var,
            outputs,
            run_windows,
            shell,
            silent,
            tools,
            usage,
        } = self;
        [
            alias,
            depends,
            sources,
            wait_for,
            depends_post,
            env_var,
            outputs,
            tools,
        ]
        .iter()
        .all(|values| values.is_empty())
            && [dir, confirm, description, run_windows, shell, usage]
                .iter()
                .all(|value| value.is_none())
            && !(*hide || *quiet || *raw || *silent)
    }

    /// The `[tasks.<name>]` keys the flags and `run` set, as TOML values, in the order
    /// `mise tasks add` writes them.
    pub(super) fn toml_entries(&self, run: &[String]) -> Result<Vec<(&'static str, Value)>> {
        let mut entries: Vec<(&'static str, Value)> = vec![];
        let mut list = |key, values: &[String]| {
            if !values.is_empty() {
                entries.push((key, Value::Array(values.iter().collect::<Array>())));
            }
        };
        list("depends", &self.depends);
        list("depends_post", &self.depends_post);
        list("wait_for", &self.wait_for);
        if let Some(description) = &self.description {
            entries.push(("description", description.into()));
        }
        if !self.alias.is_empty() {
            entries.push(("alias", Value::Array(self.alias.iter().collect())));
        }
        if let Some(dir) = &self.dir {
            entries.push(("dir", dir.into()));
        }
        if self.hide {
            entries.push(("hide", true.into()));
        }
        if self.raw {
            entries.push(("raw", true.into()));
        }
        if !self.sources.is_empty() {
            entries.push(("sources", Value::Array(self.sources.iter().collect())));
        }
        if !self.outputs.is_empty() {
            entries.push(("outputs", Value::Array(self.outputs.iter().collect())));
        }
        if let Some(shell) = &self.shell {
            entries.push(("shell", shell.into()));
        }
        if self.quiet {
            entries.push(("quiet", true.into()));
        }
        if self.silent {
            entries.push(("silent", true.into()));
        }
        if let Some(confirm) = &self.confirm {
            entries.push(("confirm", confirm.into()));
        }
        let env = self.env_pairs()?;
        if !env.is_empty() {
            entries.push(("env", Value::InlineTable(inline_table(&env))));
        }
        let tools = self.tool_pairs()?;
        if !tools.is_empty() {
            entries.push(("tools", Value::InlineTable(inline_table(&tools))));
        }
        if let Some(usage) = &self.usage {
            entries.push(("usage", usage.into()));
        }
        if !run.is_empty() {
            entries.push(("run", shell_words::join(run).into()));
        }
        if let Some(run_windows) = &self.run_windows {
            entries.push(("run_windows", run_windows.into()));
        }
        Ok(entries)
    }

    /// The `#MISE` and `#USAGE` header lines of a file task with these fields.
    pub(super) fn header_lines(&self) -> Result<Vec<String>> {
        let mut lines = vec![];
        if !self.depends.is_empty() {
            lines.push(header_line("depends", basic_strings(&self.depends)));
        }
        if !self.depends_post.is_empty() {
            lines.push(header_line(
                "depends_post",
                basic_strings(&self.depends_post),
            ));
        }
        if !self.wait_for.is_empty() {
            lines.push(header_line("wait_for", basic_strings(&self.wait_for)));
        }
        if !self.alias.is_empty() {
            lines.push(header_line("alias", basic_strings(&self.alias)));
        }
        if let Some(description) = &self.description {
            lines.push(header_line("description", basic_string(description)));
        }
        if let Some(dir) = &self.dir {
            lines.push(header_line("dir", basic_string(dir)));
        }
        if self.hide {
            lines.push("#MISE hide=true".to_string());
        }
        if self.raw {
            lines.push("#MISE raw=true".to_string());
        }
        if !self.sources.is_empty() {
            lines.push(header_line("sources", basic_strings(&self.sources)));
        }
        if !self.outputs.is_empty() {
            lines.push(header_line("outputs", basic_strings(&self.outputs)));
        }
        if self.quiet {
            lines.push("#MISE quiet=true".to_string());
        }
        if self.silent {
            lines.push("#MISE silent=true".to_string());
        }
        if let Some(confirm) = &self.confirm {
            lines.push(header_line("confirm", basic_string(confirm)));
        }
        let env = self.env_pairs()?;
        if !env.is_empty() {
            lines.push(format!("#MISE env={}", header_inline_table(&env)));
        }
        let tools = self.tool_pairs()?;
        if !tools.is_empty() {
            lines.push(format!("#MISE tools={}", header_inline_table(&tools)));
        }
        if let Some(usage) = &self.usage {
            lines.extend(usage.lines().map(|line| format!("#USAGE {line}")));
        }
        Ok(lines)
    }

    fn env_pairs(&self) -> Result<Vec<(&str, &str)>> {
        let pairs = self
            .env_var
            .iter()
            .map(|arg| match arg.split_once('=') {
                Some((key, value)) if !key.is_empty() => Ok((key, value)),
                _ => bail!("--env-var takes KEY=VALUE, got {arg:?}"),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(last_wins(pairs))
    }

    fn tool_pairs(&self) -> Result<Vec<(&str, &str)>> {
        let pairs = self
            .tools
            .iter()
            .map(|arg| match split_tool_version(arg) {
                ("", _) => bail!("--tools takes TOOL@VERSION, got {arg:?}"),
                (tool, version) => Ok((tool, version.unwrap_or("latest"))),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(last_wins(pairs))
    }
}

/// One pair per key, with the value given last, in the order the keys first appeared. A table
/// cannot repeat a key, and a `#MISE` header that did would not parse.
fn last_wins<'a>(pairs: Vec<(&'a str, &'a str)>) -> Vec<(&'a str, &'a str)> {
    let mut unique: IndexMap<&str, &str> = IndexMap::new();
    for (key, value) in pairs {
        unique.insert(key, value);
    }
    unique.into_iter().collect()
}

fn inline_table(pairs: &[(&str, &str)]) -> InlineTable {
    let mut table = InlineTable::new();
    for (key, value) in pairs {
        table.insert(*key, (*value).into());
    }
    table
}

/// An inline table on one line, with every string basic so that a newline is escaped instead of
/// ending the `#MISE` header line.
fn header_inline_table(pairs: &[(&str, &str)]) -> String {
    let pairs = pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{} = {}",
                TomlKeyBuilder::new(key).as_basic().to_toml_key(),
                basic_string(value).to_toml_value()
            )
        })
        .collect::<Vec<_>>();
    format!("{{ {} }}", pairs.join(", "))
}

/// A `#MISE key=value` header line, with the value written as TOML so that it is
/// quoted and escaped the way the header parser expects.
fn header_line(key: &str, value: impl toml_writer::WriteTomlValue) -> String {
    format!("#MISE {key}={}", value.to_toml_value())
}

/// A single-line basic TOML string, so that a newline in the value is escaped
/// instead of continuing the header on a line without the `#MISE` prefix.
fn basic_string(value: &str) -> TomlString<'_> {
    TomlStringBuilder::new(value).as_basic()
}

fn basic_strings(values: &[String]) -> Vec<TomlString<'_>> {
    values.iter().map(|value| basic_string(value)).collect()
}

/// Fails unless `key` is a field a `[tasks.<name>]` table accepts.
pub(super) fn ensure_task_field(key: &str) -> Result<()> {
    let fields = mise_config::de::struct_fields::<TaskDecl>();
    if !fields.contains(&key) {
        bail!("{key} is not a task field; see https://mise.jdx.dev/tasks/task-configuration.html");
    }
    Ok(())
}

/// Fails unless `path` is a TOML config file, the only kind a task can be written to.
pub(super) fn ensure_toml(path: &Path) -> Result<()> {
    if !is_toml(path) {
        bail!(
            "{} is not a TOML config file; TOML tasks are written to files such as mise.toml",
            display_path(path)
        );
    }
    Ok(())
}

fn is_toml(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}

/// Reads a config file for editing; a missing file is an empty document.
pub(super) fn read_doc(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    Ok(file::read_to_string(path)?.parse()?)
}

/// The `[tasks]` table of `doc`, created when `create` is set and it is missing.
pub(super) fn tasks_table<'a>(
    doc: &'a mut DocumentMut,
    path: &Path,
    create: bool,
) -> Result<Option<&'a mut dyn TableLike>> {
    if create && !doc.contains_key("tasks") {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc.insert("tasks", Item::Table(table));
    }
    match doc.get_mut("tasks") {
        None => Ok(None),
        Some(item) => match item.as_table_like_mut() {
            Some(tasks) => Ok(Some(tasks)),
            None => bail!("tasks in {} is not a table", display_path(path)),
        },
    }
}

/// Leaves out a `[tasks]` header that no longer holds anything but `[tasks.<name>]` tables, as
/// after the last `name = "cmd"` shorthand in it became a table or was removed. A header with a
/// comment stays, since the comment would go with it.
pub(super) fn hide_empty_tasks_header(doc: &mut DocumentMut) {
    if let Some(tasks) = doc.get_mut("tasks").and_then(Item::as_table_mut)
        && !tasks.is_empty()
        && tasks.iter().all(|(_, item)| item.is_table())
        && !tasks
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .is_some_and(|prefix| prefix.contains('#'))
    {
        tasks.set_implicit(true);
    }
}

/// The decoded value of task `name` in TOML source `text`, as `mise tasks --json` reports it.
pub(super) fn task_value(text: &str, name: &str) -> Result<Option<toml::Value>> {
    let mut doc: toml::Table = toml::from_str(text)?;
    Ok(match doc.remove("tasks") {
        Some(toml::Value::Table(mut tasks)) => tasks.remove(name),
        _ => None,
    })
}

/// Whether TOML config file `path` has a `[tasks.<name>]` entry.
fn defines_task(path: &Path, name: &str) -> Result<bool> {
    let mut doc = read_doc(path)?;
    Ok(tasks_table(&mut doc, path, false)?.is_some_and(|tasks| tasks.contains_key(name)))
}

/// A file that defines a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Definition {
    /// A `[tasks.<name>]` table in a config file.
    Toml(PathBuf),
    /// A file task's script.
    File(PathBuf),
    /// Anything else, such as a TOML file a `task_config.includes` entry names, which these
    /// commands do not edit.
    Other(PathBuf),
}

impl Definition {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Toml(path) | Self::File(path) | Self::Other(path) => path,
        }
    }
}

/// Every loaded config file that has a `[tasks.<name>]` table, highest precedence first, then
/// every file task script named `name`, including one that a TOML task shadows. A task defined
/// some other way, such as in a file a `task_config.includes` entry names or inferred from a
/// `package.json`, is reported as [`Definition::Other`], which these commands refuse to edit.
pub(super) async fn find_definitions(name: &str) -> Result<Vec<Definition>> {
    let config = Config::get().await?;
    let mut definitions = vec![];
    for path in config.config_files.keys() {
        if is_toml(path) && path.is_file() && defines_task(path, name)? {
            definitions.push(Definition::Toml(path.clone()));
        }
    }
    // In one config root a TOML task with a command takes the place of a same-named script,
    // so the loaded tasks never show that script. Look for it in the root's task directories.
    let roots = definitions
        .iter()
        .filter_map(|def| config.config_files.get(def.path()))
        .map(|cf| cf.config_root())
        .unique()
        .collect::<Vec<_>>();
    for root in roots {
        for script in scripts_in_root(&config, &root, name).await? {
            if !definitions.iter().any(|def| def.path() == script) {
                definitions.push(Definition::File(script));
            }
        }
    }
    for task in config.tasks_with_shadowed().await? {
        let source = &task.config_source;
        if task.name != name
            || config.config_files.contains_key(source)
            || definitions.iter().any(|def| def.path() == source)
        {
            continue;
        }
        definitions.push(if task.is_file_task() {
            Definition::File(source.clone())
        } else {
            Definition::Other(source.clone())
        });
    }
    if definitions.is_empty()
        && let Some(task) = config.tasks().await?.get(name)
    {
        definitions.push(Definition::Other(task.config_source.clone()));
    }
    Ok(definitions)
}

/// The scripts in the task directories of config root `root` that define task `name`.
async fn scripts_in_root(config: &Arc<Config>, root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    let mut scripts = vec![];
    for include in config::task_includes_for_dir(root, &config.config_files)? {
        if !include.is_dir() {
            continue;
        }
        let base = include.join(name.replace(':', MAIN_SEPARATOR_STR));
        // `build` is `build`, `build.sh` or `build/_default`, and the script's own name decides.
        let mut candidates = vec![base.clone(), base.join("_default")];
        if let (Some(parent), Some(stem)) = (base.parent(), base.file_name()) {
            candidates.extend(
                file::ls(parent)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|path| path.file_stem() == Some(stem)),
            );
        }
        for path in candidates.into_iter().unique() {
            if path.is_file()
                && Task::from_path(config, &path, &include, root)
                    .await
                    .is_ok_and(|task| task.name == name)
            {
                scripts.push(path);
            }
        }
    }
    Ok(scripts)
}

/// The one definition of `name` that `mise tasks set` or `mise tasks rm` should change: the
/// task in the file the target flags choose, or else the only file that defines it.
pub(super) async fn choose_definition(name: &str, target: &TargetArgs) -> Result<Definition> {
    if target.is_set() {
        let path = target.config_path()?;
        if !is_toml(&path) {
            // `--path` may name a file task's script, but only the one that defines this task:
            // anything else would let these commands delete or edit an arbitrary file.
            return match find_definitions(name).await?.into_iter().find(
                |def| matches!(def, Definition::File(script) if file::same_file(script, &path)),
            ) {
                Some(definition) => Ok(definition),
                None => bail!(
                    "{} is neither a TOML config file nor the script of task {name}",
                    display_path(&path)
                ),
            };
        }
        if !path.is_file() || !defines_task(&path, name)? {
            bail!("no task named {name} in {}", display_path(&path));
        }
        return Ok(Definition::Toml(path));
    }
    let definitions = find_definitions(name).await?;
    match definitions.as_slice() {
        [] => bail!("no task named {name}"),
        [definition] => Ok(definition.clone()),
        _ => {
            let paths = definitions
                .iter()
                .map(|def| display_path(def.path()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "task {name} is defined in several files ({paths}); choose one with --path, --env or --global"
            )
        }
    }
}

/// Whether config file `path` loads without asking for trust already, before a write.
fn is_already_trusted(path: &Path) -> bool {
    Settings::safe_mode() || config::is_global_config(path) || config_file::is_path_trusted(path)
}

/// Writes config file `path` the way `mise set` does: asks for trust beforehand, for the files the
/// record will cover, and records trust for the file's config root after. Returns whether the
/// write newly trusted that root.
pub(super) fn write_config(path: &Path, content: &str) -> Result<bool> {
    let already_trusted = is_already_trusted(path);
    config_file::trust_check_before_write(path)?;
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write(path, content)?;
    config_file::trust_after_write(path)?;
    Ok(!already_trusted)
}

/// What a command did to a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    Added,
    Replaced,
    Updated,
    Unchanged,
    Removed,
}

/// The kind of file that defines a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Toml,
    File,
}

/// One task a command added, changed or removed, as `--json` prints it.
#[derive(Debug, Serialize)]
pub(super) struct Report {
    pub(super) task: String,
    pub(super) action: Action,
    pub(super) kind: Kind,
    /// The config file or script that was (or, with `--dry-run`, would be) written or removed.
    pub(super) path: PathBuf,
    /// The task's table as it now is, or as it was before `mise tasks rm`. A file task has none.
    pub(super) table: Option<serde_json::Value>,
    /// Whether this command trusted the config root of `path`, which it did not need before.
    pub(super) newly_trusted: bool,
    /// The config root that was trusted, when `newly_trusted` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) trust_root: Option<PathBuf>,
    pub(super) dry_run: bool,
}

impl Report {
    pub(super) fn new(task: &str, action: Action, kind: Kind, path: &Path) -> Self {
        Self {
            task: task.to_string(),
            action,
            kind,
            path: path.to_path_buf(),
            table: None,
            newly_trusted: false,
            trust_root: None,
            dry_run: false,
        }
    }

    pub(super) fn with_table(mut self, table: Option<toml::Value>) -> Result<Self> {
        self.table = table.map(serde_json::to_value).transpose()?;
        Ok(self)
    }

    pub(super) fn trusted(mut self, newly_trusted: bool) -> Self {
        self.newly_trusted = newly_trusted;
        self.trust_root = newly_trusted.then(|| config_file::config_trust_root(&self.path));
        self
    }

    /// Prints what happened for a person, on stderr.
    pub(super) fn print(&self) {
        let path = display_path(&self.path);
        let task = &self.task;
        let would = if self.dry_run { "would be " } else { "" };
        match self.action {
            Action::Added => info!("task {task} {would}added to {path}"),
            Action::Replaced => info!("task {task} {would}replaced in {path}"),
            Action::Updated => info!("task {task} {would}updated in {path}"),
            Action::Unchanged => info!("task {task} in {path} is unchanged"),
            Action::Removed => info!("task {task} {would}removed from {path}"),
        }
        if let Some(root) = &self.trust_root {
            info!(
                "trusted {} so that mise loads {path}; `mise trust --untrust {}` undoes it",
                display_path(root),
                display_path(root)
            );
        }
    }
}
