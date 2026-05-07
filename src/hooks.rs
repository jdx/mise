use crate::cmd::cmd;
use crate::config::config_file::mise_toml::EnvList;
use crate::config::config_file::toml::deserialize_arr;
use crate::config::env_directive::EnvDirective;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_executor::{TaskExecutor, TaskExecutorConfig};
use crate::task::task_output::TaskOutput;
use crate::task::task_output_handler::{OutputHandler, OutputHandlerConfig};
use crate::task::{RunEntry, Task};
use crate::tera::get_tera;
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, hook_env};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;
use tokio::sync::OnceCell;

/// Represents installed tool info for hooks
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledToolInfo {
    pub name: String,
    pub version: String,
}

impl From<&ToolVersion> for InstalledToolInfo {
    fn from(tv: &ToolVersion) -> Self {
        Self {
            name: tv.ba().short.clone(),
            version: tv.version.clone(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Hooks {
    Enter,
    Leave,
    Cd,
    Preinstall,
    Postinstall,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookDef {
    Entry(HookEntryDef),
    Array(Vec<HookEntryDef>),
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookEntryDef {
    Script(String),
    Run(HookRunDef),
    ScriptTable(HookScriptDef),
    TaskRef(HookTaskRefDef),
}

#[derive(Debug, Clone)]
pub struct HookRunDef {
    run: Vec<RunEntry>,
    run_windows: Vec<RunEntry>,
    shell: Option<String>,
}

impl<'de> serde::Deserialize<'de> for HookRunDef {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawHookRunDef {
            #[serde(default, deserialize_with = "deserialize_arr")]
            run: Vec<RunEntry>,
            #[serde(default, deserialize_with = "deserialize_arr")]
            run_windows: Vec<RunEntry>,
            shell: Option<String>,
        }

        let raw = RawHookRunDef::deserialize(deserializer)?;
        if raw.run.is_empty() && raw.run_windows.is_empty() {
            return Err(serde::de::Error::custom("expected `run` or `run_windows`"));
        }

        Ok(Self {
            run: raw.run,
            run_windows: raw.run_windows,
            shell: raw.shell,
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookScriptDef {
    #[serde(deserialize_with = "deserialize_arr")]
    script: Vec<String>,
    shell: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookTaskRefDef {
    task: String,
}

impl HookDef {
    pub fn into_hooks(self, hook_type: Hooks) -> Vec<Hook> {
        match self {
            HookDef::Entry(entry) => vec![entry.into_hook(hook_type)],
            HookDef::Array(arr) => arr
                .into_iter()
                .map(|entry| entry.into_hook(hook_type))
                .collect(),
        }
    }
}

impl HookEntryDef {
    fn into_hook(self, hook: Hooks) -> Hook {
        let action = match self {
            HookEntryDef::Script(script) => HookAction::Run {
                run: vec![RunEntry::Script(script)],
                run_windows: vec![],
                shell: None,
                legacy_script: false,
                ignored_shell: None,
            },
            HookEntryDef::Run(def) => HookAction::Run {
                run: def.run,
                run_windows: def.run_windows,
                shell: def.shell,
                legacy_script: false,
                ignored_shell: None,
            },
            HookEntryDef::ScriptTable(def) => match (hook, def.shell) {
                (Hooks::Enter | Hooks::Leave | Hooks::Cd, Some(shell)) => {
                    HookAction::CurrentShell {
                        script: def.script,
                        shell,
                    }
                }
                (_, shell) => HookAction::Run {
                    run: def.script.into_iter().map(RunEntry::Script).collect(),
                    run_windows: vec![],
                    shell: None,
                    legacy_script: true,
                    ignored_shell: shell,
                },
            },
            HookEntryDef::TaskRef(def) => HookAction::Run {
                run: vec![RunEntry::SingleTask {
                    task: def.task,
                    args: vec![],
                    env: Default::default(),
                }],
                run_windows: vec![],
                shell: None,
                legacy_script: false,
                ignored_shell: None,
            },
        };
        Hook {
            hook,
            action,
            global: false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Hook {
    pub hook: Hooks,
    pub action: HookAction,
    /// Whether this hook comes from a global config (skip directory matching)
    pub global: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum HookAction {
    Run {
        run: Vec<RunEntry>,
        run_windows: Vec<RunEntry>,
        shell: Option<String>,
        legacy_script: bool,
        ignored_shell: Option<String>,
    },
    CurrentShell {
        script: Vec<String>,
        shell: String,
    },
}

impl Hook {
    pub fn render_templates<F>(&mut self, mut render: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<String>,
    {
        match &mut self.action {
            HookAction::Run {
                run,
                run_windows,
                shell,
                ignored_shell,
                ..
            } => {
                render_run_entries(run, &mut render)?;
                render_run_entries(run_windows, &mut render)?;
                if let Some(s) = shell {
                    *s = render(s)?;
                }
                if let Some(s) = ignored_shell {
                    *s = render(s)?;
                }
            }
            HookAction::CurrentShell { script, shell } => {
                for s in script {
                    *s = render(s)?;
                }
                *shell = render(shell)?;
            }
        }
        Ok(())
    }
}

fn render_run_entries<F>(entries: &mut [RunEntry], render: &mut F) -> Result<()>
where
    F: FnMut(&str) -> Result<String>,
{
    for entry in entries {
        match entry {
            RunEntry::Script(s) => *s = render(s)?,
            RunEntry::SingleTask { task, args, env } => {
                *task = render(task)?;
                for arg in args {
                    *arg = render(arg)?;
                }
                for value in env.values_mut() {
                    *value = render(value)?;
                }
            }
            RunEntry::TaskGroup { tasks } => {
                for task in tasks {
                    *task = render(task)?;
                }
            }
        }
    }
    Ok(())
}

pub static SCHEDULED_HOOKS: Lazy<Mutex<IndexSet<Hooks>>> = Lazy::new(Default::default);

pub fn schedule_hook(hook: Hooks) {
    let mut mu = SCHEDULED_HOOKS.lock().unwrap();
    mu.insert(hook);
}

pub async fn run_all_hooks(config: &Arc<Config>, ts: &Toolset, shell: &dyn Shell) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    let hooks = {
        let mut mu = SCHEDULED_HOOKS.lock().unwrap();
        mu.drain(..).collect::<Vec<_>>()
    };
    for hook in hooks {
        run_one_hook(config, ts, hook, Some(shell)).await;
    }
}

async fn all_hooks(config: &Arc<Config>) -> &'static Vec<(PathBuf, Hook)> {
    static ALL_HOOKS: OnceCell<Vec<(PathBuf, Hook)>> = OnceCell::const_new();
    ALL_HOOKS
        .get_or_init(async || {
            let mut hooks = config.hooks().await.cloned().unwrap_or_default();
            let cur_configs = config.config_files.keys().cloned().collect::<IndexSet<_>>();
            let prev_configs = &hook_env::PREV_SESSION.loaded_configs;
            let old_configs = prev_configs.difference(&cur_configs);
            for p in old_configs {
                if let Ok(cf) = config_file::parse(p).await
                    && let Ok(mut h) = cf.hooks()
                {
                    let is_global = cf.project_root().is_none();
                    if is_global {
                        for hook in &mut h {
                            hook.global = true;
                        }
                    }
                    let root = cf.project_root().unwrap_or_else(|| cf.config_root());
                    hooks.extend(h.into_iter().map(|h| (root.clone(), h)));
                }
            }
            hooks
        })
        .await
}

#[async_backtrace::framed]
pub async fn run_one_hook(
    config: &Arc<Config>,
    ts: &Toolset,
    hook: Hooks,
    shell: Option<&dyn Shell>,
) {
    run_one_hook_with_context(config, ts, hook, shell, None).await
}

/// Run a hook with optional installed tools context (for postinstall hooks)
#[async_backtrace::framed]
pub async fn run_one_hook_with_context(
    config: &Arc<Config>,
    ts: &Toolset,
    hook: Hooks,
    shell: Option<&dyn Shell>,
    installed_tools: Option<&[InstalledToolInfo]>,
) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    for (root, h) in all_hooks(config).await {
        if hook != h.hook {
            continue;
        }
        if let HookAction::CurrentShell {
            shell: hook_shell, ..
        } = &h.action
            && hook_shell != &shell.map(|s| s.to_string()).unwrap_or_default()
        {
            continue;
        }
        trace!("running hook {hook} in {root:?}");
        // Global hooks skip directory matching — they fire for all projects
        if !h.global {
            match (hook, hook_env::dir_change()) {
                (Hooks::Enter, Some((old, new))) => {
                    if !new.starts_with(root) {
                        continue;
                    }
                    if old.as_ref().is_some_and(|old| old.starts_with(root)) {
                        continue;
                    }
                }
                (Hooks::Leave, Some((old, new))) => {
                    if new.starts_with(root) {
                        continue;
                    }
                    if old.as_ref().is_some_and(|old| !old.starts_with(root)) {
                        continue;
                    }
                }
                (Hooks::Cd, Some((_old, new))) if !new.starts_with(root) => {
                    continue;
                }
                // Pre/postinstall hooks only run if CWD is under the config root
                (Hooks::Preinstall | Hooks::Postinstall, _) => {
                    if let Some(cwd) = dirs::CWD.as_ref()
                        && !cwd.starts_with(root)
                    {
                        continue;
                    }
                }
                _ => {}
            }
        }
        match &h.action {
            HookAction::CurrentShell { script, .. } => {
                if let Some(shell) = shell {
                    // Set hook environment variables so shell hooks can access them
                    println!(
                        "{}",
                        shell.set_env("MISE_PROJECT_ROOT", &root.to_string_lossy())
                    );
                    println!(
                        "{}",
                        shell.set_env("MISE_CONFIG_ROOT", &root.to_string_lossy())
                    );
                    if let Some(cwd) = dirs::CWD.as_ref() {
                        println!(
                            "{}",
                            shell.set_env("MISE_ORIGINAL_CWD", &cwd.to_string_lossy())
                        );
                    }
                    if let Some((Some(old), _new)) = hook_env::dir_change() {
                        println!(
                            "{}",
                            shell.set_env("MISE_PREVIOUS_DIR", &old.to_string_lossy())
                        );
                    }
                    if let Some(tools) = installed_tools
                        && let Ok(json) = serde_json::to_string(tools)
                    {
                        println!("{}", shell.set_env("MISE_INSTALLED_TOOLS", &json));
                    }
                }
                for line in script {
                    println!("{line}");
                }
            }
            HookAction::Run { .. } => {
                if let Err(e) = execute(config, ts, root, h, installed_tools).await {
                    // Warn but continue running remaining hooks of this type
                    warn!("{hook} hook in {} failed: {e}", root.display());
                }
            }
        }
    }
}

async fn execute(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    installed_tools: Option<&[InstalledToolInfo]>,
) -> Result<()> {
    Settings::get().ensure_experimental("hooks")?;
    let HookAction::Run {
        run,
        run_windows,
        shell,
        legacy_script,
        ignored_shell,
    } = &hook.action
    else {
        return Ok(());
    };
    if *legacy_script {
        deprecated_at!(
            "2026.9.0",
            "2027.3.0",
            "hook_script_table_spawned_run",
            "hook tables using `script` for spawned commands are deprecated. Use `run` instead."
        );
    }
    if ignored_shell.is_some() && matches!(hook.hook, Hooks::Preinstall | Hooks::Postinstall) {
        let hook_name = hook.hook.to_string().to_lowercase();
        warn!(
            "`shell` is ignored for {} hooks that use `script`; use `run = ...` with `shell = \"bash -c\"` to choose a spawned shell.",
            hook_name
        );
    }

    // Preinstall hooks skip `tools=true` env directives since the tools
    // providing those env vars aren't installed yet (fixes #6162)
    let mut env = if hook.hook == Hooks::Preinstall {
        ts.full_env_without_tools(config).await?
    } else {
        ts.full_env(config).await?
    };

    let mut hook_vars = BTreeMap::new();
    if let Some(cwd) = dirs::CWD.as_ref() {
        hook_vars.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    hook_vars.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    if let Some((Some(old), _new)) = hook_env::dir_change() {
        hook_vars.insert(
            "MISE_PREVIOUS_DIR".to_string(),
            old.to_string_lossy().to_string(),
        );
    }
    // Add installed tools info for postinstall hooks
    if let Some(tools) = installed_tools
        && let Ok(json) = serde_json::to_string(tools)
    {
        hook_vars.insert("MISE_INSTALLED_TOOLS".to_string(), json);
    }
    hook_vars.insert(
        "MISE_CONFIG_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    // Prevent recursive hook execution (e.g. hook runs `mise run` which spawns
    // a shell that activates mise and re-triggers hooks)
    hook_vars.insert("MISE_NO_HOOKS".to_string(), "1".to_string());
    env.extend(hook_vars.clone());

    let hook_env_directives = hook_vars
        .iter()
        .map(|(k, v)| EnvDirective::Val(k.clone(), v.clone(), Default::default()))
        .collect_vec();
    let task_env = hook_env_directives
        .iter()
        .filter_map(|directive| match directive {
            EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .collect_vec();
    let mut task = Task {
        name: format!("hook:{}", hook.hook),
        display_name: format!("hook:{}", hook.hook),
        config_source: root.join("mise.toml"),
        config_root: Some(root.to_path_buf()),
        run: run.clone(),
        run_windows: run_windows.clone(),
        shell: shell.clone(),
        quiet: true,
        env: EnvList(hook_env_directives),
        ..Default::default()
    };
    if cfg!(windows) && !task.run_windows.is_empty() {
        render_run_entries_for_hook(config, root, &env, &mut task.run_windows).await?;
    } else {
        render_run_entries_for_hook(config, root, &env, &mut task.run).await?;
    }
    let output_handler = OutputHandler::new(OutputHandlerConfig {
        output: Some(TaskOutput::Quiet),
        silent: false,
        quiet: true,
        raw: false,
        is_linear: true,
        jobs: Some(1),
    });
    let executor = TaskExecutor::new(
        TaskContextBuilder::default(),
        output_handler,
        TaskExecutorConfig {
            force: true,
            cd: None,
            shell: None,
            tool: vec![],
            timings: false,
            continue_on_error: false,
            dry_run: false,
            skip_deps: false,
            stdout_to_stderr: true,
            sandbox: Default::default(),
        },
    );
    if should_run_task_entries_with_mise(config, root, task.run()) {
        execute_run_entries_with_mise(&executor, config, root, &task, &env, &task_env).await?;
    } else {
        executor
            .run_task_run_entries(config, &task, &env, &task_env)
            .await?;
    }
    Ok(())
}

fn should_run_task_entries_with_mise(
    config: &Arc<Config>,
    root: &Path,
    entries: &[RunEntry],
) -> bool {
    entries
        .iter()
        .any(|entry| !matches!(entry, RunEntry::Script(_)))
        && config.project_root.as_deref() != Some(root)
}

async fn execute_run_entries_with_mise(
    executor: &TaskExecutor,
    config: &Arc<Config>,
    root: &Path,
    task: &Task,
    env: &BTreeMap<String, String>,
    task_env: &[(String, String)],
) -> Result<()> {
    for entry in task.run() {
        match entry {
            RunEntry::Script(_) => {
                let script_task = Task {
                    run: vec![entry.clone()],
                    run_windows: vec![],
                    ..task.clone()
                };
                executor
                    .run_task_run_entries(config, &script_task, env, task_env)
                    .await?;
            }
            RunEntry::SingleTask {
                task: spec,
                args,
                env: entry_env,
            } => {
                let resolved = crate::task::resolve_task_pattern(spec, Some(task));
                let (name, spec_args) = crate::task::task_list::split_task_spec(&resolved);
                let task_args = if args.is_empty() {
                    spec_args
                } else {
                    args.clone()
                };
                let mut cmd_env = env.clone();
                cmd_env.extend(entry_env.iter().map(|(k, v)| (k.clone(), v.clone())));
                run_mise_task(
                    root,
                    vec![name.to_string()].into_iter().chain(task_args),
                    &cmd_env,
                )?;
            }
            RunEntry::TaskGroup { tasks } => {
                let mut args = vec![];
                for (i, spec) in tasks.iter().enumerate() {
                    if i > 0 {
                        args.push(":::".to_string());
                    }
                    let resolved = crate::task::resolve_task_pattern(spec, Some(task));
                    let (name, task_args) = crate::task::task_list::split_task_spec(&resolved);
                    args.push(name.to_string());
                    args.extend(task_args);
                }
                run_mise_task(root, args.into_iter(), env)?;
            }
        }
    }
    Ok(())
}

fn run_mise_task(
    root: &Path,
    args: impl IntoIterator<Item = String>,
    env: &BTreeMap<String, String>,
) -> Result<()> {
    let mise_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mise"));
    let root = root.to_string_lossy().to_string();
    let args = ["--cd".to_string(), root, "run".to_string()]
        .into_iter()
        .chain(args)
        .collect_vec();
    cmd(mise_bin, args).stdout_to_stderr().full_env(env).run()?;
    Ok(())
}

async fn render_run_entries_for_hook(
    config: &Arc<Config>,
    root: &Path,
    env: &BTreeMap<String, String>,
    entries: &mut [RunEntry],
) -> Result<()> {
    let mut tera = get_tera(Some(root));
    let mut tera_ctx = config.tera_ctx.clone();
    tera_ctx.insert("env", env);
    for entry in entries {
        if entry.has_tera_template() {
            *entry = entry.render(&mut tera, &tera_ctx)?;
        }
    }
    Ok(())
}
