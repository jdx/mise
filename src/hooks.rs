use crate::cmd::cmd;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
use crate::tera::get_tera;
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, hook_env};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
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
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookDef {
    /// Simple run string: `enter = "echo hello"`
    RunString(String),
    /// Table with run: `enter = { run = "echo hello" }`
    Run { run: String, shell: Option<String> },
    /// Table with script and optional shell: `enter = { script = "echo hello", shell = "bash" }`
    ScriptTable {
        script: String,
        shell: Option<String>,
    },
    /// Task reference: `enter = { task = "setup" }`
    TaskRef { task: String },
    /// Array of hook definitions: `enter = ["echo hello", { task = "setup" }]`
    Array(Vec<HookDef>),
}

impl HookDef {
    /// Convert to a list of Hook structs with the given hook type
    pub fn into_hooks(self, hook_type: Hooks) -> Vec<Hook> {
        match self {
            HookDef::RunString(script) => vec![Hook {
                hook: hook_type,
                action: HookAction::Run {
                    run: script,
                    shell: None,
                    legacy_script: false,
                    ignored_shell: None,
                },
                global: false,
            }],
            HookDef::Run { run, shell } => vec![Hook {
                hook: hook_type,
                action: HookAction::Run {
                    run,
                    shell,
                    legacy_script: false,
                    ignored_shell: None,
                },
                global: false,
            }],
            HookDef::ScriptTable { script, shell } => vec![Hook {
                hook: hook_type,
                action: match (hook_type, shell) {
                    (Hooks::Enter | Hooks::Leave | Hooks::Cd, Some(shell)) => {
                        HookAction::CurrentShell { script, shell }
                    }
                    (_, shell) => HookAction::Run {
                        run: script,
                        shell: None,
                        legacy_script: true,
                        ignored_shell: shell,
                    },
                },
                global: false,
            }],
            HookDef::TaskRef { task } => vec![Hook {
                hook: hook_type,
                action: HookAction::Task { task_name: task },
                global: false,
            }],
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
    pub action: HookAction,
    /// Whether this hook comes from a global config (skip directory matching)
    pub global: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum HookAction {
    Run {
        run: String,
        shell: Option<String>,
        legacy_script: bool,
        ignored_shell: Option<String>,
    },
    CurrentShell {
        script: String,
        shell: String,
    },
    Task {
        task_name: String,
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
                shell,
                ignored_shell,
                ..
            } => {
                *run = render(run)?;
                if let Some(s) = shell {
                    *s = render(s)?;
                }
                if let Some(s) = ignored_shell {
                    *s = render(s)?;
                }
            }
            HookAction::CurrentShell { script, shell } => {
                *script = render(script)?;
                *shell = render(shell)?;
            }
            HookAction::Task { task_name } => {
                *task_name = render(task_name)?;
            }
        }
        Ok(())
    }
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
    let shell_name = shell.map(|s| s.to_string()).unwrap_or_default();
    for (root, h) in all_hooks(config).await {
        if hook != h.hook || !matches_shell(h, &shell_name) {
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
        run_matched_hook(config, ts, root, h, shell, installed_tools).await;
    }
}

pub async fn run_enter_hooks_for_newly_loaded_configs(
    config: &Arc<Config>,
    ts: &Toolset,
    shell: &dyn Shell,
) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    if hook_env::dir_change().is_some() {
        return;
    }
    let Some(cwd) = dirs::CWD.as_ref() else {
        return;
    };
    let newly_loaded_roots = config
        .config_files
        .iter()
        .filter(|(path, _)| !hook_env::PREV_SESSION.loaded_configs.contains(*path))
        .filter_map(|(_, cf)| cf.project_root())
        .filter(|root| cwd.starts_with(root))
        .collect::<IndexSet<_>>();
    if newly_loaded_roots.is_empty() {
        return;
    }
    let shell_name = shell.to_string();
    for (root, h) in config.hooks().await.cloned().unwrap_or_default() {
        if h.hook != Hooks::Enter || h.global || !cwd.starts_with(&root) {
            continue;
        }
        if !matches_shell(&h, &shell_name) {
            continue;
        }
        if !newly_loaded_roots.contains(&root) {
            continue;
        }
        run_matched_hook(config, ts, &root, &h, Some(shell), None).await;
    }
}

async fn run_matched_hook(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    shell: Option<&dyn Shell>,
    installed_tools: Option<&[InstalledToolInfo]>,
) {
    let hook_type = hook.hook;
    match &hook.action {
        HookAction::Task { task_name } => {
            if let Err(e) = execute_task(config, ts, root, hook, task_name, installed_tools).await {
                warn!("{hook_type} hook in {} failed: {e}", root.display());
            }
        }
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
            println!("{script}");
        }
        HookAction::Run { .. } => {
            if let Err(e) = execute(config, ts, root, hook, installed_tools).await {
                // Warn but continue running remaining hooks of this type
                warn!("{hook_type} hook in {} failed: {e}", root.display());
            }
        }
    }
}

fn matches_shell(hook: &Hook, shell_name: &str) -> bool {
    if let HookAction::CurrentShell { shell, .. } = &hook.action {
        shell == shell_name
    } else {
        true
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
            "`shell` is ignored for {} hooks that use `script`; use `run = ...` with `shell = \"bash -c\"` to choose an inline shell command.",
            hook_name
        );
    }
    let shell = shell
        .as_ref()
        .map(|shell| shell_words::split(shell))
        .transpose()?
        .unwrap_or(Settings::get().default_inline_shell()?);

    // Preinstall hooks skip `tools=true` env directives since the tools
    // providing those env vars aren't installed yet (fixes #6162)
    let (tera_ctx, mut env) = if hook.hook == Hooks::Preinstall {
        let env = ts.full_env_without_tools(config).await?;
        let mut ctx = config.tera_ctx.clone();
        ctx.insert("env", &env);
        (ctx, env)
    } else {
        let ctx = ts.tera_ctx(config).await?.clone();
        let env = ts.full_env(config).await?;
        (ctx, env)
    };
    let mut tera = get_tera(Some(root));
    let rendered_script = tera.render_str(run, &tera_ctx)?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(rendered_script.as_str()))
        .collect_vec();
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
    env.insert(
        "MISE_CONFIG_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    // Prevent recursive hook execution (e.g. hook runs `mise run` which spawns
    // a shell that activates mise and re-triggers hooks)
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());
    cmd(&shell[0], args)
        .stdout_to_stderr()
        // .dir(root)
        .full_env(env)
        .run()?;
    Ok(())
}

async fn execute_task(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    task_name: &str,
    installed_tools: Option<&[InstalledToolInfo]>,
) -> Result<()> {
    Settings::get().ensure_experimental("hooks")?;

    let mise_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mise"));

    let mut env = if hook.hook == Hooks::Preinstall {
        ts.full_env_without_tools(config).await?
    } else {
        ts.full_env(config).await?
    };
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
    if let Some(tools) = installed_tools
        && let Ok(json) = serde_json::to_string(tools)
    {
        env.insert("MISE_INSTALLED_TOOLS".to_string(), json);
    }
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());

    cmd(
        mise_bin,
        ["--cd", &root.to_string_lossy(), "run", task_name],
    )
    .stdout_to_stderr()
    .full_env(env)
    .run()?;
    Ok(())
}
