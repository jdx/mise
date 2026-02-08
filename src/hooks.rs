use crate::cmd::cmd;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
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

/// Represents a hook definition in TOML config.
/// Supports string, table, or array formats via serde untagged deserialization.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookDef {
    /// Simple script string: `enter = "echo hello"`
    Script(String),
    /// Table with script and optional shell: `enter = { script = "echo hello", shell = "bash" }`
    Table {
        script: String,
        shell: Option<String>,
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
                script,
                shell: None,
                global: false,
            }],
            HookDef::Table { script, shell } => vec![Hook {
                hook: hook_type,
                script,
                shell,
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
    pub script: String,
    pub shell: Option<String>,
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
            println!("{}", h.script);
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
    let shell = Settings::get().default_inline_shell()?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(hook.script.as_str()))
        .collect_vec();
    // Preinstall hooks skip `tools=true` env directives since the tools
    // providing those env vars aren't installed yet (fixes #6162)
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
