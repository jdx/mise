use crate::cmd::cmd;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, hook_env};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;
use std::{iter::once, sync::Arc};
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
    #[serde(rename = "pre_task")]
    PreTask,
    #[serde(rename = "post_task")]
    PostTask,
}

/// Represents a hook definition in TOML config.
/// Supports string, table, or array formats via serde untagged deserialization.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookDef {
    /// Simple script string: `enter = "echo hello"`
    Script(String),
    /// Table with script/run and optional shell/task filter:
    /// `enter = { script = "echo hello", shell = "bash" }`
    /// `pre_task = { script = "echo hello", task = "deploy" }`
    /// `pre_task = { run = "setup", task = "deploy" }`
    Table {
        /// Shell script to execute. One of `script` or `run` must be set.
        script: Option<String>,
        /// Task name to run instead of a script. One of `script` or `run` must be set.
        run: Option<String>,
        shell: Option<String>,
        /// Optional task name filter for pre_task/post_task hooks.
        /// Supports glob patterns (e.g., "deploy:*").
        task: Option<String>,
    },
    /// Array of hook definitions: `enter = ["echo hello", { script = "echo world" }]`
    Array(Vec<HookDef>),
}

impl HookDef {
    /// Convert to a list of Hook structs with the given hook type
    pub fn into_hooks(self, hook_type: Hooks) -> Vec<Hook> {
        match self {
            HookDef::Script(script) => vec![Hook {
                hook: hook_type,
                script: Some(script),
                run: None,
                shell: None,
                task: None,
                global: false,
            }],
            HookDef::Table {
                script,
                run,
                shell,
                task,
            } => {
                if script.is_some() && run.is_some() {
                    warn!(
                        "hook definition has both 'script' and 'run', 'run' will take precedence"
                    );
                }
                if script.is_none() && run.is_none() {
                    warn!("hook definition has neither 'script' nor 'run', skipping");
                    return vec![];
                }
                vec![Hook {
                    hook: hook_type,
                    script,
                    run,
                    shell,
                    task,
                    global: false,
                }]
            }
            HookDef::Array(arr) => arr
                .into_iter()
                .flat_map(|d| d.into_hooks(hook_type))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Hook {
    pub hook: Hooks,
    /// Shell script to execute. Mutually exclusive with `run`.
    pub script: Option<String>,
    /// Task name to run instead of a script. Mutually exclusive with `script`.
    pub run: Option<String>,
    pub shell: Option<String>,
    /// Optional task name filter for pre_task/post_task hooks.
    /// Supports glob patterns (e.g., "deploy:*").
    pub task: Option<String>,
    /// Whether this hook comes from a global config (skip directory matching)
    pub global: bool,
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
        if hook != h.hook || (h.shell.is_some() && h.shell != shell.map(|s| s.to_string())) {
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
                (Hooks::Cd, Some((_old, new))) => {
                    if !new.starts_with(root) {
                        continue;
                    }
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
        if h.shell.is_some() {
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
            if let Some(ref script) = h.script {
                println!("{}", script);
            }
        } else if let Err(e) = execute(config, ts, root, h, installed_tools).await {
            // Warn but continue running remaining hooks of this type
            warn!("{hook} hook in {} failed: {e}", root.display());
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

    // Preinstall hooks skip `tools=true` env directives since the tools
    // providing those env vars aren't installed yet (fixes #6162)
    let mut env = if hook.hook == Hooks::Preinstall {
        ts.full_env_without_tools(config).await?
    } else {
        ts.full_env(config).await?
    };

    setup_hook_env(&mut env, root, installed_tools);
    run_hook_command(hook, env, None)?;
    Ok(())
}

/// Run pre_task or post_task hooks matching the given task name.
/// Called from the task executor before/after task execution.
pub async fn run_task_hooks(
    config: &Arc<Config>,
    hook_type: Hooks,
    task_name: &str,
    task_env: &BTreeMap<String, String>,
    dir: &Path,
) -> Result<()> {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return Ok(());
    }
    debug_assert!(
        hook_type == Hooks::PreTask || hook_type == Hooks::PostTask,
        "run_task_hooks called with non-task hook type"
    );

    let hooks = config.hooks().await.cloned().unwrap_or_default();
    for (root, h) in &hooks {
        if h.hook != hook_type {
            continue;
        }
        // Filter by task name if a task pattern is specified
        if let Some(ref task_pattern) = h.task
            && !task_matches(task_pattern, task_name)
        {
            continue;
        }
        // Directory scope: only run if CWD is under the config root (unless global)
        if !h.global
            && let Some(cwd) = dirs::CWD.as_ref()
            && !cwd.starts_with(root)
        {
            continue;
        }
        trace!("running {hook_type} hook for task {task_name} in {root:?}");
        if let Err(e) = execute_task_hook(task_env, root, h, task_name, dir).await {
            warn!(
                "{hook_type} hook for task {task_name} in {} failed: {e}",
                root.display()
            );
            return Err(e);
        }
    }
    Ok(())
}

/// Check if a task name matches a hook's task filter pattern.
/// Supports glob patterns (e.g., "terraform:*" matches "terraform:plan").
fn task_matches(pattern: &str, task_name: &str) -> bool {
    if let Ok(pat) = glob::Pattern::new(pattern) {
        pat.matches(task_name)
    } else {
        pattern == task_name
    }
}

/// Execute a single task hook with the task's environment.
/// Supports both `script` (shell command) and `run` (task name) hooks.
async fn execute_task_hook(
    task_env: &BTreeMap<String, String>,
    root: &Path,
    hook: &Hook,
    task_name: &str,
    dir: &Path,
) -> Result<()> {
    Settings::get().ensure_experimental("hooks")?;

    let mut env = task_env.clone();
    env.insert("MISE_TASK_NAME".to_string(), task_name.to_string());
    setup_hook_env(&mut env, root, None);
    run_hook_command(hook, env, Some(dir))?;
    Ok(())
}

/// Set up common hook environment variables
fn setup_hook_env(
    env: &mut BTreeMap<String, String>,
    root: &Path,
    installed_tools: Option<&[InstalledToolInfo]>,
) {
    if let Some(cwd) = dirs::CWD.as_ref() {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    env.insert(
        "MISE_CONFIG_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    if let Some((Some(old), _new)) = hook_env::dir_change() {
        env.insert(
            "MISE_PREVIOUS_DIR".to_string(),
            old.to_string_lossy().to_string(),
        );
    }
    // Add installed tools info for postinstall hooks
    if let Some(tools) = installed_tools
        && let Ok(json) = serde_json::to_string(tools)
    {
        env.insert("MISE_INSTALLED_TOOLS".to_string(), json);
    }
    // Prevent recursive hook execution
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());
}

/// Execute a hook command (either `run` task or `script`)
fn run_hook_command(hook: &Hook, env: BTreeMap<String, String>, dir: Option<&Path>) -> Result<()> {
    if let Some(ref run_task) = hook.run {
        // MISE_NO_HOOKS=1 is already set to prevent recursive hook execution
        let mise_bin = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("mise"))
            .to_string_lossy()
            .to_string();
        let mut cmd = cmd(&mise_bin, ["run", run_task.as_str()])
            .stdout_to_stderr()
            .full_env(env);
        if let Some(dir) = dir {
            cmd = cmd.dir(dir);
        }
        cmd.run()?;
    } else if let Some(ref script) = hook.script {
        let shell = Settings::get().default_inline_shell()?;
        let args = shell
            .iter()
            .skip(1)
            .map(|s| s.as_str())
            .chain(once(script.as_str()))
            .collect_vec();
        let mut cmd = cmd(&shell[0], args).stdout_to_stderr().full_env(env);
        if let Some(dir) = dir {
            cmd = cmd.dir(dir);
        }
        cmd.run()?;
    }
    Ok(())
}
