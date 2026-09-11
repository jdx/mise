//! Register changed definitions and emit non-blocking pitchfork session commands.
use super::runtime::{self, Runtime};
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::shell::Shell;
use crate::toolset::Toolset;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

const SESSION_ENV: &str = "__MISE_DAEMON_SESSIONS";

/// Shell-local bookkeeping travels with the existing activation environment.
/// Pitchfork owns process liveness, sessions, readiness, and autostop.
#[derive(Default, Serialize, Deserialize)]
struct Sessions {
    pid: u32,
    roots: BTreeMap<PathBuf, PathBuf>,
}

pub(crate) async fn emit(
    config: &Arc<Config>,
    ts: &Toolset,
    env: &EnvMap,
    pid: Option<u32>,
    shell: &dyn Shell,
) -> Result<String> {
    if Settings::no_hooks() || Settings::safe_mode() || !Settings::get().experimental {
        return Ok(String::new());
    }
    let set = config.daemons()?;
    let Some(pid) = pid.filter(|p| *p > 0) else {
        if set.auto() {
            hint!(
                "daemons-activation",
                "[daemons] auto lifecycle requires updated shell activation",
                "restart your shell or evaluate mise activate again"
            );
        }
        return Ok(String::new());
    };
    let previous: Sessions = crate::env::var(SESSION_ENV)
        .ok()
        .and_then(|s| crate::hook_env::deserialize(s).ok())
        .filter(|s: &Sessions| s.pid == pid)
        .unwrap_or_default();
    let mut next = Sessions {
        pid,
        ..Default::default()
    };
    let mut output = String::new();
    let roots: Vec<_> = set
        .roots()
        .into_iter()
        .filter(|root| set.for_root(root).auto())
        .collect();
    for (root, bin) in &previous.roots {
        if !roots.contains(root) {
            output.push_str(&session_command(bin, root, pid, false));
        }
    }
    for root in roots {
        let result = async {
            let scoped_set = set.for_root(&root);
            let previous_state = runtime::read_state(&root)?;
            let search_path = std::env::join_paths(
                env.get(&*crate::env::PATH_KEY)
                    .into_iter()
                    .flat_map(std::env::split_paths)
                    .filter(|path| path != &crate::dirs::shims()),
            )?;
            let bin = which::which_in("pitchfork", Some(search_path), &root)
                .ok()
                .or_else(|| previous_state.bin.is_file().then_some(previous_state.bin))
                .ok_or_else(|| {
                    eyre::eyre!("auto lifecycle requires pitchfork; run mise use pitchfork")
                })?;
            let runtime = Runtime {
                bin,
                env: env.clone(),
            };
            runtime::validate_tools(&scoped_set, config, ts).await?;
            let (_state, _lock) = runtime.prepare(&root, &scoped_set).await?;
            Ok::<_, eyre::Report>(runtime.bin)
        }
        .await;
        match result {
            Ok(bin) => {
                output.push_str(&session_command(&bin, &root, pid, true));
                next.roots.insert(root, bin);
            }
            Err(err) => {
                hint!(
                    "daemons-auto",
                    "daemon auto lifecycle: {err:#}; inspect with",
                    "mise daemons status"
                );
                // Preserve ownership so departure can still release an existing session.
                if let Some(bin) = previous.roots.get(&root) {
                    next.roots.insert(root, bin.clone());
                }
            }
        }
    }
    if !next.roots.is_empty() || !previous.roots.is_empty() {
        output.push_str(&shell.set_env(SESSION_ENV, &crate::hook_env::serialize(&next)?));
    }
    Ok(output)
}

fn session_command(bin: &std::path::Path, root: &std::path::Path, pid: u32, enter: bool) -> String {
    let action = if enter { "enter" } else { "leave" };
    let command = format!(
        "unset PITCHFORK_CONFIG; {} project {action} --pid {pid} --directory {} >/dev/null 2>&1 || printf '%s\\n' 'mise: daemon session {action} failed; run mise daemons status or mise daemons logs' >&2",
        super::presets::quote(bin.to_string_lossy()),
        super::presets::quote(root.to_string_lossy()),
    );
    // sh is available in all supported activation shells, including fish. The
    // subprocess receives the newly applied mise environment from shell eval.
    format!(
        "command sh -c {} </dev/null >/dev/null &\n",
        super::presets::quote(command)
    )
}
