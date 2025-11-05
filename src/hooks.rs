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
    pub os: Option<String>,
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
                if let Ok(cf) = config_file::parse(p).await
                    && let Ok(h) = cf.hooks()
                {
                    hooks.extend(h.into_iter().map(|h| (cf.config_root(), h)));
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
    let settings = Settings::get();
    let current_os = settings.os();
    for (root, h) in all_hooks(config).await {
        if hook != h.hook 
            || (h.shell.is_some() && h.shell != shell.map(|s| s.to_string()))
            || (h.os.is_some() && h.os.as_deref() != Some(current_os)) {
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
                os: None,
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
                let os = tbl
                    .get("os")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                Ok(vec![Hook {
                    hook,
                    script: script.to_string(),
                    shell,
                    os,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiple_hooks_with_different_os() {
        // Test that multiple [[hooks.enter]] sections with different OS values don't overwrite each other
        let toml_str = r#"
            [[hooks.enter]]
            os = "linux"
            script = "echo linux"

            [[hooks.enter]]
            os = "macos"
            script = "echo macos"

            [[hooks.enter]]
            os = "windows"
            script = "echo windows"

            [[hooks.enter]]
            script = "echo any"
        "#;

        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let hooks_value = value.get("hooks").unwrap();
        let enter_value = hooks_value.get("enter").unwrap();
        
        let hooks = Hook::from_toml(Hooks::Enter, enter_value.clone()).unwrap();
        
        // Should have 4 hooks total
        assert_eq!(hooks.len(), 4);
        
        // Verify each hook is present
        assert!(hooks.iter().any(|h| h.os.as_deref() == Some("linux") && h.script == "echo linux"));
        assert!(hooks.iter().any(|h| h.os.as_deref() == Some("macos") && h.script == "echo macos"));
        assert!(hooks.iter().any(|h| h.os.as_deref() == Some("windows") && h.script == "echo windows"));
        assert!(hooks.iter().any(|h| h.os.is_none() && h.script == "echo any"));
    }

    #[test]
    fn test_hook_from_toml_single_string() {
        let value = toml::Value::String("echo test".to_string());
        let hooks = Hook::from_toml(Hooks::Enter, value).unwrap();
        
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].script, "echo test");
        assert_eq!(hooks[0].shell, None);
        assert_eq!(hooks[0].os, None);
    }

    #[test]
    fn test_hook_from_toml_table_with_os() {
        let toml_str = r#"
            script = "echo test"
            os = "linux"
            shell = "bash"
        "#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let hooks = Hook::from_toml(Hooks::Enter, value).unwrap();
        
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].script, "echo test");
        assert_eq!(hooks[0].os, Some("linux".to_string()));
        assert_eq!(hooks[0].shell, Some("bash".to_string()));
    }
}
