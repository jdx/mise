use crate::cmd::cmd;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
use crate::toolset::Toolset;
use crate::{dirs, hook_env};
use eyre::{Result, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;
use std::{iter::once, sync::Arc};
use tokio::sync::OnceCell;

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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Hook {
    pub hook: Hooks,
    pub script: String,
    pub shell: Option<String>,
}

pub static SCHEDULED_HOOKS: Lazy<Mutex<IndexSet<Hooks>>> = Lazy::new(Default::default);

pub fn schedule_hook(hook: Hooks) {
    let mut mu = SCHEDULED_HOOKS.lock().unwrap();
    mu.insert(hook);
}

pub async fn run_all_hooks(config: &Arc<Config>, ts: &Toolset, shell: &dyn Shell) {
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
                if let Ok(cf) = config_file::parse(p) {
                    if let Ok(h) = cf.hooks() {
                        hooks.extend(h.into_iter().map(|h| (cf.config_root(), h)));
                    }
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
    for (root, h) in all_hooks(config).await {
        if hook != h.hook || (h.shell.is_some() && h.shell != shell.map(|s| s.to_string())) {
            continue;
        }
        trace!("running hook {hook} in {root:?}");
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
            _ => {}
        }
        if h.shell.is_some() {
            println!("{}", h.script);
        } else if let Err(e) = execute(config, ts, root, h).await {
            warn!("error executing hook: {e}");
        }
    }
}

impl Hook {
    pub fn from_toml(hook: Hooks, value: toml::Value) -> Result<Vec<Self>> {
        match value {
            toml::Value::String(run) => Ok(vec![Hook {
                hook,
                script: run,
                shell: None,
            }]),
            toml::Value::Table(tbl) => {
                let script = tbl
                    .get("script")
                    .ok_or_else(|| eyre!("missing `script` key"))?;
                let script = script
                    .as_str()
                    .ok_or_else(|| eyre!("`run` must be a string"))?;
                let shell = tbl
                    .get("shell")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                Ok(vec![Hook {
                    hook,
                    script: script.to_string(),
                    shell,
                }])
            }
            toml::Value::Array(arr) => {
                let mut hooks = vec![];
                for v in arr {
                    hooks.extend(Self::from_toml(hook, v)?);
                }
                Ok(hooks)
            }
            v => panic!("invalid hook value: {v}"),
        }
    }
}

async fn execute(config: &Arc<Config>, ts: &Toolset, root: &Path, hook: &Hook) -> Result<()> {
    Settings::get().ensure_experimental("hooks")?;
    let shell = Settings::get().default_inline_shell()?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(hook.script.as_str()))
        .collect_vec();
    let mut env = ts.full_env(config).await?;
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
    // TODO: this should be different but I don't have easy access to it
    // env.insert("MISE_CONFIG_ROOT".to_string(), root.to_string_lossy().to_string());
    cmd(&shell[0], args)
        .stdout_to_stderr()
        // .dir(root)
        .full_env(env)
        .run()?;
    Ok(())
}
