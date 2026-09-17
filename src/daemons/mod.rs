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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Declaration {
    Preset(String),
    Definition(toml::Table),
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
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        if entries.is_empty() {
            continue;
        }
        if !Settings::get().experimental {
            warn_once!("{EXPERIMENTAL}; ignoring daemon declarations");
            continue;
        }
        if Settings::safe_mode() && !crate::config::is_global_config(cf.get_path()) {
            continue;
        }
        for (name, declaration) in entries {
            declarations.insert(
                name,
                (
                    declaration,
                    cf.get_path().to_path_buf(),
                    cf.project_root().unwrap_or_else(|| cf.config_root()),
                ),
            );
        }
    }
    let mut set = DaemonSet::default();
    for (name, (declaration, source, root)) in declarations {
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
            bail!("invalid daemon name {name:?}; use letters, numbers, '.', '_' or '-'");
        }
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
        if preset.is_some() && (task.is_some() || args.is_some()) {
            bail!("[daemons.{name}] cannot combine preset with task");
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
            // An explicit `mise` value is the user's call; otherwise a task
            // daemon opts out of pitchfork's `mise x` wrapper below, and its
            // setup steps have to reach the tool environment some other way.
            let wrap_init = task.is_some() && !table.contains_key("mise");
            if let Some(task) = &task {
                if task.is_empty() {
                    bail!("[daemons.{name}] task must not be empty");
                }
                // `--skip-deps` keeps a task that requires daemons from
                // recursively starting this one. mise is already the entry
                // point, so pitchfork must not wrap it in `mise x --`.
                let mut run = format!(
                    "exec {} run --skip-deps {}",
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
                table
                    .entry("mise".to_string())
                    .or_insert(toml::Value::Boolean(false));
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
                let steps: Vec<String> = if wrap_init {
                    init.iter().map(|s| presets::in_tool_env(s)).collect()
                } else {
                    init.clone()
                };
                let run = table["run"].as_str().unwrap().to_string();
                table.insert(
                    "run".into(),
                    toml::Value::String(presets::with_init(&steps, &run)),
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
    Ok(set)
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
        }
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
            Some(
                format!("exec {mise} run --skip-deps 'dev:core' -- '--port' 'it'\\''s 3000'")
                    .as_str()
            )
        );
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
            Some(format!("exec {mise} run --skip-deps 'dev'").as_str())
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
        // A task daemon opts out of pitchfork's `mise x` wrapper, so its setup
        // steps have to enter the tool environment themselves.
        let wrapped = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = 'npm ci'\n",
        )]);
        let run = load(&wrapped).unwrap().daemons["core"].table["run"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(run.starts_with(&presets::in_tool_env("npm ci")), "{run}");
        assert!(run.ends_with("run --skip-deps 'dev'"), "{run}");
        // An explicit `mise` value is the user's call, so the step is left alone.
        let explicit = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = 'npm ci'\nmise = true\n",
        )]);
        assert!(
            load(&explicit).unwrap().daemons["core"].table["run"]
                .as_str()
                .unwrap()
                .starts_with("npm ci && ")
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
