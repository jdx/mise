//! Project daemons: custom pitchfork definitions and embedded database presets.
pub(crate) mod hook_env;
pub(crate) mod presets;
pub(crate) mod runtime;
pub(crate) mod tasks;

use crate::config::env_directive::EnvDirective;
use crate::config::{Config, ConfigMap, Settings};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Gate shared by `[daemons]` loading and by tasks that require daemons, so
/// both report the same requirement.
pub(crate) const EXPERIMENTAL: &str = "[daemons] requires experimental = true";

/// Set on a daemon that runs a task, so the `mise run` it starts does not start
/// daemons of its own and recurse.
pub(crate) const DAEMON_TASK_MARKER: &str = "MISE_DAEMON_TASK";

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Declaration {
    Preset(String),
    Definition(toml::Table),
}

/// `[daemon_groups]` entry: a bare list of members or a table with `daemons`.
/// The table form, matching `additionalProperties: false` in schema/mise.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GroupTable {
    daemons: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum GroupDeclaration {
    List(Vec<String>),
    Table(GroupTable),
}

impl GroupDeclaration {
    fn members(&self) -> &[String] {
        match self {
            GroupDeclaration::List(members) => members,
            GroupDeclaration::Table(table) => &table.daemons,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub name: String,
    pub source: PathBuf,
    pub root: PathBuf,
    /// Declared members, which may reference other groups in the same project.
    pub members: Vec<String>,
    /// Members expanded to daemon names declared in the same project root.
    pub daemons: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Daemon {
    pub name: String,
    pub source: PathBuf,
    pub root: PathBuf,
    pub table: toml::Table,
    pub preset: Option<String>,
    /// Task this daemon runs, when declared with `task = "..."`. Retained so
    /// the reference can be checked against the loaded task list, which is not
    /// available while configuration is still being parsed.
    pub task: Option<String>,
    pub tool: Option<(String, String)>,
    pub exports: IndexMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DaemonSet {
    pub daemons: IndexMap<String, Daemon>,
    /// Group names are project scoped, so nested projects may each declare one
    /// with the same name. They stay in a root-aware list until `for_root`.
    pub groups: Vec<Group>,
}

pub(crate) fn state_dir(root: &Path) -> PathBuf {
    crate::dirs::STATE
        .join("daemons")
        .join(crate::hash::hash_to_str(
            &root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        ))
}

pub(crate) fn load(files: &ConfigMap) -> Result<DaemonSet> {
    let mut declarations = IndexMap::new();
    let mut group_declarations = IndexMap::new();
    // A nearer config replaces a same-name daemon outright, so the winning
    // declaration's root is not the only project that declared that name.
    let mut declared_in: IndexMap<String, Vec<PathBuf>> = IndexMap::new();
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        let groups = cf.daemon_group_declarations();
        if entries.is_empty() && groups.is_empty() {
            continue;
        }
        if !Settings::get().experimental {
            warn_once!("{EXPERIMENTAL}; ignoring daemon declarations");
            continue;
        }
        if Settings::safe_mode() && !crate::config::is_global_config(cf.get_path()) {
            continue;
        }
        let source = cf.get_path().to_path_buf();
        let root = cf.project_root().unwrap_or_else(|| cf.config_root());
        for (name, declaration) in entries {
            let roots = declared_in.entry(name.clone()).or_default();
            if !roots.contains(&root) {
                roots.push(root.clone());
            }
            declarations.insert(name, (declaration, source.clone(), root.clone()));
        }
        for (name, declaration) in groups {
            // Keyed by root so a child project's group never displaces a parent's.
            group_declarations.insert(
                (root.clone(), name),
                (declaration, source.clone(), root.clone()),
            );
        }
    }
    let mut set = DaemonSet::default();
    for (name, (declaration, source, root)) in declarations {
        validate_name("daemon", &name)?;
        let (preset, version, mut table) = match declaration {
            Declaration::Preset(version) => (Some(name.clone()), Some(version), toml::Table::new()),
            Declaration::Definition(mut table) => {
                let preset = take_string(&mut table, "preset")?;
                let version = take_string(&mut table, "version")?;
                (preset, version, table)
            }
        };
        // `init`, `task` and `args` are mise concepts; pitchfork never sees them.
        let init = take_init(&mut table, &name)?;
        let task = take_string(&mut table, "task")?;
        let args = take_args(&mut table, &name)?;
        if preset.is_some() {
            // Name the key actually present; `args` without `task` is a
            // different mistake from `preset` with `task`.
            if task.is_some() {
                bail!("[daemons.{name}] cannot combine preset with task");
            }
            if args.is_some() {
                bail!("[daemons.{name}] args requires task, which a preset cannot use");
            }
        }
        let daemon = if let Some(preset) = preset {
            let version = version
                .ok_or_else(|| eyre::eyre!("[daemons.{name}] requires version with preset"))?;
            presets::expand(&name, &preset, &version, table, &init, &source, &root)?
        } else {
            if version.is_some() || table.contains_key("options") {
                bail!("[daemons.{name}] requires preset when specifying version or options");
            }
            if task.is_some() && table.contains_key("run") {
                bail!("[daemons.{name}] cannot set both run and task");
            }
            if args.is_some() && task.is_none() {
                bail!("[daemons.{name}] args requires task");
            }
            if let Some(task) = &task {
                if task.is_empty() {
                    bail!("[daemons.{name}] task must not be empty");
                }
                let mut run = format!(
                    "exec {} run {}",
                    presets::quote(crate::env::MISE_BIN.to_string_lossy()),
                    presets::quote(task)
                );
                let args = args.unwrap_or_default();
                if !args.is_empty() {
                    run.push_str(" --");
                    for arg in args {
                        run.push(' ');
                        run.push_str(&presets::quote(arg));
                    }
                }
                table.insert("run".into(), toml::Value::String(run));
                // The task runs through `mise run`, which would start this very
                // daemon again. This marker breaks that cycle in `tasks::start`
                // instead of `--skip-deps`, which would also have discarded the
                // task's own `depends`, leaving the daemon to supervise a stale
                // build.
                table
                    .entry("env".to_string())
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                    .as_table_mut()
                    .ok_or_else(|| eyre::eyre!("[daemons.{name}] env must be a table"))?
                    .insert(
                        DAEMON_TASK_MARKER.into(),
                        toml::Value::String("1".to_string()),
                    );
                // mise is already the entry point, so pitchfork does not need
                // to wrap a bare task daemon in `mise x`. With `init` it does:
                // the setup steps and the task then share one shell inside the
                // project's tool environment, so a step can export a variable
                // or change directory for the ones after it, exactly as it can
                // for a daemon declared with `run`.
                if init.is_empty() {
                    table
                        .entry("mise".to_string())
                        .or_insert(toml::Value::Boolean(false));
                }
            }
            if table.get("run").and_then(toml::Value::as_str).is_none() {
                bail!("[daemons.{name}] requires run, task, or preset");
            }
            if let Some(port) = table.get("port").and_then(toml::Value::as_integer) {
                if !(1..=65535).contains(&port) {
                    bail!("daemon port must be an integer from 1 to 65535");
                }
                table.insert(
                    "port".into(),
                    toml::Value::Table(toml::Table::from_iter([
                        (
                            "expect".into(),
                            toml::Value::Array(vec![toml::Value::Integer(port)]),
                        ),
                        ("bump".into(), toml::Value::Boolean(false)),
                    ])),
                );
            }
            table
                .entry("mise".to_string())
                .or_insert(toml::Value::Boolean(true));
            if !init.is_empty() {
                let run = table["run"].as_str().unwrap().to_string();
                table.insert(
                    "run".into(),
                    toml::Value::String(presets::with_init(&init, &run)),
                );
            }
            Daemon {
                name: name.clone(),
                source,
                root,
                table,
                preset: None,
                task,
                tool: None,
                exports: IndexMap::new(),
            }
        };
        set.daemons.insert(name, daemon);
    }
    load_groups(&mut set, group_declarations, &declared_in)?;
    Ok(set)
}

fn validate_name(kind: &str, name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name.contains("..")
        || name.contains("--")
        || name.starts_with('-')
        || name.ends_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        bail!("invalid {kind} name {name:?}; use letters, numbers, '.', '_' or '-'");
    }
    Ok(())
}

type GroupKey = (PathBuf, String);
type GroupDeclarations = IndexMap<GroupKey, (GroupDeclaration, PathBuf, PathBuf)>;

/// Groups are project scoped: every member resolves to a daemon declared under the
/// same project root, so a group can never select daemons outside the project.
fn load_groups(
    set: &mut DaemonSet,
    declarations: GroupDeclarations,
    declared_in: &IndexMap<String, Vec<PathBuf>>,
) -> Result<()> {
    let declares = |root: &Path, name: &str| {
        declared_in
            .get(name)
            .is_some_and(|roots| roots.iter().any(|r| r == root))
    };
    let mut groups: IndexMap<GroupKey, Group> = IndexMap::new();
    for (key, (declaration, source, root)) in declarations {
        let name = key.1.clone();
        validate_name("daemon group", &name)?;
        if declares(&root, &name) {
            bail!("[daemon_groups.{name}] conflicts with the daemon of the same name");
        }
        let members = declaration.members().to_vec();
        if members.is_empty() {
            bail!("[daemon_groups.{name}] requires at least one daemon or group");
        }
        groups.insert(
            key,
            Group {
                name,
                source,
                root,
                members,
                daemons: Vec::new(),
            },
        );
    }
    for key in groups.keys().cloned().collect::<Vec<_>>() {
        let daemons = expand_group(&groups, &key, &declares, &mut Vec::new())?;
        groups[&key].daemons = daemons;
    }
    set.groups = groups.into_values().collect();
    Ok(())
}

fn expand_group(
    groups: &IndexMap<GroupKey, Group>,
    key: &GroupKey,
    declares: &impl Fn(&Path, &str) -> bool,
    seen: &mut Vec<String>,
) -> Result<Vec<String>> {
    let name = &key.1;
    if seen.iter().any(|s| s == name) {
        seen.push(name.clone());
        bail!(
            "[daemon_groups.{name}] references itself: {}",
            seen.join(" -> ")
        );
    }
    seen.push(name.clone());
    let group = &groups[key];
    let mut expanded: Vec<String> = Vec::new();
    for member in &group.members {
        // Membership follows what this project declared, so a same-name daemon
        // redefined by a nearer config keeps the group valid. Selection is still
        // per root, so the group only ever reaches this project's own daemons.
        if declares(&group.root, member) {
            if !expanded.contains(member) {
                expanded.push(member.clone());
            }
            continue;
        }
        // Nested members resolve within the declaring project only.
        let nested = (group.root.clone(), member.clone());
        if groups.contains_key(&nested) {
            for daemon in expand_group(groups, &nested, declares, seen)? {
                if !expanded.contains(&daemon) {
                    expanded.push(daemon);
                }
            }
            continue;
        }
        bail!(
            "{}: [daemon_groups.{name}] member {member:?} is not a daemon or group declared for {}",
            group.source.display(),
            group.root.display()
        );
    }
    seen.pop();
    Ok(expanded)
}

/// Remove `init`, accepting one command or an ordered list of them.
fn take_init(table: &mut toml::Table, name: &str) -> Result<Vec<String>> {
    let Some(value) = table.remove("init") else {
        return Ok(vec![]);
    };
    let steps = match value {
        toml::Value::String(step) => vec![step],
        toml::Value::Array(steps) => steps
            .into_iter()
            .map(|step| match step {
                toml::Value::String(step) => Ok(step),
                _ => bail!("[daemons.{name}] init entries must be strings"),
            })
            .collect::<Result<Vec<_>>>()?,
        _ => bail!("[daemons.{name}] init must be a string or an array of strings"),
    };
    if steps.iter().any(|step| step.trim().is_empty()) {
        bail!("[daemons.{name}] init entries must not be empty");
    }
    Ok(steps)
}

fn take_args(table: &mut toml::Table, name: &str) -> Result<Option<Vec<String>>> {
    table
        .remove("args")
        .map(|value| match value {
            toml::Value::Array(args) => args
                .into_iter()
                .map(|arg| match arg {
                    toml::Value::String(arg) => Ok(arg),
                    _ => bail!("[daemons.{name}] args entries must be strings"),
                })
                .collect::<Result<Vec<_>>>(),
            _ => bail!("[daemons.{name}] args must be an array of strings"),
        })
        .transpose()
}

fn take_string(table: &mut toml::Table, key: &str) -> Result<Option<String>> {
    table
        .remove(key)
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| eyre::eyre!("daemon {key} must be a string"))
        })
        .transpose()
}

impl DaemonSet {
    pub(crate) fn add_tool_requests(&self, trs: &mut ToolRequestSet) -> Result<()> {
        for daemon in self.daemons.values() {
            let Some((tool, version)) = &daemon.tool else {
                continue;
            };
            let ba = crate::cli::args::BackendArg::from(tool.as_str());
            if let Some(existing) = trs.tools.get(&ba) {
                // Explicit declarations are validated against backend resolution before starting.
                if existing.iter().all(|tr| tr.source().is_mise_toml_daemon())
                    && existing.iter().any(|tr| tr.version() != *version)
                {
                    bail!(
                        "daemon {} requests {tool}@{version}, conflicting with another daemon; use one version per tool",
                        daemon.name
                    );
                }
                continue;
            }
            let source = ToolSource::MiseTomlDaemon(daemon.source.clone());
            let request = ToolRequest::new(ba.into(), version, source.clone())?;
            trs.add_version(request, &source);
        }
        Ok(())
    }

    pub(crate) fn env_entries(&self) -> Vec<(EnvDirective, PathBuf)> {
        self.daemons
            .values()
            .flat_map(|daemon| {
                daemon.exports.iter().map(|(k, v)| {
                    (
                        EnvDirective::Val(k.clone(), v.clone(), Default::default()),
                        daemon.source.clone(),
                    )
                })
            })
            .collect()
    }

    pub(crate) fn roots(&self) -> Vec<PathBuf> {
        self.daemons
            .values()
            .map(|d| d.root.clone())
            .collect::<indexmap::IndexSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn for_root(&self, root: &Path) -> Self {
        Self {
            daemons: self
                .daemons
                .iter()
                .filter(|(_, d)| d.root == root)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            groups: self
                .groups
                .iter()
                .filter(|g| g.root == root)
                .cloned()
                .collect(),
        }
    }

    /// The group named `name`. Call on a root-scoped set, where names are unique.
    pub(crate) fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// Daemon names a declared group expands to, or None when `name` is not a group.
    pub(crate) fn expand(&self, name: &str) -> Option<&[String]> {
        self.group(name).map(|g| g.daemons.as_slice())
    }

    /// Check `task = "..."` references against the loaded task list. Task
    /// loading is asynchronous and reads the filesystem, so this cannot run
    /// while `[daemons]` is parsed. The three paths that register a generated
    /// pitchfork configuration call it first: `mise daemons start`, a task that
    /// requires daemons, and the shell auto-lifecycle hook.
    pub(crate) async fn validate_tasks(&self, config: &Arc<Config>) -> Result<()> {
        if self.daemons.values().all(|d| d.task.is_none()) {
            return Ok(());
        }
        let tasks = config.tasks_with_aliases().await?;
        for daemon in self.daemons.values() {
            let Some(task) = &daemon.task else {
                continue;
            };
            if !tasks.contains_key(task) {
                bail!("daemon {} runs unknown task {task:?}", daemon.name);
            }
        }
        Ok(())
    }

    pub(crate) fn auto(&self) -> bool {
        self.daemons.values().any(|d| {
            d.table
                .get("auto")
                .and_then(toml::Value::as_array)
                .is_some_and(|a| !a.is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_file::mise_toml::MiseToml;
    use std::sync::Arc;

    fn files(entries: &[(&str, &str)]) -> ConfigMap {
        entries
            .iter()
            .map(|(path, body)| {
                let path = PathBuf::from(path);
                let cf: Arc<dyn crate::config::config_file::ConfigFile> =
                    Arc::new(MiseToml::from_str(body, &path).unwrap());
                (path, cf)
            })
            .collect()
    }

    #[tokio::test]
    async fn daemons_merge_before_expanding_tools_and_exports() {
        crate::toolset::install_state::init().await.unwrap();
        let config = files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.postgres]\nrun = 'echo custom'\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons]\npostgres = '18'\nredis = '8'\n",
            ),
        ]);
        let set = load(&config).unwrap();
        assert!(set.daemons["postgres"].tool.is_none());
        assert!(set.daemons["postgres"].exports.is_empty());
        assert_eq!(
            set.daemons["postgres"].table["run"].as_str(),
            Some("echo custom")
        );
        let mut requests = ToolRequestSet::default();
        set.add_tool_requests(&mut requests).unwrap();
        assert_eq!(requests.tools.len(), 1);
        assert!(
            requests
                .sources
                .values()
                .next()
                .unwrap()
                .is_mise_toml_daemon()
        );
    }

    #[tokio::test]
    async fn preset_names_and_version_conflicts_are_reported() {
        crate::toolset::install_state::init().await.unwrap();
        let config = files(&[("/project/mise.toml", "[daemons]\nunknown = '1'\n")]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("available presets")
        );
        let config = files(&[(
            "/project/mise.toml",
            "[daemons]\npostgres = '18'\n[daemons.analytics]\npreset = 'postgres'\nversion = '17'\n",
        )]);
        let set = load(&config).unwrap();
        assert!(
            set.add_tool_requests(&mut ToolRequestSet::default())
                .is_err()
        );
    }

    #[test]
    fn preset_run_override_keeps_initialization() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nrun = 'echo starting && exec postgres -D /data'\n",
        )]);
        let set = load(&config).unwrap();
        let run = set.daemons["db"].table["run"].as_str().unwrap();
        assert!(run.contains("daemons __init"));
        assert!(run.ends_with("&& echo starting && exec postgres -D /data"));
        let invalid = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nrun = 42\n",
        )]);
        assert!(load(&invalid).is_err());
    }

    #[test]
    fn preset_tool_override_is_used_and_not_forwarded() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.cache]\npreset = 'redis'\nversion = '8'\ntool = 'github:example/redis'\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["cache"].tool.as_ref().unwrap().0,
            "github:example/redis"
        );
        assert!(!set.daemons["cache"].table.contains_key("tool"));
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.cache]\npreset = 'redis'\nversion = '8'\ntool = 42\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("tool must be a string")
        );
    }

    #[test]
    fn task_daemons_run_mise_without_a_mise_wrapper() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev:core'\nargs = ['--port', \"it's 3000\"]\nready_port = 3000\n",
        )]);
        let set = load(&config).unwrap();
        let daemon = &set.daemons["core"];
        let mise = presets::quote(crate::env::MISE_BIN.to_string_lossy());
        assert_eq!(
            daemon.table["run"].as_str(),
            Some(format!("exec {mise} run 'dev:core' -- '--port' 'it'\\''s 3000'").as_str())
        );
        // The task keeps its own `depends`; a marker in the daemon environment
        // is what stops the nested run from starting this daemon again.
        assert_eq!(daemon.table["env"][DAEMON_TASK_MARKER].as_str(), Some("1"));
        // mise is the entry point already, so pitchfork must not re-enter it.
        assert_eq!(daemon.table["mise"].as_bool(), Some(false));
        assert_eq!(daemon.task.as_deref(), Some("dev:core"));
        // Task plumbing stays in mise; pitchfork only sees its own keys.
        assert!(!daemon.table.contains_key("task"));
        assert!(!daemon.table.contains_key("args"));
        assert_eq!(daemon.table["ready_port"].as_integer(), Some(3000));
        // A task daemon with no args does not emit a dangling separator.
        let config = files(&[("/project/mise.toml", "[daemons.core]\ntask = 'dev'\n")]);
        assert_eq!(
            load(&config).unwrap().daemons["core"].table["run"].as_str(),
            Some(format!("exec {mise} run 'dev'").as_str())
        );
    }

    #[test]
    fn conflicting_daemon_process_declarations_are_rejected() {
        for body in [
            "[daemons.core]\ntask = 'dev'\nrun = 'server'\n",
            "[daemons.core]\npreset = 'postgres'\nversion = '18'\ntask = 'dev'\n",
            "[daemons.core]\nargs = ['--port']\nrun = 'server'\n",
            "[daemons.core]\ntask = ''\n",
            "[daemons.core]\nready_port = 3000\n",
        ] {
            assert!(
                load(&files(&[("/project/mise.toml", body)])).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn conflict_errors_name_the_key_that_is_set() {
        // `args` without `task` is a different mistake from `preset` with
        // `task`, so the message must not blame a key the user never wrote.
        let err = load(&files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nargs = ['--flag']\n",
        )]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("args requires task"), "{err}");
        let err = load(&files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\ntask = 'dev'\n",
        )]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot combine preset with task"), "{err}");
    }

    #[test]
    fn init_steps_run_before_the_long_running_process() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\ninit = ['npm ci', 'npm run migrate']\nrun = 'exec npm start'\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["api"].table["run"].as_str(),
            Some("npm ci && npm run migrate && exec npm start")
        );
        // A single string is the same as a one-entry list.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\ninit = 'npm ci'\nrun = 'exec npm start'\n",
        )]);
        assert_eq!(
            load(&config).unwrap().daemons["api"].table["run"].as_str(),
            Some("npm ci && exec npm start")
        );
        // Preset initialization still comes first, before user setup.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\ninit = 'echo ready'\n",
        )]);
        let run = load(&config).unwrap().daemons["db"].table["run"]
            .as_str()
            .unwrap()
            .to_string();
        let init = run.find(" daemons __init ").unwrap();
        assert!(init < run.find("echo ready").unwrap());
        assert!(run.contains("&& echo ready && exec "));
        // A task daemon with init keeps pitchfork's `mise x` wrapper, so the
        // steps and the task share one shell that has the project's tools.
        let wrapped = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = ['npm ci', 'npm run migrate']\n",
        )]);
        let daemon = &load(&wrapped).unwrap().daemons["core"];
        let run = daemon.table["run"].as_str().unwrap().to_string();
        assert!(
            run.starts_with("npm ci && npm run migrate && exec "),
            "{run}"
        );
        assert!(run.ends_with("run 'dev'"), "{run}");
        assert_eq!(daemon.table["mise"].as_bool(), Some(true));
        // An explicit `mise` value stays the user's call.
        let explicit = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = 'npm ci'\nmise = false\n",
        )]);
        assert_eq!(
            load(&explicit).unwrap().daemons["core"].table["mise"].as_bool(),
            Some(false)
        );
        // `init` is consumed by mise and never reaches pitchfork.
        assert!(
            !load(&config).unwrap().daemons["db"]
                .table
                .contains_key("init")
        );
        for body in [
            "[daemons.api]\ninit = 1\nrun = 'server'\n",
            "[daemons.api]\ninit = [1]\nrun = 'server'\n",
            "[daemons.api]\ninit = ' '\nrun = 'server'\n",
        ] {
            assert!(
                load(&files(&[("/project/mise.toml", body)])).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn custom_daemons_accept_integer_ports() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\nport = 3000\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["api"].table["port"]["expect"][0].as_integer(),
            Some(3000)
        );
    }

    #[test]
    fn groups_expand_members_and_nested_groups() {
        let set = load(&files(&[(
            "/project/mise.toml",
            r#"
[daemons.postgres]
run = 'postgres'
[daemons.nats]
run = 'nats'
[daemons.core]
run = 'core'
[daemons.core2]
run = 'core2'
[daemons.node0]
run = 'node0'
[daemon_groups]
default = ["postgres", "nats", "core", "node0"]
two-cluster = ["default", "core2"]
[daemon_groups.explicit]
daemons = ["core", "core2"]
"#,
        )]))
        .unwrap();
        assert_eq!(
            set.group("default").unwrap().daemons,
            ["postgres", "nats", "core", "node0"]
        );
        // A nested group expands in place and members are deduplicated.
        assert_eq!(
            set.group("two-cluster").unwrap().daemons,
            ["postgres", "nats", "core", "node0", "core2"]
        );
        assert_eq!(set.group("explicit").unwrap().daemons, ["core", "core2"]);
        assert_eq!(set.expand("default").unwrap().len(), 4);
        assert!(set.expand("postgres").is_none());
        // Roots are absolutized, so derive them instead of hardcoding a path.
        let root = set.daemons["postgres"].root.clone();
        assert_eq!(set.for_root(&root).groups.len(), 3);
        assert!(set.for_root(root.parent().unwrap()).groups.is_empty());
    }

    #[test]
    fn groups_nest_more_than_one_level() {
        let set = load(&files(&[(
            "/project/mise.toml",
            r#"
[daemons.a]
run = 'a'
[daemons.b]
run = 'b'
[daemons.c]
run = 'c'
[daemon_groups]
one = ["a"]
two = ["one", "b"]
three = ["two", "c"]
"#,
        )]))
        .unwrap();
        assert_eq!(set.group("three").unwrap().daemons, ["a", "b", "c"]);
        // A cycle is still caught through three levels.
        let err = load(&files(&[(
            "/project/mise.toml",
            "[daemons.a]\nrun = 'a'\n[daemon_groups]\none = ['two']\ntwo = ['three']\nthree = ['one']\n",
        )]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("references itself"), "{err}");
    }

    #[test]
    fn the_table_form_rejects_unknown_keys() {
        // Matches `additionalProperties: false` in the schema, and catches a typo
        // that would otherwise leave the group silently empty. Rejection happens
        // when the configuration is parsed, before daemons are loaded.
        let parse = |body: &str| MiseToml::from_str(body, &PathBuf::from("/project/mise.toml"));
        assert!(parse("[daemon_groups.web]\ndaemons = ['api']\n").is_ok());
        let err = parse("[daemon_groups.web]\ndaemons = ['api']\ndeamons = ['api']\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("TOML"), "{err}");
        // The list form is unaffected.
        assert!(parse("[daemon_groups]\nweb = ['api']\n").is_ok());
    }

    #[test]
    fn same_group_name_in_two_projects_is_kept() {
        let set = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.web]\nrun = 'web'\n[daemon_groups]\ndefault = ['web']\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\ndefault = ['api']\n",
            ),
        ]))
        .unwrap();
        assert_eq!(set.groups.len(), 2);
        let child = set.daemons["web"].root.clone();
        let parent = set.daemons["api"].root.clone();
        assert_ne!(child, parent);
        assert_eq!(
            set.for_root(&child).group("default").unwrap().daemons,
            ["web"]
        );
        assert_eq!(
            set.for_root(&parent).group("default").unwrap().daemons,
            ["api"]
        );
    }

    #[test]
    fn a_child_overriding_a_daemon_keeps_the_parent_group_loadable() {
        // The child replaces the parent's postgres, which is documented behavior.
        // The parent's group still names a daemon the parent declared, so the
        // configuration has to keep loading.
        let set = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.postgres]\nrun = 'child postgres'\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons.postgres]\nrun = 'parent postgres'\n[daemons.api]\nrun = 'api'\n[daemon_groups]\ndefault = ['postgres', 'api']\n",
            ),
        ]))
        .unwrap();
        let parent = set.daemons["api"].root.clone();
        assert_eq!(
            set.for_root(&parent).group("default").unwrap().daemons,
            ["postgres", "api"]
        );
        // The overriding definition wins, and selection stays per root, so the
        // parent's group reaches only the daemons that project still owns.
        assert_eq!(
            set.daemons["postgres"].table["run"].as_str(),
            Some("child postgres")
        );
        assert!(!set.for_root(&parent).daemons.contains_key("postgres"));
    }

    #[test]
    fn nested_group_references_stay_inside_one_project() {
        // `shared` exists only in the parent, so the child cannot reference it.
        let err = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.web]\nrun = 'web'\n[daemon_groups]\nall = ['web', 'shared']\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nshared = ['api']\n",
            ),
        ]))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("is not a daemon or group declared for"),
            "{err}"
        );
    }

    #[test]
    fn groups_are_scoped_to_one_project_root() {
        // A daemon declared by an outer project is not a valid member of an inner group.
        let err = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nall = ['api', 'outer']\n",
            ),
            ("/parent/mise.toml", "[daemons.outer]\nrun = 'outer'\n"),
        ]))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("is not a daemon or group declared for"),
            "{err}"
        );
    }

    #[test]
    fn invalid_groups_are_rejected() {
        let cases = [
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['missing']\n",
                "is not a daemon or group",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = []\n",
                "requires at least one daemon or group",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\napi = ['api']\n",
                "conflicts with the daemon of the same name",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['other']\nother = ['web']\n",
                "references itself",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['web']\n",
                "references itself",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\n'bad name' = ['api']\n",
                "invalid daemon group name",
            ),
        ];
        for (body, expected) in cases {
            let err = load(&files(&[("/project/mise.toml", body)]))
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{body:?} produced {err}");
        }
    }

    #[test]
    fn daemon_names_cannot_escape_persistent_data_directory() {
        for name in [".", "..", "../outside", "a/b", "a\\b"] {
            let config = files(&[(
                "/project/mise.toml",
                &format!(
                    "[daemons.{}]\npreset = 'postgres'\nversion = '18'",
                    toml::Value::String(name.into())
                ),
            )]);
            assert!(load(&config).is_err());
        }
    }
}
