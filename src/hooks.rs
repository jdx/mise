use crate::cmd::cmd;
use crate::config::{Config, SETTINGS};
use crate::toolset::Toolset;
use crate::{dirs, hook_env};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::iter::once;
use std::path::Path;
use std::sync::Mutex;

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
    pub run: String,
}

pub static SCHEDULED_HOOKS: Lazy<Mutex<IndexSet<Hooks>>> = Lazy::new(Default::default);

pub fn schedule_hook(hook: Hooks) {
    let mut mu = SCHEDULED_HOOKS.lock().unwrap();
    mu.insert(hook);
}

pub fn run_all_hooks(ts: &Toolset) {
    let mut mu = SCHEDULED_HOOKS.lock().unwrap();
    for hook in mu.drain(..) {
        run_one_hook(ts, hook);
    }
}

pub fn run_one_hook(ts: &Toolset, hook: Hooks) {
    let config = Config::get();
    let hooks = config.hooks().unwrap_or_default();
    for (root, h) in hooks {
        if hook != h.hook {
            continue;
        }
        trace!("running hook {hook} in {root:?}");
        match (hook, hook_env::dir_change()) {
            (Hooks::Enter, Some((old, new))) => {
                if !new.starts_with(&root) {
                    continue;
                }
                if old.starts_with(&root) {
                    continue;
                }
            }
            (Hooks::Leave, Some((old, new))) => {
                warn!("leave hook not yet implemented");
                if new.starts_with(&root) {
                    continue;
                }
                if !old.starts_with(&root) {
                    continue;
                }
            }
            _ => {}
        }
        if let Err(e) = execute(ts, &root, &h) {
            warn!("error executing hook: {e}");
        }
    }
}

fn execute(ts: &Toolset, root: &Path, hook: &Hook) -> Result<()> {
    SETTINGS.ensure_experimental("hooks")?;
    #[cfg(unix)]
    let shell = shell_words::split(&SETTINGS.unix_default_inline_shell_args)?;
    #[cfg(windows)]
    let shell = shell_words::split(&SETTINGS.windows_default_inline_shell_args)?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(hook.run.as_str()))
        .collect_vec();
    let mut env = ts.full_env()?;
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
    if let Some((old, _new)) = hook_env::dir_change() {
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
